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
    History,
    ChevronDown,
    ChevronUp,
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
    lastError: string | null;
  }

  interface GpuMiningStatus {
    running: boolean;
    hashRate: number;
    activeDevices: string[];
    lastError: string | null;
  }

  interface MinedBlock {
    blockNumber: number;
    timestamp: number;
    rewardWei: string;
    rewardChi: number;
    difficulty: number;
  }

  type MiningMode = 'cpu' | 'gpu';

  const hardwareThreads = typeof navigator !== 'undefined' ? navigator.hardwareConcurrency || 4 : 4;
  const savedThreads = typeof window !== 'undefined' ? localStorage.getItem('chiral-mining-threads') : null;
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

  let minedBlocks = $state<MinedBlock[]>([]);
  let isLoadingHistory = $state(false);
  let showHistory = $state(true);
  let isLoading = $state(true);
  let isStartingMining = $state(false);
  let maxThreads = $state(hardwareThreads);
  let refreshInterval: ReturnType<typeof setInterval> | null = null;
  let miningThreads = $state(
    savedThreads ? Math.min(parseInt(savedThreads, 10) || 1, hardwareThreads) : 1
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

  $effect(() => {
    if (typeof window !== 'undefined') {
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
      loadMinedBlocks();
      refreshInterval = setInterval(() => {
        loadStatus();
      }, 5000);
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
        lastError: String(error)
      };
    }
  }

  async function handleStartMining() {
    if (!isTauri()) return;

    isStartingMining = true;
    try {
      if ($walletAccount?.address) {
        await invoke('set_miner_address', { address: $walletAccount.address });
      }

      if (miningMode === 'gpu') {
        if (!gpuCapabilities?.supported) {
          throw new Error(
            'GPU miner is not available. Install ethminer or set CHIRAL_GPU_MINER_PATH.'
          );
        }
        await invoke('start_gpu_mining', {
          deviceIds: selectedGpuDevices.length > 0 ? selectedGpuDevices : null
        });
        toasts.show(
          `GPU mining started${
            selectedGpuDevices.length > 0 ? ` (${selectedGpuDevices.length} device(s))` : ''
          }!`,
          'success'
        );
      } else {
        await invoke('start_mining', { threads: miningThreads });
        toasts.show(`CPU mining started with ${miningThreads} thread(s)!`, 'success');
      }

      await Promise.all([loadStatus(), loadGpuCapabilities()]);
    } catch (error) {
      log.error('Failed to start mining:', error);
      toasts.show(`Failed to start mining: ${error}`, 'error');
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
      toasts.show('Mining stopped', 'info');
      await Promise.all([loadStatus(), loadGpuCapabilities()]);
    } catch (error) {
      log.error('Failed to stop mining:', error);
      toasts.show(`Failed to stop mining: ${error}`, 'error');
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
  }

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

  let totalHistoryReward = $derived(minedBlocks.reduce((sum, b) => sum + b.rewardChi, 0));

  function formatTimestamp(ts: number): string {
    if (ts === 0) return 'Unknown';
    return new Date(ts * 1000).toLocaleString();
  }

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
      <p class="text-gray-600 dark:text-gray-400 mt-1">Mine CHI tokens on the Chiral Network</p>
    </div>
    <button
      onclick={refreshAll}
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
    <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6">
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

      <!-- Mining Stats Grid -->
      <div class="grid grid-cols-2 md:grid-cols-3 gap-4 mb-4">
        <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-4">
          <div class="flex items-center gap-2 mb-2">
            <Zap class="w-4 h-4 text-yellow-500" />
            <span class="text-sm text-gray-600 dark:text-gray-400">Hash Rate</span>
          </div>
          <p class="text-2xl font-bold dark:text-white">
            {isAnyMining ? formatHashRate(displayHashRate) : '0 H/s'}
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
            {(miningStatus?.totalMinedChi ?? 0).toFixed(4)} CHI
          </p>
        </div>
        <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-4">
          <div class="flex items-center gap-2 mb-2">
            <Clock class="w-4 h-4 text-purple-500" />
            <span class="text-sm text-gray-600 dark:text-gray-400">Session Time</span>
          </div>
          <p class="text-2xl font-bold dark:text-white">
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
          <p class="text-2xl font-bold dark:text-white">
            {#if activeMiningBackend === 'gpu'}
              {isAnyMining ? `${activeGpuCount}` : '0'}
            {:else}
              {isAnyMining ? `${miningThreads} / ${maxThreads}` : `0 / ${maxThreads}`}
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

      <!-- Mining Backend Mode -->
      <div class="mb-4">
        <div class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
          Mining Backend
        </div>
        <div class="grid grid-cols-2 gap-2">
          <button
            onclick={() => (miningMode = 'cpu')}
            disabled={isAnyMining}
            class="px-3 py-2 rounded-lg border text-sm font-medium transition-colors disabled:opacity-50 {miningMode === 'cpu' ? 'bg-primary-100 dark:bg-primary-900/30 border-primary-300 dark:border-primary-700 text-primary-700 dark:text-primary-300' : 'bg-white dark:bg-gray-700 border-gray-200 dark:border-gray-600 text-gray-700 dark:text-gray-300'}"
          >
            CPU Miner
          </button>
          <button
            onclick={() => (miningMode = 'gpu')}
            disabled={isAnyMining}
            class="px-3 py-2 rounded-lg border text-sm font-medium transition-colors disabled:opacity-50 {miningMode === 'gpu' ? 'bg-primary-100 dark:bg-primary-900/30 border-primary-300 dark:border-primary-700 text-primary-700 dark:text-primary-300' : 'bg-white dark:bg-gray-700 border-gray-200 dark:border-gray-600 text-gray-700 dark:text-gray-300'}"
          >
            GPU Miner
          </button>
        </div>
      </div>

      {#if miningMode === 'cpu'}
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
            disabled={isAnyMining}
            class="w-full h-2 bg-gray-200 dark:bg-gray-600 rounded-lg appearance-none cursor-pointer disabled:opacity-50"
          />
          <div class="flex justify-between text-xs text-gray-500 dark:text-gray-400 mt-1">
            <span>1 thread</span>
            <span>{maxThreads} threads (max)</span>
          </div>
        </div>
      {:else}
        <!-- GPU Control -->
        <div class="mb-4">
          {#if !gpuCapabilities?.supported}
            <div class="rounded-lg border border-amber-200 dark:border-amber-700 bg-amber-50 dark:bg-amber-900/20 p-3">
              <p class="text-sm text-amber-800 dark:text-amber-300">
                GPU miner is unavailable. Install `ethminer` or set `CHIRAL_GPU_MINER_PATH`.
              </p>
            </div>
          {:else}
            <div class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">
              GPU Devices ({selectedGpuDevices.length} selected)
            </div>
            {#if gpuCapabilities.devices.length === 0}
              <div class="rounded-lg border border-gray-200 dark:border-gray-700 bg-gray-50 dark:bg-gray-700 p-3 text-sm text-gray-600 dark:text-gray-300">
                No devices were reported by the miner binary. You can still try starting GPU mining with auto-detection.
              </div>
            {:else}
              <div class="space-y-2 max-h-44 overflow-y-auto rounded-lg border border-gray-200 dark:border-gray-700 p-3 bg-gray-50 dark:bg-gray-700">
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
                    <span>{device.name}</span>
                  </label>
                {/each}
              </div>
            {/if}
          {/if}
        </div>
      {/if}

      {#if gpuMiningStatus?.lastError}
        <div class="mb-4 rounded-lg border border-red-200 dark:border-red-700 bg-red-50 dark:bg-red-900/20 p-3">
          <p class="text-sm text-red-700 dark:text-red-300">{gpuMiningStatus.lastError}</p>
        </div>
      {/if}

      <!-- Mining Controls -->
      <div class="flex gap-3">
        {#if isAnyMining}
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
            disabled={isStartingMining || (miningMode === 'gpu' && !gpuCapabilities?.supported)}
            class="flex-1 px-4 py-3 bg-yellow-500 text-white rounded-lg hover:bg-yellow-600 transition-colors flex items-center justify-center gap-2 disabled:opacity-50"
          >
            {#if isStartingMining}
              <Loader2 class="w-5 h-5 animate-spin" />
              Starting...
            {:else}
              <Pickaxe class="w-5 h-5" />
              {#if miningMode === 'gpu'}
                Start GPU Mining
              {:else}
                Start CPU Mining
              {/if}
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
                — {totalHistoryReward.toFixed(2)} CHI earned
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
              <p class="text-xs text-gray-400 dark:text-gray-500 mt-1">Start mining to earn CHI block rewards.</p>
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
                <p class="text-lg font-bold text-emerald-600 dark:text-emerald-400">{totalHistoryReward.toFixed(2)} CHI</p>
              </div>
              <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-3">
                <p class="text-xs text-gray-500 dark:text-gray-400">Reward per Block</p>
                <p class="text-lg font-bold dark:text-white">{minedBlocks[0]?.rewardChi ?? 0} CHI</p>
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
                        +{block.rewardChi} CHI
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
