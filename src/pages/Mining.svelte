<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Button from '$lib/components/ui/button.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import Progress from '$lib/components/ui/progress.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import { blockReward, miningState, type MiningHistoryPoint, wallet, accurateTotals, isCalculatingAccurateTotals, settings } from '$lib/stores';
  import { get } from 'svelte/store';
  import { Cpu, Zap, TrendingUp, Award, Play, Pause, Coins, Thermometer, AlertCircle, Terminal, X, RefreshCw, Calculator, DollarSign } from 'lucide-svelte'

  import { onDestroy, onMount, getContext } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import { getVersion } from "@tauri-apps/api/app";
  import { listen } from '@tauri-apps/api/event'
  import { t } from 'svelte-i18n';
  import { goto } from '@mateothegreat/svelte5-router';
  import { walletService } from '$lib/wallet';
  import TemporaryAccountWarning from '$lib/components/TemporaryAccountWarning.svelte';
  import { showToast } from '$lib/toast';
  import { gethSyncStatus } from '$lib/services/gethService';
  import { dhtService } from '$lib/dht';
  type TranslateParams = { values?: Record<string, unknown>; default?: string };
  // const tr = (key: string, params?: TranslateParams) => get(t)(key, params);
  const tr = (key: string, params?: TranslateParams): string =>
  $t(key, params);

  // Client mode detection (forced OR automatic NAT-based)
  let isClientMode = false;
  let clientModeReason: "forced" | "nat" | null = null;


  // Local UI state only
  let isTauri = false
  let isGethRunning = false
  let currentBlock = 0
  let totalHashes = 0
  let lastHashUpdate = Date.now()
  let cpuThreads = navigator.hardwareConcurrency || 4
  let selectedThreads = Math.floor(cpuThreads / 2)
  let error = ''

  // Blockchain sync status - updated from gethSyncStatus store
  $: isSyncing = $gethSyncStatus?.syncing ?? false
  $: syncProgress = $gethSyncStatus?.progress_percent ?? 0
  $: syncCurrentBlock = $gethSyncStatus?.current_block ?? 0
  $: syncHighestBlock = $gethSyncStatus?.highest_block ?? 0
  $: syncBlocksRemaining = $gethSyncStatus?.blocks_remaining ?? 0
  $: syncEstimatedSecondsRemaining = $gethSyncStatus?.estimated_seconds_remaining ?? null
  let lastSyncNotificationShown = false


  // Temporary account tracking
  let isTemporaryAccount = false
  let showTemporaryAccountWarning = false 

  // Network statistics
  let networkHashRate = '0 H/s'
  let networkDifficulty = '0'
  let peerCount = 0

  // Statistics - preserve across page navigation
  let sessionStartTime = $miningState.isMining ? 
    $miningState.sessionStartTime || Date.now() : 
    Date.now()
  let estimatedTimeToBlock = 0
  $: powerConsumption = hasRealPower ? realPowerConsumption : 0
  $: efficiency = ($miningState.hashRate === '0 H/s' || !hasRealPower) ? 0 : parseHashRate($miningState.hashRate) / powerConsumption
  let temperature = 0.0
  let hasRealTemperature = false
  let temperatureLoading = true // Add loading state for temperature checks
  let hasCompletedFirstCheck = false // Track if we've completed the first temperature check
  let temperatureUnit: 'C' | 'F' = 'C'

  // Power monitoring
  let realPowerConsumption = 0.0
  let hasRealPower = false
  let powerLoading = true // Add loading state for power checks

  // Profitability Calculator state
  let showCalculator = false
  let electricityCostPerKwh = 0.12 // Default: $0.12 per kWh (US average)
  let estimatedPowerUsageWatts = 100 // Default: 100W
  let chiralPriceUSD = 0.50 // Default: $0.50 per Chiral (user can adjust)
  let calculatorHashRate = 0 // Will use current mining hash rate
  let useCurrentHashRate = true

  // Profitability calculation results
  $: {
    // Use current hash rate if mining, otherwise use manual input
    if (useCurrentHashRate && $miningState.isMining) {
      calculatorHashRate = parseHashRate($miningState.hashRate)
    }
  }

  $: dailyBlocks = calculateDailyBlocks(calculatorHashRate, parseDifficulty(networkDifficulty))
  $: dailyRevenue = dailyBlocks * $blockReward * chiralPriceUSD
  $: dailyPowerCostKwh = (estimatedPowerUsageWatts / 1000) * 24
  $: dailyPowerCostUSD = dailyPowerCostKwh * electricityCostPerKwh
  $: dailyProfit = dailyRevenue - dailyPowerCostUSD
  $: monthlyProfit = dailyProfit * 30
  $: yearlyProfit = dailyProfit * 365
  // $: breakEvenDays = dailyProfit > 0 ? 0 : Infinity // No upfront hardware cost in this model (unused)
  $: profitMargin = dailyRevenue > 0 ? ((dailyProfit / dailyRevenue) * 100) : 0
  $: isProfitable = dailyProfit > 0

  // Track if we've synced with accurate totals to avoid repeated fetches
  let hasSyncedWithAccurateTotals = false;

  // Reset sync flag when accurateTotals is cleared (account change/logout)
  $: if (!$accurateTotals) {
    hasSyncedWithAccurateTotals = false;
  }

  // Re-fetch blocks count from backend when accurateTotals is calculated
  // This ensures the Mining page picks up the initialized backend counter
  // Note: We only sync if NOT actively mining, to avoid interrupting live updates
  $: if ($accurateTotals && !hasSyncedWithAccurateTotals && !$miningState.isMining && isTauri) {
    hasSyncedWithAccurateTotals = true;
    // Re-fetch from backend to get the initialized count
    // The backend counter is now properly initialized with historical + session blocks
    (async () => {
      try {
        const currentWallet = get(wallet);
        if (currentWallet?.address) {
          const blocksCount = await invoke<number>('get_blocks_mined', {
            address: currentWallet.address
          });
          const reward = get(blockReward) || 2;
          miningState.update((state) => ({
            ...state,
            blocksFound: blocksCount,
            totalRewards: blocksCount * reward,
          }));
        }
      } catch (error) {
        console.error('[Mining Page] Failed to sync blocks count after accurate totals:', error);
      }
    })();
  }

  function calculateDailyBlocks(hashRate: number, networkDiff: number): number {
    if (hashRate === 0 || networkDiff === 0) return 0
    
    // Simplified calculation: blocks per day = (your hashrate / network difficulty) * blocks per day
    // Assuming ~10 blocks per minute average (Bitcoin-like), ~14400 blocks per day
    const blocksPerDay = 14400
    const yourShare = hashRate / (networkDiff || 1)
    return yourShare * blocksPerDay
  }

  // Uptime tick (forces template to re-render every second while mining)
  let uptimeNow: number = Date.now()
  let uptimeInterval: number | null = null
  
  // Mining history is now stored in the miningState store
  // let recentBlocks: RecentBlock[] = []

  // Mock mining intervals
  let statsInterval: number | null = null
  let miningMonitorUnlisten: (() => void) | null = null
  let scanProgressUnlisten: (() => void) | null = null
  
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

  function parseDifficulty(difficultyStr: string): number {
    const match = difficultyStr.match(/^([\d.]+)\s*([KGMTP]?)H?\/?s?$/i)
    if (!match) return 0
    
    const value = parseFloat(match[1])
    const unit = match[2].toUpperCase()
    
    switch (unit) {
      case 'K': return value * 1000
      case 'M': return value * 1000000
      case 'G': return value * 1000000000
      case 'T': return value * 1000000000000
      case 'P': return value * 1000000000000000
      default: return value
    }
  }

  $: displayedTemperature = temperatureUnit === 'F' ? toFahrenheit(temperature).toFixed(1) : temperature.toFixed(1);

  function parseHashRate(rateStr: string): number {
    const match = rateStr.match(/^~?\s*([\d.]+)\s*([KMGT])H\/s$/i);
    
    // Fallback: If the regex fails (e.g., if the string is just "0 H/s"), return 0.
    if (!match) return 0;
    
    // Match indices: 1 is the value, 2 is the unit (K, M, G, T)
    const value = parseFloat(match[1]);
    const unit = match[2] ? match[2].toUpperCase() : ''; // Handle base H/s case if needed, though this regex forces a unit
    
    switch (unit) {
      case 'K': return value * 1000;
      case 'M': return value * 1000000;
      case 'G': return value * 1000000000;
      case 'T': return value * 1000000000000;
      default: return value; // Assumes base H/s if no unit is captured (e.g., if regex simplified)
    }
  }


  $: {
    const localHashRateNum = parseHashRate($miningState.hashRate);
    const networkDifficultyNum = parseDifficulty(networkDifficulty);
    
    if (localHashRateNum > 0 && networkDifficultyNum > 0) {
        // ETB = Difficulty / Hashrate (in seconds)
        estimatedTimeToBlock = networkDifficultyNum / localHashRateNum;
    } else {
        estimatedTimeToBlock = 0;
    }
  }



  // Function to convert Celsius to Fahrenheit
  function toFahrenheit(celsius: number): number {
    return (celsius * 9/5) + 32;
  }

  async function updateSyncStatus() {
    try {
      // Note: Sync status is already being reactively tracked via gethSyncStatus store
      // We only check for sync completion notification here
      const wasSyncing = isSyncing

      // Show notification when sync completes
      if (wasSyncing && !isSyncing && !lastSyncNotificationShown && $miningState.isMining) {
        showToast(tr('toasts.mining.syncComplete'), 'success')
        lastSyncNotificationShown = true
      }

      // Reset notification flag when syncing starts again
      if (isSyncing) {
        lastSyncNotificationShown = false
      }
    } catch (e) {
      console.error('Failed to get sync status:', e)
    }
  }

  function formatTimeRemaining(seconds: number | null): string {
    if (seconds === null || seconds === 0) return 'Complete'
    if (seconds < 60) return `${Math.round(seconds)}s`
    if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${Math.round(seconds % 60)}s`
    return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`
  }

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
  // Example: "INFO [09-14|16:32:29.577] Message..." → { prefix: 'INFO', rest: '[09-14|16:32:29.577] Message...' }
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

  // Decentralized Pool Mining State
  interface MiningPool {
    id: string;
    name: string;
    url: string;
    description: string;
    fee_percentage: number;
    miners_count: number;
    total_hashrate: string;
    last_block_time: number;
    blocks_found_24h: number;
    region: string;
    status: 'Active' | 'Maintenance' | 'Full' | 'Offline';
    min_payout: number;
    payment_method: string;
    created_by?: string;
    worker?: string;
    password?: string;
  }

  interface PoolStats {
    connected_miners: number;
    pool_hashrate: string;
    your_hashrate: string;
    your_share_percentage: number;
    shares_submitted: number;
    shares_accepted: number;
    estimated_payout_24h: number;
    last_share_time: number;
  }

  interface JoinedPoolInfo {
    pool: MiningPool;
    stats: PoolStats;
    joined_at: number;
  }

  // Pool state
  let availablePools: MiningPool[] = [];
  let currentPool: JoinedPoolInfo | null = null;
  let poolError: string = '';
  let isDiscovering: boolean = false;
  let showCreatePool: boolean = false;
  let showPoolList: boolean = false;
  let editingPool: MiningPool | null = null;
  let showPoolManager: boolean = false;
  let poolConnected: boolean = false;
  let poolStats = {
    connectedMiners: 0,
    poolHashrate: '0 H/s',
    yourHashrate: '0 H/s',
    yourSharePercentage: 0,
    sharesSubmitted: 0,
    sharesAccepted: 0,
    estimatedPayout24h: 0,
    lastShareTime: 0
  };
  
  // Pool creation form
  let newPool = {
    name: '',
    description: '',
    fee_percentage: 1.0,
    min_payout: 1.0,
    payment_method: 'PPLNS',
    region: 'Global',
    url: '',
    worker: '',
    password: ''
  };

  // Computed values
  $: poolConnected = currentPool !== null;
  $: if (currentPool) {
    poolStats = {
      connectedMiners: currentPool.stats.connected_miners,
      poolHashrate: currentPool.stats.pool_hashrate,
      yourHashrate: currentPool.stats.your_hashrate,
      yourSharePercentage: currentPool.stats.your_share_percentage,
      sharesSubmitted: currentPool.stats.shares_submitted,
      sharesAccepted: currentPool.stats.shares_accepted,
      estimatedPayout24h: currentPool.stats.estimated_payout_24h,
      lastShareTime: currentPool.stats.last_share_time
    };
  }

  const navigation = getContext('navigation') as { setCurrentPage: (page: string) => void };
  
  // Computed values for threads based on intensity
  const maxThreads = cpuThreads
  // Intensity slider directly controls selectedThreads
  $: if (!$miningState.isMining) {
    selectedThreads = Math.ceil(($miningState.minerIntensity / 100) * maxThreads)
  }


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

  // Track if backend has an active account
  let hasActiveAccount = false;

  let hoveredPoint: MiningHistoryPoint | null = null;
  let hoveredIndex: number | null = null;

  // Check if in client mode (forced OR automatic NAT-based)
  async function checkClientMode() {
    try {
      // Check if pure client mode is forced
      if ($settings.pureClientMode) {
        isClientMode = true;
        clientModeReason = "forced";
        return;
      }

      // Check DHT health to see if behind NAT
      const health = await dhtService.getHealth();
      if (health) {
        if (health.reachability === "private") {
          isClientMode = true;
          clientModeReason = "nat";
        } else if (health.reachability === "public") {
          isClientMode = false;
          clientModeReason = null;
        }
      }
    } catch (error) {
      console.error("Failed to check client mode:", error);
    }
  }

  onMount(async () => {
    try{
      getVersion()
      isTauri = true
    }
    catch{
      isTauri = false
    }

    // Check if account exists in backend
    if (isTauri) {
      try {
        hasActiveAccount = await invoke<boolean>("has_active_account");
      } catch (error) {
        console.error("Failed to check account status:", error);
        hasActiveAccount = false;
      }
    }

    await checkGethStatus()
    await updateNetworkStats()
    await checkClientMode()  // Check if in client mode (forced or NAT-based)

    // Always fetch initial mining stats (blocksFound, totalRewards) on mount
    if (isTauri) {
      // Clear any stale cache first
      await invoke('clear_blocks_cache');
      await walletService.refreshTransactions()
      await walletService.refreshBalance()

      // Trigger accurate totals calculation if not already available or calculating
      // This ensures "mined rewards" shows accurate blockchain data, not just session data
      if (!$accurateTotals && !$isCalculatingAccurateTotals) {
        walletService.calculateAccurateTotals().catch((error) => {
          console.warn('[Mining Page] Failed to calculate accurate totals:', error);
        });
      }

      // Start power sensor detection
      await updatePowerConsumption()

      // Start mining monitor for real-time updates
      try {
        await invoke('start_mining_monitor', { dataDir: './bin/geth-data' });

        // Listen for block mined events
        const unlistenBlockMined = await listen('block_mined', async () => {
          // Immediately update blocks found counter from backend
          try {
            const currentWallet = get(wallet);
            if (currentWallet?.address) {
              const blocksCount = await invoke<number>('get_blocks_mined', {
                address: currentWallet.address
              });
              miningState.update((state) => ({
                ...state,
                blocksFound: blocksCount,
                totalRewards: blocksCount * (get(blockReward) || 2)
              }));
            }
          } catch (error) {
            console.error('[Mining Page] Failed to update blocks count:', error);
          }

          // Wait minimal time for block propagation, then refresh mining stats and balance
          setTimeout(async () => {
            try {
              await walletService.refreshTransactions();
              // Refresh balance to update wallet.balance with new mining rewards
              await walletService.refreshBalance();
            } catch (error) {
              console.error('[Mining Page] Backend refresh failed:', error);
            }
          }, 500); // Wait only 500ms since we know the exact block
        });

        // Note: mining_scan_progress events are no longer used for UI updates
        // We now rely on get_blocks_mined (backend counter) as the single source of truth
        // This avoids double-counting issues between the scan counter and the mined blocks counter

        // Store unlisten functions for cleanup
        miningMonitorUnlisten = unlistenBlockMined;
        scanProgressUnlisten = null;
      } catch (error) {
        console.error('[Mining Page] [X] FAILED to start mining monitor:', error);
      }

      // Fetch mining stats on page load
    }
    
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
      await updatePowerConsumption()
    }
    
    // Initialize pool state
    try {
      const poolInfo = await invoke('get_current_pool_info') as JoinedPoolInfo | null;
      if (poolInfo) {
        currentPool = poolInfo;
        startPoolStatsUpdates();
      }
    } catch (e) {
      console.error('Failed to get current pool info:', e);
    }
    
    // Start polling for system stats (power, temperature, network)
    // Power usage should be shown permanently, not just when mining
    statsInterval = setInterval(async () => {
      if ($miningState.isMining) {
        // Update real-time mining stats (hashrate, etc.)
        await updateMiningStats();
      }
      await updateNetworkStats();
      await updateSyncStatus(); // Check blockchain sync status
      if (isTauri) {
        await updateCpuTemperature();
        await updatePowerConsumption(); // Update power consumption continuously
      }
    }, 10000) as unknown as number; // Poll every 10 seconds for more responsive power updates
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
      // Check if Geth is running before making blockchain calls
      const gethRunning = await invoke<boolean>('is_geth_running');
      if (!gethRunning) {
        return; // Silently skip if Geth is not running
      }

      const [rate, block] = await Promise.all([
        invoke('get_miner_hashrate') as Promise<string>,
        invoke('get_current_block') as Promise<number>
      ]);

      currentBlock = block

      // Always try to get continuous hash rate estimates when mining
      let finalHashRate = rate;

      // If RPC returns 0 but we're mining, show "Mining Active"
      if (rate === '0 H/s' && $miningState.isMining) {
        finalHashRate = "Mining Active";
      }


      if ($miningState.isMining) {
        try {
          // Get continuous hash rate estimates from backend
          const [, hashRateFromLogs] = await invoke('get_miner_performance', {
            dataDir: './bin/geth-data'
          }) as [number, number]

          // Handle backend response properly

          if (hashRateFromLogs === 0) {
            // Mining active but no blocks mined yet - show status
            finalHashRate = $miningState.hashRate === "0 H/s" ? "Mining Active" : $miningState.hashRate;
          } else {
            // Have actual hash rate data from block mining
            finalHashRate = formatHashRate(hashRateFromLogs);
          }

        } catch (perfError) {
          console.log('Performance fetch failed:', perfError);
          // If performance fetch fails, keep current rate
          finalHashRate = $miningState.hashRate || rate;
        }
      }
      
      $miningState.hashRate = finalHashRate;

      // Force reactivity by triggering store update every time
      miningState.set($miningState);

      
      // Convert hashRate string to number for chart
      let hashRateNum = 0
      // Clean up the rate string (remove ~ and text in parentheses)
      hashRateNum = parseHashRate($miningState.hashRate)
      
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
      
      // Mining stats are now updated only when blocks are mined (not polled)
      // This avoids expensive blockchain scans during regular polling
      
      const results = await Promise.all(promises)

      networkDifficulty = results[0][0]
      networkHashRate = results[0][1]
      currentBlock = results[1]
      peerCount = results[2]
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

  async function updatePowerConsumption() {
    try {
      const power = await invoke('get_power_consumption') as number
      if (power && power > 0) {
        realPowerConsumption = power
        hasRealPower = true
      } else {
        hasRealPower = false
      }
    } catch (e) {
      console.error('Failed to get power consumption:', e)
      hasRealPower = false
    } finally {
      powerLoading = false
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
  
  async function createTemporaryAccount() {
    try {
      // Generate a temporary account using walletService
      // This already sets both backend and frontend account state
      const tempAccount = await walletService.createAccount()

      // Mark as temporary
      isTemporaryAccount = true
      showTemporaryAccountWarning = true

      return tempAccount
    } catch (e) {
      console.error('Failed to create temporary account:', e)
      throw e
    }
  }

  async function startMining() {
    // Auto-create temporary account if none exists
    if (isTauri) {
      try {
        const hasAccount = await invoke<boolean>("has_active_account");
        if (!hasAccount) {
          try {
            await createTemporaryAccount()
          } catch (e) {
            error = $t('mining.errors.noAccount')
            return
          }
        }
      } catch (error) {
        console.error("Failed to verify account status:", error);
        error = $t('mining.errors.noAccount')
        return;
      }
    }

    if (!isGethRunning) {
      error = $t('mining.errors.gethNotRunning')
      return
    }

    // Check if blockchain is still syncing (only when Geth is running)
    if (isGethRunning && isSyncing) {
      error = `Cannot start mining while blockchain is syncing (${syncProgress.toFixed(1)}% complete). Please wait for sync to finish.`
      return
    }

    error = ''
    validationError = null
    
    try {
      // Get account address from backend
      const accountAddress = await invoke<string>("get_active_account_address");
      
      await invoke('start_miner', {
        address: accountAddress,
        threads: selectedThreads,
        dataDir: './bin/geth-data'
      })
      $miningState.isMining = true
      sessionStartTime = Date.now()
      // Store session start time in the store for persistence
      $miningState.sessionStartTime = sessionStartTime
      $miningState.activeThreads = selectedThreads  // Use selectedThreads
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

  // Temporary account warning handlers
  function handleDismissWarning() {
    showTemporaryAccountWarning = false
  }

  function handleCreateWallet() {
    // Navigate to Account page to create a permanent wallet
    goto('/account')
  }

  // Decentralized Pool Functions
  async function discoverPools() {
    if (isTauri) {
      try {
        const hasAccount = await invoke<boolean>("has_active_account");
        if (!hasAccount) {
          poolError = 'Please create or import an account first.';
          return;
        }
      } catch (error) {
        console.error("Failed to verify account status:", error);
        poolError = 'Please create or import an account first.';
        return;
      }
    }
    
    isDiscovering = true;
    poolError = '';
    
    try {
      const pools = await invoke('discover_mining_pools') as MiningPool[];
      availablePools = pools;
      showPoolList = true;
      await invoke('update_pool_discovery'); // Update pool stats
    } catch (e) {
      console.error('[X] Pool discovery error:', e);
      poolError = String(e);
    } finally {
      isDiscovering = false;
    }
  }

  async function joinPool(pool: MiningPool) {
    if (isTauri) {
      try {
        const hasAccount = await invoke<boolean>("has_active_account");
        if (!hasAccount) {
          poolError = 'Please create or import an account first.';
          return;
        }
      } catch (error) {
        console.error("Failed to verify account status:", error);
        poolError = 'Please create or import an account first.';
        return;
      }
    }
    
    if (pool.status === 'Offline') {
      poolError = 'Cannot join offline pool.';
      return;
    }
    
    poolError = '';
    
    try {
      // Get account address from backend
      const accountAddress = await invoke<string>("get_active_account_address");
      
      const joinedInfo = await invoke('join_mining_pool', { 
        poolId: pool.id, 
        address: accountAddress
      }) as JoinedPoolInfo;
      
      currentPool = joinedInfo;
      showPoolList = false;
      
      // Start periodic stats updates
      startPoolStatsUpdates();
    } catch (e) {
      poolError = String(e);
    }
  }

  async function leavePool() {
    if (!currentPool) return;
    
    try {
      await invoke('leave_mining_pool');
      currentPool = null;
      stopPoolStatsUpdates();
    } catch (e) {
      poolError = String(e);
    }
  }

  async function createNewPool() {
    if (isTauri) {
      try {
        const hasAccount = await invoke<boolean>("has_active_account");
        if (!hasAccount) {
          poolError = 'Please create or import an account first.';
          return;
        }
      } catch (error) {
        console.error("Failed to verify account status:", error);
        poolError = 'Please create or import an account first.';
        return;
      }
    }
    
    if (!newPool.name.trim()) {
      poolError = 'Pool name is required.';
      return;
    }
    if (!newPool.url.trim()) {
      poolError = 'Pool URL is required.';
      return;
    }
    
    poolError = '';
    
    try {
      // Get account address from backend
      const accountAddress = await invoke<string>("get_active_account_address");
      
      const createdPool = await invoke('create_mining_pool', {
        address: accountAddress,
        name: newPool.name,
        url: newPool.url,
        description: newPool.description,
        feePercentage: newPool.fee_percentage,
        minPayout: newPool.min_payout,
        paymentMethod: newPool.payment_method,
        region: newPool.region,
      }) as MiningPool;
      
      // Reset form
      newPool = {
        name: '',
        description: '',
        fee_percentage: 1.0,
        min_payout: 1.0,
        payment_method: 'PPLNS',
        region: 'Global',
        url: '',
        worker: '',
        password: ''
      };
      
      showCreatePool = false;
      
      // Automatically join the created pool
      await joinPool(createdPool);
    } catch (e) {
      poolError = String(e);
    }
  }

  // Pool stats updates
  let poolStatsInterval: number | null = null;
  
  function startPoolStatsUpdates() {
    if (poolStatsInterval) return;
    
    poolStatsInterval = setInterval(async () => {
      if (!currentPool) {
        stopPoolStatsUpdates();
        return;
      }
      
      try {
        const stats = await invoke('get_pool_stats') as PoolStats;
        if (stats && currentPool) {
          currentPool.stats = stats;
          currentPool = { ...currentPool }; // Trigger reactivity
        }
      } catch (e) {
        console.error('Failed to update pool stats:', e);
      }
    }, 5000) as unknown as number; // Update every 5 seconds
  }
  
  function stopPoolStatsUpdates() {
    if (poolStatsInterval) {
      clearInterval(poolStatsInterval);
      poolStatsInterval = null;
    }
  }

  function getPoolStatusColor(status: string): string {
    switch (status) {
      case 'Active': return 'text-green-500';
      case 'Maintenance': return 'text-yellow-500';
      case 'Full': return 'text-orange-500';
      case 'Offline': return 'text-red-500';
      default: return 'text-gray-500';
    }
  }

  function formatTimestamp(timestamp: number): string {
    const now = Date.now() / 1000;
    const diff = now - timestamp;
    
    if (diff < 60) return `${Math.floor(diff)}s ago`;
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
  }

  // Pool management functions
  function editPool(pool: MiningPool) {
    editingPool = { ...pool };
    showPoolManager = true;
  }

  function cancelPoolEdit() {
    editingPool = null;
    showPoolManager = false;
  }

  function deletePool(poolId: string) {
    // Remove pool from available pools
    availablePools = availablePools.filter(p => p.id !== poolId);
    
    // If this was the current pool, leave it
    if (currentPool && currentPool.pool.id === poolId) {
      leavePool();
    }
  }

  // Save changes to an existing pool (edit functionality)
  function savePool() {
    if (!editingPool) return;
    
    // Store the editing pool in a local variable to satisfy TypeScript
    const poolToEdit = editingPool;
    
    // Find and update the pool in availablePools
    availablePools = availablePools.map(pool =>
      pool.id === poolToEdit.id
        ? {
            ...pool,
            ...poolToEdit,
            id: poolToEdit.id ?? pool.id, // Ensure id is always a string
            name: poolToEdit.name ?? pool.name,
            url: poolToEdit.url ?? pool.url,
            description: poolToEdit.description ?? pool.description,
            fee_percentage: poolToEdit.fee_percentage ?? pool.fee_percentage,
            miners_count: poolToEdit.miners_count ?? pool.miners_count,
            total_hashrate: poolToEdit.total_hashrate ?? pool.total_hashrate,
            last_block_time: poolToEdit.last_block_time ?? pool.last_block_time,
            blocks_found_24h: poolToEdit.blocks_found_24h ?? pool.blocks_found_24h,
            region: poolToEdit.region ?? pool.region,
            status: poolToEdit.status ?? pool.status,
            min_payout: poolToEdit.min_payout ?? pool.min_payout,
            payment_method: poolToEdit.payment_method ?? pool.payment_method,
            created_by: poolToEdit.created_by ?? pool.created_by,
            worker: poolToEdit.worker ?? pool.worker ?? '',
            password: poolToEdit.password ?? pool.password ?? ''
          }
        : pool
    );
    editingPool = null;
    showPoolManager = false;
  }

  // Add a new pool (for modal form)
  async function addPool() {
    // Basic validation
    if (!newPool.name.trim()) return;
    
    // Get account address from backend
    let createdByAddress = '';
    if (isTauri) {
      try {
        createdByAddress = await invoke<string>("get_active_account_address");
      } catch (error) {
        console.error("Failed to get account address:", error);
      }
    }
    
    // Generate a unique id for the new pool
    const id = `pool-${Date.now()}`;
    availablePools = [
      ...availablePools,
      {
        ...newPool,
        id,
        status: 'Active',
        miners_count: 1,
        total_hashrate: '0 H/s',
        last_block_time: 0,
        blocks_found_24h: 0,
        created_by: createdByAddress,
      }
    ];
    // Reset form and close modal
    newPool = {
      name: '',
      description: '',
      fee_percentage: 1.0,
      min_payout: 1.0,
      payment_method: 'PPLNS',
      region: 'Global',
      url: '',
      worker: '',
      password: ''
    };
    showPoolManager = false;
  }

  // Define pools variable as an alias for availablePools for compatibility
  $: pools = availablePools;

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
  
  function formatUptime(now: number = Date.now()) {
    const uptime = now - sessionStartTime
    const hours = Math.floor(uptime / 3600000)
    const minutes = Math.floor((uptime % 3600000) / 60000)
    const seconds = Math.floor((uptime % 60000) / 1000)
    return `${hours}h ${minutes}m ${seconds}s`
  }

  function formatTimeFromSeconds(totalSeconds: number): string {
      if (totalSeconds < 60) {
          return `${totalSeconds.toFixed(0)}s`;
      }

      const seconds = Math.floor(totalSeconds);
      const hours = Math.floor(seconds / 3600);
      const minutes = Math.floor((seconds % 3600) / 60);
      const remainingSeconds = seconds % 60;

      let parts = [];
      if (hours > 0) {
          parts.push(`${hours}h`);
      }
      if (minutes > 0) {
          parts.push(`${minutes}m`);
      }
      // Only show seconds if the total time is less than an hour, or if minutes is 0
      if (hours === 0 && minutes < 5) { // Show seconds when time is short
          parts.push(`${remainingSeconds}s`);
      }

      return parts.join(' ');
  }

  function formatHashRate(rate: number | string): string {
    if (typeof rate === 'string') return rate
    if (rate < 1000) return `${rate.toFixed(1)} H/s`
    if (rate < 1000000) return `${(rate / 1000).toFixed(2)} KH/s`
    if (rate < 1000000000) return `${(rate / 1000000).toFixed(2)} MH/s`
    return `${(rate / 1000000000).toFixed(2)} GH/s`
  }

  function formatDifficulty(difficulty: string | number): string {
    if (typeof difficulty === 'string') return difficulty
    if (difficulty < 1000) return `${difficulty.toFixed(0)} H/s`
    if (difficulty < 1000000) return `${(difficulty / 1000).toFixed(2)} KH/s`
    if (difficulty < 1000000000) return `${(difficulty / 1000000).toFixed(2)} MH/s`
    if (difficulty < 1000000000000) return `${(difficulty / 1000000000).toFixed(2)} GH/s`
    return `${(difficulty / 1000000000000).toFixed(2)} TH/s`
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
  // Remove old pool management code - replaced with decentralized system
  
  onDestroy(async () => {
    // Don't stop mining when leaving the page - preserve state
    // Only clean up intervals
    if (statsInterval) {
      clearInterval(statsInterval)
    }
    if (uptimeInterval) {
      clearInterval(uptimeInterval)
    }
    if (miningMonitorUnlisten) {
      miningMonitorUnlisten();
    }
    if (scanProgressUnlisten) {
      scanProgressUnlisten();
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

  <!-- Client Mode Warning -->
  {#if isClientMode}
    <Card class="p-4 bg-yellow-50 border-yellow-200">
      <div class="flex items-start gap-3">
        <div class="text-yellow-600 mt-0.5">
          [WARN]
        </div>
        <div class="flex-1 space-y-2">
          <p class="text-sm font-semibold text-yellow-800">
            {#if clientModeReason === "forced"}
              {$t('pureClientMode.banner.title.forcedMining')}
            {:else if clientModeReason === "nat"}
              {$t('pureClientMode.banner.title.natMining')}
            {:else}
              {$t('pureClientMode.banner.title.defaultMining')}
            {/if}
          </p>
          <p class="text-sm text-yellow-700">
            {#if clientModeReason === "forced"}
              {$t('pureClientMode.warnings.forcedModeMining')}
            {:else if clientModeReason === "nat"}
              {$t('pureClientMode.warnings.natDetectedMining')}
            {:else}
              {$t('pureClientMode.banner.description.defaultMining')}
            {/if}
          </p>
          <p class="text-xs text-yellow-600">
            {#if clientModeReason === "forced"}
              {$t('pureClientMode.banner.help.forcedMining')}
            {:else if clientModeReason === "nat"}
              {$t('pureClientMode.banner.help.natMining')}
            {:else}
              {$t('pureClientMode.banner.help.defaultMining')}
            {/if}
          </p>
        </div>
      </div>
    </Card>
  {/if}

  <!-- Temporary Account Warning -->
  {#if showTemporaryAccountWarning && isTemporaryAccount}
    <TemporaryAccountWarning
      onDismiss={handleDismissWarning}
      onCreateWallet={handleCreateWallet}
    />
  {/if}

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
          <p class="text-2xl font-bold">{($miningState.totalRewards || 0).toFixed(4)} Chiral</p>
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
          {#if powerLoading}
            <p class="text-2xl font-bold text-blue-500">--W</p>
            <p class="text-xs text-muted-foreground mt-1">Detecting power sources...</p>
          {:else if hasRealPower}
            <p class="text-2xl font-bold">{powerConsumption.toFixed(0)}W</p>
            <p class="text-xs text-muted-foreground mt-1">
              {efficiency.toFixed(2)} {$t('mining.hw')}
            </p>
          {:else}
            <p class="text-2xl font-bold text-gray-500">N/A</p>
            <p class="text-xs text-muted-foreground mt-1">
              Hardware sensor not available
            </p>
          {/if}
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
            <p class="text-2xl font-bold {temperature > 80 ? 'text-red-500' : temperature > 70 ? 'text-orange-500' : temperature > 60 ? 'text-yellow-500' : 'text-green-500'}">{displayedTemperature}°{temperatureUnit}</p>
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
                {temperature > 85 ? $t('mining.temperatureStatus.critical') : temperature > 75 ? $t('mining.temperatureStatus.hot') : temperature > 65 ? $t('mining.temperatureStatus.warm') : $t('mining.temperatureStatus.normal')}
              </p>
            {:else}
              <Progress value={0} max={100} class="h-2 bg-gray-200 [&>div]:bg-gray-400 [&>div]:opacity-50" />
              <p class="text-xs text-muted-foreground mt-1">Hardware sensor not available</p>
            {/if}
          </div>
        </div>
        <div class="flex flex-col gap-2">
            <Button size="icon" variant="outline" onclick={() => temperatureUnit = temperatureUnit === 'C' ? 'F' : 'C'}>
                {temperatureUnit === 'C' ? '°F' : '°C'}
            </Button>
            <div class="p-2 {temperatureLoading ? 'bg-blue-500/20' : hasRealTemperature ? (temperature > 80 ? 'bg-red-500/20' : temperature > 70 ? 'bg-orange-500/20' : temperature > 60 ? 'bg-yellow-500/20' : 'bg-green-500/20') : 'bg-gray-500/20'} rounded-lg">
                <Thermometer class="h-5 w-5 {temperatureLoading ? 'text-blue-500 animate-pulse' : hasRealTemperature ? (temperature > 80 ? 'text-red-500' : temperature > 70 ? 'text-orange-500' : temperature > 60 ? 'text-yellow-500' : 'text-green-500') : 'text-gray-500'}" />
            </div>
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
      <!-- Decentralized Pool Mining Section -->
      <div class="col-span-full">
        <div class="bg-muted/30 rounded-lg p-4 space-y-4">
          <div class="flex items-center justify-between">
            <h3 class="text-lg font-semibold">{$t('mining.decentralizedPools')}</h3>
            <div class="flex gap-2">
              <Button variant="outline" size="sm" onclick={discoverPools} disabled={isDiscovering || $miningState.isMining}>
                {#if isDiscovering}
                  <RefreshCw class="h-4 w-4 mr-2 animate-spin" />
                  {$t('mining.discovering')}
                {:else}
                  <RefreshCw class="h-4 w-4 mr-2" />
                  {$t('mining.discoverPools')}
                {/if}
              </Button>
              <Button variant="secondary" size="sm" onclick={() => showCreatePool = true} disabled={$miningState.isMining}>
                <Coins class="h-4 w-4 mr-2" />
                {$t('mining.createPool')}
              </Button>
            </div>
          </div>

          {#if currentPool}
            <!-- Currently Connected Pool -->
            <div class="bg-green-500/10 border border-green-500/20 rounded-lg p-4">
              <div class="flex items-center justify-between mb-3">
                <div class="flex items-center gap-2">
                  <div class="w-3 h-3 bg-green-500 rounded-full"></div>
                  <h4 class="font-semibold text-green-700">{$t('mining.poolDetails.connectedTo')}: {currentPool.pool.name}</h4>
                </div>
                <Button variant="destructive" size="sm" onclick={leavePool} disabled={$miningState.isMining}>
                  {$t('mining.poolDetails.leave')}
                </Button>
              </div>

              <div class="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
                <div>
                  <p class="text-muted-foreground">{$t('mining.poolDetails.yourHashrate')}</p>
                  <p class="font-semibold">{currentPool.stats.your_hashrate}</p>
                </div>
                <div>
                  <p class="text-muted-foreground">{$t('mining.poolDetails.poolHashrate')}</p>
                  <p class="font-semibold">{currentPool.stats.pool_hashrate}</p>
                </div>
                <div>
                  <p class="text-muted-foreground">{$t('mining.poolDetails.yourShare')}</p>
                  <p class="font-semibold">{currentPool.stats.your_share_percentage.toFixed(2)}%</p>
                </div>
                <div>
                  <p class="text-muted-foreground">{$t('mining.poolDetails.est24hPayout')}</p>
                  <p class="font-semibold">{currentPool.stats.estimated_payout_24h.toFixed(4)} Chiral</p>
                </div>
                <div>
                  <p class="text-muted-foreground">{$t('mining.poolDetails.shares')}</p>
                  <p class="font-semibold">{currentPool.stats.shares_accepted}/{currentPool.stats.shares_submitted}</p>
                </div>
                <div>
                  <p class="text-muted-foreground">{$t('mining.poolDetails.connectedMiners')}</p>
                  <p class="font-semibold">{currentPool.stats.connected_miners}</p>
                </div>
                <div>
                  <p class="text-muted-foreground">{$t('mining.poolDetails.poolFee')}</p>
                  <p class="font-semibold">{currentPool.pool.fee_percentage}%</p>
                </div>
                <div>
                  <p class="text-muted-foreground">{$t('mining.poolDetails.lastShare')}</p>
                  <p class="font-semibold">{formatTimestamp(currentPool.stats.last_share_time)}</p>
                </div>
              </div>
            </div>
          {:else}
            <!-- Solo Mining Status -->
            <div class="bg-blue-500/10 border border-blue-500/20 rounded-lg p-4">
              <div class="flex items-center gap-2">
                <div class="w-3 h-3 bg-blue-500 rounded-full"></div>
                <h4 class="font-semibold text-blue-700">{$t('mining.poolDetails.soloMining')}</h4>
              </div>
              <p class="text-sm text-muted-foreground mt-2">
                {$t('mining.poolDetails.soloMiningDesc')}
              </p>
            </div>
          {/if}

          {#if poolError}
            <div class="bg-red-500/10 border border-red-500/20 rounded-lg p-3">
              <p class="text-sm text-red-600">{poolError}</p>
            </div>
          {/if}
        </div>
      </div>

      <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
        
        <div>
          <Label for="thread-count">{$t('mining.cpuThreads', { values: { cpuThreads } })}</Label>
          <Input
                  id="thread-count"
                  type="number"
                  bind:value={selectedThreads}
                  oninput={(e: Event) => {
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
                  oninput={(e: Event) => {
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
            onclick={() => $miningState.isMining ? stopMining() : startMining()}
            class="min-w-[150px]"
            disabled={isInvalid || !isGethRunning || isSyncing}
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
            onclick={toggleLogs}
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
              {$t('mining.errors.gethNotRunning')} <button onclick={() => { navigation.setCurrentPage('network'); goto('/network'); }} class="underline font-medium">{$t('mining.networkPage')}</button>
            </p>
          </div>
        </div>
      {/if}
      {#if !hasActiveAccount && isGethRunning}
        <div class="bg-blue-500/10 border border-blue-500/20 rounded-lg p-3 mt-2">
          <div class="flex items-center gap-2">
            <AlertCircle class="h-4 w-4 text-blue-500 flex-shrink-0" />
                        <p class="text-sm text-blue-600">
                          {@html $t('mining.errors.noAccountToStart', { values: { link: '<a href="/account" class="underline font-medium">' + $t('mining.accountPage') + '</a>' } })}
                        </p>
          </div>
        </div>
      {/if}

      <!-- Blockchain Sync Status -->
      {#if isGethRunning && isSyncing}
        <div class="bg-blue-500/10 border border-blue-500/20 rounded-lg p-4 mt-2">
          <div class="space-y-3">
            <div class="flex items-center justify-between">
              <div class="flex items-center gap-2">
                <RefreshCw class="h-4 w-4 text-blue-500 animate-spin" />
                <span class="text-sm font-medium text-blue-600">Blockchain Syncing...</span>
              </div>
              <span class="text-xs text-blue-600">{syncProgress.toFixed(1)}%</span>
            </div>
            <Progress value={syncProgress} max={100} class="h-2 [&>div]:bg-blue-500" />
            <div class="grid grid-cols-2 gap-4 text-xs text-blue-600">
              <div>
                <span class="text-muted-foreground">Current:</span> #{syncCurrentBlock.toLocaleString()}
              </div>
              <div>
                <span class="text-muted-foreground">Highest:</span> #{syncHighestBlock.toLocaleString()}
              </div>
              <div>
                <span class="text-muted-foreground">Remaining:</span> {syncBlocksRemaining.toLocaleString()} blocks
              </div>
              <div>
                <span class="text-muted-foreground">ETA:</span> {formatTimeRemaining(syncEstimatedSecondsRemaining)}
              </div>
            </div>
            <p class="text-xs text-blue-600 mt-2">
              [WAIT] Mining will begin automatically when sync completes
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
          <Badge variant="outline">{$blockReward} Chiral</Badge>
        </div>
        <div class="flex justify-between items-center">
          <span class="text-sm text-muted-foreground">{$t('mining.estTimeToBlock')}</span>
          <Badge variant="outline">
            {estimatedTimeToBlock > 0 ?
              `~${formatTimeFromSeconds(estimatedTimeToBlock)}` : $t('mining.calculating')}
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

  <!-- Mining Profitability Calculator -->
  <Card class="p-6">
    <div class="flex items-center justify-between mb-4">
      <div class="flex items-center gap-2">
        <Calculator class="w-5 h-5" />
        <h2 class="text-lg font-semibold">{$t('mining.calculator.title')}</h2>
      </div>
      <button
        onclick={() => showCalculator = !showCalculator}
        class="px-3 py-1 rounded bg-primary text-primary-foreground hover:bg-primary/90 transition-colors text-sm"
      >
        {showCalculator ? $t('mining.calculator.hide') : $t('mining.calculator.show')} {$t('mining.calculator.title')}
      </button>
    </div>

    {#if showCalculator}
      <div class="space-y-4">
        <!-- Input Section -->
        <div class="grid grid-cols-1 md:grid-cols-2 gap-4 pb-4 border-b">
          <div>
            <label for="electricity-cost" class="block text-sm font-medium mb-1.5">
              {$t('mining.calculator.electricityCost')}
            </label>
            <input
              id="electricity-cost"
              type="number"
              bind:value={electricityCostPerKwh}
              step="0.01"
              min="0"
              class="w-full px-3 py-2 rounded border bg-background"
              placeholder="0.12"
            />
            <p class="text-xs text-muted-foreground mt-1">{$t('mining.calculator.electricityCostHint')}</p>
          </div>

          <div>
            <label for="power-usage" class="block text-sm font-medium mb-1.5">
              {$t('mining.calculator.powerUsage')}
            </label>
            <input
              id="power-usage"
              type="number"
              bind:value={estimatedPowerUsageWatts}
              step="10"
              min="0"
              class="w-full px-3 py-2 rounded border bg-background"
              placeholder="100"
            />
            <p class="text-xs text-muted-foreground mt-1">{$t('mining.calculator.powerUsageHint')}</p>
          </div>

          <div>
            <label for="chiral-price" class="block text-sm font-medium mb-1.5">
              <DollarSign class="w-3 h-3 inline" /> {$t('mining.calculator.chiralPrice')}
            </label>
            <input
              id="chiral-price"
              type="number"
              bind:value={chiralPriceUSD}
              step="0.01"
              min="0"
              class="w-full px-3 py-2 rounded border bg-background"
              placeholder="0.50"
            />
            <p class="text-xs text-muted-foreground mt-1">{$t('mining.calculator.chiralPriceHint')}</p>
          </div>

          <div>
            <label for="calculator-hashrate" class="block text-sm font-medium mb-1.5">
              {$t('mining.calculator.hashRate')}
            </label>
            <div class="flex gap-2">
              <input
                id="calculator-hashrate"
                type="number"
                bind:value={calculatorHashRate}
                disabled={useCurrentHashRate}
                step="1000"
                min="0"
                class="flex-1 px-3 py-2 rounded border bg-background disabled:opacity-50"
                placeholder="10000"
              />
              <label class="flex items-center gap-1.5 px-3 py-2 rounded border bg-background cursor-pointer hover:bg-accent">
                <input
                  type="checkbox"
                  bind:checked={useCurrentHashRate}
                  onchange={() => {
                    if (useCurrentHashRate) {
                      calculatorHashRate = parseHashRate($miningState.hashRate);
                    }
                  }}
                  class="w-4 h-4"
                />
                <span class="text-sm whitespace-nowrap">{$t('mining.calculator.useCurrent')}</span>
              </label>
            </div>
            <p class="text-xs text-muted-foreground mt-1">
              {useCurrentHashRate ? $t('mining.calculator.usingCurrent') : $t('mining.calculator.enterCustom')}
            </p>
          </div>
        </div>

        <!-- Results Section -->
        <div class="space-y-3">
          <h3 class="text-sm font-semibold text-muted-foreground">{$t('mining.calculator.estimatedProfitability')}</h3>
          
          <div class="grid grid-cols-1 md:grid-cols-3 gap-3">
            <div class="p-3 rounded border bg-card">
              <p class="text-xs text-muted-foreground mb-1">{$t('mining.calculator.daily')}</p>
              <p class="text-lg font-semibold {isProfitable ? 'text-green-600' : 'text-red-600'}">
                ${dailyProfit.toFixed(2)}
              </p>
              <p class="text-xs text-muted-foreground mt-1">
                {$t('mining.calculator.revenue')}: ${dailyRevenue.toFixed(2)} - {$t('mining.calculator.cost')}: ${dailyPowerCostUSD.toFixed(2)}
              </p>
            </div>

            <div class="p-3 rounded border bg-card">
              <p class="text-xs text-muted-foreground mb-1">{$t('mining.calculator.monthly')}</p>
              <p class="text-lg font-semibold {isProfitable ? 'text-green-600' : 'text-red-600'}">
                ${monthlyProfit.toFixed(2)}
              </p>
              <p class="text-xs text-muted-foreground mt-1">
                ≈ {dailyBlocks.toFixed(2)} blocks/day × 30
              </p>
            </div>

            <div class="p-3 rounded border bg-card">
              <p class="text-xs text-muted-foreground mb-1">{$t('mining.calculator.yearly')}</p>
              <p class="text-lg font-semibold {isProfitable ? 'text-green-600' : 'text-red-600'}">
                ${yearlyProfit.toFixed(2)}
              </p>
              <p class="text-xs text-muted-foreground mt-1">
                ≈ {dailyBlocks.toFixed(2)} blocks/day × 365
              </p>
            </div>
          </div>

          <div class="flex items-center justify-between p-3 rounded border {isProfitable ? 'bg-green-50 dark:bg-green-950 border-green-200 dark:border-green-800' : 'bg-red-50 dark:bg-red-950 border-red-200 dark:border-red-800'}">
            <div>
              <p class="text-sm font-medium {isProfitable ? 'text-green-900 dark:text-green-100' : 'text-red-900 dark:text-red-100'}">
                {isProfitable ? $t('mining.calculator.profitable') : $t('mining.calculator.notProfitable')}
              </p>
              <p class="text-xs {isProfitable ? 'text-green-700 dark:text-green-300' : 'text-red-700 dark:text-red-300'}">
                {isProfitable ? $t('mining.calculator.profitableDesc') : $t('mining.calculator.notProfitableDesc')}
              </p>
            </div>
            <div class="text-right">
              <p class="text-sm font-medium {isProfitable ? 'text-green-900 dark:text-green-100' : 'text-red-900 dark:text-red-100'}">
                {$t('mining.calculator.profitMargin')}
              </p>
              <p class="text-lg font-bold {isProfitable ? 'text-green-700 dark:text-green-300' : 'text-red-700 dark:text-red-300'}">
                {profitMargin.toFixed(1)}%
              </p>
            </div>
          </div>

          {#if calculatorHashRate > 0 && networkDifficulty}
            <div class="text-xs text-muted-foreground space-y-1 p-3 rounded bg-muted/30">
              <p><strong>{$t('mining.calculator.calculationDetails')}</strong></p>
              <p>• {$t('mining.calculator.yourHashrate')}: {formatHashRate(calculatorHashRate)}</p>
              <p>• {$t('mining.calculator.networkDiff')}: {formatDifficulty(networkDifficulty)}</p>
              <p>• {$t('mining.calculator.expectedBlocks')}: {dailyBlocks.toFixed(4)} ({$blockReward} {$t('mining.calculator.chiralEach')})</p>
              <p>• {$t('mining.calculator.powerConsumption')}: {dailyPowerCostKwh.toFixed(2)} {$t('mining.calculator.kwhPerDay')}</p>
              <p>• {$t('mining.calculator.breakEvenPrice')}: ${dailyPowerCostUSD > 0 ? (dailyPowerCostUSD / (dailyBlocks * $blockReward)).toFixed(4) : '0.0000'}{$t('mining.calculator.perChiral')}</p>
            </div>
          {:else}
            <div class="text-sm text-muted-foreground text-center py-4">
              {$t('mining.calculator.enableMining')}
            </div>
          {/if}
        </div>
      </div>
    {/if}
  </Card>
  
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
            <select id="page-size-select" bind:value={pageSize} onchange={() => { currentPage = 1 }} class="px-2 py-1 rounded border bg-background text-sm">
              {#each pageSizes as s}
                <option value={s}>{s}</option>
              {/each}
            </select>
          </div>

          <div class="flex items-center gap-2">
            <button class="px-2 py-1 rounded border bg-background text-sm" onclick={() => { if (currentPage > 1) currentPage -= 1 }} disabled={currentPage <= 1}>
              {$t('prev')}
            </button>
            <div class="text-sm text-muted-foreground">{currentPage} / {totalPages}</div>
            <button class="px-2 py-1 rounded border bg-background text-sm" onclick={() => { if (currentPage < totalPages) currentPage += 1 }} disabled={currentPage >= totalPages}>
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
                  +{block.reward.toFixed(4)} Chiral
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
        <Button size="sm" variant={chartType === 'bar' ? 'default' : 'outline'} onclick={() => chartType = 'bar'}>{$t('mining.bar')}</Button>
        <Button size="sm" variant={chartType === 'line' ? 'default' : 'outline'} onclick={() => chartType = 'line'}>{$t('mining.line')}</Button>
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
                  onmouseenter={() => { hoveredPoint = point; hoveredIndex = i; }}
                  onmouseleave={() => { hoveredPoint = null; hoveredIndex = null; }}
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
                    onmouseenter={() => { hoveredPoint = point; hoveredIndex = i; }}
                    onmouseleave={() => { hoveredPoint = null; hoveredIndex = null; }}
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
          <Button size="sm" variant="ghost" onclick={toggleLogs}>
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
            <input id="auto-refresh" type="checkbox" bind:checked={autoRefresh} onchange={() => {
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
                  onclick={() => logFilters[level] = !enabled}
                >
                  {level.toUpperCase()}
                </button>
              {/each}
            </div>
          </div>

          <div class="flex items-center gap-2">
            <Button size="sm" variant="outline" onclick={fetchLogs}>
              <RefreshCw class="h-3 w-3 mr-1" />
              {$t('mining.refresh')}
            </Button>
            <Button size="sm" variant="outline" onclick={() => logs = []}>
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
          <Button variant="ghost" size="sm" onclick={cancelPoolEdit}>
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
                </div>
              </div>
              <div class="flex gap-2">
                {#if pool.id !== 'solo'}
                  <Button
                    variant="outline"
                    size="sm"
                    onclick={() => editPool(pool)}
                  >
                    Edit
                  </Button>
                  <Button
                    variant="destructive"
                    size="sm"
                    onclick={() => deletePool(pool.id)}
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
                oninput={(e: Event) => {
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
                oninput={(e: Event) => {
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
                value={editingPool ? (editingPool.worker ?? '') : newPool.worker}
                oninput={(e: Event) => {
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
                value={editingPool ? (editingPool.password ?? '') : newPool.password}
                oninput={(e: Event) => {
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
              <Button onclick={savePool}>
                Save Changes
              </Button>
            {:else}
              <Button onclick={addPool}>
                Add Pool
              </Button>
            {/if}
            <Button variant="outline" onclick={cancelPoolEdit}>
              Cancel
            </Button>
          </div>
        </div>
      </div>
    </div>
  {/if}

  <!-- Old pool UI removed - using new decentralized system -->


<!-- Pool Discovery Modal -->
{#if showPoolList}
  <div class="fixed inset-0 bg-background/80 backdrop-blur-sm z-50 flex items-center justify-center p-4">
    <Card class="w-full max-w-4xl max-h-[80vh] flex flex-col">
      <div class="p-4 border-b flex items-center justify-between">
        <h2 class="text-lg font-semibold">{$t('mining.poolDetails.availablePools')}</h2>
        <Button size="sm" variant="ghost" onclick={() => showPoolList = false}>
          <X class="h-4 w-4" />
        </Button>
      </div>

      <div class="flex-1 overflow-y-auto p-4">
        {#if availablePools.length === 0}
          <div class="text-center py-8">
            <p class="text-muted-foreground">{$t('mining.poolDetails.noPoolsFound')}</p>
          </div>
        {:else}
          <div class="space-y-3">
            {#each availablePools as pool}
              <div class="border rounded-lg p-4 hover:bg-muted/50">
                <div class="flex items-start justify-between">
                  <div class="flex-1">
                    <div class="flex items-center gap-2 mb-2">
                      <h3 class="font-semibold">{pool.name}</h3>
                      <Badge variant="outline" class={getPoolStatusColor(pool.status)}>
                        {pool.status}
                      </Badge>
                      <Badge variant="secondary">{pool.region}</Badge>
                    </div>
                    
                    <p class="text-sm text-muted-foreground mb-3">{pool.description}</p>
                    
                    <div class="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
                      <div>
                        <p class="text-muted-foreground">{$t('mining.poolDetails.miners')}</p>
                        <p class="font-medium">{pool.miners_count}</p>
                      </div>
                      <div>
                        <p class="text-muted-foreground">{$t('mining.poolDetails.hashrate')}</p>
                        <p class="font-medium">{pool.total_hashrate}</p>
                      </div>
                      <div>
                        <p class="text-muted-foreground">{$t('mining.poolDetails.fee')}</p>
                        <p class="font-medium">{pool.fee_percentage}%</p>
                      </div>
                      <div>
                        <p class="text-muted-foreground">{$t('mining.poolDetails.lastBlock')}</p>
                        <p class="font-medium">{pool.last_block_time > 0 ? formatTimestamp(pool.last_block_time) : $t('mining.poolDetails.never')}</p>
                      </div>
                      <div>
                        <p class="text-muted-foreground">{$t('mining.poolDetails.blocks24h')}</p>
                        <p class="font-medium">{pool.blocks_found_24h}</p>
                      </div>
                      <div>
                        <p class="text-muted-foreground">{$t('mining.poolDetails.minPayout')}</p>
                        <p class="font-medium">{pool.min_payout} Chiral</p>
                      </div>
                      <div>
                        <p class="text-muted-foreground">{$t('mining.poolDetails.payment')}</p>
                        <p class="font-medium">{pool.payment_method}</p>
                      </div>
                      <div>
                        <p class="text-muted-foreground">{$t('mining.poolDetails.url')}</p>
                        <p class="font-mono text-xs">{pool.url}</p>
                      </div>
                    </div>
                  </div>

                  <div class="ml-4">
                    <Button
                      size="sm"
                      onclick={() => joinPool(pool)}
                      disabled={pool.status !== 'Active' || $miningState.isMining}
                    >
                      {$t('mining.poolDetails.join')}
                    </Button>
                  </div>
                </div>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    </Card>
  </div>
{/if}

<!-- Create Pool Modal -->
{#if showCreatePool}
  <div class="fixed inset-0 bg-background/80 backdrop-blur-sm z-50 flex items-center justify-center p-4">
    <Card class="w-full max-w-2xl">
      <div class="p-4 border-b flex items-center justify-between">
        <h2 class="text-lg font-semibold">{$t('mining.poolDetails.createNew')}</h2>
        <Button size="sm" variant="ghost" onclick={() => showCreatePool = false}>
          <X class="h-4 w-4" />
        </Button>
      </div>

      <div class="p-4 space-y-4">
        <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
          <div>
            <Label for="pool-name">{$t('mining.poolDetails.poolName')} *</Label>
            <Input
              id="pool-name"
              bind:value={newPool.name}
              placeholder={$t('mining.poolDetails.poolNamePlaceholder')}
              class="mt-2"
            />
          </div>
          <div>
            <Label for="pool-region">{$t('mining.poolDetails.region')}</Label>
            <select bind:value={newPool.region} class="w-full mt-2 px-3 py-2 border rounded-md bg-background">
              <option value="Global">Global</option>
              <option value="Americas">Americas</option>
              <option value="Europe">Europe</option>
              <option value="Asia">Asia</option>
              <option value="Oceania">Oceania</option>
            </select>
          </div>
        </div>
        
        <div>
          <Label for="pool-description">{$t('mining.poolDetails.description')}</Label>
          <Input
            id="pool-description"
            bind:value={newPool.description}
            placeholder={$t('mining.poolDetails.descriptionPlaceholder')}
            class="mt-2"
          />
        </div>

        <div>
          <Label for="pool-url">{$t('mining.poolDetails.url')} *</Label>
          <Input
            id="pool-url"
            bind:value={newPool.url}
            placeholder="stratum://pool.example.com:3333"
            class="mt-2"
          />
        </div>

        <div class="grid grid-cols-1 md:grid-cols-3 gap-4">
          <div>
            <Label for="pool-fee">{$t('mining.poolDetails.feePercentage')}</Label>
            <Input
              id="pool-fee"
              type="number"
              bind:value={newPool.fee_percentage}
              min="0"
              max="10"
              step="0.1"
              class="mt-2"
            />
          </div>
          <div>
            <Label for="pool-payout">{$t('mining.poolDetails.minPayoutChiral')}</Label>
            <Input
              id="pool-payout"
              type="number"
              bind:value={newPool.min_payout}
              min="0.1"
              step="0.1"
              class="mt-2"
            />
          </div>
          <div>
            <Label for="pool-payment">{$t('mining.poolDetails.paymentMethod')}</Label>
            <select bind:value={newPool.payment_method} class="w-full mt-2 px-3 py-2 border rounded-md bg-background">
              <option value="PPLNS">PPLNS</option>
              <option value="PPS">PPS</option>
              <option value="PPS+">PPS+</option>
              <option value="PROP">PROP</option>
            </select>
          </div>
        </div>
        
        <div class="bg-blue-500/10 border border-blue-500/20 rounded-lg p-3">
          <p class="text-sm text-blue-600">
            <strong>{$t('mining.poolDetails.note')}:</strong> {$t('mining.poolDetails.createNote')}
          </p>
        </div>

        <div class="flex justify-end gap-2 pt-4">
          <Button variant="outline" onclick={() => showCreatePool = false}>
            {$t('mining.poolDetails.cancel')}
          </Button>
          <Button onclick={createNewPool} disabled={!newPool.name.trim()}>
            {$t('mining.createPool')}
          </Button>
        </div>
      </div>
    </Card>
  </div>
{/if}
