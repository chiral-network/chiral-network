<script lang="ts">
  import Button from '$lib/components/ui/button.svelte'
  import Card from '$lib/components/ui/card.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import { ShieldCheck, ShieldX, Globe, Activity, Plus, Power, Trash2 } from 'lucide-svelte'
  import { onMount, onDestroy } from 'svelte';
  import { proxyNodes, connectProxy, disconnectProxy, removeProxy, listProxies, getProxyOptimizationStatus } from '$lib/proxy';
  import { ProxyLatencyOptimizationService } from '$lib/services/proxyLatencyOptimization';
  import { t } from 'svelte-i18n'
  import DropDown from '$lib/components/ui/dropDown.svelte'
  import { ProxyAuthService } from '$lib/proxyAuth';
  import { privacyStore, proxyRoutingService, initializeProxyRouting, persistPrivacyProfile } from '$lib/services/proxyRoutingService';
  import { computeProxyWeight } from '$lib/services/routingWeights';
  
  let newNodeAddress = ''
  let proxyEnabled = true
  let isAddressValid = true
  let addressError = ''
  let showConfirmDialog = false
  let nodeToRemove: any = null
  let connectionTimeouts = new Map<string, ReturnType<typeof setTimeout>>()
  let reconnectIntervals = new Map<string, ReturnType<typeof setInterval>>()
  let autoReconnectEnabled = true
  let performanceHistory = new Map<string, {
    totalAttempts: number
    successfulConnections: number
    lastSuccessTime?: Date
    lastFailureTime?: Date
    averageLatency?: number
    uptimePercentage: number
  }>()
  
  // Proxy latency optimization variables
  let optimizationStatus = ""
  let isTestingOptimization = false
  let testResults = ""
  
  const LS_PROXY_ENABLED = 'proxy.enabled';
  const LS_PROXY_AUTO_RECONNECT = 'proxy.autoReconnect';
  const ipv4Regex = /^(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?):[0-9]{1,5}$/
  const domainRegex = /^[a-zA-Z0-9][a-zA-Z0-9-]*[a-zA-Z0-9]*\.([a-zA-Z]{2,}|[a-zA-Z]{2,}\.[a-zA-Z]{2,}):[0-9]{1,5}$/
  const enodeRegex = /^enode:\/\/[a-fA-F0-9]{128}@(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?):[0-9]{1,5}$/
  let statusFilter = 'all'
  let sortBy: 'status' | 'latency' | 'bandwidth' | 'smart' = 'status'
  
  $: statusOptions = [
    { value: 'all', label: $t('All') },
    { value: 'online', label: $t('Online') },
    { value: 'offline', label: $t('Offline') },
    { value: 'connecting', label: $t('Connecting') },
    { value: 'error', label: 'Error' }
  ]

  $: sortOptions = [
    { value: 'status', label: $t('Sort by status') },
    { value: 'latency', label: $t('Sort by latency') },
    { value: 'bandwidth', label: $t('Sort by bandwidth') },
    { value: 'smart', label: $t('Sort by smart (quality)') }
  ]

  $: filteredNodes = $proxyNodes.filter(node => {
      if (statusFilter === 'all') {
          return true
      }
      return node.status === statusFilter
  })

  
  $: sortedNodes = [...filteredNodes].sort((a, b) => {
      if (sortBy === 'status') {
        // Preserve upstream status precedence and include error state with a safe fallback
        const statusOrder: Record<string, number> = { online: 1, connecting: 2, offline: 3, error: 4 }
        const aOrder = statusOrder[(a.status as string) || 'offline'] ?? 5
        const bOrder = statusOrder[(b.status as string) || 'offline'] ?? 5
        return aOrder - bOrder
      }

      if (sortBy === 'latency') {
        // Ascending: lower latency first; undefined latencies go to the end
        const aLat = a.latency ?? Number.POSITIVE_INFINITY
        const bLat = b.latency ?? Number.POSITIVE_INFINITY
        return aLat - bLat
      }

      if (sortBy === 'bandwidth') {
        const aBw = a.latency != null ? Math.round(100 - a.latency) : Number.NEGATIVE_INFINITY
        const bBw = b.latency != null ? Math.round(100 - b.latency) : Number.NEGATIVE_INFINITY
        return bBw - aBw
      }
      // smart
      const aW = computeProxyWeight({
        latencyMs: a.latency,
        uptimePct: getUptimePct(a.address),
        status: a.status as any,
        recentFailures: getRecentFailures(a.address),
      })
      const bW = computeProxyWeight({
        latencyMs: b.latency,
        uptimePct: getUptimePct(b.address),
        status: b.status as any,
        recentFailures: getRecentFailures(b.address),
      })
      return bW - aW
  });

  
  onMount(() => {
      // Initialize proxy routing with stored privacy preferences
      initializeProxyRouting();
      
      listProxies();
      
      // Initialize proxy latency optimization
      updateOptimizationStatus();

      // Clean up expired authentication tokens
      ProxyAuthService.cleanupExpiredTokens();

      // Restore preferences
      try {
      const pe = localStorage.getItem(LS_PROXY_ENABLED);
      if (pe !== null) proxyEnabled = pe === 'true';
      const ar = localStorage.getItem(LS_PROXY_AUTO_RECONNECT);
      if (ar !== null) autoReconnectEnabled = ar === 'true';
      } catch {}
  });

  localStorage.setItem(LS_PROXY_ENABLED, String(proxyEnabled));
  localStorage.setItem(LS_PROXY_AUTO_RECONNECT, String(autoReconnectEnabled));

  function validateAddress(address: string): { valid: boolean; error: string } {
      if (!address || address.trim() === '') {
          return { valid: false, error: $t('proxy.validation.emptyAddress') }
      }

      const trimmed = address.trim()

      if (trimmed.includes(' ')) {
          return { valid: false, error: $t('proxy.validation.noSpaces') }
      }

      // Check for enode format first
      if (trimmed.startsWith('enode://')) {
          if (enodeRegex.test(trimmed)) {
              return { valid: true, error: '' }
          } else {
              return { valid: false, error: $t('proxy.validation.invalidEnode') }
          }
      }

      // Standard host:port validation
      if (!trimmed.includes(':')) {
          return { valid: false, error: $t('proxy.validation.missingPort') }
      }

      const [host, portStr] = trimmed.split(':')

      if (!host) {
          return { valid: false, error: $t('proxy.validation.invalidHostname') }
      }

      const port = parseInt(portStr)
      if (isNaN(port) || port < 1 || port > 65535) {
          return { valid: false, error: $t('proxy.validation.invalidPort') }
      }

      if (port < 1024 && port !== 80 && port !== 443) {
          return { valid: false, error: $t('proxy.validation.systemPort') }
      }

      if (!ipv4Regex.test(trimmed) && !domainRegex.test(trimmed)) {
          return { valid: false, error: $t('proxy.validation.invalidFormat') }
      }

      return { valid: true, error: '' }
  }

  function updatePerformanceHistory(address: string, success: boolean, latency?: number) {
      const current = performanceHistory.get(address) || {
          totalAttempts: 0,
          successfulConnections: 0,
          uptimePercentage: 0
      }

      current.totalAttempts++
      if (success) {
          current.successfulConnections++
          current.lastSuccessTime = new Date()
          if (latency !== undefined) {
              current.averageLatency = current.averageLatency
                  ? (current.averageLatency + latency) / 2
                  : latency
          }
      } else {
          current.lastFailureTime = new Date()
      }

      current.uptimePercentage = Math.round((current.successfulConnections / current.totalAttempts) * 100)
      performanceHistory.set(address, current)
  }

  function getPerformanceStats(address: string) {
      return performanceHistory.get(address) || {
          totalAttempts: 0,
          successfulConnections: 0,
          uptimePercentage: 0
      }
  }

  function startConnectionTimeout(address: string) {
      // Clear any existing timeout
      const existingTimeout = connectionTimeouts.get(address)
      if (existingTimeout) {
          clearTimeout(existingTimeout)
      }

      // Set new timeout (15 seconds)
      const timeout = setTimeout(() => {
          // Find the node and mark as error (timeout)
          proxyNodes.update(nodes => {
              return nodes.map(node =>
                  node.address === address && node.status === 'connecting'
                      ? { ...node, status: 'error' as const, error: $t('proxy.errors.connectionTimeout') }
                      : node
              )
          })
          connectionTimeouts.delete(address)

          // Track timeout as failure
          updatePerformanceHistory(address, false)

          // Start auto-reconnect for timed out connections
          startAutoReconnect(address)
      }, 15000)

      connectionTimeouts.set(address, timeout)
  }

  function clearConnectionTimeout(address: string) {
      const timeout = connectionTimeouts.get(address)
      if (timeout) {
          clearTimeout(timeout)
          connectionTimeouts.delete(address)
      }
  }

  function startAutoReconnect(address: string) {
      if (!autoReconnectEnabled) return

      // Clear any existing reconnect interval
      const existingInterval = reconnectIntervals.get(address)
      if (existingInterval) {
          clearInterval(existingInterval)
      }

      // Set up reconnect attempts every 30 seconds
      const interval = setInterval(() => {
          const node = $proxyNodes.find(n => n.address === address)
          if (!node || node.status === 'online') {
              // Stop reconnecting if node is removed or online
              clearInterval(interval)
              reconnectIntervals.delete(address)
              return
          }

          if (node.status === 'offline' || node.status === 'error') {
              console.log(`Auto-reconnecting to proxy: ${address}`)
              startConnectionTimeout(address)
              connectProxy(address, "dummy-token")
          }
      }, 30000) // Retry every 30 seconds

      reconnectIntervals.set(address, interval)
  }

  function stopAutoReconnect(address: string) {
      const interval = reconnectIntervals.get(address)
      if (interval) {
          clearInterval(interval)
          reconnectIntervals.delete(address)
      }
  }

  function addNode() {
      const isDuplicate = $proxyNodes.some(node => node.address === newNodeAddress.trim())
      if (isDuplicate) {
          alert($t('proxy.alreadyAdded'))
          return
      }

      const validation = validateAddress(newNodeAddress)
      if (!validation.valid) {
          addressError = validation.error
          return
      }

      // Start connection timeout
      startConnectionTimeout(newNodeAddress.trim())

      // Generate secure authentication token
      ProxyAuthService.generateProxyToken(newNodeAddress.trim()).then(token => {
          connectProxy(newNodeAddress.trim(), token);
          newNodeAddress = ''
          addressError = ''
      }).catch(error => {
          console.error('Failed to generate proxy auth token:', error);
          // Fallback to a basic token if generation fails
          const fallbackToken = ProxyAuthService.generateFallbackToken(newNodeAddress.trim());
          connectProxy(newNodeAddress.trim(), fallbackToken);
          newNodeAddress = ''
          addressError = ''
      });
  }

  function requestRemoveNode(node: any) {
    nodeToRemove = node;
    showConfirmDialog = true
  }

  function confirmRemoveNode() {
    if (nodeToRemove && nodeToRemove.address) {
      clearConnectionTimeout(nodeToRemove.address)
      stopAutoReconnect(nodeToRemove.address)
      removeProxy(nodeToRemove.address)
    }
    showConfirmDialog = false
    nodeToRemove = null
  }

  function cancelRemoveNode() {
    showConfirmDialog = false
    nodeToRemove = null
  }

  function toggleNode(node: any) {
      if (node.status === 'online') {
          clearConnectionTimeout(node.address)
          stopAutoReconnect(node.address)
          disconnectProxy(node.address);
      } else {
          // Start connection timeout for reconnection attempts
          startConnectionTimeout(node.address)

          // Get or generate authentication token
          ProxyAuthService.getProxyToken(node.address).then(existingToken => {
              if (existingToken) {
                  // Use existing valid token
                  connectProxy(node.address, existingToken);
              } else {
                  // Generate new token
                  ProxyAuthService.generateProxyToken(node.address).then(token => {
                      connectProxy(node.address, token);
                  }).catch(error => {
                      console.error('Failed to generate proxy auth token:', error);
                      // Fallback to a basic token if generation fails
                      const fallbackToken = ProxyAuthService.generateFallbackToken(node.address);
                      connectProxy(node.address, fallbackToken);
                  });
              }
          }).catch(error => {
              console.error('Failed to get proxy auth token:', error);
              // Fallback to generating a new token
              ProxyAuthService.generateProxyToken(node.address).then(token => {
                  connectProxy(node.address, token);
              }).catch(error => {
                  console.error('Failed to generate proxy auth token:', error);
                  // Fallback to a basic token if generation fails
                  const fallbackToken = ProxyAuthService.generateFallbackToken(node.address);
                  connectProxy(node.address, fallbackToken);
              });
          });
      }
  }

  function getUptimePct(address: string | undefined) {
    if (!address) return 0
    const stats = getPerformanceStats(address)
    return stats.uptimePercentage ?? 0
  }

  function getRecentFailures(address: string | undefined) {
    if (!address) return 0
    const stats = getPerformanceStats(address)
    const failures = stats.totalAttempts - stats.successfulConnections
    return Math.max(0, failures)
  }

  // Proxy latency optimization functions
  async function updateOptimizationStatus() {
      try {
          optimizationStatus = await getProxyOptimizationStatus();
      } catch (e) {
          console.error('Failed to get optimization status:', e);
          optimizationStatus = $t('proxy.errors.optimizationUnavailable');
      }
  }

  async function testProxyLatencyOptimization() {
    isTestingOptimization = true;
    testResults = "";

    try {
      const isTauriAvailable = await ProxyLatencyOptimizationService.isTauriAvailable();
      if (!isTauriAvailable) {
        testResults = $t('proxy.errors.tauriUnavailable');
        return;
      }

      console.log("[TEST] Testing Proxy Latency Optimization...");
      testResults = "[TEST] Running comprehensive proxy optimization tests...\n";
      
      // Test 1: Update some proxy latencies
      console.log("Test 1: Updating proxy latencies...");
      testResults += "\n[STATS] Test 1: Updating proxy latencies...\n";
      
      await ProxyLatencyOptimizationService.updateProxyLatency("test-proxy-1", 50);
      await ProxyLatencyOptimizationService.updateProxyLatency("test-proxy-2", undefined);
      await ProxyLatencyOptimizationService.updateProxyLatency("test-proxy-3", 30);
      await ProxyLatencyOptimizationService.updateProxyLatency("test-proxy-4", 100);
      await ProxyLatencyOptimizationService.updateProxyLatency("test-proxy-5", 25);
      
      testResults += "[OK] Updated 5 test proxies with varying latencies\n";
      
      // Test 2: Get optimization status
      console.log("Test 2: Getting optimization status...");
      testResults += "\n[UP] Test 2: Checking optimization status...\n";
      
      const status = await ProxyLatencyOptimizationService.getOptimizationStatus();
      testResults += `[OK] Optimization enabled: ${status}\n`;

      testResults += "\n[DONE] All proxy latency optimization tests completed successfully!";
      
      // Update the main optimization status
      await updateOptimizationStatus();
    } catch (error) {
      console.error("[X] Proxy latency optimization test failed:", error);
      testResults += `\n[X] Test failed with error: ${error}`;
    } finally {
      isTestingOptimization = false;
    }
  }

  
  $: activeNodes = $proxyNodes.filter(n => n.status === 'online').length
  $: totalBandwidth = $proxyNodes.reduce((sum, n) => sum + (n.status === 'online' ? (n.latency ? Math.round(100 - n.latency) : 50) : 0), 0)
  $: {
      const validation = validateAddress(newNodeAddress)
      isAddressValid = validation.valid
      if (newNodeAddress.trim() !== '' && !validation.valid) {
          addressError = validation.error
      } else {
          addressError = ''
      }
  }

