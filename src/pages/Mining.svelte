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

  interface MiningBalanceDiagnostic {
    address: string;
    localBalanceWei: string;
    localBalanceChi: number;
    localError: string | null;
    canonicalBalanceWei: string;
    canonicalBalanceChi: number;
    canonicalError: string | null;
    diverged: boolean;
  }

  let balanceDiagnostic = $state<MiningBalanceDiagnostic | null>(null);
  // Reset-local-chain affordance for the divergence path. The button on
  // the divergence banner opens a confirm dialog that warns the user
  // their local-fork mining rewards are unrecoverable; on confirm we
  // call `reset_local_chain` (stops Geth + wipes chaindata) and let the
  // user restart Geth to re-sync from canonical.
  let showResetConfirm = $state(false);
  let resettingChain = $state(false);

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

      // Compare what the local Geth reports vs the canonical RPC for
      // the miner's address. A divergence means the local node is on
      // a private fork (or the canonical RPC is unreachable) — the
      // user's "I mined N CHI on this page but my wallet shows 0"
      // experience.
      //
      // Only run the diagnostic against Geth's actual etherbase
      // (`mining.minerAddress`, derived from `eth_coinbase`). Falling
      // back to `$walletAccount.address` would query a different
      // address than Geth is mining to, masking real mismatches and
      // producing false reassurance: e.g. user has wallet 0xBob but
      // Geth has no etherbase set — the fallback queries 0xBob's
      // balance on local + canonical, both 0, no divergence flag,
      // even though the user can see "Total Mined: 5 CHI" because
      // Geth happened to mine to keystore account 0.
      const minerAddr = mining?.minerAddress;
      if (minerAddr) {
        balanceDiagnostic = await invoke<MiningBalanceDiagnostic>(
          'get_mining_balance_diagnostic',
          { address: minerAddr }
        ).catch(() => null);
      } else {
        balanceDiagnostic = null;
      }
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

  async function confirmResetLocalChain() {
    showResetConfirm = false;
    resettingChain = true;
    try {
      await invoke('reset_local_chain');
      // Drop the divergence banner immediately so the user sees the
      // reset took effect; loadStatus() below will repopulate from the
      // freshly-stopped Geth.
      balanceDiagnostic = null;
      toasts.detail(
        'Local chain reset',
        'Geth stopped and chaindata wiped. Start Geth again to re-sync from canonical.',
        'success',
      );
      await loadStatus();
    } catch (err: any) {
      toasts.detail('Reset failed', String(err), 'error');
    } finally {
      resettingChain = false;
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

<div class="p-4 sm:p-6 space-y-4 max-w-[1400px] mx-auto">
  <div class="flex items-center justify-between gap-3">
    <div>
      <h1 class="text-2xl font-bold text-gray-900 dark:text-white">Mining</h1>
      <p class="text-sm text-gray-500 dark:text-gray-400 mt-0.5">Earn CHI by mining blocks with your CPU or GPU.</p>
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
    <!-- Geth Not Running Locally — direct user to the Network page. -->
    <div class="bg-white dark:bg-gray-800 rounded-2xl border border-gray-200 dark:border-gray-700 p-5">
      <div class="flex items-start gap-3 mb-3">
        <div class="p-2 rounded-lg bg-yellow-100 dark:bg-yellow-900/30 shrink-0">
          <AlertTriangle class="w-5 h-5 text-yellow-600 dark:text-yellow-400" />
        </div>
        <div class="min-w-0">
          <h2 class="text-base font-semibold text-gray-900 dark:text-white">Local blockchain node required</h2>
          <p class="text-sm text-gray-600 dark:text-gray-400 mt-0.5">
            {#if !gethStatus?.installed}
              Geth isn't installed yet — download it from the Network page before you can mine.
            {:else}
              Start your local Geth node from the Network page to begin mining.
            {/if}
          </p>
        </div>
      </div>
      <button
        onclick={() => goto('/network')}
        class="w-full px-4 py-2.5 bg-primary-600 text-white rounded-lg hover:bg-primary-700 transition-colors flex items-center justify-center gap-2 text-sm font-medium"
      >
        <Globe class="w-4 h-4" />
        Open Network page
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
            <h2 class="text-lg font-semibold text-gray-900 dark:text-white">Mining</h2>
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

      <!-- Mining stats — icon-pill cards, same idiom as Drive/Download. -->
      <div class="grid grid-cols-2 md:grid-cols-3 gap-2 mb-4">
        <div class="flex items-center gap-3 px-3 py-2.5 rounded-xl bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700">
          <div class="p-2 rounded-lg bg-yellow-100 dark:bg-yellow-900/30 shrink-0">
            <Zap class="w-4 h-4 text-yellow-600 dark:text-yellow-400 {isAnyMining ? 'animate-pulse' : ''}" />
          </div>
          <div class="min-w-0">
            <div class="text-[11px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Hash rate</div>
            <div class="text-sm font-semibold text-gray-900 dark:text-white tabular-nums truncate">
              {isAnyMining ? formatHashRate(displayHashRate) : '0 H/s'}
            </div>
          </div>
        </div>
        <div class="flex items-center gap-3 px-3 py-2.5 rounded-xl bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700">
          <div class="p-2 rounded-lg bg-red-100 dark:bg-red-900/30 shrink-0">
            <Blocks class="w-4 h-4 text-red-600 dark:text-red-400" />
          </div>
          <div class="min-w-0">
            <div class="text-[11px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Block height</div>
            <div class="text-sm font-semibold text-gray-900 dark:text-white tabular-nums truncate">
              {gethStatus?.currentBlock?.toLocaleString() ?? '0'}
            </div>
          </div>
        </div>
        <div class="flex items-start gap-3 px-3 py-2.5 rounded-xl bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700">
          <div class="p-2 rounded-lg bg-amber-100 dark:bg-amber-900/30 shrink-0">
            <Coins class="w-4 h-4 text-amber-600 dark:text-amber-400" />
          </div>
          <div class="min-w-0">
            <div class="text-[11px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Total mined</div>
            <div class="text-sm font-semibold text-gray-900 dark:text-white tabular-nums truncate">
              {(miningStatus?.totalMinedChi ?? 0).toFixed(4)} CHI
            </div>
            {#if balanceDiagnostic?.canonicalError}
              <p class="text-[11px] mt-1 text-amber-600 dark:text-amber-400" title={balanceDiagnostic.canonicalError}>
                ⚠ Canonical RPC unreachable — wallet may show stale 0
              </p>
            {:else if balanceDiagnostic?.diverged}
              <div class="mt-1 text-[11px] text-red-600 dark:text-red-400 space-y-1">
                <p>
                  ⚠ Local Geth is on a private fork ({balanceDiagnostic.canonicalBalanceChi.toFixed(4)} CHI on canonical).
                </p>
                <p class="text-red-500 dark:text-red-400/80">
                  Rewards on this fork won't appear in your wallet — reset to re-sync from canonical.
                </p>
                <button
                  onclick={() => showResetConfirm = true}
                  disabled={resettingChain}
                  class="mt-1 px-2 py-0.5 text-[11px] rounded border border-red-400 dark:border-red-500/60 hover:bg-red-50 dark:hover:bg-red-900/30 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  {resettingChain ? 'Resetting…' : 'Reset local chain'}
                </button>
              </div>
            {/if}
          </div>
        </div>
        <div class="flex items-center gap-3 px-3 py-2.5 rounded-xl bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700">
          <div class="p-2 rounded-lg bg-purple-100 dark:bg-purple-900/30 shrink-0">
            <Clock class="w-4 h-4 text-purple-600 dark:text-purple-400" />
          </div>
          <div class="min-w-0">
            <div class="text-[11px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Session time</div>
            <div class="text-sm font-semibold text-gray-900 dark:text-white tabular-nums truncate {isAnyMining ? 'animate-pulse' : ''}">
              {isAnyMining ? miningElapsed : '--:--:--'}
            </div>
          </div>
        </div>
        <div class="flex items-center gap-3 px-3 py-2.5 rounded-xl bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700">
          {#if activeMiningBackend === 'gpu'}
            <div class="p-2 rounded-lg bg-cyan-100 dark:bg-cyan-900/30 shrink-0">
              <Monitor class="w-4 h-4 text-cyan-600 dark:text-cyan-400" />
            </div>
          {:else}
            <div class="p-2 rounded-lg bg-green-100 dark:bg-green-900/30 shrink-0">
              <Cpu class="w-4 h-4 text-green-600 dark:text-green-400" />
            </div>
          {/if}
          <div class="min-w-0">
            <div class="text-[11px] uppercase tracking-wider text-gray-500 dark:text-gray-400">
              {activeMiningBackend === 'gpu' ? 'GPUs active' : 'Threads active'}
            </div>
            <div class="text-sm font-semibold text-gray-900 dark:text-white tabular-nums truncate">
              {#if activeMiningBackend === 'gpu'}
                {isAnyMining ? `${activeGpuCount}` : '0'}
              {:else}
                {isAnyMining ? `${miningThreads} / ${maxThreads}` : `0 / ${maxThreads}`}
              {/if}
              <span class="text-[11px] font-normal text-gray-500 dark:text-gray-400 ml-1">
                · {activeMiningBackend === 'gpu' ? activeGpuUtilization : cpuUtilizationPercent}%
              </span>
            </div>
          </div>
        </div>
      </div>

      <!-- Miner Address -->
      <div class="mb-4 flex items-start gap-3 px-3 py-2.5 rounded-xl bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700">
        <div class="p-2 rounded-lg bg-green-100 dark:bg-green-900/30 shrink-0">
          <TrendingUp class="w-4 h-4 text-green-600 dark:text-green-400" />
        </div>
        <div class="min-w-0 flex-1">
          <div class="text-[11px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Miner address</div>
          <p class="text-sm font-mono truncate text-gray-900 dark:text-gray-200">
            {miningStatus?.minerAddress || $walletAccount?.address || 'Not set'}
          </p>
        {#if miningStatus?.minerAddress && $walletAccount?.address && miningStatus.minerAddress.toLowerCase() !== $walletAccount.address.toLowerCase()}
          <!-- Geth's active etherbase differs from the connected wallet
               — the user is mining to a different address than the one
               their wallet shows. Likely cause: Geth was started with a
               different etherbase (e.g. via daemon CLI) before the
               wallet was connected. handleStartMining() syncs them on
               next CPU/GPU mining start, but rewards already mined
               under the old etherbase land in that other address. -->
          <p class="mt-1 text-xs text-amber-600 dark:text-amber-400">
            ⚠ Mining to a different address than your connected wallet ({$walletAccount.address.slice(0, 6)}…{$walletAccount.address.slice(-4)}). Click "Start mining" to switch Geth to your wallet.
          </p>
        {/if}
        </div>
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

<!-- Reset-local-chain confirm dialog. Shown when the divergence banner's
     "Reset local chain" button is clicked. Wipes chaindata so the next
     Geth start re-syncs from canonical. Mining rewards on the local
     fork are unrecoverable — the dialog says so plainly. -->
{#if showResetConfirm}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
    onclick={() => showResetConfirm = false}
    onkeydown={(e) => { if (e.key === 'Escape') showResetConfirm = false; }}
  >
    <div
      class="bg-white dark:bg-gray-800 rounded-2xl shadow-2xl p-6 max-w-md w-full mx-4"
      onclick={(e) => e.stopPropagation()}
    >
      <h3 class="text-lg font-semibold text-gray-900 dark:text-white mb-2 flex items-center gap-2">
        <AlertTriangle class="w-5 h-5 text-red-500" />
        Reset local chain?
      </h3>
      <p class="text-sm text-gray-600 dark:text-gray-300 mb-3">
        This will stop Geth and delete the local chain data. After
        reset, start Geth again and it'll re-sync from the canonical
        bootstrap node.
      </p>
      <p class="text-sm text-red-600 dark:text-red-400 mb-4">
        Any CHI mined on the current local fork is unrecoverable — those
        blocks were never on the canonical chain. Your wallet keys are
        unaffected.
      </p>
      <div class="flex justify-end gap-3">
        <button
          onclick={() => showResetConfirm = false}
          class="px-4 py-2 text-sm font-medium rounded-lg text-gray-700 dark:text-gray-300 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 transition"
        >Cancel</button>
        <button
          onclick={confirmResetLocalChain}
          class="px-4 py-2 text-sm font-medium rounded-lg text-white bg-red-600 hover:bg-red-700 transition"
        >Reset chain</button>
      </div>
    </div>
  </div>
{/if}
