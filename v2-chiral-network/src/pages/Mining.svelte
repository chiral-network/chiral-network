<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { walletAccount } from '$lib/stores';
  import { toasts } from '$lib/toastStore';
  import {
    Pickaxe,
    Play,
    Square,
    Cpu,
    Zap,
    TrendingUp,
    Clock,
    RefreshCw,
    Download,
    AlertTriangle,
    Check,
    Loader2,
    Settings,
    BarChart3
  } from 'lucide-svelte';

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

  interface DownloadProgress {
    downloaded: number;
    total: number;
    percentage: number;
    status: string;
  }

  // State
  let gethStatus = $state<GethStatus | null>(null);
  let miningStatus = $state<MiningStatus | null>(null);
  let isLoading = $state(true);
  let isStartingGeth = $state(false);
  let isStartingMining = $state(false);
  let isDownloading = $state(false);
  let downloadProgress = $state<DownloadProgress | null>(null);
  let miningThreads = $state(1);
  let maxThreads = $state(navigator.hardwareConcurrency || 4);
  let refreshInterval: ReturnType<typeof setInterval> | null = null;
  let unlistenDownload: (() => void) | null = null;

  // Check if Tauri is available
  function isTauri(): boolean {
    return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
  }

  // Load status on mount
  onMount(async () => {
    if (isTauri()) {
      await loadStatus();

      // Set up download progress listener
      unlistenDownload = await listen<DownloadProgress>('geth-download-progress', (event) => {
        downloadProgress = event.payload;
        if (event.payload.percentage >= 100) {
          isDownloading = false;
          loadStatus();
        }
      });

      // Refresh status every 5 seconds
      refreshInterval = setInterval(loadStatus, 5000);
    }
    isLoading = false;
  });

  onDestroy(() => {
    if (refreshInterval) {
      clearInterval(refreshInterval);
    }
    if (unlistenDownload) {
      unlistenDownload();
    }
  });

  // Load Geth and mining status
  async function loadStatus() {
    if (!isTauri()) return;

    try {
      const [geth, mining] = await Promise.all([
        invoke<GethStatus>('get_geth_status'),
        invoke<MiningStatus>('get_mining_status').catch(() => null)
      ]);

      gethStatus = geth;
      miningStatus = mining;
    } catch (error) {
      console.error('Failed to load status:', error);
    }
  }

  // Download Geth
  async function handleDownloadGeth() {
    if (!isTauri()) return;

    isDownloading = true;
    downloadProgress = { downloaded: 0, total: 0, percentage: 0, status: 'Starting download...' };

    try {
      await invoke('download_geth');
      toasts.show('Geth downloaded successfully!', 'success');
      await loadStatus();
    } catch (error) {
      console.error('Failed to download Geth:', error);
      toasts.show(`Download failed: ${error}`, 'error');
    } finally {
      isDownloading = false;
    }
  }

  // Start Geth
  async function handleStartGeth() {
    if (!isTauri() || !$walletAccount) return;

    isStartingGeth = true;
    try {
      await invoke('start_geth', { minerAddress: $walletAccount.address });
      toasts.show('Geth started successfully!', 'success');
      await loadStatus();
    } catch (error) {
      console.error('Failed to start Geth:', error);
      toasts.show(`Failed to start Geth: ${error}`, 'error');
    } finally {
      isStartingGeth = false;
    }
  }

  // Stop Geth
  async function handleStopGeth() {
    if (!isTauri()) return;

    try {
      await invoke('stop_geth');
      toasts.show('Geth stopped', 'info');
      await loadStatus();
    } catch (error) {
      console.error('Failed to stop Geth:', error);
      toasts.show(`Failed to stop Geth: ${error}`, 'error');
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
      console.error('Failed to start mining:', error);
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
      console.error('Failed to stop mining:', error);
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

  // Format bytes
  function formatBytes(bytes: number): string {
    if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(2)} GB`;
    if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(2)} MB`;
    if (bytes >= 1e3) return `${(bytes / 1e3).toFixed(2)} KB`;
    return `${bytes} B`;
  }
</script>

<div class="p-6 space-y-6 max-w-4xl mx-auto">
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-3xl font-bold">Mining</h1>
      <p class="text-gray-600 mt-1">Mine CHR tokens on the Chiral Network</p>
    </div>
    <button
      onclick={loadStatus}
      disabled={isLoading}
      class="p-2 hover:bg-gray-100 rounded-lg transition-colors disabled:opacity-50"
      title="Refresh status"
    >
      <RefreshCw class="w-5 h-5 {isLoading ? 'animate-spin' : ''}" />
    </button>
  </div>

  {#if isLoading}
    <div class="flex items-center justify-center py-12">
      <Loader2 class="w-8 h-8 animate-spin text-gray-400" />
    </div>
  {:else}
    <!-- Geth Status Card -->
    <div class="bg-white rounded-xl shadow-sm border border-gray-200 p-6">
      <div class="flex items-center justify-between mb-4">
        <div class="flex items-center gap-3">
          <div class="p-2 bg-blue-100 rounded-lg">
            <Cpu class="w-6 h-6 text-blue-600" />
          </div>
          <div>
            <h2 class="font-semibold">Blockchain Node (Geth)</h2>
            <p class="text-sm text-gray-500">Core-Geth for Chiral Network</p>
          </div>
        </div>
        <div class="flex items-center gap-2">
          {#if gethStatus?.running}
            <span class="flex items-center gap-2 px-3 py-1 bg-green-100 text-green-700 rounded-full text-sm">
              <span class="w-2 h-2 bg-green-500 rounded-full animate-pulse"></span>
              Running
            </span>
          {:else if gethStatus?.installed}
            <span class="flex items-center gap-2 px-3 py-1 bg-gray-100 text-gray-700 rounded-full text-sm">
              <span class="w-2 h-2 bg-gray-400 rounded-full"></span>
              Stopped
            </span>
          {:else}
            <span class="flex items-center gap-2 px-3 py-1 bg-yellow-100 text-yellow-700 rounded-full text-sm">
              <AlertTriangle class="w-4 h-4" />
              Not Installed
            </span>
          {/if}
        </div>
      </div>

      {#if !gethStatus?.installed}
        <!-- Download Geth Section -->
        <div class="bg-yellow-50 border border-yellow-200 rounded-lg p-4 mb-4">
          <div class="flex items-start gap-3">
            <AlertTriangle class="w-5 h-5 text-yellow-600 flex-shrink-0 mt-0.5" />
            <div>
              <p class="font-medium text-yellow-800">Geth Not Installed</p>
              <p class="text-sm text-yellow-700 mt-1">
                You need to download Core-Geth to mine on the Chiral Network.
              </p>
            </div>
          </div>
        </div>

        {#if isDownloading && downloadProgress}
          <div class="space-y-2">
            <div class="flex justify-between text-sm">
              <span>{downloadProgress.status}</span>
              <span>{downloadProgress.percentage.toFixed(1)}%</span>
            </div>
            <div class="w-full bg-gray-200 rounded-full h-2">
              <div
                class="bg-blue-600 h-2 rounded-full transition-all"
                style="width: {downloadProgress.percentage}%"
              ></div>
            </div>
            {#if downloadProgress.total > 0}
              <p class="text-xs text-gray-500 text-right">
                {formatBytes(downloadProgress.downloaded)} / {formatBytes(downloadProgress.total)}
              </p>
            {/if}
          </div>
        {:else}
          <button
            onclick={handleDownloadGeth}
            disabled={isDownloading}
            class="w-full px-4 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors flex items-center justify-center gap-2 disabled:opacity-50"
          >
            <Download class="w-5 h-5" />
            Download Geth
          </button>
        {/if}
      {:else}
        <!-- Geth Stats -->
        <div class="grid grid-cols-2 md:grid-cols-4 gap-4 mb-4">
          <div class="bg-gray-50 rounded-lg p-3">
            <p class="text-xs text-gray-500">Block Height</p>
            <p class="text-lg font-bold">{gethStatus?.currentBlock?.toLocaleString() || 0}</p>
          </div>
          <div class="bg-gray-50 rounded-lg p-3">
            <p class="text-xs text-gray-500">Peers</p>
            <p class="text-lg font-bold">{gethStatus?.peerCount || 0}</p>
          </div>
          <div class="bg-gray-50 rounded-lg p-3">
            <p class="text-xs text-gray-500">Chain ID</p>
            <p class="text-lg font-bold">{gethStatus?.chainId || 'N/A'}</p>
          </div>
          <div class="bg-gray-50 rounded-lg p-3">
            <p class="text-xs text-gray-500">Sync Status</p>
            <p class="text-lg font-bold">{gethStatus?.syncing ? 'Syncing' : 'Synced'}</p>
          </div>
        </div>

        <!-- Geth Controls -->
        <div class="flex gap-3">
          {#if gethStatus?.running}
            <button
              onclick={handleStopGeth}
              class="flex-1 px-4 py-2 bg-red-600 text-white rounded-lg hover:bg-red-700 transition-colors flex items-center justify-center gap-2"
            >
              <Square class="w-4 h-4" />
              Stop Geth
            </button>
          {:else}
            <button
              onclick={handleStartGeth}
              disabled={isStartingGeth}
              class="flex-1 px-4 py-2 bg-green-600 text-white rounded-lg hover:bg-green-700 transition-colors flex items-center justify-center gap-2 disabled:opacity-50"
            >
              {#if isStartingGeth}
                <Loader2 class="w-4 h-4 animate-spin" />
                Starting...
              {:else}
                <Play class="w-4 h-4" />
                Start Geth
              {/if}
            </button>
          {/if}
        </div>
      {/if}
    </div>

    <!-- Mining Control Card -->
    {#if gethStatus?.installed && gethStatus?.running}
      <div class="bg-white rounded-xl shadow-sm border border-gray-200 p-6">
        <div class="flex items-center justify-between mb-4">
          <div class="flex items-center gap-3">
            <div class="p-2 {miningStatus?.mining ? 'bg-yellow-100' : 'bg-gray-100'} rounded-lg">
              <Pickaxe class="w-6 h-6 {miningStatus?.mining ? 'text-yellow-600' : 'text-gray-600'}" />
            </div>
            <div>
              <h2 class="font-semibold">Mining</h2>
              <p class="text-sm text-gray-500">Earn CHR by mining blocks</p>
            </div>
          </div>
          <div class="flex items-center gap-2">
            {#if miningStatus?.mining}
              <span class="flex items-center gap-2 px-3 py-1 bg-yellow-100 text-yellow-700 rounded-full text-sm">
                <span class="w-2 h-2 bg-yellow-500 rounded-full animate-pulse"></span>
                Mining
              </span>
            {:else}
              <span class="flex items-center gap-2 px-3 py-1 bg-gray-100 text-gray-700 rounded-full text-sm">
                <span class="w-2 h-2 bg-gray-400 rounded-full"></span>
                Idle
              </span>
            {/if}
          </div>
        </div>

        <!-- Mining Stats -->
        <div class="grid grid-cols-2 gap-4 mb-4">
          <div class="bg-gray-50 rounded-lg p-4">
            <div class="flex items-center gap-2 mb-2">
              <Zap class="w-4 h-4 text-yellow-500" />
              <span class="text-sm text-gray-600">Hash Rate</span>
            </div>
            <p class="text-2xl font-bold">
              {miningStatus?.mining ? formatHashRate(miningStatus?.hashRate || 0) : '0 H/s'}
            </p>
          </div>
          <div class="bg-gray-50 rounded-lg p-4">
            <div class="flex items-center gap-2 mb-2">
              <TrendingUp class="w-4 h-4 text-green-500" />
              <span class="text-sm text-gray-600">Miner Address</span>
            </div>
            <p class="text-sm font-mono truncate">
              {miningStatus?.minerAddress || $walletAccount?.address || 'Not set'}
            </p>
          </div>
        </div>

        <!-- Thread Control -->
        <div class="mb-4">
          <label for="threads" class="block text-sm font-medium text-gray-700 mb-2">
            Mining Threads ({miningThreads} / {maxThreads})
          </label>
          <input
            id="threads"
            type="range"
            min="1"
            max={maxThreads}
            bind:value={miningThreads}
            disabled={miningStatus?.mining}
            class="w-full h-2 bg-gray-200 rounded-lg appearance-none cursor-pointer disabled:opacity-50"
          />
          <div class="flex justify-between text-xs text-gray-500 mt-1">
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
    {:else if gethStatus?.installed}
      <!-- Geth Not Running Warning -->
      <div class="bg-white rounded-xl shadow-sm border border-gray-200 p-6">
        <div class="flex items-center gap-3 mb-4">
          <div class="p-2 bg-gray-100 rounded-lg">
            <Pickaxe class="w-6 h-6 text-gray-400" />
          </div>
          <div>
            <h2 class="font-semibold text-gray-700">Mining</h2>
            <p class="text-sm text-gray-500">Start Geth to begin mining</p>
          </div>
        </div>
        <div class="bg-gray-50 rounded-lg p-4 text-center text-gray-500">
          <AlertTriangle class="w-8 h-8 mx-auto mb-2 opacity-50" />
          <p>Geth must be running to mine CHR tokens.</p>
          <p class="text-sm">Start Geth above to enable mining.</p>
        </div>
      </div>
    {/if}

    <!-- Info Card -->
    <div class="bg-white rounded-xl shadow-sm border border-gray-200 p-6">
      <div class="flex items-center gap-3 mb-4">
        <div class="p-2 bg-purple-100 rounded-lg">
          <BarChart3 class="w-6 h-6 text-purple-600" />
        </div>
        <div>
          <h2 class="font-semibold">Mining Information</h2>
          <p class="text-sm text-gray-500">How mining works on Chiral Network</p>
        </div>
      </div>
      <div class="space-y-3 text-sm text-gray-600">
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
