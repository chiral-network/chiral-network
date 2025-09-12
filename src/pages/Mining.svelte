<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Button from '$lib/components/ui/button.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import Progress from '$lib/components/ui/progress.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import DropDown from "$lib/components/ui/dropDown.svelte";
  import { Cpu, Zap, TrendingUp, Award, Play, Pause, Coins, Thermometer, AlertCircle, Terminal, X, RefreshCw } from 'lucide-svelte'
  import { onDestroy, onMount } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import { etcAccount, miningState } from '$lib/stores'
    import { getVersion } from "@tauri-apps/api/app";
  // Interfaces
  interface MiningHistoryPoint {
    timestamp: number
    hashRate: number
    power: number
  }
  
  interface RecentBlock {
    id: string
    hash: string
    reward: number
    timestamp: Date
    difficulty: number
    nonce: number
  }
  
  // Local UI state only
  let isTauri = false
  let isGethRunning = false
  let currentBlock = 0
  let totalHashes = 0
  let currentDifficulty = 4
  let cpuThreads = navigator.hardwareConcurrency || 4
  let selectedThreads = Math.floor(cpuThreads / 2)
  let error = ''
  
  // Network statistics
  let networkHashRate = '0 H/s'
  let networkDifficulty = '0'
  let blockReward = 5 // Chiral per block
  let peerCount = 0

  // Statistics
  let sessionStartTime = Date.now()
  let estimatedTimeToBlock = 0
  $: powerConsumption = $miningState.realPowerUsage ?? ($miningState.activeThreads * 15)
  $: efficiency = $miningState.hashRate === '0 H/s' || powerConsumption <= 0 ? 0 : parseHashRate($miningState.hashRate) / powerConsumption
  let temperature = 45.0
  $: if (isTauri) {
    temperature = $miningState.realTemperature ?? 0
  } else {
    temperature = 45 + ($miningState.activeThreads * 3.5)
  }

  // Uptime tick (forces template to re-render every second while mining)
  let uptimeNow: number = Date.now()
  let uptimeInterval: number | null = null
  
  // Mining history
  let miningHistory: MiningHistoryPoint[] = []
  let recentBlocks: RecentBlock[] = []
  
  // Mock mining intervals  
  let statsInterval: number | null = null
  
  // Logs
  let showLogs = false
  let logs: string[] = []
  let logsInterval: number | null = null



 // Bar or Line chart toggle
  let chartType: 'bar' | 'line' = 'bar';

  // Threads and intensity warnings
  let threadsWarning = '';
  let intensityWarning = '';

  let validationError: string | null = null;
  
  // Computed values for actual threads based on intensity
  const maxThreads = cpuThreads
  $: actualThreads = Math.ceil(($miningState.minerIntensity / 100) * maxThreads)
  // Don't directly modify store in reactive statement to avoid infinite loops


  // Threads warning
  $: {
    const numThreads = Number(selectedThreads);
    threadsWarning = (numThreads < 1 || numThreads > cpuThreads)
            ? `Threads must be between 1 and ${cpuThreads}`
            : '';
  }

  // Intensity warning
  $: {
    const numIntensity = Number($miningState.minerIntensity);
    intensityWarning = (numIntensity < 1 || numIntensity > 100)
            ? `Intensity must be between 1 and 100`
            : '';
  }

  // Button disabled if either warning exists
  $: isInvalid = !!threadsWarning || !!intensityWarning;

  onMount(async () => {
    try{
      getVersion()
      isTauri = true
    }
    catch{
      isTauri = false
    }

    await checkGethStatus()
    await updateNetworkStats()
    
    // If mining is already active from before, update stats immediately
    if ($miningState.isMining) {
      await updateMiningStats()
    }
    if (isTauri) {
      await updateCpuTemperature()
    }
    await updateSystemStats() // Initial call
    
    // Start polling for mining stats
    statsInterval = setInterval(async () => {
      if ($miningState.isMining) {
        await updateMiningStats()
      }
      await updateNetworkStats()
      await updateSystemStats()
      if (isTauri) {
        await updateCpuTemperature()
      }
    }, 2000) as unknown as number
  })
  
  async function checkGethStatus() {
    try {
      isGethRunning = await invoke('is_geth_running') as boolean
      if (isGethRunning) {
        // Only check backend status if we don't have a mining state already
        // This prevents losing state when switching pages
        if (!$miningState.isMining) {
          const status = await invoke('get_miner_status') as boolean
          if (status) {
            $miningState.isMining = status
            sessionStartTime = Date.now()
            startUptimeTimer()
          }
        } else {
          // If we already think we're mining, just restart the uptime timer
          startUptimeTimer()
        }
      }
    } catch (e) {
      console.error('Failed to check geth status:', e)
    }
  }
  
  async function updateMiningStats() {
    try {
      const [rate, block] = await Promise.all([
        invoke('get_miner_hashrate') as Promise<string>,
        invoke('get_current_block') as Promise<number>
      ])
      $miningState.hashRate = rate
      currentBlock = block
      
      // Convert hashRate string to number for chart
      let hashRateNum = 0
      if (rate.includes('GH/s')) {
        hashRateNum = parseFloat(rate) * 1000000000
      } else if (rate.includes('MH/s')) {
        hashRateNum = parseFloat(rate) * 1000000
      } else if (rate.includes('KH/s')) {
        hashRateNum = parseFloat(rate) * 1000
      } else {
        hashRateNum = parseFloat(rate) || 0
      }
      
      // Update mining history for chart
      if ($miningState.isMining) {
        miningHistory = [...miningHistory.slice(-29), {
          timestamp: Date.now(),
          hashRate: hashRateNum,
          power: powerConsumption
        }]
        
        // Simulate finding blocks occasionally (very low probability)
        if (Math.random() < 0.001) {
          findBlock()
        }
      }
    } catch (e) {
      console.error('Failed to update mining stats:', e)
    }
  }
  
  async function updateNetworkStats() {
    try {
      if (isGethRunning) {
        const [stats, block, peers] = await Promise.all([
          invoke('get_network_stats') as Promise<[string, string]>,
          invoke('get_current_block') as Promise<number>,
          invoke('get_network_peer_count') as Promise<number>
        ])
        
        networkDifficulty = stats[0]
        networkHashRate = stats[1]
        currentBlock = block
        peerCount = peers
      }
    } catch (e) {
      console.error('Failed to update network stats:', e)
    }
  }

  async function updateSystemStats() {
    try {
      const [powerUsage, cpuUsage] = await Promise.all([
        invoke('get_cpu_package_power') as Promise<number | null>,
        invoke('get_cpu_usage') as Promise<number>
      ])
      
      $miningState.realPowerUsage = powerUsage ?? undefined
      $miningState.cpuUsage = cpuUsage
    } catch (e) {
      console.error('Failed to update system stats:', e)
      $miningState.realPowerUsage = undefined
    }
  }

  async function updateCpuTemperature() {
    try {
      const temp = await invoke('get_cpu_temperature') as number | null
      $miningState.realTemperature = temp ?? undefined
    } catch (e) {
      console.error('Failed to get CPU temperature:', e)
      $miningState.realTemperature = undefined
    }
  }
  
  function startUptimeTimer() {
    uptimeNow = Date.now()
    if (!uptimeInterval) {
      uptimeInterval = setInterval(() => {
        uptimeNow = Date.now()
      }, 1000) as unknown as number
    }
  }
  
  async function startMining() {
    if (!$etcAccount) {
      error = 'Please create a Chiral Network account first in the Account page'
      return
    }
    
    if (!isGethRunning) {
      error = 'Chiral node is not running. Please start it from the Network page'
      return
    }
    
    error = ''
    validationError = null
    
    try {
      // Show message that we're starting mining
      error = 'Starting mining... (may restart node if needed)'
      
      await invoke('start_miner', {
        address: $etcAccount.address,
        threads: selectedThreads,
        dataDir: './geth-data'
      })
      
      error = '' // Clear the status message
      $miningState.isMining = true
      sessionStartTime = Date.now()
      $miningState.activeThreads = actualThreads  // Use computed actualThreads
      startUptimeTimer()
      
      // Start updating stats
      await updateMiningStats()
      await updateSystemStats() // Get real system data
      
      // Re-check geth status since it might have restarted
      isGethRunning = true
    } catch (e) {
      error = String(e)
      console.error('Failed to start mining:', e)
    }
  }
  
  async function stopMining() {
    try {
      await invoke('stop_miner')
      $miningState.isMining = false
      $miningState.hashRate = '0 H/s'
      $miningState.activeThreads = 0
      
      // stop uptime ticker
      if (uptimeInterval) {
        clearInterval(uptimeInterval as unknown as number)
        uptimeInterval = null
      }
    } catch (e) {
      error = String(e)
      console.error('Failed to stop mining:', e)
    }
  }
  
  function findBlock() {
    $miningState.blocksFound++
    const reward = 5 + Math.random() * 2
    $miningState.totalRewards += reward
    
    recentBlocks = [{
      id: `block-${Date.now()}`,
      hash: `0x${Math.random().toString(16).substring(2, 10)}...${Math.random().toString(16).substring(2, 6)}`,
      reward: reward,
      timestamp: new Date(),
      difficulty: currentDifficulty,
      nonce: Math.floor(Math.random() * 1000000)
    }, ...recentBlocks.slice(0, 4)]
  }
  
  function formatUptime(now: number = Date.now()) {
    const uptime = now - sessionStartTime
    const hours = Math.floor(uptime / 3600000)
    const minutes = Math.floor((uptime % 3600000) / 60000)
    const seconds = Math.floor((uptime % 60000) / 1000)
    return `${hours}h ${minutes}m ${seconds}s`
  }
  
  function parseHashRate(rateStr: string): number {
    const match = rateStr.match(/^([\d.]+)\s*([KMGT]?)H\/s$/i)
    if (!match) return 0
    
    const value = parseFloat(match[1])
    const unit = match[2].toUpperCase()
    
    switch (unit) {
      case 'K': return value * 1000
      case 'M': return value * 1000000
      case 'G': return value * 1000000000
      case 'T': return value * 1000000000000
      default: return value
    }
  }

  function formatHashRate(rate: number | string): string {
    if (typeof rate === 'string') return rate
    if (rate < 1000) return `${rate.toFixed(1)} H/s`
    if (rate < 1000000) return `${(rate / 1000).toFixed(2)} KH/s`
    if (rate < 1000000000) return `${(rate / 1000000).toFixed(2)} MH/s`
    return `${(rate / 1000000000).toFixed(2)} GH/s`
  }
  
  function formatNumber(num: number): string {
    if (num < 1000) return num.toString()
    if (num < 1000000) return `${(num / 1000).toFixed(1)}K`
    if (num < 1000000000) return `${(num / 1000000).toFixed(1)}M`
    return `${(num / 1000000000).toFixed(1)}B`
  }
  
  async function fetchLogs() {
    try {
      const result = await invoke('get_miner_logs', {
        dataDir: './geth-data',
        lines: 100
      }) as string[]
      logs = result
    } catch (e) {
      console.error('Failed to fetch logs:', e)
    }
  }
  
  function toggleLogs() {
    showLogs = !showLogs
    if (showLogs) {
      fetchLogs()
      // Start auto-refresh of logs
      logsInterval = setInterval(fetchLogs, 2000) as unknown as number
    } else {
      // Stop auto-refresh
      if (logsInterval) {
        clearInterval(logsInterval)
        logsInterval = null
      }
    }
  }
  
  // Mock pool options
  const pools = [
    { value: 'solo', label: 'Solo Mining' },
    { value: 'pool1', label: 'ChiralPool #1' },
    { value: 'pool2', label: 'ChiralPool #2' },
    { value: 'pool3', label: 'Community Pool' }
  ]
  
  onDestroy(async () => {
    if ($miningState.isMining) {
      await stopMining()
    }
    if (statsInterval) {
      clearInterval(statsInterval)
    }
    if (uptimeInterval) {
      clearInterval(uptimeInterval)
    }
    if (logsInterval) {
      clearInterval(logsInterval)
    }
  })

