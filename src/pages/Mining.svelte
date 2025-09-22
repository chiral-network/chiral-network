<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Button from '$lib/components/ui/button.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import Progress from '$lib/components/ui/progress.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import DropDown from "$lib/components/ui/dropDown.svelte";
  import type { MiningHistoryPoint } from '$lib/stores';
  import { Cpu, Zap, TrendingUp, Award, Play, Pause, Coins, Thermometer, AlertCircle, Terminal, X, RefreshCw } from 'lucide-svelte'
  import { onDestroy, onMount, getContext } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import { etcAccount, miningState, transactions, type Transaction } from '$lib/stores'
  import { getVersion } from "@tauri-apps/api/app";
  import { t } from 'svelte-i18n';
  import { goto } from '@mateothegreat/svelte5-router';

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
  let temperature = 0.0
  let hasRealTemperature = false
  let temperatureLoading = true // Add loading state for temperature checks
  let hasCompletedFirstCheck = false // Track if we've completed the first temperature check

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
  // Wrap toggle for logs (true = wrap lines, false = preserve long lines with horizontal scroll)
  let wrapLogs: boolean = true

  // Log filtering
  let logFilters: { [key: string]: boolean } = {
    error: true,
    warn: true,
    info: true,
    other: true
  }

  // Computed filtered logs
  $: filteredLogs = logs.filter(log => {
    const level = detectLogLevel(log)
    return logFilters[level]
  })

  // Determine log level from a log line and return a semantic level
  function detectLogLevel(line: string): 'error' | 'warn' | 'info' | 'other' {
    if (!line) return 'other'
    const l = line.toLowerCase()
    if (l.includes('error') || l.includes('err') || l.includes('fatal')) return 'error'
    if (l.includes('warn') || l.includes('warning')) return 'warn'
    if (l.includes('info') || l.includes('[i]') || l.includes('notice')) return 'info'
    return 'other'
  }

  // Map level to Tailwind classes (text color + subtle opacity for less-important levels)
  function logLevelClass(line: string): string {
    const level = detectLogLevel(line)
    switch (level) {
      case 'error':
        return 'text-red-400'
      case 'warn':
        return 'text-amber-400'
      case 'info':
        return 'text-blue-300'
      default:
        return 'text-muted-foreground'
    }
  }

  // Parse a log line and extract a severity prefix (if present) and the rest of the message.
  // Example: "INFO [09-14|16:32:29.577] Message..." -> { prefix: 'INFO', rest: '[09-14|16:32:29.577] Message...' }
  function splitLogPrefix(line: string): { prefix: string | null; rest: string } {
    if (!line) return { prefix: null, rest: '' }
    // Match leading ALL-CAPS token (usually INFO, WARN, ERROR, DEBUG) optionally followed by a timestamp bracket
    const m = line.match(/^([A-Z]{3,7})\b\s*(.*)$/)
    if (m) {
      return { prefix: m[1], rest: m[2] }
    }
    return { prefix: null, rest: line }
  }



 // Bar or Line chart toggle
  let chartType: 'bar' | 'line' = 'bar';

  // Threads and intensity warnings
  let threadsWarning = '';
  let intensityWarning = '';

  let validationError: string | null = null;

  const navigation = getContext('navigation') as { setCurrentPage: (page: string) => void };
  
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


  let hoveredPoint: MiningHistoryPoint | null = null;
  let hoveredIndex: number | null = null;

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
    try {
      seenHashes = new Set(($miningState.recentBlocks ?? []).map((b: any) => b.hash))
    } catch{} 
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
        await appendNewBlocksFromBackend()
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
              $miningState.blocksFound = blocksFound; 
              //Visualization Now Handled By Backend
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
          const balance = parseFloat(results[3]);
          if (!isNaN(balance)) {
            // Only update if backend balance is higher
            if (balance > ($miningState.totalRewards ?? 0)) {
              $miningState.totalRewards = balance;

              // Mark all "pending" mining reward txs as completed
              transactions.update(list =>
                list.map(tx =>
                  tx.status === 'pending' ? { ...tx, status: 'completed' } : tx
                )
              );
            }
          }
        }
                
        // Update blocks mined from blockchain query
        if (results[4] !== undefined) {
          const blocksMined = results[4] as number;
          if (blocksMined > $miningState.blocksFound) {
            $miningState.blocksFound = blocksMined;
          }
        }
      }
    } catch (e) {
      console.error('Failed to update network stats:', e)
    }
  }

  async function updateCpuTemperature() {
    // Only show loading state for the very first check
    if (!hasCompletedFirstCheck) {
      temperatureLoading = true
    }
    
    try {
      const temp = await invoke('get_cpu_temperature') as number
      if (temp && temp > 0) {
        temperature = temp
        hasRealTemperature = true
      } else {
        hasRealTemperature = false
      }
    } catch (e) {
      console.error('Failed to get CPU temperature:', e)
      hasRealTemperature = false
    } finally {
      if (!hasCompletedFirstCheck) {
        temperatureLoading = false
        hasCompletedFirstCheck = true
      }
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
      
      // Update power consumption estimates
      powerConsumption = $miningState.activeThreads * 25 * ($miningState.minerIntensity / 100)
      
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

      // ❌ Remove session payout, since pushRecentBlock already logs rewards

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
  // Simulation removed; recent blocks come from backend

  // Keep a set of hashes we've already shown to avoid duplicates
  let seenHashes = new Set<string>();

  // Pagination for recent blocks
  let pageSizes = [5, 10, 20, 50]
  let pageSize: number = 10
  let currentPage: number = 1

  // Derived values
  $: totalBlocks = ($miningState.recentBlocks || []).length
  $: totalPages = Math.max(1, Math.ceil(totalBlocks / pageSize))
  $: {
    // Clamp currentPage when totalPages or pageSize changes
    if (currentPage > totalPages) currentPage = totalPages
    if (currentPage < 1) currentPage = 1
  }

  // When recentBlocks array changes (new block added or removed) we rely on
  // pushRecentBlock to set currentPage = 1 so the newest block is visible.

  $: displayedBlocks = ($miningState.recentBlocks || []).slice((currentPage - 1) * pageSize, currentPage * pageSize)

function pushRecentBlock(b: {
  hash: string;
  nonce?: number;
  difficulty?: number;
  timestamp?: Date;
  number?: number;
  reward?: number;
}) {
  const reward = typeof b.reward === "number" ? b.reward : blockReward;

  const item = {
    id: `block-${b.hash}-${b.timestamp?.getTime() ?? Date.now()}`,
    hash: b.hash,
    reward,
    timestamp: b.timestamp ?? new Date(),
    difficulty: b.difficulty ?? currentDifficulty,
    nonce: b.nonce ?? 0,
    number: b.number ?? 0,
  };

  // Add block to recentBlocks
  $miningState.recentBlocks = [item, ...($miningState.recentBlocks ?? [])].slice(0, 50);

  // Reset pagination so newest block is visible
  currentPage = 1;

  // 💰 Immediately credit wallet balance (optimistic update)
  $miningState.totalRewards = ($miningState.totalRewards ?? 0) + reward;

  // 💳 Add a transaction entry for this block
  const tx: Transaction = {
    id: Date.now(),
    type: 'received',
    amount: reward,
    from: 'Mining reward',
    date: new Date(),
    description: `Block reward (#${item.number})`,
    status: 'pending' // mark as pending until backend confirms
  };
  transactions.update(list => [tx, ...list]);
}

  async function appendNewBlocksFromBackend() {
    try {
      if (!($etcAccount && $miningState.isMining)) return;
      const lookback = 2000;
      const limit = 50;
      const blocks = await invoke('get_recent_mined_blocks_pub', {
        address: $etcAccount.address,
        lookback,
        limit
      }) as Array<{ hash: string, nonce?: string, difficulty?: string, timestamp: number, number: number, reward?: number }>;
      for (const b of blocks) {
        if (seenHashes.has(b.hash)) continue;
        seenHashes.add(b.hash);
        pushRecentBlock({
          hash: b.hash,
          nonce: b.nonce ? parseInt(b.nonce, 16) : undefined,
          difficulty: b.difficulty ? parseInt(b.difficulty, 16) : undefined,
          timestamp: new Date((b.timestamp || 0) * 1000),
          number: b.number,
          reward: typeof b.reward === 'number' ? b.reward : undefined
        });
      }
      // Hard de-duplication by hash as a safety net
      const uniq = new Map<string, any>();
      for (const it of ($miningState.recentBlocks ?? [])) {
        if (!uniq.has(it.hash)) uniq.set(it.hash, it);
      }
  $miningState.recentBlocks = Array.from(uniq.values()).slice(0, 50);
    } catch (e) {
      console.error('Failed to append recent blocks:', e);
    } 
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
  // Pool structure with full configuration
  interface Pool {
    id: string
    name: string
    url: string
    worker: string
    password: string
  }

  let pools: Pool[] = []
  let poolOptions: { value: string; label: string }[] = []

  // Initialize with default pools
  $: {
    pools = [
      { id: 'solo', name: 'Solo Mining', url: '', worker: '', password: '' },
      { id: 'pool1', name: 'Pool 1', url: 'stratum://pool1.example.com:3333', worker: 'worker1', password: 'x' },
      { id: 'pool2', name: 'Pool 2', url: 'stratum://pool2.example.com:3333', worker: 'worker2', password: 'x' },
      { id: 'pool3', name: 'Community Pool', url: 'stratum://community.example.com:3333', worker: 'worker3', password: 'x' }
    ]
    
    // Create dropdown options
    poolOptions = pools.map(pool => ({
      value: pool.id,
      label: pool.name
    }))
  }

  // Current pool management state
  let poolConnected = false
  let poolUrl = 'stratum://localhost:3333'
  let poolWorker = 'worker1'
  let poolPassword = 'x'
  
  // Pool management UI state
  let showPoolManager = false
  let editingPool: Pool | null = null
  let newPool: Pool = { id: '', name: '', url: '', worker: '', password: '' }
  let poolStats = {
    connectedMiners: 0,
    totalShares: 0,
    currentDifficulty: 1000000,
    poolHashrate: 0
  }
  let poolStatsInterval: ReturnType<typeof setInterval> | null = null

  // Mock pool stats simulation
  function startPoolStatsSimulation() {
    if (poolStatsInterval) {
      clearInterval(poolStatsInterval)
    }
    
    poolStatsInterval = setInterval(() => {
      if (poolConnected) {
        // Simulate realistic pool stats
        poolStats.connectedMiners = Math.max(1, poolStats.connectedMiners + Math.floor(Math.random() * 3) - 1)
        poolStats.totalShares += Math.floor(Math.random() * 5)
        poolStats.poolHashrate = poolStats.connectedMiners * (85000 + Math.random() * 15000) // 85-100 KH/s per miner
        poolStats.currentDifficulty = 1000000 + Math.floor(Math.random() * 100000)
      }
    }, 3000) // Update every 3 seconds
  }

  function stopPoolStatsSimulation() {
    if (poolStatsInterval) {
      clearInterval(poolStatsInterval)
      poolStatsInterval = null
    }
  }

  // Pool management functions
  function getCurrentPool(): Pool | null {
    return pools.find(pool => pool.id === $miningState.selectedPool) || null
  }

  function updateCurrentPoolConfig() {
    const currentPool = getCurrentPool()
    if (currentPool) {
      poolUrl = currentPool.url
      poolWorker = currentPool.worker
      poolPassword = currentPool.password
    }
  }

  function addPool() {
    if (newPool.name && newPool.url) {
      const poolId = `pool_${Date.now()}`
      pools = [...pools, { ...newPool, id: poolId }]
      newPool = { id: '', name: '', url: '', worker: '', password: '' }
      showPoolManager = false
    }
  }

  function editPool(pool: Pool) {
    editingPool = { ...pool }
    showPoolManager = true
  }

  function savePool() {
    if (editingPool && editingPool.name && editingPool.url) {
      pools = pools.map(pool => 
        pool.id === editingPool!.id ? editingPool! : pool
      )
      editingPool = null
      showPoolManager = false
    }
  }

  function deletePool(poolId: string) {
    if (poolId !== 'solo') { // Don't delete solo mining
      pools = pools.filter(pool => pool.id !== poolId)
      if ($miningState.selectedPool === poolId) {
        $miningState.selectedPool = 'solo'
      }
    }
  }

  function cancelPoolEdit() {
    editingPool = null
    newPool = { id: '', name: '', url: '', worker: '', password: '' }
    showPoolManager = false
  }

  // Update pool config when selection changes
  $: if ($miningState.selectedPool) {
    updateCurrentPoolConfig()
  }
  
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
    if (poolStatsInterval) {
      clearInterval(poolStatsInterval)
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
          {#if temperatureLoading}
            <p class="text-2xl font-bold text-blue-500">--°C</p>
          {:else if hasRealTemperature}
            <p class="text-2xl font-bold {temperature > 80 ? 'text-red-500' : temperature > 70 ? 'text-orange-500' : temperature > 60 ? 'text-yellow-500' : 'text-green-500'}">{temperature.toFixed(1)}°C</p>
          {:else}
            <p class="text-2xl font-bold text-gray-500">N/A</p>
          {/if}
          <div class="mt-1">
            {#if temperatureLoading}
              <p class="text-xs text-muted-foreground mt-1">Detecting temperature sensors...</p>
            {:else if hasRealTemperature}
              <Progress 
                value={Math.min(temperature, 100)} 
                max={100} 
                class="h-2 {temperature > 80 ? '[&>div]:bg-red-500' : temperature > 70 ? '[&>div]:bg-orange-500' : temperature > 60 ? '[&>div]:bg-yellow-500' : '[&>div]:bg-green-500'}"
              />
              <p class="text-xs text-muted-foreground mt-1">
                {temperature > 85 ? 'Critical' : temperature > 75 ? 'Hot' : temperature > 65 ? 'Warm' : 'Normal'}
              </p>
            {:else}
              <Progress value={0} max={100} class="h-2 opacity-30" />
              <p class="text-xs text-muted-foreground mt-1">Hardware sensor not available</p>
            {/if}
          </div>
        </div>
        <div class="p-2 {temperatureLoading ? 'bg-blue-500/20' : hasRealTemperature ? (temperature > 80 ? 'bg-red-500/20' : temperature > 70 ? 'bg-orange-500/20' : temperature > 60 ? 'bg-yellow-500/20' : 'bg-green-500/20') : 'bg-gray-500/20'} rounded-lg">
          <Thermometer class="h-5 w-5 {temperatureLoading ? 'text-blue-500 animate-pulse' : hasRealTemperature ? (temperature > 80 ? 'text-red-500' : temperature > 70 ? 'text-orange-500' : temperature > 60 ? 'text-yellow-500' : 'text-green-500') : 'text-gray-500'}" />
        </div>
      </div>
    </Card>
    
    <!-- Pool Status Card (only show when pool is selected) -->
    {#if $miningState.selectedPool !== 'solo'}
      <Card class="p-4">
        <div class="flex items-center justify-between">
          <div>
            <p class="text-sm text-muted-foreground">Pool Status</p>
            <p class="text-2xl font-bold">{poolConnected ? 'Connected' : 'Disconnected'}</p>
            <p class="text-xs text-muted-foreground mt-1">
              {poolConnected ? `${poolStats.connectedMiners} miners` : 'Not connected to pool'}
            </p>
          </div>
          <div class="p-2 {poolConnected ? 'bg-green-500/10' : 'bg-red-500/10'} rounded-lg">
            <div class="w-3 h-3 rounded-full {poolConnected ? 'bg-green-500' : 'bg-red-500'}"></div>
          </div>
        </div>
      </Card>
    {/if}
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
          <div class="flex gap-2 items-end">
            <div class="flex-1">
              <DropDown
                id="pool-select"
                options={poolOptions}
                bind:value={$miningState.selectedPool}
                disabled={$miningState.isMining}
              />
            </div>
            <Button
              variant="outline"
              size="sm"
              disabled={$miningState.isMining}
              on:click={() => showPoolManager = true}
            >
              Manage Pools
            </Button>
          </div>
        </div>
        
        <!-- Pool Management Mockup -->
        {#if $miningState.selectedPool !== 'solo'}
          <div class="col-span-full space-y-4 p-4 bg-muted/50 rounded-lg">
            <h3 class="font-semibold text-lg">Pool Management</h3>
            
            <!-- Pool Configuration -->
            <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
              <div>
                <Label for="pool-url">Pool URL</Label>
                <Input
                  id="pool-url"
                  bind:value={poolUrl}
                  placeholder="stratum://localhost:3333"
                  disabled={$miningState.isMining}
                  class="mt-2"
                />
              </div>
              <div>
                <Label for="pool-worker">Worker Name</Label>
                <Input
                  id="pool-worker"
                  bind:value={poolWorker}
                  placeholder="worker1"
                  disabled={$miningState.isMining}
                  class="mt-2"
                />
              </div>
              <div>
                <Label for="pool-password">Password</Label>
                <Input
                  id="pool-password"
                  bind:value={poolPassword}
                  placeholder="x"
                  disabled={$miningState.isMining}
                  class="mt-2"
                />
              </div>
            </div>
            
            <!-- Connection Status -->
            <div class="flex items-center gap-2">
              <div class="w-2 h-2 rounded-full {poolConnected ? 'bg-green-500' : 'bg-red-500'}"></div>
              <span class="text-sm font-medium">
                {poolConnected ? 'Connected to Pool' : 'Not Connected'}
              </span>
            </div>
            
            <!-- Pool Actions -->
            <div class="flex gap-2">
              {#if !poolConnected}
                <Button 
                  disabled={$miningState.isMining}
                  size="sm"
                  on:click={() => {
                    poolConnected = true
                    startPoolStatsSimulation()
                  }}
                >
                  Connect to Pool
                </Button>
              {:else}
                <Button 
                  disabled={$miningState.isMining}
                  variant="outline"
                  size="sm"
                  on:click={() => {
                    poolConnected = false
                    stopPoolStatsSimulation()
                  }}
                >
                  Disconnect
                </Button>
              {/if}
              
              <Button 
                disabled={$miningState.isMining}
                variant="secondary"
                size="sm"
              >
                Start Pool Server
              </Button>
              <Button 
                disabled={$miningState.isMining}
                variant="destructive"
                size="sm"
              >
                Stop Pool Server
              </Button>
            </div>
            
            <!-- Pool Statistics (when connected) -->
            {#if poolConnected}
              <div class="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
                <div>
                  <p class="text-muted-foreground">Connected Miners</p>
                  <p class="font-semibold">{poolStats.connectedMiners}</p>
                </div>
                <div>
                  <p class="text-muted-foreground">Pool Hashrate</p>
                  <p class="font-semibold">{poolStats.poolHashrate.toFixed(2)} H/s</p>
                </div>
                <div>
                  <p class="text-muted-foreground">Total Shares</p>
                  <p class="font-semibold">{poolStats.totalShares}</p>
                </div>
                <div>
                  <p class="text-muted-foreground">Difficulty</p>
                  <p class="font-semibold">{poolStats.currentDifficulty.toLocaleString()}</p>
                </div>
              </div>
            {/if}
          </div>
        {/if}
        
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
              {$t('mining.errors.gethNotRunning')} <button on:click={() => { navigation.setCurrentPage('network'); goto('/network'); }} class="underline font-medium">{$t('mining.networkPage')}</button>
            </p>
          </div>
        </div>
      {/if}
      {#if !$etcAccount && isGethRunning}
        <div class="bg-blue-500/10 border border-blue-500/20 rounded-lg p-3 mt-2">
          <div class="flex items-center gap-2">
            <AlertCircle class="h-4 w-4 text-blue-500 flex-shrink-0" />
            <p class="text-sm text-blue-600">
              {$t('mining.errors.noAccountLink')} <button on:click={() => { navigation.setCurrentPage('account'); goto('/account'); }} class="underline font-medium">{$t('mining.accountPage')}</button>
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
        <div class="flex items-center justify-between mb-2">
          <div class="flex items-center gap-2">
            <label for="page-size-select" class="text-sm text-muted-foreground">{$t('Page Size')}:</label>
            <select id="page-size-select" bind:value={pageSize} on:change={() => { currentPage = 1 }} class="px-2 py-1 rounded border bg-background text-sm">
              {#each pageSizes as s}
                <option value={s}>{s}</option>
              {/each}
            </select>
          </div>

          <div class="flex items-center gap-2">
            <button class="px-2 py-1 rounded border bg-background text-sm" on:click={() => { if (currentPage > 1) currentPage -= 1 }} disabled={currentPage <= 1}>
              {$t('prev')}
            </button>
            <div class="text-sm text-muted-foreground">{currentPage} / {totalPages}</div>
            <button class="px-2 py-1 rounded border bg-background text-sm" on:click={() => { if (currentPage < totalPages) currentPage += 1 }} disabled={currentPage >= totalPages}>
              {$t('next')}
            </button>
          </div>
        </div>

        <div class="space-y-2 max-h-80 overflow-y-auto pr-1">
          {#each displayedBlocks ?? [] as block}
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

      <!-- Chart with Y-axis, gradients, tooltips, and axis labels -->
      <div class="flex h-48 gap-2">
        <!-- Y-axis labels -->
        <div class="flex flex-col justify-between text-xs text-muted-foreground pr-2">
          <span>{Math.max(...($miningState.miningHistory || []).map(h => h.hashRate)).toFixed(0)} H/s</span>
          <span>{(Math.max(...($miningState.miningHistory || []).map(h => h.hashRate)) / 2).toFixed(0)} H/s</span>
          <span>0</span>
        </div>

        <div class="relative flex-1">
          <!-- Gridlines -->
          <div class="absolute inset-0 flex flex-col justify-between">
            <div class="border-t border-muted-foreground/20"></div>
            <div class="border-t border-muted-foreground/20"></div>
            <div class="border-t border-muted-foreground/20"></div>
          </div>

          {#if chartType === 'bar'}
            <!-- Bar Chart -->
            <div class="flex items-end gap-1 h-full">
              {#each ($miningState.miningHistory || []) as point, i}
                <div
                  role="button"
                  tabindex="0"
                  class="flex-1 bg-gradient-to-t from-blue-400/40 to-blue-500/80 hover:from-blue-500/60 hover:to-blue-600/90 transition-all rounded-t-md shadow-sm relative group"
                  style="height: {(point.hashRate / Math.max(...($miningState.miningHistory || []).map(h => h.hashRate))) * 100}%"
                  title="{formatHashRate(point.hashRate)}"
                  on:mouseenter={() => { hoveredPoint = point; hoveredIndex = i; }}
                  on:mouseleave={() => { hoveredPoint = null; hoveredIndex = null; }}
                  aria-label={formatHashRate(point.hashRate) + ' at ' + new Date(point.timestamp).toLocaleTimeString()}
                >
                  {#if hoveredIndex === i && hoveredPoint}
                    <div
                      class="absolute left-1/2 -translate-x-1/2 -top-8 z-10 px-2 py-1 rounded bg-primary text-white text-xs shadow-lg pointer-events-none"
                      style="white-space:nowrap;"
                    >
                      {formatHashRate(hoveredPoint.hashRate)}<br/>{new Date(hoveredPoint.timestamp).toLocaleTimeString()}
                      <span class="absolute left-1/2 -translate-x-1/2 top-full w-0 h-0 border-l-6 border-l-transparent border-r-6 border-r-transparent border-t-6 border-t-primary"></span>
                    </div>
                  {/if}
                </div>
              {/each}
            </div>
          {:else}
            <!-- Line Chart -->
            <div class="relative h-full">
              <svg class="w-full h-full" viewBox="0 0 100 100" preserveAspectRatio="none">
                <!-- Line -->
                <polyline
                  fill="none"
                  stroke="rgb(59, 130, 246)"
                  stroke-width="1.5"
                  stroke-linecap="round"
                  stroke-linejoin="round"
                  points={($miningState.miningHistory || []).map((point, i) => {
                    const x = (i / Math.max(($miningState.miningHistory || []).length - 1, 1)) * 100;
                    const maxHash = Math.max(...($miningState.miningHistory || []).map(h => h.hashRate)) || 1;
                    const y = 100 - ((point.hashRate / maxHash) * 100);
                    return `${x},${y}`;
                  }).join(' ')}
                  class="drop-shadow-sm"
                />
                <!-- Area under the line -->
                <polygon
                  fill="url(#miningGradient)"
                  opacity="0.3"
                  points={($miningState.miningHistory || []).map((point, i) => {
                    const x = (i / Math.max(($miningState.miningHistory || []).length - 1, 1)) * 100;
                    const maxHash = Math.max(...($miningState.miningHistory || []).map(h => h.hashRate)) || 1;
                    const y = 100 - ((point.hashRate / maxHash) * 100);
                    return `${x},${y}`;
                  }).join(' ') + ` 100,100 0,100`}
                />
                <!-- Data points -->
                {#each ($miningState.miningHistory || []) as point, i}
                  <circle
                    cx={i / Math.max(($miningState.miningHistory || []).length - 1, 1) * 100}
                    cy={100 - ((point.hashRate / Math.max(...($miningState.miningHistory || []).map(h => h.hashRate)) || 1) * 100)}
                    r="1.2"
                    fill="rgb(59, 130, 246)"
                    stroke="white"
                    stroke-width="0.2"
                    class="cursor-pointer hover:r-2 transition-all"
                    role="button"
                    tabindex="0"
                    aria-label={formatHashRate(point.hashRate) + ' at ' + new Date(point.timestamp).toLocaleTimeString()}
                    on:mouseenter={() => { hoveredPoint = point; hoveredIndex = i; }}
                    on:mouseleave={() => { hoveredPoint = null; hoveredIndex = null; }}
                  />
                {/each}
                <!-- Gradient definition -->
                <defs>
                  <linearGradient id="miningGradient" x1="0%" y1="0%" x2="0%" y2="100%">
                    <stop offset="0%" style="stop-color:rgb(59, 130, 246);stop-opacity:0.4" />
                    <stop offset="100%" style="stop-color:rgb(59, 130, 246);stop-opacity:0.05" />
                  </linearGradient>
                </defs>
              </svg>

              <!-- Tooltip for line chart -->
              {#if hoveredPoint && hoveredIndex !== null}
                <div
                  class="absolute z-10 px-2 py-1 rounded bg-primary text-white text-xs shadow-lg pointer-events-none"
                  style="
                    left: {(hoveredIndex / Math.max(($miningState.miningHistory || []).length - 1, 1)) * 100}%;
                    top: {100 - ((hoveredPoint.hashRate / Math.max(...($miningState.miningHistory || []).map(h => h.hashRate)) || 1) * 100)}%;
                    transform: translate(-50%, -100%);
                    margin-top: -8px;
                    white-space: nowrap;"
                >
                  {formatHashRate(hoveredPoint.hashRate)}<br/>{new Date(hoveredPoint.timestamp).toLocaleTimeString()}
                  <span class="absolute left-1/2 -translate-x-1/2 top-full w-0 h-0 border-l-6 border-l-transparent border-r-6 border-r-transparent border-t-6 border-t-primary"></span>
                </div>
              {/if}
            </div>
          {/if}
        </div>
      </div>
      <!-- X-axis labels -->
      <div class="flex justify-between mt-2 text-xs text-muted-foreground">
        <span>{($miningState.miningHistory || [])[0] ? new Date(($miningState.miningHistory || [])[0].timestamp).toLocaleTimeString() : ''}</span>
        <span>{($miningState.miningHistory || [])[($miningState.miningHistory || []).length - 1] ? new Date(($miningState.miningHistory || [])[($miningState.miningHistory || []).length - 1].timestamp).toLocaleTimeString() : ''}</span>
      </div>
      <p class="text-xs text-muted-foreground text-center mt-2">{$t('mining.last5Minutes')}</p>
    </Card>
  {/if}
  </div>

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
          {#if filteredLogs.length === 0}
            <p class="text-xs text-muted-foreground">{$t('mining.noLogs')}</p>
            {:else}
            <div class="bg-secondary/50 rounded-lg p-2 max-h-[60vh] overflow-y-auto text-left font-mono text-xs">
              <!-- When wrapping is disabled, allow horizontal scroll and preserve whitespace -->
              <div class={wrapLogs ? 'w-full' : 'w-full overflow-x-auto'}>
                {#each filteredLogs.slice(-500) as log}
                  {@const split = splitLogPrefix(log)}
                  <p class="font-mono {wrapLogs ? 'whitespace-pre-wrap break-words' : 'whitespace-pre'}">
                    {#if split.prefix}
                      <span class={logLevelClass(split.prefix)}>{split.prefix}</span>
                      <span class="text-muted-foreground"> {split.rest}</span>
                    {:else}
                      <span class="text-muted-foreground">{split.rest}</span>
                    {/if}
                  </p>
                {/each}
              </div>
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
            
            <!-- Wrap toggle -->
            <div class="flex items-center gap-2 ml-3">
              <input id="wrap-logs" type="checkbox" bind:checked={wrapLogs} />
              <label for="wrap-logs" class="text-sm text-muted-foreground">{$t('mining.wrapLogs')}</label>
            </div>
          </div>

          <!-- Log Level Filters -->
          <div class="flex items-center gap-2">
            <span class="text-sm text-muted-foreground">{$t('mining.filterByLevel')}:</span>
            <div class="flex gap-1">
              {#each Object.entries(logFilters) as [level, enabled]}
                <button
                  class="px-2 py-1 text-xs rounded {enabled ? 'bg-primary text-primary-foreground' : 'bg-secondary text-secondary-foreground'}"
                  on:click={() => logFilters[level] = !enabled}
                >
                  {level.toUpperCase()}
                </button>
              {/each}
            </div>
          </div>

          <div class="flex items-center gap-2">
            <Button size="sm" variant="outline" on:click={fetchLogs}>
              <RefreshCw class="h-3 w-3 mr-1" />
              {$t('mining.refresh')}
            </Button>
            <Button size="sm" variant="outline" on:click={() => logs = []}>
              {$t('mining.clear')}
            </Button>
          </div>
        </div>
      </Card>
    </div>
  {/if}

  <!-- Pool Management Modal -->
  {#if showPoolManager}
    <div class="fixed inset-0 bg-black/50 flex items-center justify-center z-50">
      <div class="bg-background rounded-lg p-6 w-full max-w-2xl mx-4 max-h-[80vh] overflow-y-auto">
        <div class="flex items-center justify-between mb-4">
          <h2 class="text-xl font-semibold">
            {editingPool ? 'Edit Pool' : 'Manage Pools'}
          </h2>
          <Button variant="ghost" size="sm" on:click={cancelPoolEdit}>
            <X class="h-4 w-4" />
          </Button>
        </div>

        <!-- Pool List -->
        <div class="space-y-3 mb-6">
          <h3 class="font-medium">Existing Pools</h3>
          {#each pools as pool (pool.id)}
            <div class="flex items-center justify-between p-3 border rounded-lg">
              <div class="flex-1">
                <div class="font-medium">{pool.name}</div>
                <div class="text-sm text-muted-foreground">
                  {pool.url || 'Solo Mining'}
                  {#if pool.worker}
                    • {pool.worker}
                  {/if}
                </div>
              </div>
              <div class="flex gap-2">
                {#if pool.id !== 'solo'}
                  <Button
                    variant="outline"
                    size="sm"
                    on:click={() => editPool(pool)}
                  >
                    Edit
                  </Button>
                  <Button
                    variant="destructive"
                    size="sm"
                    on:click={() => deletePool(pool.id)}
                  >
                    Delete
                  </Button>
                {/if}
              </div>
            </div>
          {/each}
        </div>

        <!-- Add/Edit Pool Form -->
        <div class="space-y-4">
          <h3 class="font-medium">
            {editingPool ? 'Edit Pool' : 'Add New Pool'}
          </h3>
          
          <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div>
              <Label for="pool-name">Pool Name</Label>
              <Input
                id="pool-name"
                value={editingPool ? editingPool.name : newPool.name}
                on:input={(e: Event) => {
                  const target = e.target as HTMLInputElement
                  if (editingPool) {
                    editingPool.name = target.value
                  } else {
                    newPool.name = target.value
                  }
                }}
                placeholder="My Mining Pool"
                class="mt-2"
              />
            </div>
            <div>
              <Label for="pool-url">Pool URL</Label>
              <Input
                id="pool-url"
                value={editingPool ? editingPool.url : newPool.url}
                on:input={(e: Event) => {
                  const target = e.target as HTMLInputElement
                  if (editingPool) {
                    editingPool.url = target.value
                  } else {
                    newPool.url = target.value
                  }
                }}
                placeholder="stratum://pool.example.com:3333"
                class="mt-2"
              />
            </div>
            <div>
              <Label for="pool-worker">Worker Name</Label>
              <Input
                id="pool-worker"
                value={editingPool ? editingPool.worker : newPool.worker}
                on:input={(e: Event) => {
                  const target = e.target as HTMLInputElement
                  if (editingPool) {
                    editingPool.worker = target.value
                  } else {
                    newPool.worker = target.value
                  }
                }}
                placeholder="worker1"
                class="mt-2"
              />
            </div>
            <div>
              <Label for="pool-password">Password</Label>
              <Input
                id="pool-password"
                value={editingPool ? editingPool.password : newPool.password}
                on:input={(e: Event) => {
                  const target = e.target as HTMLInputElement
                  if (editingPool) {
                    editingPool.password = target.value
                  } else {
                    newPool.password = target.value
                  }
                }}
                placeholder="x"
                class="mt-2"
              />
            </div>
          </div>

          <div class="flex gap-2 pt-4">
            {#if editingPool}
              <Button on:click={savePool}>
                Save Changes
              </Button>
            {:else}
              <Button on:click={addPool}>
                Add Pool
              </Button>
            {/if}
            <Button variant="outline" on:click={cancelPoolEdit}>
              Cancel
            </Button>
          </div>
        </div>
      </div>
    </div>
  {/if} 