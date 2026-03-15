<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import {
    gethService,
    gethStatus,
    miningStatus,
    downloadProgress,
    isDownloading
  } from '$lib/services/gethService';
  import { walletAccount } from '$lib/stores';
  import { toasts } from '$lib/toastStore';
  import {
    Server,
    Download,
    Play,
    Square,
    Cpu,
    Users,
    Box,
    Loader2,
    CheckCircle,
    XCircle,
    Pickaxe
  } from 'lucide-svelte';
  import { logger } from '$lib/logger';
  const log = logger('GethStatus');

  // State
  let isInstalled = $state(false);
  let isStarting = $state(false);
  let isStopping = $state(false);
  let isStartingMining = $state(false);
  let miningThreads = $state(1);

  // Check if Tauri is available
  function isTauri(): boolean {
    return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
  }

  onMount(async () => {
    if (!isTauri()) return;

    try {
      isInstalled = await gethService.isInstalled();
      if (isInstalled) {
        await gethService.initialize();
      }
    } catch (error) {
      log.error('Failed to initialize Geth service:', error);
    }
  });

  onDestroy(() => {
    gethService.stopStatusPolling();
  });

  async function handleDownload() {
    try {
      await gethService.download();
      isInstalled = await gethService.isInstalled();
      toasts.show('Geth installed successfully', 'success');
    } catch (error) {
      log.error('Failed to download Geth:', error);
      toasts.show('Failed to download Geth: ' + error, 'error');
    }
  }

  async function handleStart() {
    isStarting = true;
    try {
      const minerAddr = $walletAccount?.address;
      await gethService.start(minerAddr);
      toasts.show('Geth started successfully', 'success');
    } catch (error) {
      log.error('Failed to start Geth:', error);
      toasts.show('Failed to start Geth: ' + error, 'error');
    } finally {
      isStarting = false;
    }
  }

  async function handleStop() {
    isStopping = true;
    try {
      await gethService.stop();
      toasts.show('Geth stopped', 'info');
    } catch (error) {
      log.error('Failed to stop Geth:', error);
      toasts.show('Failed to stop Geth: ' + error, 'error');
    } finally {
      isStopping = false;
    }
  }

  async function handleStartMining() {
    isStartingMining = true;
    try {
      // Set miner address first
      if ($walletAccount?.address) {
        await gethService.setMinerAddress($walletAccount.address);
      }
      await gethService.startMining(miningThreads);
      toasts.show('Mining started', 'success');
    } catch (error) {
      log.error('Failed to start mining:', error);
      toasts.show('Failed to start mining: ' + error, 'error');
    } finally {
      isStartingMining = false;
    }
  }

  async function handleStopMining() {
    try {
      await gethService.stopMining();
      toasts.show('Mining stopped', 'info');
    } catch (error) {
      log.error('Failed to stop mining:', error);
      toasts.show('Failed to stop mining: ' + error, 'error');
    }
  }

  function formatHashRate(hashRate: number): string {
    if (hashRate >= 1e9) return (hashRate / 1e9).toFixed(2) + ' GH/s';
    if (hashRate >= 1e6) return (hashRate / 1e6).toFixed(2) + ' MH/s';
    if (hashRate >= 1e3) return (hashRate / 1e3).toFixed(2) + ' KH/s';
    return hashRate + ' H/s';
  }
</script>

