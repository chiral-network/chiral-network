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
    Thermometer,
    Gauge,
    Bolt,
    Coins,
    Clock,
    Cpu
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
  }

  // State
  let gethStatus = $state<GethStatus | null>(null);
  let miningStatus = $state<MiningStatus | null>(null);
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

  // Estimated stats (simulated from hash rate)
  let estimatedTemp = $state(0);
  let estimatedWattage = $state(0);
  let totalMined = $state(0);

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

  // Update estimated stats when mining status changes
  $effect(() => {
    if (miningStatus?.mining) {
      const hr = miningStatus.hashRate || 0;
      // Estimate CPU temperature based on thread count (baseline 45C + thread contribution)
      estimatedTemp = Math.round(45 + miningThreads * 8 + Math.random() * 3);
      // Estimate wattage: ~15W per thread under mining load
      estimatedWattage = Math.round(15 * miningThreads + Math.random() * 5);
    } else {
      estimatedTemp = 0;
      estimatedWattage = 0;
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
    // Load saved total mined
    const savedMined = typeof window !== 'undefined' ? localStorage.getItem('chiral-total-mined') : null;
    if (savedMined) totalMined = parseFloat(savedMined) || 0;

    if (isTauri()) {
      await loadStatus();
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
        class="w-full px-4 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors flex items-center justify-center gap-2"
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
            <Thermometer class="w-4 h-4 text-red-500" />
            <span class="text-sm text-gray-600 dark:text-gray-400">CPU Temperature</span>
          </div>
          <p class="text-2xl font-bold dark:text-white">
            {miningStatus?.mining ? `~${estimatedTemp}°C` : '--'}
          </p>
        </div>
        <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-4">
          <div class="flex items-center gap-2 mb-2">
            <Bolt class="w-4 h-4 text-blue-500" />
            <span class="text-sm text-gray-600 dark:text-gray-400">Power Draw</span>
          </div>
          <p class="text-2xl font-bold dark:text-white">
            {miningStatus?.mining ? `~${estimatedWattage}W` : '--'}
          </p>
        </div>
        <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-4">
          <div class="flex items-center gap-2 mb-2">
            <Coins class="w-4 h-4 text-amber-500" />
            <span class="text-sm text-gray-600 dark:text-gray-400">Total Mined</span>
          </div>
          <p class="text-2xl font-bold dark:text-white">
            {totalMined.toFixed(4)} CHR
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
  {/if}
</div>
