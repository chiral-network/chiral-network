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
  import { t } from 'svelte-i18n';

  // Interfaces - MiningHistoryPoint is now defined in stores.ts
  
  // interface RecentBlock {
  //   id: string
  //   hash: string
  //   reward: number
  //   timestamp: Date
  //   difficulty: number
  //   nonce: number
  // }
  
  // Local UI state only
  let isTauri = false
  let isGethRunning = false
  let currentBlock = 0
  let totalHashes = 0
  let currentDifficulty = 4
  let lastHashUpdate = Date.now()
  let cpuThreads = navigator.hardwareConcurrency || 4
  let selectedThreads = Math.floor(cpuThreads / 2)
  let error = ''
  
  // Network statistics
  let networkHashRate = '0 H/s'
  let networkDifficulty = '0'
  let blockReward = 5 // Chiral per block
  let peerCount = 0

  // Statistics - preserve across page navigation
  let sessionStartTime = $miningState.isMining ? 
    $miningState.sessionStartTime || Date.now() : 
    Date.now()
  let estimatedTimeToBlock = 0
  $: powerConsumption = $miningState.activeThreads * 15
  $: efficiency = $miningState.hashRate === '0 H/s' ? 0 : parseHashRate($miningState.hashRate) / powerConsumption
  let temperature = 45.0
  $: if (!isTauri) {
    temperature = 45 + ($miningState.activeThreads * 3.5)
  }

  // Uptime tick (forces template to re-render every second while mining)
  let uptimeNow: number = Date.now()
  let uptimeInterval: number | null = null
  
  // Mining history is now stored in the miningState store
  // let recentBlocks: RecentBlock[] = []
  
  // Mock mining intervals  
  let statsInterval: number | null = null
  
  // Logs
  let showLogs = false
  let logs: string[] = []
  let logsInterval: number | null = null
  // simplified log view — no font/wrap controls
  // Auto-refresh toggle for logs modal
  let autoRefresh: boolean = true



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
            ? $t('mining.errors.threads', { values: { cpuThreads } })
            : '';
  }

  // Intensity warning
  $: {
    const numIntensity = Number($miningState.minerIntensity);
    intensityWarning = (numIntensity < 1 || numIntensity > 100)
            ? $t('mining.errors.intensity')
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
    
    // If mining is already active from before, restore session and update stats
    if ($miningState.isMining) {
      // Restore session start time if it exists
      if ($miningState.sessionStartTime) {
        sessionStartTime = $miningState.sessionStartTime
      }
      startUptimeTimer()
      await updateMiningStats()
    }
    if (isTauri) {
      await updateCpuTemperature()
    }
    
    // Start polling for mining stats
    statsInterval = setInterval(async () => {
      if ($miningState.isMining) {
        await updateMiningStats()
      }
      await updateNetworkStats()
      if (isTauri) {
        await updateCpuTemperature()
      }
    }, 1000) as unknown as number
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
      
      currentBlock = block
      
      // Try to get real hash rate from logs if standard API returns 0
      if (rate === '0 H/s' && $miningState.isMining) {
        try {
          // Get mining performance from logs
          const [blocksFound, hashRateFromLogs] = await invoke('get_miner_performance', { 
            dataDir: './bin/geth-data' 
          }) as [number, number]
          
          if (hashRateFromLogs > 0) {
            // Use actual hash rate from logs
            $miningState.hashRate = formatHashRate(hashRateFromLogs)
            if (blocksFound > $miningState.blocksFound) {
              const newBlocks = blocksFound - $miningState.blocksFound;
              const rewardPerBlock = 5.0;
              $miningState.totalRewards += newBlocks * rewardPerBlock;
              $miningState.blocksFound = blocksFound;
              // Add each new block to recentBlocks
              for (let i = 0; i < newBlocks; i++) {
                findBlock();
              }
            }
          } else if ($miningState.activeThreads > 0) {
            // Fall back to simulation if no log data yet
            const elapsed = (Date.now() - sessionStartTime) / 1000 // seconds
            const baseRate = $miningState.activeThreads * 85000 // 85 KH/s per thread
            const variation = Math.sin(elapsed / 10) * baseRate * 0.1 // ±10% variation
            const simulatedRate = baseRate + variation
            $miningState.hashRate = `~${formatHashRate(simulatedRate)}`
          }
        } catch (perfError) {
          // If performance fetch fails, fall back to simulation
          if ($miningState.activeThreads > 0) {
            const elapsed = (Date.now() - sessionStartTime) / 1000
            const baseRate = $miningState.activeThreads * 85000
            const variation = Math.sin(elapsed / 10) * baseRate * 0.1
            const simulatedRate = baseRate + variation
            $miningState.hashRate = `~${formatHashRate(simulatedRate)}`
          }
        }
      } else if (rate !== '0 H/s') {
        // Use actual rate if available from standard API
        $miningState.hashRate = rate
      }
      
      // Convert hashRate string to number for chart
      let hashRateNum = 0
      // Clean up the rate string (remove ~ and text in parentheses)
      const cleanRate = $miningState.hashRate.replace(/[~()a-zA-Z \.]+/g, '').trim()
      
      if ($miningState.hashRate.includes('GH/s')) {
        hashRateNum = parseFloat(cleanRate) * 1000000000
      } else if ($miningState.hashRate.includes('MH/s')) {
        hashRateNum = parseFloat(cleanRate) * 1000000
      } else if ($miningState.hashRate.includes('KH/s')) {
        hashRateNum = parseFloat(cleanRate) * 1000
      } else {
        hashRateNum = parseFloat(cleanRate) || 0
      }
      
      // Update mining history for chart
      if ($miningState.isMining) {
        $miningState.miningHistory = [...($miningState.miningHistory || []).slice(-29), {
          timestamp: Date.now(),
          hashRate: hashRateNum,
          power: powerConsumption
        }]
        
        // Update total hashes based on hashrate and time elapsed
        const timeDelta = (Date.now() - lastHashUpdate) / 1000 // seconds
        totalHashes += Math.floor(hashRateNum * timeDelta)
        lastHashUpdate = Date.now()
        
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
        const promises: Promise<any>[] = [
          invoke('get_network_stats') as Promise<[string, string]>,
          invoke('get_current_block') as Promise<number>,
          invoke('get_network_peer_count') as Promise<number>
        ]
        
        // Also fetch account balance and blocks mined if we have an account and are mining
        if ($etcAccount && $miningState.isMining) {
          promises.push(invoke('get_account_balance', { 
            address: $etcAccount.address 
          }) as Promise<string>)
          promises.push(invoke('get_blocks_mined', { 
            address: $etcAccount.address 
          }) as Promise<number>)
        }
        
        const results = await Promise.all(promises)
        
        networkDifficulty = results[0][0]
        networkHashRate = results[0][1]
        currentBlock = results[1]
        peerCount = results[2]
        
        // Update total rewards from actual balance
        if (results[3] !== undefined) {
          const balance = parseFloat(results[3])
          if (!isNaN(balance) && balance > 0) {
            // Use actual balance as total rewards
            $miningState.totalRewards = balance
          }
        }
        
        // Update blocks mined from blockchain query
        if (results[4] !== undefined) {
          const blocksMined = results[4] as number;
          if (blocksMined > $miningState.blocksFound) {
            const newBlocks = blocksMined - $miningState.blocksFound;
            $miningState.blocksFound = blocksMined;
            for (let i = 0; i < newBlocks; i++) {
              findBlock();
            }
          }
        }
      }
    } catch (e) {
      console.error('Failed to update network stats:', e)
    }
  }

  async function updateCpuTemperature() {
    try {
      const temp = await invoke('get_cpu_temperature') as number
      console.log(temp)
      if (temp > 0) {
        temperature = temp
      }
    } catch (e) {
      console.error('Failed to get CPU temperature:', e)
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
      error = $t('mining.errors.noAccount')
      return
    }
    
    if (!isGethRunning) {
      error = $t('mining.errors.gethNotRunning')
      return
    }
    
    error = ''
    validationError = null
    
    try {
      // Show message that we're starting mining
      error = $t('mining.starting')
      
      await invoke('start_miner', {
        address: $etcAccount.address,
        threads: selectedThreads,
        dataDir: './bin/geth-data'
      })
      
      error = '' // Clear the status message
      $miningState.isMining = true
      sessionStartTime = Date.now()
      // Store session start time in the store for persistence
      $miningState.sessionStartTime = sessionStartTime
      $miningState.activeThreads = actualThreads  // Use computed actualThreads
      totalHashes = 0 // Reset total hashes
      lastHashUpdate = Date.now()
      startUptimeTimer()
      
      // Start updating stats
      await updateMiningStats()
      
      // Update power and temperature estimates
      powerConsumption = $miningState.activeThreads * 25 * ($miningState.minerIntensity / 100)
      if (!isTauri) {
        temperature = 45 + ($miningState.activeThreads * 3) + ($miningState.minerIntensity / 10)
      }
      
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
      // Clear session start time
      $miningState.sessionStartTime = undefined
      // Clear mining history when stopping
      $miningState.miningHistory = []
      
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
    
    $miningState.recentBlocks = [{
      id: `block-${Date.now()}`,
      hash: `0x${Math.random().toString(16).substring(2, 10)}...${Math.random().toString(16).substring(2, 6)}`,
      reward: reward,
      timestamp: new Date(),
      difficulty: currentDifficulty,
      nonce: Math.floor(Math.random() * 1000000)
    }, ...($miningState.recentBlocks ?? []).slice(0, 4)]
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
        dataDir: './bin/geth-data',
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
      // Start auto-refresh of logs if enabled
      if (autoRefresh) {
        logsInterval = setInterval(fetchLogs, 2000) as unknown as number
      }
    } else {
      // Stop auto-refresh
      if (logsInterval) {
        clearInterval(logsInterval)
        logsInterval = null
      }
    }
  }
  
  // Mock pool options
  $: pools = [
    { value: 'solo', label: $t('mining.pools.solo') },
    { value: 'pool1', label: $t('mining.pools.pool1') },
    { value: 'pool2', label: $t('mining.pools.pool2') },
    { value: 'pool3', label: $t('mining.pools.community') }
  ]
  
  onDestroy(async () => {
    // Don't stop mining when leaving the page - preserve state
    // Only clean up intervals
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
    <h1 class="text-3xl font-bold">{$t('mining.title')}</h1>
    <p class="text-muted-foreground mt-2">{$t('mining.subtitle')}</p>
  </div>
  
  <!-- Mining Status Cards -->
  <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
    <Card class="p-4">
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-muted-foreground">{$t('mining.hashRate')}</p>
          <p class="text-2xl font-bold">{$miningState.hashRate}</p>
          <p class="text-xs text-muted-foreground mt-1">
            {$miningState.isMining ? `${$miningState.activeThreads} ${$t('mining.threads')}` : $t('mining.notMining')}
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
          <p class="text-sm text-muted-foreground">{$t('mining.totalRewards')}</p>
          <p class="text-2xl font-bold">{$miningState.totalRewards.toFixed(2)} Chiral</p>
          <p class="text-xs text-green-600 flex items-center gap-1 mt-1">
            <TrendingUp class="h-3 w-3" />
            {$miningState.blocksFound} {$t('mining.blocksFound')}
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
          <p class="text-sm text-muted-foreground">{$t('mining.powerUsage')}</p>
          <p class="text-2xl font-bold">{powerConsumption.toFixed(0)}W</p>
          <p class="text-xs text-muted-foreground mt-1">
            {efficiency.toFixed(2)} {$t('mining.hw')}
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
          <p class="text-sm text-muted-foreground">{$t('mining.temperature')}</p>
          <p class="text-2xl font-bold">{temperature.toFixed(1)}°C</p>
          <div class="mt-1">
            <Progress 
              value={temperature} 
              max={100} 
              class="h-1 {temperature > 80 ? 'bg-red-500' : temperature > 60 ? 'bg-yellow-500' : ''}"
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
      <h2 class="text-lg font-semibold">{$t('mining.control')}</h2>
      <Badge variant={$miningState.isMining ? 'default' : 'secondary'}>
        {$miningState.isMining ? $t('mining.active') : $t('mining.stopped')}
      </Badge>
    </div>
    
    <div class="space-y-4">
      <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
        <div class="relative">
          <Label for="pool-select">{$t('mining.pool')}</Label>
          <DropDown
            id="pool-select"
            options={pools}
            bind:value={$miningState.selectedPool}
            disabled={$miningState.isMining}
          />

        </div>
        
        <div>
          <Label for="thread-count">{$t('mining.cpuThreads', { values: { cpuThreads } })}</Label>
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
          <Label for="intensity">{$t('mining.intensity')}</Label>
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
            {$t('mining.session')}: <span class="font-medium">{$miningState.isMining ? formatUptime(uptimeNow) : '0h 0m 0s'}</span>
          </p>
          <p class="text-muted-foreground">
            {$t('mining.totalHashes')}: <span class="font-medium">{formatNumber(totalHashes)}</span>
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
              {$t('mining.stop')}
            {:else}
              <Play class="h-4 w-4 mr-2" />
              {$t('mining.start')}
            {/if}
          </Button>
          <Button
            size="lg"
            variant="outline"
            on:click={toggleLogs}
            title={$t('mining.showLogs')}
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
              {$t('mining.errors.gethNotRunning')} <a href="/network" class="underline font-medium">{$t('mining.networkPage')}</a>
            </p>
          </div>
        </div>
      {/if}
      {#if !$etcAccount && isGethRunning}
        <div class="bg-blue-500/10 border border-blue-500/20 rounded-lg p-3 mt-2">
          <div class="flex items-center gap-2">
            <AlertCircle class="h-4 w-4 text-blue-500 flex-shrink-0" />
            <p class="text-sm text-blue-600">
              {$t('mining.errors.noAccountLink')} <a href="/account" class="underline font-medium">{$t('mining.accountPage')}</a>
            </p>
          </div>
        </div>
      {/if}
    </div>
  </Card>
  
  <!-- Mining Statistics -->
  <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
    <Card class="p-6">
      <h2 class="text-lg font-semibold mb-4">{$t('mining.networkStats')}</h2>
      <div class="space-y-3">
        <div class="flex justify-between items-center">
          <span class="text-sm text-muted-foreground">{$t('mining.networkHashRate')}</span>
          <Badge variant="outline">{networkHashRate}</Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm text-muted-foreground">{$t('mining.networkDifficulty')}</span>
          <Badge variant="outline">{networkDifficulty}</Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm text-muted-foreground">{$t('mining.blockHeight')}</span>
          <Badge variant="outline">#{currentBlock}</Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm text-muted-foreground">{$t('mining.blockReward')}</span>
          <Badge variant="outline">{blockReward} Chiral</Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm text-muted-foreground">{$t('mining.estTimeToBlock')}</span>
          <Badge variant="outline">
            {estimatedTimeToBlock > 0 ? `~${Math.floor(estimatedTimeToBlock / 60)} min` : $t('mining.calculating')}
          </Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm text-muted-foreground">{$t('mining.activeMiners')}</span>
          <Badge variant="outline">{peerCount}</Badge>
        </div>
      </div>
    </Card>
    
    <Card class="p-6">
      <h2 class="text-lg font-semibold mb-4">{$t('mining.poolInfo')}</h2>
      {#if $miningState.selectedPool === 'solo'}
        <div class="space-y-3">
          <p class="text-sm text-muted-foreground">
            {$t('mining.soloInfo')}
          </p>
          <div class="pt-2 space-y-2">
            <div class="flex justify-between">
              <span class="text-sm">{$t('mining.yourShare')}</span>
              <span class="text-sm font-medium">100%</span>
            </div>
            <div class="flex justify-between">
              <span class="text-sm">{$t('mining.poolFee')}</span>
              <span class="text-sm font-medium">0%</span>
            </div>
            <div class="flex justify-between">
              <span class="text-sm">{$t('mining.minPayout')}</span>
              <span class="text-sm font-medium">{$t('mining.na')}</span>
            </div>
          </div>
        </div>
      {:else}
        <div class="space-y-3">
          <div class="flex justify-between">
            <span class="text-sm">{$t('mining.poolHashRate')}</span>
            <span class="text-sm font-medium">850 MH/s</span>
          </div>
          <div class="flex justify-between">
            <span class="text-sm">{$t('mining.poolMiners')}</span>
            <span class="text-sm font-medium">342</span>
          </div>
          <div class="flex justify-between">
            <span class="text-sm">{$t('mining.yourShare')}</span>
            <span class="text-sm font-medium">{(parseHashRate($miningState.hashRate) / 850000000 * 100).toFixed(4)}%</span>
          </div>
          <div class="flex justify-between">
            <span class="text-sm">{$t('mining.poolFee')}</span>
            <span class="text-sm font-medium">1%</span>
          </div>
          <div class="flex justify-between">
            <span class="text-sm">{$t('mining.minPayout')}</span>
            <span class="text-sm font-medium">10 Chiral</span>
          </div>
          <div class="flex justify-between">
            <span class="text-sm">{$t('mining.paymentMethod')}</span>
            <span class="text-sm font-medium">PPLNS</span>
          </div>
        </div>
      {/if}
    </Card>
  </div>
  
  <!-- Recent Blocks -->
  <Card class="p-6">
    <h2 class="text-lg font-semibold mb-4">{$t('mining.recentBlocks')}</h2>
    {#if (!$miningState.recentBlocks || $miningState.recentBlocks.length === 0)}
      <p class="text-sm text-muted-foreground text-center py-8">
        {$t('mining.noBlocksFound')}
      </p>
    {:else}
      <div class="space-y-2">
        {#each $miningState.recentBlocks ?? [] as block}
          <div class="flex items-center justify-between p-3 bg-secondary rounded-lg">
            <div class="flex items-center gap-3">
              <Award class="h-4 w-4 text-yellow-500" />
              <div>
                <p class="text-sm font-medium">{$t('mining.blockFound')}</p>
                <p class="text-xs text-muted-foreground">
                  {$t('mining.hash')}: {block.hash} • {$t('mining.nonce')}: {block.nonce}
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
  {#if ($miningState.miningHistory || []).length > 0}
    <Card class="p-6">
      <h2 class="text-lg font-semibold mb-4">{$t('mining.hashRateHistory')}</h2>

      <!-- Chart Type Toggle -->
      <div class="flex items-center gap-2 mb-2">
        <span class="text-sm text-muted-foreground">{$t('mining.chartType')}:</span>
        <Button size="sm" variant={chartType === 'bar' ? 'default' : 'outline'} on:click={() => chartType = 'bar'}>{$t('mining.bar')}</Button>
        <Button size="sm" variant={chartType === 'line' ? 'default' : 'outline'} on:click={() => chartType = 'line'}>{$t('mining.line')}</Button>
      </div>

      <!-- Chart Rendering -->
      {#if chartType === 'bar'}
        <div class="h-32 flex items-end gap-1">
          {#each ($miningState.miningHistory || []) as point}
            <div
                    class="flex-1 bg-primary/20 hover:bg-primary/30 transition-all rounded-t"
                    style="height: {(point.hashRate / Math.max(...($miningState.miningHistory || []).map(h => h.hashRate))) * 100}%; transition: height 0.5s ease;"
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
                    points={($miningState.miningHistory || []).map((point, i) => {
        const x = (i / Math.max(($miningState.miningHistory || []).length - 1, 1)) * 380 + 10; // 10px margin
        const maxHash = Math.max(...($miningState.miningHistory || []).map(h => h.hashRate)) || 1;
        const y = 118 - ((point.hashRate / maxHash) * 100); // 10px margin top/bottom
        return `${x},${y}`;
      }).join(" ")}
            />

            <!-- Data points -->
            {#each ($miningState.miningHistory || []) as point, i}
              {@const x = (i / Math.max(($miningState.miningHistory || []).length - 1, 1)) * 380 + 10}
              {@const maxHash = Math.max(...($miningState.miningHistory || []).map(h => h.hashRate)) || 1}
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

      <p class="text-xs text-muted-foreground text-center mt-2">{$t('mining.last5Minutes')}</p>
    </Card>
  {/if}

  <!-- Logs Modal -->
  {#if showLogs}
    <div class="fixed inset-0 bg-background/80 backdrop-blur-sm z-50 flex items-center justify-center p-4">
      <Card class="w-full max-w-4xl max-h-[80vh] flex flex-col">
        <div class="p-4 border-b flex items-center justify-between">
          <h2 class="text-lg font-semibold">{$t('mining.logs')}</h2>
          <Button size="sm" variant="ghost" on:click={toggleLogs}>
            <X class="h-4 w-4" />
          </Button>
        </div>
        <div class="flex-1 p-4">
          {#if logs.length === 0}
            <p class="text-xs text-muted-foreground">{$t('mining.noLogs')}</p>
          {:else}
            <div class="bg-secondary/50 rounded-lg p-2 max-h-[60vh] overflow-y-auto text-left font-mono text-xs">
              {#each logs.slice(-500) as log}
                <p class="font-mono text-muted-foreground whitespace-pre-wrap break-all">{log}</p>
              {/each}
            </div>
          {/if}
        </div>

        <div class="p-4 border-t flex items-center justify-between">
          <div class="flex items-center gap-3">
            <input id="auto-refresh" type="checkbox" bind:checked={autoRefresh} on:change={() => {
              // If modal is open, start/stop the interval immediately
              if (showLogs) {
                if (autoRefresh && !logsInterval) {
                  logsInterval = setInterval(fetchLogs, 2000) as unknown as number
                } else if (!autoRefresh && logsInterval) {
                  clearInterval(logsInterval)
                  logsInterval = null
                }
              }
            }} />
            <label for="auto-refresh" class="text-sm text-muted-foreground">{$t('mining.autoRefresh')}</label>
          </div>

          <div class="flex gap-2">
            {#if !autoRefresh}
              <Button size="sm" variant="outline" on:click={fetchLogs}>
                <RefreshCw class="h-3 w-3 mr-1" />
                {$t('mining.refresh')}
              </Button>
            {/if}
            <Button size="sm" variant="outline" on:click={() => logs = []}>
              {$t('mining.clear')}
            </Button>
          </div>
        </div>
      </Card>
    </div>
  {/if}
</div>