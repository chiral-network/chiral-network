<script lang="ts">
 import { onMount, onDestroy } from'svelte';
 import { invoke } from'@tauri-apps/api/core';
 import { goto } from'@mateothegreat/svelte5-router';
 import { walletAccount } from'$lib/stores';
 import { toasts } from'$lib/toastStore';
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
 } from'lucide-svelte';
 import { logger } from'$lib/logger';
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

 interface MinedBlock {
 blockNumber: number;
 timestamp: number;
 rewardWei: string;
 rewardChi: number;
 difficulty: number;
 }

 type MiningMode ='cpu' |'gpu';
 const MIN_UTILIZATION_PERCENT = 10;
 const MAX_UTILIZATION_PERCENT = 100;

 function clampUtilizationPercent(value: number): number {
 if (!Number.isFinite(value)) return MAX_UTILIZATION_PERCENT;
 return Math.max(MIN_UTILIZATION_PERCENT, Math.min(MAX_UTILIZATION_PERCENT, Math.round(value)));
 }

 const hardwareThreads = typeof navigator !=='undefined' ? navigator.hardwareConcurrency || 4 : 4;
 const savedThreads = typeof window !=='undefined' ? localStorage.getItem('chiral-mining-threads') : null;
 const savedCpuUtilizationRaw =
 typeof window !=='undefined' ? localStorage.getItem('chiral-cpu-utilization-percent') : null;
 const savedGpuUtilizationRaw =
 typeof window !=='undefined' ? localStorage.getItem('chiral-gpu-utilization-percent') : null;
 const savedMode = typeof window !=='undefined' ? localStorage.getItem('chiral-mining-mode') : null;
 const savedGpuDevicesRaw =
 typeof window !=='undefined' ? localStorage.getItem('chiral-gpu-devices') : null;
 let initialGpuDevices: string[] = [];
 if (savedGpuDevicesRaw) {
 try {
 const parsed = JSON.parse(savedGpuDevicesRaw);
 if (Array.isArray(parsed)) {
 initialGpuDevices = parsed.filter((v): v is string => typeof v ==='string');
 }
 } catch {
 initialGpuDevices = [];
 }
 }

 let gethStatus = $state<GethStatus | null>(null);
 let miningStatus = $state<MiningStatus | null>(null);
 let gpuCapabilities = $state<GpuMiningCapabilities | null>(null);
 let gpuMiningStatus = $state<GpuMiningStatus | null>(null);
 let miningMode = $state<MiningMode>(savedMode ==='gpu' ?'gpu' :'cpu');
 let selectedGpuDevices = $state<string[]>(initialGpuDevices);

 let minedBlocks = $state<MinedBlock[]>([]);
 let isLoadingHistory = $state(false);
 let showHistory = $state(true);
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
 gpuMiningStatus?.running ?'gpu' : miningStatus?.mining ?'cpu' :'none'
 );
 let isAnyMining = $derived(activeMiningBackend !=='none');
 let displayHashRate = $derived(
 activeMiningBackend ==='gpu' ? gpuMiningStatus?.hashRate || 0 : miningStatus?.hashRate || 0
 );
 let activeGpuCount = $derived(gpuMiningStatus?.activeDevices?.length || 0);
 let activeGpuUtilization = $derived(
 gpuMiningStatus?.running ? gpuMiningStatus.utilizationPercent : gpuUtilizationPercent
 );

 $effect(() => {
 if (typeof window !=='undefined') {
 localStorage.setItem('chiral-cpu-utilization-percent', cpuUtilizationPercent.toString());
 localStorage.setItem('chiral-mining-threads', miningThreads.toString());
 }
 });

 $effect(() => {
 if (typeof window !=='undefined') {
 localStorage.setItem('chiral-mining-mode', miningMode);
 }
 });

 $effect(() => {
 if (typeof window !=='undefined') {
 localStorage.setItem('chiral-gpu-devices', JSON.stringify(selectedGpuDevices));
 }
 });

 $effect(() => {
 if (typeof window !=='undefined') {
 localStorage.setItem('chiral-gpu-utilization-percent', gpuUtilizationPercent.toString());
 }
 });

 $effect(() => {
 if (isAnyMining && !miningStartTime) {
 const saved = typeof window !=='undefined' ? localStorage.getItem('chiral-mining-start') : null;
 miningStartTime = saved ? parseInt(saved, 10) : Date.now();
 if (!saved && typeof window !=='undefined') {
 localStorage.setItem('chiral-mining-start', miningStartTime.toString());
 }
 elapsedInterval = setInterval(updateElapsed, 1000);
 } else if (!isAnyMining && miningStartTime) {
 miningStartTime = null;
 if (typeof window !=='undefined') {
 localStorage.removeItem('chiral-mining-start');
 }
 if (elapsedInterval) {
 clearInterval(elapsedInterval);
 elapsedInterval = null;
 }
 miningElapsed ='00:00:00';
 }
 });

 function updateElapsed() {
 if (!miningStartTime) return;
 const diff = Math.floor((Date.now() - miningStartTime) / 1000);
 const h = Math.floor(diff / 3600)
 .toString()
 .padStart(2,'0');
 const m = Math.floor((diff % 3600) / 60)
 .toString()
 .padStart(2,'0');
 const s = (diff % 60).toString().padStart(2,'0');
 miningElapsed = `${h}:${m}:${s}`;
 }

 function isTauri(): boolean {
 return typeof window !=='undefined' &&'__TAURI_INTERNALS__' in window;
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
 utilizationPercent: gpuUtilizationPercent,
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

 if (miningMode ==='gpu') {
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
 toasts.show(
 `GPU mining started${
 selectedGpuDevices.length > 0 ? ` (${selectedGpuDevices.length} device(s))` :''
 } at ${gpuUtilizationPercent}% utilization target!`,
'success'
 );
 } else {
 await invoke('start_mining', { threads: miningThreads });
 toasts.show(
 `CPU mining started with ${miningThreads} thread(s) (${cpuUtilizationPercent}% target)!`,
'success'
 );
 }

 await Promise.all([loadStatus(), loadGpuCapabilities()]);
 } catch (error) {
 log.error('Failed to start mining:', error);
 toasts.show(`Failed to start mining: ${error}`,'error');
 } finally {
 isStartingMining = false;
 }
 }

 async function handleStopMining() {
 if (!isTauri()) return;

 try {
 if (activeMiningBackend ==='gpu') {
 await invoke('stop_gpu_mining');
 } else {
 await invoke('stop_mining');
 }
 toasts.show('Mining stopped','info');
 await Promise.all([loadStatus(), loadGpuCapabilities()]);
 } catch (error) {
 log.error('Failed to stop mining:', error);
 toasts.show(`Failed to stop mining: ${error}`,'error');
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
 if (ts === 0) return'Unknown';
 return new Date(ts * 1000).toLocaleString();
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
 <h1 class="text-2xl font-bold">Mining</h1>
 <p class="text-white/50 mt-1">Mine CHI tokens on the Chiral Network</p>
 </div>
 <button
 onclick={refreshAll}
 disabled={isLoading}
 class="p-2 hover:bg-white/[0.05] rounded-lg transition-colors disabled:opacity-50 focus:outline-none"
 title="Refresh status"
 >
 <RefreshCw class="w-5 h-5 {isLoading ?'animate-spin' :''}" />
 </button>
 </div>

 {#if isLoading}
 <div class="flex items-center justify-center py-12">
 <Loader2 class="w-8 h-8 animate-spin text-white/50" />
 </div>
 {:else if !gethStatus?.installed || !gethStatus?.localRunning}
 <!-- Geth Not Running Locally - Direct to Network Page -->
 <div class="bg-white/[0.05] rounded-xl shadow-black/5 border border-white/[0.06] p-6">
 <div class="flex items-center gap-3 mb-4">
 <div class="p-2 bg-yellow-100 rounded-lg">
 <AlertTriangle class="w-6 h-6 text-yellow-400" />
 </div>
 <div>
 <h2 class="font-semibold">Local Blockchain Node Required</h2>
 <p class="text-sm text-white/40">
 {#if !gethStatus?.installed}
 Geth is not installed
 {:else}
 Local Geth node is not running
 {/if}
 </p>
 </div>
 </div>
 <div class="bg-yellow-500/[0.1] border border-yellow-200 rounded-lg p-4 mb-4">
 <p class="text-sm text-yellow-800">
 {#if !gethStatus?.installed}
 You need to download and start a local Geth node before you can mine CHI tokens.
 {:else}
 Mining requires a local Geth node. Start the node from the Network page to begin mining.
 {/if}
 </p>
 <p class="text-sm text-yellow-400 mt-2">
 Go to the <strong>Network</strong> page to start your local blockchain node.
 </p>
 </div>
 <button
 onclick={() => goto('/network')}
 class="w-full px-4 py-3 bg-violet-500/80 border border-primary-400/30 text-white rounded-lg hover:bg-violet-500/90 transition-colors flex items-center justify-center gap-2"
 >
 <Globe class="w-5 h-5" />
 Go to Network Page
 </button>
 </div>
 {:else}
 <!-- Mining Control Card -->
 <div class="bg-white/[0.05] rounded-xl shadow-black/5 border border-white/[0.06] p-6">
 <div class="flex items-center justify-between mb-4">
 <div class="flex items-center gap-3">
 <div class="p-2 {isAnyMining ?'bg-yellow-100' :'bg-white/[0.05]'} rounded-lg">
 <Pickaxe class="w-6 h-6 {isAnyMining ?'text-yellow-400' :'text-white/50'}" />
 </div>
 <div>
 <h2 class="font-semibold">Mining</h2>
 <p class="text-sm text-white/40">Earn CHI by mining blocks with CPU or GPU</p>
 </div>
 </div>
 <div class="flex items-center gap-2">
 {#if isAnyMining}
 <span class="flex items-center gap-2 px-3 py-1 bg-yellow-100 text-yellow-400 rounded-full text-sm">
 <span class="w-2 h-2 bg-yellow-500/[0.1]0 rounded-full animate-pulse"></span>
 Mining ({activeMiningBackend.toUpperCase()})
 </span>
 {:else}
 <span class="flex items-center gap-2 px-3 py-1 bg-white/[0.05] text-white/50 rounded-full text-sm">
 <span class="w-2 h-2 bg-white/[0.12] rounded-full"></span>
 Idle
 </span>
 {/if}
 </div>
 </div>

 <!-- Mining Stats Grid -->
 <div class="grid grid-cols-2 md:grid-cols-3 gap-4 mb-4">
 <div class="bg-white/[0.05] border border-white/[0.06] rounded-lg p-4">
 <div class="flex items-center gap-2 mb-2">
 <Zap class="w-4 h-4 text-yellow-500" />
 <span class="text-sm text-white/50">Hash Rate</span>
 </div>
 <p class="text-2xl font-bold tabular-nums">
 {isAnyMining ? formatHashRate(displayHashRate) :'0 H/s'}
 </p>
 </div>
 <div class="bg-white/[0.05] border border-white/[0.06] rounded-lg p-4">
 <div class="flex items-center gap-2 mb-2">
 <Blocks class="w-4 h-4 text-red-500" />
 <span class="text-sm text-white/50">Block Height</span>
 </div>
 <p class="text-2xl font-bold tabular-nums">
 {gethStatus?.currentBlock?.toLocaleString() ??'0'}
 </p>
 </div>
 <div class="bg-white/[0.05] border border-white/[0.06] rounded-lg p-4">
 <div class="flex items-center gap-2 mb-2">
 <Coins class="w-4 h-4 text-amber-500" />
 <span class="text-sm text-white/50">Total Mined</span>
 </div>
 <p class="text-2xl font-bold tabular-nums">
 {(miningStatus?.totalMinedChi ?? 0).toFixed(4)} CHI
 </p>
 </div>
 <div class="bg-white/[0.05] border border-white/[0.06] rounded-lg p-4">
 <div class="flex items-center gap-2 mb-2">
 <Clock class="w-4 h-4 text-purple-500" />
 <span class="text-sm text-white/50">Session Time</span>
 </div>
 <p class="text-2xl font-bold tabular-nums">
 {isAnyMining ? miningElapsed :'--:--:--'}
 </p>
 </div>
 <div class="bg-white/[0.05] border border-white/[0.06] rounded-lg p-4">
 <div class="flex items-center gap-2 mb-2">
 {#if activeMiningBackend ==='gpu'}
 <Monitor class="w-4 h-4 text-violet-400" />
 <span class="text-sm text-white/50">GPUs Active</span>
 {:else}
 <Cpu class="w-4 h-4 text-green-500" />
 <span class="text-sm text-white/50">Threads Active</span>
 {/if}
 </div>
 <p class="text-2xl font-bold tabular-nums">
 {#if activeMiningBackend ==='gpu'}
 {isAnyMining ? `${activeGpuCount}` :'0'}
 {:else}
 {isAnyMining ? `${miningThreads} / ${maxThreads}` : `0 / ${maxThreads}`}
 {/if}
 </p>
 <p class="text-xs text-white/40 mt-1">
 {#if activeMiningBackend ==='gpu'}
 Target {activeGpuUtilization}%
 {:else}
 Target {cpuUtilizationPercent}%
 {/if}
 </p>
 </div>
 </div>

 <!-- Miner Address -->
 <div class="mb-4 p-3 bg-white/[0.05] rounded-lg">
 <div class="flex items-center gap-2 mb-1">
 <TrendingUp class="w-4 h-4 text-green-500" />
 <span class="text-sm text-white/50">Miner Address</span>
 </div>
 <p class="text-sm font-mono truncate">
 {miningStatus?.minerAddress || $walletAccount?.address ||'Not set'}
 </p>
 </div>

 <!-- Mining Backend Mode -->
 <div class="mb-4">
 <div class="block text-sm font-medium text-white/50 mb-2">
 Mining Backend
 </div>
 <div class="grid grid-cols-2 gap-2">
 <button
 onclick={() => (miningMode ='cpu')}
 disabled={isAnyMining}
 class="px-3 py-2 rounded-lg border text-sm font-medium transition-colors disabled:opacity-50 {miningMode ==='cpu' ?'bg-violet-900/20 border-primary-300 text-primary-700' :'bg-white/[0.05] border-white/[0.06] text-white/50'}"
 >
 CPU Miner
 </button>
 <button
 onclick={() => (miningMode ='gpu')}
 disabled={isAnyMining}
 class="px-3 py-2 rounded-lg border text-sm font-medium transition-colors disabled:opacity-50 {miningMode ==='gpu' ?'bg-violet-900/20 border-primary-300 text-primary-700' :'bg-white/[0.05] border-white/[0.06] text-white/50'}"
 >
 GPU Miner
 </button>
 </div>
 </div>

 {#if miningMode ==='cpu'}
 <!-- CPU Utilization Control -->
 <div class="mb-4">
 <label for="cpu-utilization" class="block text-sm font-medium text-white/50 mb-2">
 CPU Utilization Target ({cpuUtilizationPercent}%)
 </label>
 <input
 id="cpu-utilization"
 type="range"
 min={MIN_UTILIZATION_PERCENT}
 max={MAX_UTILIZATION_PERCENT}
 step="1"
 bind:value={cpuUtilizationPercent}
 disabled={isAnyMining}
 class="w-full h-2 bg-white/[0.05] rounded-lg appearance-none cursor-pointer disabled:opacity-50"
 />
 <div class="flex justify-between text-xs text-white/40 mt-1">
 <span>{MIN_UTILIZATION_PERCENT}%</span>
 <span>{MAX_UTILIZATION_PERCENT}%</span>
 </div>
 <p class="text-xs text-white/40 mt-2">
 Effective CPU threads: <span class="font-medium">{miningThreads}</span> of {maxThreads}
 </p>
 </div>
 {:else}
 <!-- GPU Utilization Control -->
 <div class="mb-4">
 <label for="gpu-utilization" class="block text-sm font-medium text-white/50 mb-2">
 GPU Utilization Target ({gpuUtilizationPercent}%)
 </label>
 <input
 id="gpu-utilization"
 type="range"
 min={MIN_UTILIZATION_PERCENT}
 max={MAX_UTILIZATION_PERCENT}
 step="1"
 bind:value={gpuUtilizationPercent}
 disabled={isAnyMining}
 class="w-full h-2 bg-white/[0.05] rounded-lg appearance-none cursor-pointer disabled:opacity-50"
 />
 <div class="flex justify-between text-xs text-white/40 mt-1">
 <span>{MIN_UTILIZATION_PERCENT}%</span>
 <span>{MAX_UTILIZATION_PERCENT}%</span>
 </div>
 </div>

 <!-- GPU Control -->
 <div class="mb-4">
 {#if !gpuCapabilities?.binaryPath}
 <div class="rounded-lg border border-amber-200 bg-amber-500/[0.1]0/[0.1] p-3">
 {#if gpuCapabilities?.lastError}
 <p class="text-sm text-amber-800">
 GPU miner is unavailable: {gpuCapabilities.lastError}
 </p>
 <p class="text-xs text-amber-400 mt-1">
 You can still set `CHIRAL_GPU_MINER_PATH` manually and refresh.
 </p>
 {:else}
 <p class="text-sm text-amber-800">
 Preparing GPU miner automatically. If this stays here, click refresh.
 </p>
 {/if}
 </div>
 {:else}
 {#if gpuCapabilities?.lastError}
 <div class="mb-3 rounded-lg border border-amber-200 bg-amber-500/[0.1]0/[0.1] p-3">
 <p class="text-sm text-amber-800">
 GPU probe warning: {gpuCapabilities.lastError}
 </p>
 <p class="text-xs text-amber-400 mt-1">
 You can still start GPU mining and the app will retry with backend fallbacks automatically.
 </p>
 </div>
 {/if}
 <div class="block text-sm font-medium text-white/50 mb-2">
 GPU Devices ({selectedGpuDevices.length} selected)
 </div>
 {#if gpuCapabilities.devices.length === 0}
 <div class="rounded-lg border border-white/[0.06] bg-white/[0.05] p-3 text-sm text-white/50">
 No devices were reported by the miner binary. You can still try starting GPU mining with auto-detection.
 </div>
 {:else}
 <div class="space-y-2 max-h-44 overflow-y-auto rounded-lg border border-white/[0.06] p-3 bg-white/[0.05]">
 {#each gpuCapabilities.devices as device (device.id)}
 <label class="flex items-center gap-2 text-sm text-white/50">
 <input
 type="checkbox"
 checked={selectedGpuDevices.includes(device.id)}
 onchange={() => toggleGpuDevice(device.id)}
 disabled={isAnyMining}
 class="rounded border-white/[0.06] bg-white/[0.05]"
 />
 <span class="font-mono text-xs text-white/40">[{device.id}]</span>
 <span>{device.name}</span>
 </label>
 {/each}
 </div>
 {/if}
 {/if}
 </div>
 {/if}

 {#if gpuMiningStatus?.lastError}
 <div class="mb-4 rounded-lg border border-red-400/20 bg-red-500/[0.1]0/[0.1] p-3">
 <p class="text-sm text-red-700">{gpuMiningStatus.lastError}</p>
 </div>
 {/if}

 <!-- Mining Controls -->
 <div class="flex gap-3">
 {#if isAnyMining}
 <button
 onclick={handleStopMining}
 class="flex-1 px-4 py-3 bg-red-500/[0.1]0/[0.1]0/70 border border-red-400/30 text-white rounded-lg hover:bg-red-500/[0.1]0/[0.15]0/80 transition-colors flex items-center justify-center gap-2 focus:outline-none"
 >
 <Square class="w-5 h-5" />
 Stop Mining
 </button>
 {:else}
 <button
 onclick={handleStartMining}
 disabled={isStartingMining || (miningMode ==='gpu' && !gpuCapabilities?.binaryPath)}
 class="flex-1 px-4 py-3 bg-yellow-500/[0.1]0/70 border border-yellow-400/30 text-white rounded-lg hover:bg-yellow-500/[0.1]0/80 transition-colors flex items-center justify-center gap-2 disabled:opacity-50 focus:outline-none"
 >
 {#if isStartingMining}
 <Loader2 class="w-5 h-5 animate-spin" />
 Starting...
 {:else}
 <Pickaxe class="w-5 h-5" />
 {#if miningMode ==='gpu'}
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
 <div class="bg-white/[0.05] rounded-xl shadow-black/5 border border-white/[0.06]">
 <button
 onclick={() => showHistory = !showHistory}
 class="w-full flex items-center justify-between p-6 text-left"
 >
 <div class="flex items-center gap-3">
 <div class="p-2 bg-emerald-100 rounded-lg">
 <History class="w-6 h-6 text-emerald-600" />
 </div>
 <div>
 <h2 class="font-semibold">Mining History</h2>
 <p class="text-sm text-white/40">
 {minedBlocks.length} block{minedBlocks.length !== 1 ?'s' :''} mined
 {#if totalHistoryReward > 0}
 — {totalHistoryReward.toFixed(2)} CHI earned
 {/if}
 </p>
 </div>
 </div>
 {#if showHistory}
 <ChevronUp class="w-5 h-5 text-white/50" />
 {:else}
 <ChevronDown class="w-5 h-5 text-white/50" />
 {/if}
 </button>

 {#if showHistory}
 <div class="px-6 pb-6">
 <div class="flex justify-end mb-4">
 <button
 onclick={loadMinedBlocks}
 disabled={isLoadingHistory}
 class="text-xs px-3 py-1.5 bg-white/[0.05] hover:bg-white/[0.05] rounded transition-colors flex items-center gap-1 disabled:opacity-50"
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
 <Loader2 class="w-6 h-6 animate-spin text-white/50" />
 <span class="ml-2 text-sm text-white/40">Scanning blockchain...</span>
 </div>
 {:else if minedBlocks.length === 0}
 <div class="text-center py-8">
 <Pickaxe class="w-10 h-10 text-white/50 mx-auto mb-3" />
 <p class="text-sm text-white/40">No blocks mined yet.</p>
 <p class="text-xs text-white/50 mt-1">Start mining to earn CHI block rewards.</p>
 </div>
 {:else}
 <!-- Summary Stats -->
 <div class="grid grid-cols-3 gap-3 mb-4">
 <div class="bg-white/[0.05] border border-white/[0.06] rounded-lg p-3">
 <p class="text-xs text-white/40">Blocks Mined</p>
 <p class="text-lg font-bold">{minedBlocks.length}</p>
 </div>
 <div class="bg-white/[0.05] border border-white/[0.06] rounded-lg p-3">
 <p class="text-xs text-white/40">Total Earned</p>
 <p class="text-lg font-bold text-emerald-600">{totalHistoryReward.toFixed(2)} CHI</p>
 </div>
 <div class="bg-white/[0.05] border border-white/[0.06] rounded-lg p-3">
 <p class="text-xs text-white/40">Reward per Block</p>
 <p class="text-lg font-bold">{minedBlocks[0]?.rewardChi ?? 0} CHI</p>
 </div>
 </div>

 <!-- Block Table -->
 <div class="overflow-x-auto">
 <table class="w-full text-sm">
 <thead>
 <tr class="border-b border-white/[0.06]">
 <th class="text-left py-2 px-3 text-xs font-medium text-white/40">Block #</th>
 <th class="text-left py-2 px-3 text-xs font-medium text-white/40">Time</th>
 <th class="text-right py-2 px-3 text-xs font-medium text-white/40">Reward</th>
 <th class="text-right py-2 px-3 text-xs font-medium text-white/40">Difficulty</th>
 </tr>
 </thead>
 <tbody>
 {#each minedBlocks as block (block.blockNumber)}
 <tr class="border-b border-white/[0.06] hover:bg-white/[0.05]/50 transition-colors">
 <td class="py-2 px-3 font-mono text-xs tabular-nums">
 #{block.blockNumber.toLocaleString()}
 </td>
 <td class="py-2 px-3 text-xs text-white/50">
 {formatTimestamp(block.timestamp)}
 </td>
 <td class="py-2 px-3 text-right text-xs font-medium tabular-nums text-emerald-600">
 +{block.rewardChi} CHI
 </td>
 <td class="py-2 px-3 text-right text-xs tabular-nums text-white/40 font-mono">
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
