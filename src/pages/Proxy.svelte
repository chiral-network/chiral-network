<script lang="ts">
  import Button from '$lib/components/ui/button.svelte'
  import Card from '$lib/components/ui/card.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import { ShieldCheck, ShieldX, Globe, Activity, Plus, Power, Trash2 } from 'lucide-svelte'
  import { onMount } from 'svelte';
  import { proxyNodes, connectProxy, disconnectProxy, listProxies } from '$lib/proxy';
  import { t } from 'svelte-i18n'
  import DropDown from '$lib/components/ui/dropDown.svelte'
  
  let newNodeAddress = ''
  let proxyEnabled = true
  let isAddressValid = true
  let showConfirmDialog = false
  let nodeToRemove: any = null
  const validAddressRegex = /^[a-zA-Z0-9.-]+:[0-9]{1,5}$/
  let statusFilter = 'all'
  
  $: statusOptions = [
    { value: 'all', label: $t('All') },
    { value: 'online', label: $t('Online') },
    { value: 'offline', label: $t('Offline') },
    { value: 'connecting', label: $t('Connecting') }
  ]

  $: filteredNodes = $proxyNodes.filter(node => {
      if (statusFilter === 'all') {
          return true
      }
      return node.status === statusFilter
  })

  
  $: sortedNodes = [...filteredNodes].sort((a, b) => {
      const statusOrder = { 'online': 1, 'connecting': 2, 'offline': 3 };
      return statusOrder[a.status] - statusOrder[b.status];
  });

  
  onMount(() => {
      listProxies();
  });

  function addNode() {
      const isDuplicate = $proxyNodes.some(node => node.address === newNodeAddress.trim())
      if (isDuplicate) {
          alert($t('proxy.alreadyAdded'))
          return
      }

      if (!newNodeAddress || !validAddressRegex.test(newNodeAddress.trim())) {
          alert($t('proxy.invalidAddress'))
          return
      }

      // For now, we'll use a dummy token.
      connectProxy(newNodeAddress.trim(), "dummy-token");
      newNodeAddress = ''
  }

  function requestRemoveNode(node: any) {
    nodeToRemove = node;
    showConfirmDialog = true
  }

  function confirmRemoveNode() {
    if (nodeToRemove) {
      disconnectProxy(nodeToRemove.address)
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
          disconnectProxy(node.address);
      } else {
          // For now, we'll use a dummy token.
          connectProxy(node.address, "dummy-token");
      }
  }

  
  $: activeNodes = $proxyNodes.filter(n => n.status === 'online').length
  $: totalBandwidth = $proxyNodes.reduce((sum, n) => sum + (n.status === 'online' ? n.bandwidth : 0), 0)
  $: isAddressValid = validAddressRegex.test(newNodeAddress.trim())
</script>

<!-- Confirmation Dialog -->
{#if showConfirmDialog && nodeToRemove}
  <div class="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
    <div class="bg-white rounded-lg p-6 max-w-md w-full mx-4">
      <h3 class="text-lg font-semibold mb-4">Confirm Removal</h3>
      <p class="text-muted-foreground mb-6">
        Confirm the removal of proxy node <span class="font-medium">{nodeToRemove.address}</span>
      </p>
      <div class="flex gap-3 justify-end">
        <Button variant="outline" on:click={cancelRemoveNode}>
          Cancel
        </Button>
        <Button variant="destructive" on:click={confirmRemoveNode}>
          <Trash2 class="h-4 w-4 mr-2" />
          Remove
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
              <span class="text-xs text-green-600 font-medium">Protected</span>
            </div>
          {:else}
            <div class="flex items-center gap-1 mt-1">
              <div class="w-2 h-2 bg-red-500 rounded-full"></div>
              <span class="text-xs text-red-600 font-medium">Vulnerable</span>
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
            <span class="text-xs text-green-600 font-medium">Secured</span>
          </div>
        {:else}
          <div class="flex items-center gap-1 px-2 py-1 bg-red-100 rounded-full">
            <ShieldX class="h-3 w-3 text-red-600" />
            <span class="text-xs text-red-600 font-medium">Disabled</span>
          </div>
        {/if}
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

    
    <div class="space-y-4">
        <div>
            <Label for="new-node">{$t('proxy.addNode')}</Label>
            <div class="flex gap-2 mt-2">
                <Input
                    id="new-node"
                    bind:value={newNodeAddress}
                    placeholder={$t('proxy.enterAddress')}
                    class="flex-1 {isAddressValid || newNodeAddress === '' ? '' : 'border border-red-500 focus:ring-red-500'}"
                />
                <Button on:click={addNode} disabled={!isAddressValid || !newNodeAddress}>
                    <Plus class="h-4 w-4 mr-2" />
                    {$t('proxy.addNodeButton')}
                </Button>
            </div>
            {#if !isAddressValid && newNodeAddress !== ''}
                <p class="text-sm text-red-500 mt-1">{$t('proxy.invalidAddress')}</p>
            {/if}
        </div>

    </div>
  </Card>
  
  <Card class="p-6">
    <div class="flex items-center justify-between mb-4">
        <h2 class="text-lg font-semibold">{$t('proxy.proxyNodes')}</h2>
        <div class="w-40 flex-shrink-0">
            <DropDown
                bind:value={statusFilter}
                options={statusOptions}
            />
        </div>
    </div>
    <div class="space-y-3">
      {#each sortedNodes as node}
        <div class="p-4 bg-secondary rounded-lg">
          <div class="flex items-center justify-between mb-3">
            <div class="flex items-center gap-3">
              <div class="w-2 h-2 rounded-full {
                node.status === 'online' ? 'bg-green-500' : 
                node.status === 'offline' ? 'bg-red-500' : 
                node.status === 'connecting' ? 'bg-yellow-500' :
                'bg-gray-500'
              }"></div>
              <div>
                <p class="font-medium">{node.address}</p>
                <p class="text-xs text-muted-foreground">{node.region}</p>
              </div>
            </div>
              <Badge variant={node.status === 'online' ? 'default' :
                   node.status === 'offline' ? 'secondary' :
                   node.status === 'connecting' ? 'outline' : 'outline'}
                      class={
                        node.status === 'online' ? 'bg-green-500 text-white' :
                        node.status === 'offline' ? 'bg-red-500 text-white' :
                        node.status === 'connecting' ? 'bg-yellow-500 text-white' :
                        'bg-gray-500 text-white'
                      }
                      style="pointer-events: none;"
              >
              {node.status}
            </Badge>
          </div>
          
          <div class="grid grid-cols-2 gap-4 mb-3">
            <div>
              <p class="text-xs text-muted-foreground">{$t('proxy.bandwidth')}</p>
              <p class="text-sm font-medium">{node.bandwidth} Mbps</p>
            </div>
            <div>
              <p class="text-xs text-muted-foreground">{$t('proxy.latency')}</p>
              <p class="text-sm font-medium">{node.latency} ms</p>
            </div>
          </div>
          
          <div class="flex gap-2">
            <Button
              size="sm"
              variant="outline"
              on:click={() => toggleNode(node.id)}
            >
              <Power class="h-3 w-3 mr-1" />
              {node.status === 'online' ? $t('proxy.disconnect') : $t('proxy.connect')}
            </Button>
            <Button
              size="sm"
              variant="destructive"
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
</div>