// Clear pending timeouts when not connecting and stop auto-reconnect once online
 $: {
   $proxyNodes.forEach(node => {
     if (!node.address) return
     if (node.status !== 'connecting') {
       clearConnectionTimeout(node.address)
     }
     if (node.status === 'online') {
       stopAutoReconnect(node.address)
     }
   })
}

  // Track successful connections when nodes come online
  $: {
      $proxyNodes.forEach(node => {
          if (node.status === 'online' && node.address) {
              // Check if this is a new successful connection
              const stats = getPerformanceStats(node.address)
              const timeSinceLastSuccess = stats.lastSuccessTime
                  ? Date.now() - stats.lastSuccessTime.getTime()
                  : Infinity

              // Only update if it's been more than 5 seconds since last success (prevent spam)
              if (timeSinceLastSuccess > 5000) {
                  updatePerformanceHistory(node.address, true, node.latency)
              }
          }
      })
  }
//Clean up timers on component destroy
onDestroy(() => {
   connectionTimeouts.forEach(t => clearTimeout(t))
   reconnectIntervals.forEach(i => clearInterval(i))
   connectionTimeouts.clear()
   reconnectIntervals.clear()
 })
</script>



<!-- Confirmation Dialog -->
{#if showConfirmDialog && nodeToRemove}
  <div class="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
    <div class="bg-white rounded-lg p-6 max-w-md w-full mx-4 overflow-hidden">
      <h3 class="text-lg font-semibold mb-4">{$t('proxy.dialog.confirmRemoval')}</h3>
      <p class="text-muted-foreground mb-6 break-words">
        {$t('proxy.dialog.confirmMessage')} <span class="font-medium">{nodeToRemove.address}</span>
      </p>
      <div class="flex gap-3 justify-center">
        <Button variant="outline" on:click={cancelRemoveNode}>
          {$t('proxy.dialog.cancel')}
        </Button>
        <Button variant="destructive" on:click={confirmRemoveNode}>
          <Trash2 class="h-4 w-4 mr-2" />
          {$t('proxy.dialog.remove')}
        </Button>
      </div>
    </div>
  </div>
{/if}

<div class="space-y-6">
  <div>
    <h1 class="text-3xl font-bold">{$t('proxy.title')}</h1>
    <p class="text-muted-foreground mt-2">{$t('proxy.subtitle')}</p>
  </div>
  
  <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
    <Card class="p-4">
      <div class="flex items-center gap-3">
        <div class="relative p-2 rounded-lg transition-all duration-300 {proxyEnabled ? 'bg-green-500/10 shadow-green-500/20 shadow-lg' : 'bg-red-500/10 shadow-red-500/20 shadow-lg'}">
          {#if proxyEnabled}
            <ShieldCheck class="h-5 w-5 text-green-500 transition-all duration-300" />
            <!-- Active glow effect -->
            <div class="absolute inset-0 rounded-lg bg-green-500/20 animate-pulse"></div>
          {:else}
            <ShieldX class="h-5 w-5 text-red-500 transition-all duration-300" />
            <!-- Inactive pulse effect -->
            <div class="absolute inset-0 rounded-lg bg-red-500/10"></div>
          {/if}
        </div>
        <div>
          <p class="text-sm text-muted-foreground">{$t('proxy.status')}</p>
          <p class="text-xl font-bold transition-colors duration-300 {proxyEnabled ? 'text-green-600' : 'text-red-600'}">{proxyEnabled ? $t('proxy.active') : $t('proxy.inactive')}</p>
          {#if proxyEnabled}
            <div class="flex items-center gap-1 mt-1">
              <div class="w-2 h-2 bg-green-500 rounded-full animate-pulse"></div>
              <span class="text-xs text-green-600 font-medium">{$t('proxy.protected')}</span>
            </div>
          {:else}
            <div class="flex items-center gap-1 mt-1">
              <div class="w-2 h-2 bg-red-500 rounded-full"></div>
              <span class="text-xs text-red-600 font-medium">{$t('proxy.vulnerable')}</span>
            </div>
          {/if}
        </div>
      </div>
    </Card>
    
    <Card class="p-4">
      <div class="flex items-center gap-3">
        <div class="p-2 bg-green-500/10 rounded-lg">
          <Globe class="h-5 w-5 text-green-500" />
        </div>
        <div>
          <p class="text-sm text-muted-foreground">{$t('proxy.activeNodes')}</p>
          <p class="text-xl font-bold">{activeNodes} / {$proxyNodes.length}</p>
        </div>
      </div>
    </Card>
    
    <Card class="p-4">
      <div class="flex items-center gap-3">
        <div class="p-2 bg-blue-500/10 rounded-lg">
          <Activity class="h-5 w-5 text-blue-500" />
        </div>
        <div>
          <p class="text-sm text-muted-foreground">{$t('proxy.totalBandwidth')}</p>
          <p class="text-xl font-bold">{totalBandwidth} Mbps</p>
        </div>
      </div>
    </Card>
  </div>
  
  <Card class="p-6">
    <div class="flex items-center justify-between mb-4">
      <div class="flex items-center gap-3">
        <h2 class="text-lg font-semibold">{$t('proxy.settings')}</h2>
        {#if proxyEnabled}
          <div class="flex items-center gap-1 px-2 py-1 bg-green-100 rounded-full">
            <ShieldCheck class="h-3 w-3 text-green-600" />
            <span class="text-xs text-green-600 font-medium">{$t('proxy.secured')}</span>
          </div>
        {:else}
          <div class="flex items-center gap-1 px-2 py-1 bg-red-100 rounded-full">
            <ShieldX class="h-3 w-3 text-red-600" />
            <span class="text-xs text-red-600 font-medium">{$t('proxy.disabled')}</span>
          </div>
        {/if}
      </div>
      <div class="flex items-center gap-6">
        <div class="flex items-center gap-3">
          <span class="text-sm font-medium transition-colors duration-300 {autoReconnectEnabled ? 'text-blue-600' : 'text-gray-500'}">{$t('proxy.autoReconnect')}</span>
          <button
            type="button"
            role="switch"
            aria-checked={autoReconnectEnabled}
            aria-label="Toggle auto-reconnect {autoReconnectEnabled ? 'off' : 'on'}"
            on:click={() => (autoReconnectEnabled = !autoReconnectEnabled)}
            class="group relative inline-flex h-7 w-12 items-center rounded-full transition-all duration-300 focus:outline-none focus:ring-2 focus:ring-offset-2
               {autoReconnectEnabled ? 'bg-blue-500 focus:ring-blue-500 shadow-lg shadow-blue-500/30' : 'bg-gray-300 focus:ring-gray-400'}"
            >
            <span
              class="inline-block h-5 w-5 transform rounded-full bg-white transition-all duration-300 shadow-lg
                 {autoReconnectEnabled ? 'translate-x-6' : 'translate-x-1'}"
            >
              <!-- Mini icon inside toggle -->
              <div class="flex items-center justify-center w-full h-full">
                {#if autoReconnectEnabled}
                  <Activity class="h-2.5 w-2.5 text-blue-500" />
                {:else}
                  <Activity class="h-2.5 w-2.5 text-gray-400" />
                {/if}
              </div>
            </span>
          </button>
        </div>
        <div class="flex items-center gap-3">
          <span class="text-sm font-medium transition-colors duration-300 {proxyEnabled ? 'text-green-600' : 'text-gray-500'}">{$t('proxy.proxy')}</span>
          <button
          type="button"
          role="switch"
          aria-checked={proxyEnabled}
          aria-label="Toggle proxy {proxyEnabled ? 'off' : 'on'}"
          on:click={() => (proxyEnabled = !proxyEnabled)}
          class="group relative inline-flex h-7 w-12 items-center rounded-full transition-all duration-300 focus:outline-none focus:ring-2 focus:ring-offset-2
             {proxyEnabled ? 'bg-green-500 focus:ring-green-500 shadow-lg shadow-green-500/30' : 'bg-gray-300 focus:ring-gray-400'}"
          >
          <span
            class="inline-block h-5 w-5 transform rounded-full bg-white transition-all duration-300 shadow-lg
               {proxyEnabled ? 'translate-x-6' : 'translate-x-1'}"
          >
            <!-- Mini icon inside toggle -->
            <div class="flex items-center justify-center w-full h-full">
              {#if proxyEnabled}
                <ShieldCheck class="h-2.5 w-2.5 text-green-500" />
              {:else}
                <ShieldX class="h-2.5 w-2.5 text-gray-400" />
              {/if}
            </div>
          </span>
        </button>
        </div>
      </div>
    </div>

    <!-- Privacy Settings Section -->
    <div class="mt-6 pt-6 border-t border-gray-200 dark:border-gray-700">
      <div class="flex items-center justify-between">
        <div>
          <h3 class="text-md font-semibold mb-4 flex items-center gap-2">
            <ShieldCheck class="h-5 w-5" />
            {$t('proxy.privacy.title')}
          </h3>
        </div>
      </div>

      <div class="grid grid-cols-1 md:grid-cols-2 gap-6">
        <!-- Anonymous Mode Toggle -->
        <div class="flex items-center justify-between p-4 border border-gray-200 dark:border-gray-700 rounded-lg">
          <div>
            <Label class="text-sm font-medium">{$t('proxy.privacy.anonymousMode')}</Label>
            <p class="text-xs text-gray-500 mt-1">{$t('proxy.privacy.anonymousModeDesc')}</p>
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={$privacyStore.anonymousMode}
            aria-label="Toggle anonymous mode"
            on:click={() => {
              const newProfile = {
                anonymous: !$privacyStore.anonymousMode,
                multiHop: $privacyStore.multiHopEnabled
              };
              proxyRoutingService.setPrivacyProfile(newProfile);
              persistPrivacyProfile(newProfile);
            }}
            class="group relative inline-flex h-6 w-11 items-center rounded-full transition-all duration-300
               {$privacyStore.anonymousMode ? 'bg-blue-500' : 'bg-gray-300'}"
          >
            <span
              class="inline-block h-4 w-4 transform rounded-full bg-white transition-all duration-300
                 {$privacyStore.anonymousMode ? 'translate-x-6' : 'translate-x-1'}"
            ></span>
          </button>
        </div>

        <!-- Multi-Hop Toggle -->
        <div class="flex items-center justify-between p-4 border border-gray-200 dark:border-gray-700 rounded-lg">
          <div>
            <Label class="text-sm font-medium">{$t('proxy.privacy.multiHop')}</Label>
            <p class="text-xs text-gray-500 mt-1">{$t('proxy.privacy.multiHopDesc')}</p>
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={$privacyStore.multiHopEnabled}
            aria-label="Toggle multi-hop routing"
            on:click={() => {
              const newProfile = {
                anonymous: $privacyStore.anonymousMode,
                multiHop: !$privacyStore.multiHopEnabled
              };
              proxyRoutingService.setPrivacyProfile(newProfile);
              persistPrivacyProfile(newProfile);
            }}
            class="group relative inline-flex h-6 w-11 items-center rounded-full transition-all duration-300
               {$privacyStore.multiHopEnabled ? 'bg-purple-500' : 'bg-gray-300'}"
          >
            <span
              class="inline-block h-4 w-4 transform rounded-full bg-white transition-all duration-300
                 {$privacyStore.multiHopEnabled ? 'translate-x-6' : 'translate-x-1'}"
            ></span>
          </button>
        </div>
      </div>

      <!-- Privacy Profile Display -->
      <div class="mt-4 p-4 bg-blue-50 dark:bg-blue-900/20 border border-blue-200 dark:border-blue-800 rounded-lg">
        <p class="text-sm text-blue-900 dark:text-blue-100">
          <strong>{$t('proxy.privacy.currentMode')}</strong>
          {#if $privacyStore.anonymousMode && $privacyStore.multiHopEnabled}
            {$t('proxy.privacy.maxPrivacy')}
          {:else if $privacyStore.anonymousMode}
            {$t('proxy.privacy.anonymousOnly')}
          {:else if $privacyStore.multiHopEnabled}
            {$t('proxy.privacy.multiHopOnly')}
          {:else}
            {$t('proxy.privacy.standardMode')}
          {/if}
        </p>
      </div>
    </div>

    <!-- Proxy Stats -->
    <div class="mt-6 pt-6 border-t border-gray-200 dark:border-gray-700">
      <h3 class="text-md font-semibold mb-4 flex items-center gap-2">
        <Activity class="h-5 w-5" />
        {$t('proxy.stats.title')}
      </h3>
      <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
        <div class="p-4 border border-gray-200 dark:border-gray-700 rounded-lg">
          <p class="text-xs text-gray-500">{$t('proxy.stats.availableProxies')}</p>
          <p class="text-2xl font-bold">{proxyRoutingService.getStatistics().availableProxies}</p>
        </div>
        <div class="p-4 border border-gray-200 dark:border-gray-700 rounded-lg">
          <p class="text-xs text-gray-500">{$t('proxy.stats.totalRoutes')}</p>
          <p class="text-2xl font-bold">{proxyRoutingService.getStatistics().totalRoutes}</p>
        </div>
        <div class="p-4 border border-gray-200 dark:border-gray-700 rounded-lg">
          <p class="text-xs text-gray-500">{$t('proxy.stats.privacyProfile')}</p>
          <p class="text-sm font-medium">{$privacyStore.anonymousMode ? $t('proxy.stats.anon') : $t('proxy.stats.standard')} {$privacyStore.multiHopEnabled ? $t('proxy.stats.plusMulti') : ''}</p>
        </div>
      </div>
    </div>

    <!-- Add Node Input Section -->
    <div class="space-y-4">
      <div>
          <Label for="new-node">{$t('proxy.addNode')}</Label>
          <div class="flex gap-2 mt-2">
              <Input
                  id="new-node"
                  bind:value={newNodeAddress}
                  placeholder="example.com:8080 or enode://..."
                  class="flex-1 {isAddressValid || newNodeAddress === '' ? '' : 'border border-red-500 focus:ring-red-500'}"
                  on:keydown={(e) => {
                   const ev = (e as unknown as KeyboardEvent);
                   if (ev.key === 'Enter' && isAddressValid && newNodeAddress) addNode()
                  }}
              />
              <Button on:click={addNode} disabled={!isAddressValid || !newNodeAddress}>
                  <Plus class="h-4 w-4 mr-2" />
                  {$t('proxy.addNodeButton')}
              </Button>
          </div>
          {#if addressError}
              <p class="text-sm text-red-500 mt-1">{addressError}</p>
          {/if}
      </div>
    </div>
  </Card>
  
  <Card class="p-6">
  <div class="flex items-center justify-between mb-4 gap-3">
    <h2 class="text-lg font-semibold">{$t('proxy.proxyNodes')}</h2>
    <div class="flex items-center gap-2">
      <div class="w-40">
        <DropDown
          bind:value={statusFilter}
          options={statusOptions}
        />
      </div>
      <div class="w-44">
        <DropDown
          bind:value={sortBy}
          options={sortOptions}
        />
      </div>
    </div>
  </div>
    <div class="space-y-3">
      {#each sortedNodes as node}
        <div class="p-4 bg-secondary rounded-lg border border-border/50 hover:border-border transition-colors">
           <div class="flex justify-between items-start mb-3">
                      <div class="min-w-0 flex-1">
                        <div class="mb-1">
                          <p class="text-xs text-muted-foreground">
                            {node.address ? $t('proxy.nodeInfo.address') : $t('proxy.nodeInfo.dhtPeerId')}
                          </p>
                        </div>
                        <p class="font-mono text-sm font-medium text-foreground break-all" title={node.address || node.id}>
                          {node.address || node.id}
                        </p>
                      </div>
                      <div class="flex-shrink-0 ml-3">
                        <Badge variant={node.status === 'online' ? 'default' :
                       node.status === 'offline' ? 'secondary' :
                       node.status === 'connecting' ? 'outline' :
                       node.status === 'error' ? 'outline' : 'outline'}
                          class="transition-all duration-300 {
                            node.status === 'online' ? 'bg-green-500 text-white animate-pulse' :
                            node.status === 'offline' ? 'bg-red-500 text-white' :
                            node.status === 'connecting' ? 'bg-yellow-500 text-white animate-pulse' :
                            node.status === 'error' ? 'bg-orange-500 text-white' :
                            'bg-gray-500 text-white'
                          }"
                          style="pointer-events: none;"
                        >
                          {node.status || 'offline'}
                        </Badge>
                      </div>
          </div>
          
          <div class="grid grid-cols-4 gap-3 mb-3">
            <div class="text-center p-2 rounded bg-background border border-border/30">
              <p class="text-xs text-muted-foreground">{$t('proxy.bandwidth')}</p>
              <p class="text-sm font-bold text-blue-600">{node.latency ? Math.round(100 - node.latency) : 'N/A'}</p>
              <p class="text-xs text-muted-foreground">Mbps</p>
            </div>
            <div class="text-center p-2 rounded bg-background border border-border/30">
              <p class="text-xs text-muted-foreground">{$t('proxy.latency')}</p>
              <p class="text-sm font-bold text-purple-600">{node.latency || 'N/A'}</p>
              <p class="text-xs text-muted-foreground">ms (tcp connect)</p>
            </div>
            {#if node.address}
              {@const stats = getPerformanceStats(node.address)}
              <div class="text-center p-2 rounded bg-background border border-border/30">
                <p class="text-xs text-muted-foreground">{$t('proxy.nodeInfo.reliability')}</p>
                <p class="text-sm font-bold {
                  stats.uptimePercentage >= 80 ? 'text-green-600' :
                  stats.uptimePercentage >= 60 ? 'text-yellow-600' :
                  'text-red-600'
                }">{stats.uptimePercentage}%</p>
                <div class="w-8 h-1 bg-muted rounded-full overflow-hidden mx-auto mt-1">
                  <div
                    class="h-full transition-all duration-300 {
                      stats.uptimePercentage >= 80 ? 'bg-green-500' :
                      stats.uptimePercentage >= 60 ? 'bg-yellow-500' :
                      'bg-red-500'
                    }"
                    style="width: {stats.uptimePercentage}%"
                  ></div>
                </div>
              </div>
              <div class="text-center p-2 rounded bg-background border border-border/30">
                <p class="text-xs text-muted-foreground">{$t('proxy.nodeInfo.success')}</p>
                <p class="text-sm font-bold text-gray-600">{stats.successfulConnections}/{stats.totalAttempts}</p>
                <p class="text-xs text-muted-foreground">{$t('proxy.nodeInfo.attempts')}</p>
              </div>
            {:else}
              <div class="col-span-2"></div>
            {/if}
          </div>

          <div class="flex justify-end gap-2">
            <Button
              size="sm"
              variant="outline"
              on:click={() => toggleNode(node)}
              disabled={node.status === 'connecting'}
            >
              <Power class="h-3 w-3 mr-1" />
              {node.status === 'online' ? $t('proxy.disconnect') :
               node.status === 'connecting' ? $t('proxy.connecting') :
               node.status === 'error' ? $t('proxy.retry') :
               $t('proxy.connect')}
            </Button>
            <Button
              size="sm"
              variant="destructive"
              disabled={!node.address}
              on:click={() => requestRemoveNode(node)}
            >
              <Trash2 class="h-3 w-3 mr-1" />
              {$t('proxy.remove')}
            </Button>
          </div>
        </div>
      {/each}
    </div>
  </Card>
  
  <!-- Proxy Latency Optimization Card -->
  <Card class="bg-gradient-to-r from-purple-50 to-blue-50 border-purple-200">
    <div class="space-y-3">
      <div class="flex items-center gap-2">
        <Activity class="h-5 w-5 text-purple-600" />
        <h3 class="text-lg font-semibold text-purple-800">{$t('proxy.optimization.title')}</h3>
      </div>

      <div class="bg-white/60 rounded-lg p-4 space-y-3">
        <div>
          <h4 class="text-sm font-medium text-gray-700 mb-2">{$t('proxy.optimization.currentStatus')}</h4>
          <p class="text-sm font-medium">{optimizationStatus}</p>
          <p class="text-xs text-gray-600 mt-1">
            self-test metric: tcp connect ms | routing metric: ping rtt ms
          </p>
          <div class="flex gap-2 mt-1">
            <button
              class="text-xs text-purple-600 hover:text-purple-800"
              on:click={updateOptimizationStatus}
            >
              {$t('proxy.optimization.refreshStatus')}
            </button>
            <button
              class="text-xs text-green-600 hover:text-green-800 px-2 py-1 bg-green-50 rounded disabled:opacity-50"
              on:click={testProxyLatencyOptimization}
              disabled={isTestingOptimization}
            >
              {isTestingOptimization ? $t('proxy.optimization.runningTests') : $t('proxy.optimization.proveOptimization')}
            </button>
          </div>
        </div>

        {#if testResults}
          <div class="mt-3">
            <h4 class="text-sm font-medium text-gray-700 mb-2">{$t('proxy.optimization.testResults')}</h4>
            <div class="bg-gray-100 rounded p-3 max-h-40 overflow-y-auto">
              <pre class="text-xs text-gray-800 whitespace-pre-wrap">{testResults}</pre>
            </div>
          </div>
        {/if}
      </div>
    </div>
  </Card>
</div>