</script>

<div class="space-y-6">
  <div>
    <h1 class="text-3xl font-bold">Mining</h1>
    <p class="text-muted-foreground mt-2">Contribute computing power to secure the network and earn rewards</p>
  </div>
  
  <!-- Mining Status Cards -->
  <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
    <Card class="p-4">
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-muted-foreground">Hash Rate</p>
          <p class="text-2xl font-bold">{$miningState.hashRate}</p>
          <p class="text-xs text-muted-foreground mt-1">
            {$miningState.isMining ? `${$miningState.activeThreads} threads` : 'Not mining'}
          </p>
        </div>
        <div class="p-2 bg-primary/10 rounded-lg">
          <Cpu class="h-5 w-5 text-primary" />
        </div>
      </div>
    </Card>
    
    <Card class="p-4">
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-muted-foreground">Total Rewards</p>
          <p class="text-2xl font-bold">{$miningState.totalRewards.toFixed(2)} Chiral</p>
          <p class="text-xs text-green-600 flex items-center gap-1 mt-1">
            <TrendingUp class="h-3 w-3" />
            {$miningState.blocksFound} blocks found
          </p>
        </div>
        <div class="p-2 bg-yellow-500/10 rounded-lg">
          <Coins class="h-5 w-5 text-yellow-500" />
        </div>
      </div>
    </Card>
    
    <Card class="p-4">
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-muted-foreground">Power Usage</p>
          <p class="text-2xl font-bold">
            {$miningState.realPowerUsage !== undefined ? $miningState.realPowerUsage.toFixed(0) + 'W' : 'N/A'}
          </p>
          <p class="text-xs text-muted-foreground mt-1">
            {$miningState.realPowerUsage !== undefined ? efficiency.toFixed(2) + ' H/W' : 'N/A'}
          </p>
        </div>
        <div class="p-2 bg-amber-500/10 rounded-lg">
          <Zap class="h-5 w-5 text-amber-500" />
        </div>
      </div>
    </Card>
    
    <Card class="p-4">
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-muted-foreground">Temperature</p>
          <p class="text-2xl font-bold">
            {$miningState.realTemperature !== undefined ? $miningState.realTemperature.toFixed(1) + '°C' : (isTauri ? 'N/A' : temperature.toFixed(1) + '°C')}
          </p>
          <div class="mt-1">
            <Progress 
              value={$miningState.realTemperature !== undefined ? $miningState.realTemperature : temperature} 
              max={100} 
              class="h-1 {($miningState.realTemperature !== undefined ? $miningState.realTemperature : temperature) > 80 ? 'bg-red-500' : ($miningState.realTemperature !== undefined ? $miningState.realTemperature : temperature) > 60 ? 'bg-yellow-500' : ''}"
            />
          </div>
        </div>
        <div class="p-2 bg-red-500/10 rounded-lg">
          <Thermometer class="h-5 w-5 text-red-500" />
        </div>
      </div>
    </Card>
  </div>
  
  <!-- Mining Control -->
  <Card class="p-6">
    <div class="flex items-center justify-between mb-4">
      <h2 class="text-lg font-semibold">Mining Control</h2>
      <Badge variant={$miningState.isMining ? 'default' : 'secondary'}>
        {$miningState.isMining ? 'Mining Active' : 'Mining Stopped'}
      </Badge>
    </div>
    
    <div class="space-y-4">
      <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
        <div class="relative">
          <Label for="pool-select">Mining Pool</Label>
          <DropDown
            id="pool-select"
            options={pools}
            bind:value={$miningState.selectedPool}
            disabled={$miningState.isMining}
          />

        </div>
        
        <div>
          <Label for="thread-count">CPU Threads ({cpuThreads} available)</Label>
          <Input
                  id="thread-count"
                  type="number"
                  bind:value={selectedThreads}
                  on:input={(e: Event) => {
                      const target = e.currentTarget as HTMLInputElement;
                      selectedThreads = Number(target.value);
                    }}
                  min="1"
                  max={cpuThreads}
                  disabled={$miningState.isMining}
                  class="mt-2"
          />
          {#if threadsWarning}
            <p class="text-xs text-red-500 mt-1">{threadsWarning}</p>

          {/if}
        </div>
        
        <div>
          <Label for="intensity">Mining Intensity (%)</Label>
          <Input
                  id="intensity"
                  type="number"
                  bind:value={$miningState.minerIntensity}
                  on:input={(e: Event) => {
                      const target = e.currentTarget as HTMLInputElement;
                      $miningState.minerIntensity = Number(target.value);
                    }}
                  min="1"
                  max="100"
                  step="1"
                  disabled={$miningState.isMining}
                  class="mt-2"
          />

          {#if intensityWarning}
            <p class="text-xs text-red-500 mt-1">{intensityWarning}</p>

          {/if}
        </div>
      </div>
      
      <div class="flex items-center justify-between pt-4">
        <div class="text-sm space-y-1">
          <p class="text-muted-foreground">
            Session: <span class="font-medium">{$miningState.isMining ? formatUptime(uptimeNow) : '0h 0m 0s'}</span>
          </p>
          <p class="text-muted-foreground">
            Total Hashes: <span class="font-medium">{formatNumber(totalHashes)}</span>
          </p>
        </div>
        
        <div class="flex gap-2">
          <Button
            size="lg"
            on:click={() => $miningState.isMining ? stopMining() : startMining()}
            class="min-w-[150px]"
            disabled={isInvalid || !isGethRunning}
          >
            {#if $miningState.isMining}
              <Pause class="h-4 w-4 mr-2" />
              Stop Mining
            {:else}
              <Play class="h-4 w-4 mr-2" />
              Start Mining
            {/if}
          </Button>
          <Button
            size="lg"
            variant="outline"
            on:click={toggleLogs}
            title="Show mining logs"
          >
            <Terminal class="h-4 w-4" />
          </Button>
        </div>
      </div>
      {#if validationError}
        <p class="text-red-600 text-sm mt-2 text-right">{validationError}</p>
      {/if}
      {#if error}
        <div class="bg-red-500/10 border border-red-500/20 rounded-lg p-3 mt-2">
          <div class="flex items-center gap-2">
            <AlertCircle class="h-4 w-4 text-red-500 flex-shrink-0" />
            <p class="text-sm text-red-500">{error}</p>
          </div>
        </div>
      {/if}
      {#if !isGethRunning}
        <div class="bg-yellow-500/10 border border-yellow-500/20 rounded-lg p-3 mt-2">
          <div class="flex items-center gap-2">
            <AlertCircle class="h-4 w-4 text-yellow-500 flex-shrink-0" />
            <p class="text-sm text-yellow-600">
              Chiral node is not running. Please start it from the <a href="/network" class="underline font-medium">Network page</a>
            </p>
          </div>
        </div>
      {/if}
      {#if !$etcAccount && isGethRunning}
        <div class="bg-blue-500/10 border border-blue-500/20 rounded-lg p-3 mt-2">
          <div class="flex items-center gap-2">
            <AlertCircle class="h-4 w-4 text-blue-500 flex-shrink-0" />
            <p class="text-sm text-blue-600">
              Please create a Chiral Network account from the <a href="/account" class="underline font-medium">Account page</a> to start mining
            </p>
          </div>
        </div>
      {/if}
    </div>
  </Card>
  
  <!-- Mining Statistics -->
  <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
    <Card class="p-6">
      <h2 class="text-lg font-semibold mb-4">Network Statistics</h2>
      <div class="space-y-3">
        <div class="flex justify-between items-center">
          <span class="text-sm text-muted-foreground">Network Hash Rate</span>
          <Badge variant="outline">{networkHashRate}</Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm text-muted-foreground">Network Difficulty</span>
          <Badge variant="outline">{networkDifficulty}</Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm text-muted-foreground">Block Height</span>
          <Badge variant="outline">#{currentBlock}</Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm text-muted-foreground">Block Reward</span>
          <Badge variant="outline">{blockReward} Chiral</Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm text-muted-foreground">Est. Time to Block</span>
          <Badge variant="outline">
            {estimatedTimeToBlock > 0 ? `~${Math.floor(estimatedTimeToBlock / 60)} min` : 'Calculating...'}
          </Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm text-muted-foreground">Active Miners</span>
          <Badge variant="outline">{peerCount}</Badge>
        </div>
      </div>
    </Card>
    
    <Card class="p-6">
      <h2 class="text-lg font-semibold mb-4">Pool Information</h2>
      {#if $miningState.selectedPool === 'solo'}
        <div class="space-y-3">
          <p class="text-sm text-muted-foreground">
            You are mining solo. All block rewards will be yours, but finding blocks may take longer.
          </p>
          <div class="pt-2 space-y-2">
            <div class="flex justify-between">
              <span class="text-sm">Your Share</span>
              <span class="text-sm font-medium">100%</span>
            </div>
            <div class="flex justify-between">
              <span class="text-sm">Pool Fee</span>
              <span class="text-sm font-medium">0%</span>
            </div>
            <div class="flex justify-between">
              <span class="text-sm">Min Payout</span>
              <span class="text-sm font-medium">N/A</span>
            </div>
          </div>
        </div>
      {:else}
        <div class="space-y-3">
          <div class="flex justify-between">
            <span class="text-sm">Pool Hash Rate</span>
            <span class="text-sm font-medium">850 MH/s</span>
          </div>
          <div class="flex justify-between">
            <span class="text-sm">Pool Miners</span>
            <span class="text-sm font-medium">342</span>
          </div>
          <div class="flex justify-between">
            <span class="text-sm">Your Share</span>
            <span class="text-sm font-medium">{(parseHashRate($miningState.hashRate) / 850000000 * 100).toFixed(4)}%</span>
          </div>
          <div class="flex justify-between">
            <span class="text-sm">Pool Fee</span>
            <span class="text-sm font-medium">1%</span>
          </div>
          <div class="flex justify-between">
            <span class="text-sm">Min Payout</span>
            <span class="text-sm font-medium">10 Chiral</span>
          </div>
          <div class="flex justify-between">
            <span class="text-sm">Payment Method</span>
            <span class="text-sm font-medium">PPLNS</span>
          </div>
        </div>
      {/if}
    </Card>
  </div>
  
  <!-- Recent Blocks -->
  <Card class="p-6">
    <h2 class="text-lg font-semibold mb-4">Recent Blocks Found</h2>
    {#if recentBlocks.length === 0}
      <p class="text-sm text-muted-foreground text-center py-8">
        No blocks found yet. Start mining to earn rewards!
      </p>
    {:else}
      <div class="space-y-2">
        {#each recentBlocks as block}
          <div class="flex items-center justify-between p-3 bg-secondary rounded-lg">
            <div class="flex items-center gap-3">
              <Award class="h-4 w-4 text-yellow-500" />
              <div>
                <p class="text-sm font-medium">Block Found!</p>
                <p class="text-xs text-muted-foreground">
                  Hash: {block.hash} • Nonce: {block.nonce}
                </p>
              </div>
            </div>
            <div class="text-right">
              <Badge variant="outline" class="text-green-600">
                +{block.reward.toFixed(2)} Chiral
              </Badge>
              <p class="text-xs text-muted-foreground mt-1">
                {block.timestamp.toLocaleTimeString()}
              </p>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </Card>
  
  <!-- Hash Rate Chart (simplified) -->
  {#if miningHistory.length > 0}
    <Card class="p-6">
      <h2 class="text-lg font-semibold mb-4">Hash Rate History</h2>

      <!-- Chart Type Toggle -->
      <div class="flex items-center gap-2 mb-2">
        <span class="text-sm text-muted-foreground">Chart Type:</span>
        <Button size="sm" variant={chartType === 'bar' ? 'default' : 'outline'} on:click={() => chartType = 'bar'}>Bar</Button>
        <Button size="sm" variant={chartType === 'line' ? 'default' : 'outline'} on:click={() => chartType = 'line'}>Line</Button>
      </div>

      <!-- Chart Rendering -->
      {#if chartType === 'bar'}
        <div class="h-32 flex items-end gap-1">
          {#each miningHistory as point}
            <div
                    class="flex-1 bg-primary/20 hover:bg-primary/30 transition-all rounded-t"
                    style="height: {(point.hashRate / Math.max(...miningHistory.map(h => h.hashRate))) * 100}%; transition: height 0.5s ease;"
                    title="{formatHashRate(point.hashRate)}"
            ></div>
          {/each}
        </div>
      {:else}
        <div class="relative w-full h-32">
          <svg class="w-full h-full border border-border rounded" viewBox="0 0 400 128" preserveAspectRatio="xMinYMax meet">
            <!-- Grid background -->
            <defs>
              <pattern id="$miningState.hashRateGrid" width="40" height="32" patternUnits="userSpaceOnUse">
                <path d="M 40 0 L 0 0 0 32" fill="none" stroke="hsl(var(--border))" stroke-width="0.5" opacity="0.3"/>
              </pattern>
            </defs>
            <rect width="100%" height="100%" fill="url(#$miningState.hashRateGrid)" />

            <!-- Data line with proper scaling -->
            <polyline
                    fill="none"
                    stroke="hsl(var(--primary))"
                    stroke-width="3"
                    stroke-linecap="round"
                    stroke-linejoin="round"
                    points={miningHistory.map((point, i) => {
        const x = (i / Math.max(miningHistory.length - 1, 1)) * 380 + 10; // 10px margin
        const maxHash = Math.max(...miningHistory.map(h => h.hashRate)) || 1;
        const y = 118 - ((point.hashRate / maxHash) * 100); // 10px margin top/bottom
        return `${x},${y}`;
      }).join(" ")}
            />

            <!-- Data points -->
            {#each miningHistory as point, i}
              {@const x = (i / Math.max(miningHistory.length - 1, 1)) * 380 + 10}
              {@const maxHash = Math.max(...miningHistory.map(h => h.hashRate)) || 1}
              {@const y = 118 - ((point.hashRate / maxHash) * 100)}
              <circle
                      cx={x}
                      cy={y}
                      r="4"
                      fill="hsl(var(--primary))"
                      stroke="hsl(var(--background))"
                      stroke-width="2"
                      class="hover:r-6 transition-all cursor-pointer"
              >
                <title>{formatHashRate(point.hashRate)} at {new Date(point.timestamp).toLocaleTimeString()}</title>
              </circle>
            {/each}
          </svg>
        </div>
      {/if}

      <p class="text-xs text-muted-foreground text-center mt-2">Last 5 minutes</p>
    </Card>
  {/if}

  <!-- Logs Modal -->
  {#if showLogs}
    <div class="fixed inset-0 bg-background/80 backdrop-blur-sm z-50 flex items-center justify-center p-4">
      <Card class="w-full max-w-4xl max-h-[80vh] flex flex-col">
        <div class="p-4 border-b flex items-center justify-between">
          <h2 class="text-lg font-semibold">Mining Logs</h2>
          <Button size="sm" variant="ghost" on:click={toggleLogs}>
            <X class="h-4 w-4" />
          </Button>
        </div>
        <div class="flex-1 overflow-auto p-4 bg-black/90 font-mono text-xs">
          {#if logs.length === 0}
            <p class="text-gray-400">No logs available yet...</p>
          {:else}
            {#each logs as log}
              <div class="text-green-400 whitespace-pre-wrap break-all">{log}</div>
            {/each}
          {/if}
        </div>
        <div class="p-4 border-t flex items-center justify-between">
          <p class="text-sm text-muted-foreground">
            Auto-refresh: {logsInterval ? 'ON' : 'OFF'}
          </p>
          <div class="flex gap-2">
            <Button size="sm" variant="outline" on:click={fetchLogs}>
              <RefreshCw class="h-3 w-3 mr-1" />
              Refresh
            </Button>
            <Button size="sm" variant="outline" on:click={() => logs = []}>
              Clear
            </Button>
          </div>
        </div>
      </Card>
    </div>
  {/if}
</div>
