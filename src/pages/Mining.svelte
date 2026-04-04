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
    Blocks,
    Coins,
    Clock,
    Cpu,
    Monitor
  } from 'lucide-svelte';
  import { logger } from '$lib/logger';
  const log = logger('Mining');

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
    totalMinedChi: number;
  }

  interface GpuDevice {
    id: string;
    name: string;
  }

  interface GpuMiningCapabilities {
    supported: boolean;
    binaryPath: string | null;
    devices: GpuDevice[];
    running: boolean;
    activeDevices: string[];
    utilizationPercent: number;
    lastError: string | null;
  }

  interface GpuMiningStatus {
    running: boolean;
    hashRate: number;
    activeDevices: string[];
    utilizationPercent: number;
    lastError: string | null;
  }

  type MiningMode = 'cpu' | 'gpu';
  const MIN_UTILIZATION_PERCENT = 10;
  const MAX_UTILIZATION_PERCENT = 100;

  function clampUtilizationPercent(value: number): number {
    if (!Number.isFinite(value)) return MAX_UTILIZATION_PERCENT;
    return Math.max(MIN_UTILIZATION_PERCENT, Math.min(MAX_UTILIZATION_PERCENT, Math.round(value)));
  }

  const hardwareThreads = typeof navigator !== 'undefined' ? navigator.hardwareConcurrency || 4 : 4;
  const savedThreads = typeof window !== 'undefined' ? localStorage.getItem('chiral-mining-threads') : null;
  const savedCpuUtilizationRaw =
    typeof window !== 'undefined' ? localStorage.getItem('chiral-cpu-utilization-percent') : null;
  const savedGpuUtilizationRaw =
    typeof window !== 'undefined' ? localStorage.getItem('chiral-gpu-utilization-percent') : null;
  const savedMode = typeof window !== 'undefined' ? localStorage.getItem('chiral-mining-mode') : null;
  const savedGpuDevicesRaw =
    typeof window !== 'undefined' ? localStorage.getItem('chiral-gpu-devices') : null;
  let initialGpuDevices: string[] = [];
  if (savedGpuDevicesRaw) {
    try {
      const parsed = JSON.parse(savedGpuDevicesRaw);
      if (Array.isArray(parsed)) {
        initialGpuDevices = parsed.filter((v): v is string => typeof v === 'string');
      }
    } catch {
      initialGpuDevices = [];
    }
  }

  let gethStatus = $state<GethStatus | null>(null);
  let miningStatus = $state<MiningStatus | null>(null);
  let gpuCapabilities = $state<GpuMiningCapabilities | null>(null);
  let gpuMiningStatus = $state<GpuMiningStatus | null>(null);
  let miningMode = $state<MiningMode>(savedMode === 'gpu' ? 'gpu' : 'cpu');
  let selectedGpuDevices = $state<string[]>(initialGpuDevices);

  let isLoading = $state(true);
  let isStartingMining = $state(false);
  let maxThreads = $state(hardwareThreads);
  let refreshInterval: ReturnType<typeof setInterval> | null = null;
  const initialCpuUtilization = (() => {
    if (savedCpuUtilizationRaw) {
      return clampUtilizationPercent(parseInt(savedCpuUtilizationRaw, 10));
    }
    if (savedThreads) {
      const parsedThreads = Math.max(1, Math.min(parseInt(savedThreads, 10) || 1, hardwareThreads));
      return clampUtilizationPercent((parsedThreads / hardwareThreads) * 100);
    }
    return clampUtilizationPercent((1 / hardwareThreads) * 100);
  })();
  let cpuUtilizationPercent = $state(initialCpuUtilization);
  let gpuUtilizationPercent = $state(
    savedGpuUtilizationRaw
      ? clampUtilizationPercent(parseInt(savedGpuUtilizationRaw, 10))
      : MAX_UTILIZATION_PERCENT
  );
  let miningThreads = $derived(
    Math.max(1, Math.min(maxThreads, Math.round((maxThreads * cpuUtilizationPercent) / 100)))
  );

  let miningStartTime = $state<number | null>(null);
  let miningElapsed = $state('00:00:00');
  let elapsedInterval: ReturnType<typeof setInterval> | null = null;

  let activeMiningBackend = $derived(
    gpuMiningStatus?.running ? 'gpu' : miningStatus?.mining ? 'cpu' : 'none'
  );
  let isAnyMining = $derived(activeMiningBackend !== 'none');
  let displayHashRate = $derived(
    activeMiningBackend === 'gpu' ? gpuMiningStatus?.hashRate || 0 : miningStatus?.hashRate || 0
  );
  let activeGpuCount = $derived(gpuMiningStatus?.activeDevices?.length || 0);
  let activeGpuUtilization = $derived(
    gpuMiningStatus?.running ? gpuMiningStatus.utilizationPercent : gpuUtilizationPercent
  );

  $effect(() => {
    if (typeof window !== 'undefined') {
      localStorage.setItem('chiral-cpu-utilization-percent', cpuUtilizationPercent.toString());
      localStorage.setItem('chiral-mining-threads', miningThreads.toString());
    }
  });

  $effect(() => {
    if (typeof window !== 'undefined') {
      localStorage.setItem('chiral-mining-mode', miningMode);
    }
  });

  $effect(() => {
    if (typeof window !== 'undefined') {
      localStorage.setItem('chiral-gpu-devices', JSON.stringify(selectedGpuDevices));
    }
  });

  $effect(() => {
    if (typeof window !== 'undefined') {
      localStorage.setItem('chiral-gpu-utilization-percent', gpuUtilizationPercent.toString());
    }
  });

  $effect(() => {
    if (isAnyMining && !miningStartTime) {
      const saved = typeof window !== 'undefined' ? localStorage.getItem('chiral-mining-start') : null;
      miningStartTime = saved ? parseInt(saved, 10) : Date.now();
      if (!saved && typeof window !== 'undefined') {
        localStorage.setItem('chiral-mining-start', miningStartTime.toString());
      }
      elapsedInterval = setInterval(updateElapsed, 1000);
    } else if (!isAnyMining && miningStartTime) {
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
    const h = Math.floor(diff / 3600)
      .toString()
      .padStart(2, '0');
    const m = Math.floor((diff % 3600) / 60)
      .toString()
      .padStart(2, '0');
    const s = (diff % 60).toString().padStart(2, '0');
    miningElapsed = `${h}:${m}:${s}`;
  }

  function isTauri(): boolean {
    return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
  }

  onMount(async () => {
    if (isTauri()) {
      await Promise.all([loadStatus(), loadGpuCapabilities()]);
      refreshInterval = setInterval(() => {
        loadStatus();
      }, 10000);
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
      miningStatus = null;
      gpuMiningStatus = null;
      return;
    }

    try {
      const [geth, mining, gpu] = await Promise.all([
        invoke<GethStatus>('get_geth_status'),
        invoke<MiningStatus>('get_mining_status').catch(() => null),
        invoke<GpuMiningStatus>('get_gpu_mining_status').catch(() => null)
      ]);

      gethStatus = geth;
      miningStatus = mining;
      gpuMiningStatus = gpu;
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

  async function loadGpuCapabilities() {
    if (!isTauri()) return;

    try {
      const caps = await invoke<GpuMiningCapabilities>('get_gpu_mining_capabilities');
      gpuCapabilities = caps;

      const ids = new Set(caps.devices.map((d) => d.id));
      selectedGpuDevices = selectedGpuDevices.filter((id) => ids.has(id));
      if (selectedGpuDevices.length === 0 && caps.devices.length > 0) {
        selectedGpuDevices = [caps.devices[0].id];
      }
    } catch (error) {
      log.error('Failed to load GPU capabilities:', error);
      gpuCapabilities = {
        supported: false,
        binaryPath: null,
        devices: [],
        running: false,
        activeDevices: [],
        utilizationPercent: gpuUtilizationPercent,
        lastError: String(error)
      };
    }
  }

  async function handleStartMining() {
    if (!isTauri()) return;

    isStartingMining = true;
    try {
      if (!$walletAccount?.address) {
        throw new Error('No wallet address set. Please create or import a wallet first.');
      }

      // Set miner address before starting
      await invoke('set_miner_address', { address: $walletAccount.address });

      if (miningMode === 'gpu') {
        if (!gpuCapabilities?.binaryPath) {
          throw new Error(
            gpuCapabilities?.lastError ||
              'GPU miner is still being prepared. Wait a moment and refresh.'
          );
        }
        await invoke('start_gpu_mining', {
          deviceIds: selectedGpuDevices.length > 0 ? selectedGpuDevices : null,
          utilizationPercent: gpuUtilizationPercent
        });
        toasts.notifyDetail('miningBlock', 'GPU mining started', `${selectedGpuDevices.length || 'All'} device${selectedGpuDevices.length !== 1 ? 's' : ''} at ${gpuUtilizationPercent}% utilization`, 'success');
      } else {
        await invoke('start_mining', { threads: miningThreads });
        toasts.notifyDetail('miningBlock', 'CPU mining started', `${miningThreads} thread${miningThreads !== 1 ? 's' : ''} at ${cpuUtilizationPercent}% target`, 'success');
      }

      await Promise.all([loadStatus(), loadGpuCapabilities()]);
    } catch (error) {
      log.error('Failed to start mining:', error);
      toasts.detail('Failed to start mining', String(error), 'error');
    } finally {
      isStartingMining = false;
    }
  }

  async function handleStopMining() {
    if (!isTauri()) return;

    try {
      if (activeMiningBackend === 'gpu') {
        await invoke('stop_gpu_mining');
      } else {
        await invoke('stop_mining');
      }
      // Silent — mining status reflected in UI
      await Promise.all([loadStatus(), loadGpuCapabilities()]);
    } catch (error) {
      log.error('Failed to stop mining:', error);
      toasts.detail('Failed to stop mining', String(error), 'error');
    }
  }

  function toggleGpuDevice(id: string) {
    if (selectedGpuDevices.includes(id)) {
      selectedGpuDevices = selectedGpuDevices.filter((v) => v !== id);
      return;
    }
    selectedGpuDevices = [...selectedGpuDevices, id];
  }

  async function refreshAll() {
    await Promise.all([loadStatus(), loadGpuCapabilities()]);
    toasts.show('Mining status refreshed', 'success');
  }

  function formatHashRate(rate: number): string {
    if (rate >= 1e9) return `${(rate / 1e9).toFixed(2)} GH/s`;
    if (rate >= 1e6) return `${(rate / 1e6).toFixed(2)} MH/s`;
    if (rate >= 1e3) return `${(rate / 1e3).toFixed(2)} KH/s`;
    return `${rate} H/s`;
  }

</script>

<svelte:head><title>Mining | Chiral Network</title></svelte:head>

<div class="p-4 sm:p-6 space-y-6 max-w-6xl mx-auto">
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-2xl font-bold dark:text-white">Mining</h1>
      <p class="text-gray-600 dark:text-gray-400 mt-1">Mine CHI tokens on the Chiral Network</p>
    </div>
    <button
      onclick={refreshAll}
      disabled={isLoading}
      class="p-2 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition-colors disabled:opacity-50 dark:text-gray-300 focus:outline-none focus:ring-2 focus:ring-gray-400/30"
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
    <div class="bg-white dark:bg-gray-800 rounded-2xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
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
            You need to download and start a local Geth node before you can mine CHI tokens.
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
    <div class="bg-white dark:bg-gray-800 rounded-2xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
      <div class="flex items-center justify-between mb-4">
        <div class="flex items-center gap-3">
          <div class="p-2 {isAnyMining ? 'bg-yellow-100 dark:bg-yellow-900/30' : 'bg-gray-100 dark:bg-gray-700'} rounded-lg">
            <Pickaxe class="w-6 h-6 {isAnyMining ? 'text-yellow-600 dark:text-yellow-400' : 'text-gray-600 dark:text-gray-400'}" />
          </div>
          <div>
            <h2 class="font-semibold dark:text-white">Mining</h2>
            <p class="text-sm text-gray-500 dark:text-gray-400">Earn CHI by mining blocks with CPU or GPU</p>
          </div>
        </div>
        <div class="flex items-center gap-2">
          {#if isAnyMining}
            <span class="flex items-center gap-2 px-3 py-1 bg-yellow-100 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-400 rounded-full text-sm">
              <span class="w-2 h-2 bg-yellow-500 rounded-full animate-pulse"></span>
              Mining ({activeMiningBackend.toUpperCase()})
            </span>
          {:else}
            <span class="flex items-center gap-2 px-3 py-1 bg-gray-100 dark:bg-gray-700 text-gray-700 dark:text-gray-300 rounded-full text-sm">
              <span class="w-2 h-2 bg-gray-400 rounded-full"></span>
              Idle
            </span>
          {/if}
        </div>
      </div>

      <!-- Syncing Warning -->
      {#if gethStatus?.syncing}
        <div class="mb-4 p-3 bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-700 rounded-lg flex items-center gap-3">
          <AlertTriangle class="w-5 h-5 text-amber-500 flex-shrink-0" />
          <div>
            <p class="text-sm font-medium text-amber-800 dark:text-amber-300">Node is syncing</p>
            <p class="text-xs text-amber-700 dark:text-amber-400">
              Block {gethStatus.currentBlock.toLocaleString()} / {gethStatus.highestBlock.toLocaleString()} — mining will produce blocks once sync completes.
            </p>
          </div>
        </div>
      {/if}

      <!-- Mining Stats Grid -->
      <div class="grid grid-cols-2 md:grid-cols-3 gap-4 mb-4">
        <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-4">
          <div class="flex items-center gap-2 mb-2">
            <Zap class="w-4 h-4 text-yellow-500" />
            <span class="text-sm text-gray-600 dark:text-gray-400">Hash Rate</span>
          </div>
          <p class="text-2xl font-bold tabular-nums dark:text-white">
            {isAnyMining ? formatHashRate(displayHashRate) : '0 H/s'}
          </p>
        </div>
        <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-4">
          <div class="flex items-center gap-2 mb-2">
            <Blocks class="w-4 h-4 text-red-500" />
            <span class="text-sm text-gray-600 dark:text-gray-400">Block Height</span>
          </div>
          <p class="text-2xl font-bold tabular-nums dark:text-white">
            {gethStatus?.currentBlock?.toLocaleString() ?? '0'}
          </p>
        </div>
        <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-4">
          <div class="flex items-center gap-2 mb-2">
            <Coins class="w-4 h-4 text-amber-500" />
            <span class="text-sm text-gray-600 dark:text-gray-400">Total Mined</span>
          </div>
          <p class="text-2xl font-bold tabular-nums dark:text-white">
            {(miningStatus?.totalMinedChi ?? 0).toFixed(4)} CHI
          </p>
        </div>
        <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-4">
          <div class="flex items-center gap-2 mb-2">
            <Clock class="w-4 h-4 text-purple-500" />
            <span class="text-sm text-gray-600 dark:text-gray-400">Session Time</span>
          </div>
          <p class="text-2xl font-bold tabular-nums dark:text-white {isAnyMining ? 'animate-pulse' : ''}">
            {isAnyMining ? miningElapsed : '--:--:--'}
          </p>
        </div>
        <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-4">
          <div class="flex items-center gap-2 mb-2">
            {#if activeMiningBackend === 'gpu'}
              <Monitor class="w-4 h-4 text-cyan-500" />
              <span class="text-sm text-gray-600 dark:text-gray-400">GPUs Active</span>
            {:else}
              <Cpu class="w-4 h-4 text-green-500" />
              <span class="text-sm text-gray-600 dark:text-gray-400">Threads Active</span>
            {/if}
          </div>
          <p class="text-2xl font-bold tabular-nums dark:text-white">
            {#if activeMiningBackend === 'gpu'}
              {isAnyMining ? `${activeGpuCount}` : '0'}
            {:else}
              {isAnyMining ? `${miningThreads} / ${maxThreads}` : `0 / ${maxThreads}`}
            {/if}
          </p>
          <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
            {#if activeMiningBackend === 'gpu'}
              Target {activeGpuUtilization}%
            {:else}
              Target {cpuUtilizationPercent}%
            {/if}
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

      <!-- CPU Section -->
      <div class="mb-4 p-4 bg-gray-50 dark:bg-gray-700/50 rounded-lg border border-gray-200 dark:border-gray-700">
        <div class="flex items-center justify-between mb-3">
          <div class="flex items-center gap-2">
            <Cpu class="w-4 h-4 text-green-500" />
            <span class="text-sm font-medium text-gray-700 dark:text-gray-300">CPU Mining</span>
          </div>
          {#if activeMiningBackend === 'cpu'}
            <span class="text-xs px-2 py-0.5 bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400 rounded-full">Active</span>
          {/if}
        </div>
        <div class="mb-3">
          <label for="cpu-utilization" class="block text-xs text-gray-500 dark:text-gray-400 mb-1">
            Utilization Target ({cpuUtilizationPercent}%) — {miningThreads} of {maxThreads} threads
          </label>
          <input
            id="cpu-utilization"
            type="range"
            min={MIN_UTILIZATION_PERCENT}
            max={MAX_UTILIZATION_PERCENT}
            step="1"
            bind:value={cpuUtilizationPercent}
            disabled={isAnyMining}
            class="w-full h-2 bg-gray-200 dark:bg-gray-600 rounded-lg appearance-none cursor-pointer disabled:opacity-50"
          />
        </div>
        {#if !isAnyMining}
          <button
            onclick={() => { miningMode = 'cpu'; handleStartMining(); }}
            disabled={isStartingMining}
            class="w-full px-3 py-2 bg-yellow-500 text-white rounded-lg hover:bg-yellow-600 transition-colors flex items-center justify-center gap-2 disabled:opacity-50 text-sm"
          >
            {#if isStartingMining && miningMode === 'cpu'}
              <Loader2 class="w-4 h-4 animate-spin" />
              Starting...
            {:else}
              <Pickaxe class="w-4 h-4" />
              Start CPU Mining
            {/if}
          </button>
        {:else if activeMiningBackend === 'cpu'}
          <button
            onclick={handleStopMining}
            class="w-full px-3 py-2 bg-red-600 text-white rounded-lg hover:bg-red-700 transition-colors flex items-center justify-center gap-2 text-sm"
          >
            <Square class="w-4 h-4" />
            Stop CPU Mining
          </button>
        {/if}
      </div>

      <!-- GPU Section -->
      <div class="mb-4 p-4 bg-gray-50 dark:bg-gray-700/50 rounded-lg border border-gray-200 dark:border-gray-700">
        <div class="flex items-center justify-between mb-3">
          <div class="flex items-center gap-2">
            <Monitor class="w-4 h-4 text-cyan-500" />
            <span class="text-sm font-medium text-gray-700 dark:text-gray-300">GPU Mining</span>
          </div>
          {#if activeMiningBackend === 'gpu'}
            <span class="text-xs px-2 py-0.5 bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400 rounded-full">Active</span>
          {/if}
        </div>

        {#if !gpuCapabilities?.binaryPath}
          <div class="rounded-lg border border-amber-200 dark:border-amber-700 bg-amber-50 dark:bg-amber-900/20 p-3 mb-3">
            {#if gpuCapabilities?.lastError}
              <p class="text-sm text-amber-800 dark:text-amber-300 whitespace-pre-line">
                {gpuCapabilities.lastError}
              </p>
            {:else}
              <p class="text-sm text-amber-800 dark:text-amber-300">
                Preparing GPU miner automatically. If this stays here, click refresh.
              </p>
            {/if}
          </div>
        {:else}
          {#if gpuCapabilities?.lastError}
            <div class="mb-3 rounded-lg border border-amber-200 dark:border-amber-700 bg-amber-50 dark:bg-amber-900/20 p-3">
              <p class="text-sm text-amber-800 dark:text-amber-300">
                GPU probe warning: {gpuCapabilities.lastError}
              </p>
              <p class="text-xs text-amber-700 dark:text-amber-400 mt-1">
                You can still start GPU mining and the app will retry with backend fallbacks automatically.
              </p>
            </div>
          {/if}

          <!-- GPU Utilization -->
          <div class="mb-3">
            <label for="gpu-utilization" class="block text-xs text-gray-500 dark:text-gray-400 mb-1">
              Utilization Target ({gpuUtilizationPercent}%)
            </label>
            <input
              id="gpu-utilization"
              type="range"
              min={MIN_UTILIZATION_PERCENT}
              max={MAX_UTILIZATION_PERCENT}
              step="1"
              bind:value={gpuUtilizationPercent}
              disabled={isAnyMining}
              class="w-full h-2 bg-gray-200 dark:bg-gray-600 rounded-lg appearance-none cursor-pointer disabled:opacity-50"
            />
          </div>

          <!-- GPU Devices -->
          <div class="mb-3">
            <div class="block text-xs text-gray-500 dark:text-gray-400 mb-1">
              Devices ({selectedGpuDevices.length} selected)
            </div>
            {#if gpuCapabilities.devices.length === 0}
              <div class="rounded-lg border border-gray-200 dark:border-gray-600 bg-white dark:bg-gray-700 p-2 text-xs text-gray-500 dark:text-gray-400">
                No devices detected. GPU mining will use auto-detection.
              </div>
            {:else}
              <div class="space-y-1.5 max-h-32 overflow-y-auto rounded-lg border border-gray-200 dark:border-gray-600 p-2 bg-white dark:bg-gray-700">
                {#each gpuCapabilities.devices as device (device.id)}
                  <label class="flex items-center gap-2 text-sm text-gray-700 dark:text-gray-300">
                    <input
                      type="checkbox"
                      checked={selectedGpuDevices.includes(device.id)}
                      onchange={() => toggleGpuDevice(device.id)}
                      disabled={isAnyMining}
                      class="rounded border-gray-300 dark:border-gray-600 bg-white dark:bg-gray-800"
                    />
                    <span class="font-mono text-xs text-gray-500 dark:text-gray-400">[{device.id}]</span>
                    <span class="text-sm truncate" title={device.name}>{device.name}</span>
                  </label>
                {/each}
              </div>
            {/if}
          </div>
        {/if}

        {#if gpuMiningStatus?.lastError}
          <div class="mb-3 rounded-lg border border-red-200 dark:border-red-700 bg-red-50 dark:bg-red-900/20 p-3">
            <p class="text-sm text-red-700 dark:text-red-300">{gpuMiningStatus.lastError}</p>
          </div>
        {/if}

        {#if !isAnyMining}
          <button
            onclick={() => { miningMode = 'gpu'; handleStartMining(); }}
            disabled={isStartingMining || !gpuCapabilities?.binaryPath}
            class="w-full px-3 py-2 bg-yellow-500 text-white rounded-lg hover:bg-yellow-600 transition-colors flex items-center justify-center gap-2 disabled:opacity-50 text-sm"
          >
            {#if isStartingMining && miningMode === 'gpu'}
              <Loader2 class="w-4 h-4 animate-spin" />
              Starting...
            {:else}
              <Pickaxe class="w-4 h-4" />
              Start GPU Mining
            {/if}
          </button>
        {:else if activeMiningBackend === 'gpu'}
          <button
            onclick={handleStopMining}
            class="w-full px-3 py-2 bg-red-600 text-white rounded-lg hover:bg-red-700 transition-colors flex items-center justify-center gap-2 text-sm"
          >
            <Square class="w-4 h-4" />
            Stop GPU Mining
          </button>
        {/if}
      </div>

      {#if isAnyMining}
        <p class="text-xs text-gray-500 dark:text-gray-400 text-center">CPU and GPU mining are mutually exclusive — stop one to start the other.</p>
      {/if}
    </div>

  {/if}
</div>