<div class="space-y-4">
  <!-- Installation Status -->
  {#if !isInstalled}
    <div class="backdrop-blur-xl bg-white/10 dark:bg-white/5 border border-white/20 dark:border-white/10 rounded-2xl shadow-lg shadow-black/5 ring-1 ring-white/10 p-6">
      <div class="flex items-center gap-3 mb-4">
        <div class="p-2 bg-primary-500/15 dark:bg-primary-500/10 rounded-lg">
          <Download class="w-6 h-6 text-primary-600 dark:text-primary-400" />
        </div>
        <div>
          <h3 class="font-semibold dark:text-white">Install Geth</h3>
          <p class="text-sm text-gray-500 dark:text-gray-400">Download Core-Geth to run the blockchain node</p>
        </div>
      </div>

      {#if $isDownloading}
        <div class="space-y-2">
          <div class="flex justify-between text-sm dark:text-gray-300">
            <span>{$downloadProgress?.status || 'Downloading...'}</span>
            <span>{$downloadProgress?.percentage?.toFixed(1) || 0}%</span>
          </div>
          <div class="w-full bg-white/20 dark:bg-white/10 rounded-full h-2">
            <div
              class="bg-primary-500/80 h-2 rounded-full transition-all"
              style="width: {$downloadProgress?.percentage || 0}%"
            ></div>
          </div>
        </div>
      {:else}
        <button
          onclick={handleDownload}
          class="w-full px-4 py-2 backdrop-blur-md bg-primary-500/80 dark:bg-primary-600/70 border border-primary-400/30 text-white rounded-lg hover:bg-primary-500/90 dark:hover:bg-primary-600/80 transition-colors flex items-center justify-center gap-2"
        >
          <Download class="w-4 h-4" />
          Download Geth
        </button>
      {/if}
    </div>
  {:else}
    <!-- Geth Status Card -->
    <div class="backdrop-blur-xl bg-white/10 dark:bg-white/5 border border-white/20 dark:border-white/10 rounded-2xl shadow-lg shadow-black/5 ring-1 ring-white/10 p-6">
      <div class="flex items-center justify-between mb-4">
        <div class="flex items-center gap-3">
          <div class="p-2 {$gethStatus?.running ? 'bg-green-500/15 dark:bg-green-500/10' : 'bg-white/10 dark:bg-white/5'} rounded-lg">
            <Server class="w-6 h-6 {$gethStatus?.running ? 'text-green-600 dark:text-green-400' : 'text-gray-400'}" />
          </div>
          <div>
            <h3 class="font-semibold dark:text-white">Geth Node</h3>
            <p class="text-sm text-gray-500 dark:text-gray-400">
              {#if $gethStatus?.running}
                {#if $gethStatus?.syncing}
                  Syncing... Block {$gethStatus?.currentBlock?.toLocaleString()} / {$gethStatus?.highestBlock?.toLocaleString()}
                {:else}
                  Running - Block {$gethStatus?.currentBlock?.toLocaleString()}
                {/if}
              {:else}
                Not running
              {/if}
            </p>
          </div>
        </div>

        {#if $gethStatus?.running}
          <button
            onclick={handleStop}
            disabled={isStopping}
            class="px-4 py-2 backdrop-blur-md bg-red-500/15 dark:bg-red-500/10 border border-red-400/20 text-red-600 dark:text-red-400 rounded-lg hover:bg-red-500/25 dark:hover:bg-red-500/15 transition-colors flex items-center gap-2 disabled:opacity-50"
          >
            {#if isStopping}
              <Loader2 class="w-4 h-4 animate-spin" />
            {:else}
              <Square class="w-4 h-4" />
            {/if}
            Stop
          </button>
        {:else}
          <button
            onclick={handleStart}
            disabled={isStarting}
            class="px-4 py-2 backdrop-blur-md bg-green-500/70 dark:bg-green-600/60 border border-green-400/30 text-white rounded-lg hover:bg-green-500/80 dark:hover:bg-green-600/70 transition-colors flex items-center gap-2 disabled:opacity-50"
          >
            {#if isStarting}
              <Loader2 class="w-4 h-4 animate-spin" />
            {:else}
              <Play class="w-4 h-4" />
            {/if}
            Start
          </button>
        {/if}
      </div>

      {#if $gethStatus?.running}
        <div class="grid grid-cols-3 gap-4 mt-4">
          <div class="backdrop-blur-md bg-white/8 dark:bg-white/5 border border-white/15 dark:border-white/10 rounded-lg p-3 text-center">
            <Users class="w-5 h-5 mx-auto text-gray-400 mb-1" />
            <p class="text-lg font-semibold dark:text-white">{$gethStatus?.peerCount || 0}</p>
            <p class="text-xs text-gray-500 dark:text-gray-400">Peers</p>
          </div>
          <div class="backdrop-blur-md bg-white/8 dark:bg-white/5 border border-white/15 dark:border-white/10 rounded-lg p-3 text-center">
            <Box class="w-5 h-5 mx-auto text-gray-400 mb-1" />
            <p class="text-lg font-semibold dark:text-white">{$gethStatus?.currentBlock?.toLocaleString() || 0}</p>
            <p class="text-xs text-gray-500 dark:text-gray-400">Block</p>
          </div>
          <div class="backdrop-blur-md bg-white/8 dark:bg-white/5 border border-white/15 dark:border-white/10 rounded-lg p-3 text-center">
            {#if $gethStatus?.syncing}
              <Loader2 class="w-5 h-5 mx-auto text-yellow-500 mb-1 animate-spin" />
              <p class="text-lg font-semibold text-yellow-600 dark:text-yellow-400">Syncing</p>
            {:else}
              <CheckCircle class="w-5 h-5 mx-auto text-green-500 mb-1" />
              <p class="text-lg font-semibold text-green-600 dark:text-green-400">Synced</p>
            {/if}
            <p class="text-xs text-gray-500 dark:text-gray-400">Status</p>
          </div>
        </div>
      {/if}
    </div>

    <!-- Mining Card -->
    {#if $gethStatus?.running}
      <div class="backdrop-blur-xl bg-white/10 dark:bg-white/5 border border-white/20 dark:border-white/10 rounded-2xl shadow-lg shadow-black/5 ring-1 ring-white/10 p-6">
        <div class="flex items-center justify-between mb-4">
          <div class="flex items-center gap-3">
            <div class="p-2 {$miningStatus?.mining ? 'bg-yellow-500/15 dark:bg-yellow-500/10' : 'bg-white/10 dark:bg-white/5'} rounded-lg">
              <Pickaxe class="w-6 h-6 {$miningStatus?.mining ? 'text-yellow-600 dark:text-yellow-400' : 'text-gray-400'}" />
            </div>
            <div>
              <h3 class="font-semibold dark:text-white">Mining</h3>
              <p class="text-sm text-gray-500 dark:text-gray-400">
                {#if $miningStatus?.mining}
                  Hash Rate: {formatHashRate($miningStatus?.hashRate || 0)}
                {:else}
                  Not mining
                {/if}
              </p>
            </div>
          </div>

          {#if $miningStatus?.mining}
            <button
              onclick={handleStopMining}
              class="px-4 py-2 backdrop-blur-md bg-red-500/15 dark:bg-red-500/10 border border-red-400/20 text-red-600 dark:text-red-400 rounded-lg hover:bg-red-500/25 transition-colors flex items-center gap-2"
            >
              <Square class="w-4 h-4" />
              Stop Mining
            </button>
          {:else}
            <div class="flex items-center gap-2">
              <select
                bind:value={miningThreads}
                class="px-3 py-2 backdrop-blur-md bg-white/10 dark:bg-white/5 border border-white/20 dark:border-white/10 text-gray-900 dark:text-white rounded-lg text-sm outline-none focus:ring-2 focus:ring-primary-500/40"
              >
                {#each [1, 2, 4, 8] as threads}
                  <option value={threads}>{threads} thread{threads > 1 ? 's' : ''}</option>
                {/each}
              </select>
              <button
                onclick={handleStartMining}
                disabled={isStartingMining || !$walletAccount}
                class="px-4 py-2 backdrop-blur-md bg-yellow-500/70 dark:bg-yellow-600/60 border border-yellow-400/30 text-white rounded-lg hover:bg-yellow-500/80 transition-colors flex items-center gap-2 disabled:opacity-50"
                title={!$walletAccount ? 'Connect wallet first' : ''}
              >
                {#if isStartingMining}
                  <Loader2 class="w-4 h-4 animate-spin" />
                {:else}
                  <Pickaxe class="w-4 h-4" />
                {/if}
                Start Mining
              </button>
            </div>
          {/if}
        </div>

        {#if $miningStatus?.mining}
          <div class="backdrop-blur-md bg-yellow-500/10 dark:bg-yellow-500/5 border border-yellow-400/20 rounded-lg p-3 mt-4">
            <div class="flex items-center gap-2 text-yellow-800 dark:text-yellow-300">
              <Cpu class="w-4 h-4" />
              <span class="text-sm">
                Mining to: <span class="font-mono text-xs">{$miningStatus?.minerAddress || 'Not set'}</span>
              </span>
            </div>
          </div>
        {:else if !$walletAccount}
          <div class="backdrop-blur-md bg-white/8 dark:bg-white/5 border border-white/15 dark:border-white/10 rounded-lg p-3 mt-4">
            <p class="text-sm text-gray-600 dark:text-gray-400">
              Connect your wallet on the Account page to start mining and earn CHI.
            </p>
          </div>
        {/if}
      </div>
    {/if}
  {/if}
</div>
