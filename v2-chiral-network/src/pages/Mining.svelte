<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { goto } from '@mateothegreat/svelte5-router';
  import { walletAccount } from '$lib/stores';
  import { toasts } from '$lib/toastStore';
  import {
    Pickaxe,
    Square,
    Zap,
    TrendingUp,
    RefreshCw,
    AlertTriangle,
    Loader2,
    Globe,
    Gauge,
    Blocks,
    Coins,
    Clock,
    Cpu,
    History,
    ChevronDown,
    ChevronUp
  } from 'lucide-svelte';
  import { logger } from '$lib/logger';
  const log = logger('Mining');

  // Types
  interface GethStatus {
    installed: boolean;
    running: boolean;
    localRunning: boolean;
    syncing: boolean;
    currentBlock: number;
    highestBlock: number;
    peerCount: number;
    chainId: number;
  }

  interface MiningStatus {
    mining: boolean;
    hashRate: number;
    minerAddress: string | null;
    totalMinedWei: string;
    totalMinedChr: number;
  }

  interface MinedBlock {
    blockNumber: number;
    timestamp: number;
    rewardWei: string;
    rewardChr: number;
    difficulty: number;
  }

  // State
  let gethStatus = $state<GethStatus | null>(null);
  let miningStatus = $state<MiningStatus | null>(null);
  let minedBlocks = $state<MinedBlock[]>([]);
  let isLoadingHistory = $state(false);
  let showHistory = $state(true);
  let isLoading = $state(true);
  let isStartingMining = $state(false);
  let maxThreads = $state(navigator.hardwareConcurrency || 4);
  let refreshInterval: ReturnType<typeof setInterval> | null = null;

  // Load saved thread count from localStorage, default to 1
  const savedThreads = typeof window !== 'undefined' ? localStorage.getItem('chiral-mining-threads') : null;
  let miningThreads = $state(savedThreads ? Math.min(parseInt(savedThreads, 10) || 1, navigator.hardwareConcurrency || 4) : 1);

  // Mining session tracking
  let miningStartTime = $state<number | null>(null);
  let miningElapsed = $state('00:00:00');
  let elapsedInterval: ReturnType<typeof setInterval> | null = null;


  // Save thread count whenever it changes
  $effect(() => {
    if (typeof window !== 'undefined') {
      localStorage.setItem('chiral-mining-threads', miningThreads.toString());
    }
  });

  // Track mining elapsed time
  $effect(() => {
    if (miningStatus?.mining && !miningStartTime) {
      // Restore or start timer
      const saved = typeof window !== 'undefined' ? localStorage.getItem('chiral-mining-start') : null;
      miningStartTime = saved ? parseInt(saved, 10) : Date.now();
      if (!saved && typeof window !== 'undefined') {
        localStorage.setItem('chiral-mining-start', miningStartTime.toString());
      }
      elapsedInterval = setInterval(updateElapsed, 1000);
    } else if (!miningStatus?.mining && miningStartTime) {
      miningStartTime = null;
      if (typeof window !== 'undefined') {
        localStorage.removeItem('chiral-mining-start');
      }
      if (elapsedInterval) {
        clearInterval(elapsedInterval);
        elapsedInterval = null;
      }
      miningElapsed = '00:00:00';
    }
  });


  function updateElapsed() {
    if (!miningStartTime) return;
    const diff = Math.floor((Date.now() - miningStartTime) / 1000);
    const h = Math.floor(diff / 3600).toString().padStart(2, '0');
    const m = Math.floor((diff % 3600) / 60).toString().padStart(2, '0');
    const s = (diff % 60).toString().padStart(2, '0');
    miningElapsed = `${h}:${m}:${s}`;
  }

  // Check if Tauri is available
  function isTauri(): boolean {
    return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
  }

  // Load status on mount
  onMount(async () => {
    if (isTauri()) {
      await loadStatus();
      loadMinedBlocks();
      refreshInterval = setInterval(loadStatus, 5000);
    }
    isLoading = false;
  });

  onDestroy(() => {
    if (refreshInterval) {
      clearInterval(refreshInterval);
    }
    if (elapsedInterval) {
      clearInterval(elapsedInterval);
    }
  });

  // Load Geth and mining status
  async function loadStatus() {
    if (!isTauri()) {
      gethStatus = {
        installed: false,
        running: false,
        localRunning: false,
        syncing: false,
        currentBlock: 0,
        highestBlock: 0,
        peerCount: 0,
        chainId: 0
      };
      return;
    }

    try {
      const [geth, mining] = await Promise.all([
        invoke<GethStatus>('get_geth_status'),
        invoke<MiningStatus>('get_mining_status').catch(() => null)
      ]);

      gethStatus = geth;
      miningStatus = mining;
    } catch (error) {
      log.error('Failed to load status:', error);
      gethStatus = {
        installed: false,
        running: false,
        localRunning: false,
        syncing: false,
        currentBlock: 0,
        highestBlock: 0,
        peerCount: 0,
        chainId: 0
      };
    }
  }

  // Start Mining
  async function handleStartMining() {
    if (!isTauri()) return;

    isStartingMining = true;
    try {
      if ($walletAccount?.address) {
        await invoke('set_miner_address', { address: $walletAccount.address });
      }

      await invoke('start_mining', { threads: miningThreads });
      toasts.show(`Mining started with ${miningThreads} thread(s)!`, 'success');
      await loadStatus();
    } catch (error) {
      log.error('Failed to start mining:', error);
      toasts.show(`Failed to start mining: ${error}`, 'error');
    } finally {
      isStartingMining = false;
    }
  }

  // Stop Mining
  async function handleStopMining() {
    if (!isTauri()) return;

    try {
      await invoke('stop_mining');
      toasts.show('Mining stopped', 'info');
      await loadStatus();
    } catch (error) {
      log.error('Failed to stop mining:', error);
      toasts.show(`Failed to stop mining: ${error}`, 'error');
    }
  }

  // Load mined blocks history
  async function loadMinedBlocks() {
    if (!isTauri()) return;
    isLoadingHistory = true;
    try {
      minedBlocks = await invoke<MinedBlock[]>('get_mined_blocks', { maxBlocks: 500 });
    } catch (error) {
      log.error('Failed to load mined blocks:', error);
      minedBlocks = [];
    } finally {
      isLoadingHistory = false;
    }
  }

  // Derived: total rewards from history
  let totalHistoryReward = $derived(
    minedBlocks.reduce((sum, b) => sum + b.rewardChr, 0)
  );

  // Format timestamp to readable date/time
  function formatTimestamp(ts: number): string {
    if (ts === 0) return 'Unknown';
    return new Date(ts * 1000).toLocaleString();
  }

  // Format hash rate
  function formatHashRate(rate: number): string {
    if (rate >= 1e9) return `${(rate / 1e9).toFixed(2)} GH/s`;
    if (rate >= 1e6) return `${(rate / 1e6).toFixed(2)} MH/s`;
    if (rate >= 1e3) return `${(rate / 1e3).toFixed(2)} KH/s`;
    return `${rate} H/s`;
  }
