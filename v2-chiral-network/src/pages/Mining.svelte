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
    Check,
    Loader2,
    BarChart3,
    Globe
  } from 'lucide-svelte';
  import { logger } from '$lib/logger';
  const log = logger('Mining');

  // Types
  interface GethStatus {
    installed: boolean;
    running: boolean;
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
  let miningThreads = $state(1);
  let maxThreads = $state(navigator.hardwareConcurrency || 4);
  let refreshInterval: ReturnType<typeof setInterval> | null = null;

  // Check if Tauri is available
  function isTauri(): boolean {
    return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
  }

  // Load status on mount
  onMount(async () => {
    if (isTauri()) {
      await loadStatus();

      // Refresh status every 5 seconds
      refreshInterval = setInterval(loadStatus, 5000);
    }
    isLoading = false;
  });

  onDestroy(() => {
    if (refreshInterval) {
      clearInterval(refreshInterval);
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
      // Set default status on error
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
      // Set miner address first
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
  {:else if !gethStatus?.installed || !gethStatus?.running}
    <!-- Geth Not Running - Direct to Network Page -->
    <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
      <div class="flex items-center gap-3 mb-4">
        <div class="p-2 bg-yellow-100 dark:bg-yellow-900/30 rounded-lg">
          <AlertTriangle class="w-6 h-6 text-yellow-600 dark:text-yellow-400" />
        </div>
        <div>
          <h2 class="font-semibold dark:text-white">Blockchain Node Required</h2>
          <p class="text-sm text-gray-500 dark:text-gray-400">
            {#if !gethStatus?.installed}
              Geth is not installed
            {:else}
              Geth is not running
            {/if}
          </p>
        </div>
      </div>
      <div class="bg-yellow-50 dark:bg-yellow-900/30 border border-yellow-200 dark:border-yellow-800 rounded-lg p-4 mb-4">
        <p class="text-sm text-yellow-800 dark:text-yellow-300">
          {#if !gethStatus?.installed}
            You need to download and start Geth before you can mine CHR tokens.
          {:else}
            You need to start Geth before you can mine CHR tokens.
          {/if}
        </p>
        <p class="text-sm text-yellow-700 dark:text-yellow-400 mt-2">
          Go to the <strong>Network</strong> page to manage your blockchain node connection.
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
    <!-- Mining Control Card - Geth is installed and running -->
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

        <!-- Mining Stats -->
        <div class="grid grid-cols-2 gap-4 mb-4">
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
              <TrendingUp class="w-4 h-4 text-green-500" />
              <span class="text-sm text-gray-600 dark:text-gray-400">Miner Address</span>
            </div>
            <p class="text-sm font-mono truncate dark:text-gray-300">
              {miningStatus?.minerAddress || $walletAccount?.address || 'Not set'}
            </p>
          </div>
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

    <!-- Info Card -->
    <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
      <div class="flex items-center gap-3 mb-4">
        <div class="p-2 bg-purple-100 dark:bg-purple-900/30 rounded-lg">
          <BarChart3 class="w-6 h-6 text-purple-600 dark:text-purple-400" />
        </div>
        <div>
          <h2 class="font-semibold dark:text-white">Mining Information</h2>
          <p class="text-sm text-gray-500 dark:text-gray-400">How mining works on Chiral Network</p>
        </div>
      </div>
      <div class="space-y-3 text-sm text-gray-600 dark:text-gray-400">
        <div class="flex items-start gap-3">
          <Check class="w-5 h-5 text-green-500 flex-shrink-0 mt-0.5" />
          <p>Mining helps secure the Chiral Network and process transactions</p>
        </div>
        <div class="flex items-start gap-3">
          <Check class="w-5 h-5 text-green-500 flex-shrink-0 mt-0.5" />
          <p>Miners earn CHR tokens as block rewards for finding valid blocks</p>
        </div>
        <div class="flex items-start gap-3">
          <Check class="w-5 h-5 text-green-500 flex-shrink-0 mt-0.5" />
          <p>More threads = higher hash rate, but also higher CPU usage</p>
        </div>
        <div class="flex items-start gap-3">
          <Check class="w-5 h-5 text-green-500 flex-shrink-0 mt-0.5" />
          <p>Block rewards are sent directly to your wallet address</p>
        </div>
      </div>
    </div>
  {/if}
</div>