</script>

<div class="p-6 space-y-6">
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-3xl font-bold dark:text-white">Mining</h1>
      <p class="text-gray-600 dark:text-gray-400 mt-1">Mine CHR tokens on the Chiral Network</p>
    </div>
    <button
      onclick={loadStatus}
      disabled={isLoading}
      class="p-2 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition-colors disabled:opacity-50 dark:text-gray-300"
      title="Refresh status"
    >
      <RefreshCw class="w-5 h-5 {isLoading ? 'animate-spin' : ''}" />
    </button>
  </div>

  {#if isLoading}
    <div class="flex items-center justify-center py-12">
      <Loader2 class="w-8 h-8 animate-spin text-gray-400" />
    </div>
  {:else if !gethStatus?.installed || !gethStatus?.localRunning}
    <!-- Geth Not Running Locally - Direct to Network Page -->
    <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
      <div class="flex items-center gap-3 mb-4">
        <div class="p-2 bg-yellow-100 dark:bg-yellow-900/30 rounded-lg">
          <AlertTriangle class="w-6 h-6 text-yellow-600 dark:text-yellow-400" />
        </div>
        <div>
          <h2 class="font-semibold dark:text-white">Local Blockchain Node Required</h2>
          <p class="text-sm text-gray-500 dark:text-gray-400">
            {#if !gethStatus?.installed}
              Geth is not installed
            {:else}
              Local Geth node is not running
            {/if}
          </p>
        </div>
      </div>
      <div class="bg-yellow-50 dark:bg-yellow-900/30 border border-yellow-200 dark:border-yellow-800 rounded-lg p-4 mb-4">
        <p class="text-sm text-yellow-800 dark:text-yellow-300">
          {#if !gethStatus?.installed}
            You need to download and start a local Geth node before you can mine CHR tokens.
          {:else}
            Mining requires a local Geth node. Start the node from the Network page to begin mining.
          {/if}
        </p>
        <p class="text-sm text-yellow-700 dark:text-yellow-400 mt-2">
          Go to the <strong>Network</strong> page to start your local blockchain node.
        </p>
      </div>
      <button
        onclick={() => goto('/network')}
        class="w-full px-4 py-3 bg-primary-600 text-white rounded-lg hover:bg-primary-700 transition-colors flex items-center justify-center gap-2"
      >
        <Globe class="w-5 h-5" />
        Go to Network Page
      </button>
    </div>
  {:else}
    <!-- Mining Control Card -->
    <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
      <div class="flex items-center justify-between mb-4">
        <div class="flex items-center gap-3">
          <div class="p-2 {miningStatus?.mining ? 'bg-yellow-100 dark:bg-yellow-900/30' : 'bg-gray-100 dark:bg-gray-700'} rounded-lg">
            <Pickaxe class="w-6 h-6 {miningStatus?.mining ? 'text-yellow-600 dark:text-yellow-400' : 'text-gray-600 dark:text-gray-400'}" />
          </div>
          <div>
            <h2 class="font-semibold dark:text-white">Mining</h2>
            <p class="text-sm text-gray-500 dark:text-gray-400">Earn CHR by mining blocks</p>
          </div>
        </div>
        <div class="flex items-center gap-2">
          {#if miningStatus?.mining}
            <span class="flex items-center gap-2 px-3 py-1 bg-yellow-100 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-400 rounded-full text-sm">
              <span class="w-2 h-2 bg-yellow-500 rounded-full animate-pulse"></span>
              Mining
            </span>
          {:else}
            <span class="flex items-center gap-2 px-3 py-1 bg-gray-100 dark:bg-gray-700 text-gray-700 dark:text-gray-300 rounded-full text-sm">
              <span class="w-2 h-2 bg-gray-400 rounded-full"></span>
              Idle
            </span>
          {/if}
        </div>
      </div>

      <!-- Mining Stats Grid -->
      <div class="grid grid-cols-2 md:grid-cols-3 gap-4 mb-4">
        <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-4">
          <div class="flex items-center gap-2 mb-2">
            <Zap class="w-4 h-4 text-yellow-500" />
            <span class="text-sm text-gray-600 dark:text-gray-400">Hash Rate</span>
          </div>
          <p class="text-2xl font-bold dark:text-white">
            {miningStatus?.mining ? formatHashRate(miningStatus?.hashRate || 0) : '0 H/s'}
          </p>
        </div>
        <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-4">
          <div class="flex items-center gap-2 mb-2">
            <Blocks class="w-4 h-4 text-red-500" />
            <span class="text-sm text-gray-600 dark:text-gray-400">Block Height</span>
          </div>
          <p class="text-2xl font-bold dark:text-white">
            {gethStatus?.currentBlock?.toLocaleString() ?? '0'}
          </p>
        </div>
        <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-4">
          <div class="flex items-center gap-2 mb-2">
            <Coins class="w-4 h-4 text-amber-500" />
            <span class="text-sm text-gray-600 dark:text-gray-400">Total Mined</span>
          </div>
          <p class="text-2xl font-bold dark:text-white">
            {(miningStatus?.totalMinedChr ?? 0).toFixed(4)} CHR
          </p>
        </div>
        <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-4">
          <div class="flex items-center gap-2 mb-2">
            <Clock class="w-4 h-4 text-purple-500" />
            <span class="text-sm text-gray-600 dark:text-gray-400">Session Time</span>
          </div>
          <p class="text-2xl font-bold dark:text-white">
            {miningStatus?.mining ? miningElapsed : '--:--:--'}
          </p>
        </div>
        <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-4">
          <div class="flex items-center gap-2 mb-2">
            <Cpu class="w-4 h-4 text-green-500" />
            <span class="text-sm text-gray-600 dark:text-gray-400">Threads Active</span>
          </div>
          <p class="text-2xl font-bold dark:text-white">
            {miningStatus?.mining ? `${miningThreads} / ${maxThreads}` : `0 / ${maxThreads}`}
          </p>
        </div>
      </div>

      <!-- Miner Address -->
      <div class="mb-4 p-3 bg-gray-50 dark:bg-gray-700 rounded-lg">
        <div class="flex items-center gap-2 mb-1">
          <TrendingUp class="w-4 h-4 text-green-500" />
          <span class="text-sm text-gray-600 dark:text-gray-400">Miner Address</span>
        </div>
        <p class="text-sm font-mono truncate dark:text-gray-300">
          {miningStatus?.minerAddress || $walletAccount?.address || 'Not set'}
        </p>
      </div>

      <!-- Thread Control -->
      <div class="mb-4">
        <label for="threads" class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
          Mining Threads ({miningThreads} / {maxThreads})
        </label>
        <input
          id="threads"
          type="range"
          min="1"
          max={maxThreads}
          bind:value={miningThreads}
          disabled={miningStatus?.mining}
          class="w-full h-2 bg-gray-200 dark:bg-gray-600 rounded-lg appearance-none cursor-pointer disabled:opacity-50"
        />
        <div class="flex justify-between text-xs text-gray-500 dark:text-gray-400 mt-1">
          <span>1 thread</span>
          <span>{maxThreads} threads (max)</span>
        </div>
      </div>

      <!-- Mining Controls -->
      <div class="flex gap-3">
        {#if miningStatus?.mining}
          <button
            onclick={handleStopMining}
            class="flex-1 px-4 py-3 bg-red-600 text-white rounded-lg hover:bg-red-700 transition-colors flex items-center justify-center gap-2"
          >
            <Square class="w-5 h-5" />
            Stop Mining
          </button>
        {:else}
          <button
            onclick={handleStartMining}
            disabled={isStartingMining}
            class="flex-1 px-4 py-3 bg-yellow-500 text-white rounded-lg hover:bg-yellow-600 transition-colors flex items-center justify-center gap-2 disabled:opacity-50"
          >
            {#if isStartingMining}
              <Loader2 class="w-5 h-5 animate-spin" />
              Starting...
            {:else}
              <Pickaxe class="w-5 h-5" />
              Start Mining
            {/if}
          </button>
        {/if}
      </div>
    </div>

    <!-- Mining History -->
    <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700">
      <button
        onclick={() => showHistory = !showHistory}
        class="w-full flex items-center justify-between p-6 text-left"
      >
        <div class="flex items-center gap-3">
          <div class="p-2 bg-emerald-100 dark:bg-emerald-900/30 rounded-lg">
            <History class="w-6 h-6 text-emerald-600 dark:text-emerald-400" />
          </div>
          <div>
            <h2 class="font-semibold dark:text-white">Mining History</h2>
            <p class="text-sm text-gray-500 dark:text-gray-400">
              {minedBlocks.length} block{minedBlocks.length !== 1 ? 's' : ''} mined
              {#if totalHistoryReward > 0}
                — {totalHistoryReward.toFixed(2)} CHR earned
              {/if}
            </p>
          </div>
        </div>
        {#if showHistory}
          <ChevronUp class="w-5 h-5 text-gray-400" />
        {:else}
          <ChevronDown class="w-5 h-5 text-gray-400" />
        {/if}
      </button>

      {#if showHistory}
        <div class="px-6 pb-6">
          <div class="flex justify-end mb-4">
            <button
              onclick={loadMinedBlocks}
              disabled={isLoadingHistory}
              class="text-xs px-3 py-1.5 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 rounded transition-colors flex items-center gap-1 disabled:opacity-50 dark:text-gray-300"
            >
              {#if isLoadingHistory}
                <Loader2 class="w-3 h-3 animate-spin" />
              {:else}
                <RefreshCw class="w-3 h-3" />
              {/if}
              Refresh
            </button>
          </div>
          {#if isLoadingHistory && minedBlocks.length === 0}
            <div class="flex items-center justify-center py-8">
              <Loader2 class="w-6 h-6 animate-spin text-gray-400" />
              <span class="ml-2 text-sm text-gray-500 dark:text-gray-400">Scanning blockchain...</span>
            </div>
          {:else if minedBlocks.length === 0}
            <div class="text-center py-8">
              <Pickaxe class="w-10 h-10 text-gray-300 dark:text-gray-600 mx-auto mb-3" />
              <p class="text-sm text-gray-500 dark:text-gray-400">No blocks mined yet.</p>
              <p class="text-xs text-gray-400 dark:text-gray-500 mt-1">Start mining to earn CHR block rewards.</p>
            </div>
          {:else}
            <!-- Summary Stats -->
            <div class="grid grid-cols-3 gap-3 mb-4">
              <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-3">
                <p class="text-xs text-gray-500 dark:text-gray-400">Blocks Mined</p>
                <p class="text-lg font-bold dark:text-white">{minedBlocks.length}</p>
              </div>
              <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-3">
                <p class="text-xs text-gray-500 dark:text-gray-400">Total Earned</p>
                <p class="text-lg font-bold text-emerald-600 dark:text-emerald-400">{totalHistoryReward.toFixed(2)} CHR</p>
              </div>
              <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-3">
                <p class="text-xs text-gray-500 dark:text-gray-400">Reward per Block</p>
                <p class="text-lg font-bold dark:text-white">{minedBlocks[0]?.rewardChr ?? 0} CHR</p>
              </div>
            </div>

            <!-- Block Table -->
            <div class="overflow-x-auto">
              <table class="w-full text-sm">
                <thead>
                  <tr class="border-b border-gray-200 dark:border-gray-700">
                    <th class="text-left py-2 px-3 text-xs font-medium text-gray-500 dark:text-gray-400">Block #</th>
                    <th class="text-left py-2 px-3 text-xs font-medium text-gray-500 dark:text-gray-400">Time</th>
                    <th class="text-right py-2 px-3 text-xs font-medium text-gray-500 dark:text-gray-400">Reward</th>
                    <th class="text-right py-2 px-3 text-xs font-medium text-gray-500 dark:text-gray-400">Difficulty</th>
                  </tr>
                </thead>
                <tbody>
                  {#each minedBlocks as block (block.blockNumber)}
                    <tr class="border-b border-gray-100 dark:border-gray-700/50 hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors">
                      <td class="py-2 px-3 font-mono text-xs dark:text-gray-300">
                        #{block.blockNumber.toLocaleString()}
                      </td>
                      <td class="py-2 px-3 text-xs text-gray-600 dark:text-gray-400">
                        {formatTimestamp(block.timestamp)}
                      </td>
                      <td class="py-2 px-3 text-right text-xs font-medium text-emerald-600 dark:text-emerald-400">
                        +{block.rewardChr} CHR
                      </td>
                      <td class="py-2 px-3 text-right text-xs text-gray-500 dark:text-gray-400 font-mono">
                        {block.difficulty.toLocaleString()}
                      </td>
                    </tr>
                  {/each}
                </tbody>
              </table>
            </div>
          {/if}
        </div>
      {/if}
    </div>
  {/if}
</div>
