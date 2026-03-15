<script lang="ts">
 import { onMount, onDestroy } from'svelte';
 import { invoke } from'@tauri-apps/api/core';
 import { listen } from'@tauri-apps/api/event';
 import { networkConnected } from'$lib/stores';
 import { dhtService, type DhtHealthInfo } from'$lib/dhtService';
 import { toasts } from'$lib/toastStore';
 import {
 Bug,
 RefreshCw,
 Loader2,
 ChevronDown,
 ChevronUp,
 Trash2,
 Copy,
 Download,
 Filter,
 Globe,
 Server,
 Activity,
 AlertTriangle,
 Check,
 Info,
 Terminal,
 Pickaxe,
 FileText
 } from'lucide-svelte';
 import { logger } from'$lib/logger';
 const log = logger('Diagnostics');

 // Log entry type
 interface LogEntry {
 id: number;
 timestamp: Date;
 level:'info' |'warn' |'error' |'debug';
 source: string;
 message: string;
 }

 // State
 let logEntries = $state<LogEntry[]>([]);
 let nextLogId = 0;
 let logFilter = $state<'all' |'info' |'warn' |'error' |'debug'>('all');
 let sourceFilter = $state<'all' |'dht' |'bootstrap' |'geth' |'mining' |'system'>('all');
 let maxLogEntries = 500;

 // DHT diagnostics
 let dhtHealth = $state<DhtHealthInfo | null>(null);
 let isLoadingDht = $state(false);

 // Bootstrap diagnostics
 interface BootstrapHealthReport {
 totalNodes: number;
 healthyNodes: number;
 nodes: {
 enode: string;
 name: string;
 region: string;
 reachable: boolean;
 latencyMs: number | null;
 error: string | null;
 lastChecked: number;
 }[];
 timestamp: number;
 isHealthy: boolean;
 healthyEnodeString: string;
 }

 let bootstrapHealth = $state<BootstrapHealthReport | null>(null);
 let isLoadingBootstrap = $state(false);

 // Geth diagnostics
 interface GethStatus {
 installed: boolean;
 running: boolean;
 syncing: boolean;
 currentBlock: number;
 highestBlock: number;
 peerCount: number;
 chainId: number;
 }

 let gethStatus = $state<GethStatus | null>(null);
 let isLoadingGeth = $state(false);

 // Mining diagnostics
 interface MiningStatus {
 mining: boolean;
 hashRate: number;
 minerAddress: string | null;
 totalMinedWei: string;
 totalMinedChi: number;
 }

 let miningStatus = $state<MiningStatus | null>(null);
 let isLoadingMining = $state(false);
 let showMiningSection = $state(true);

 // Geth log viewer
 let gethLogContent = $state('');
 let isLoadingGethLog = $state(false);
 let showGethLogSection = $state(true);
 let gethLogLines = $state(100);

 // Auto-refresh
 let autoRefreshInterval: ReturnType<typeof setInterval> | null = null;
 let autoRefreshEnabled = $state(true);
 let autoRefreshSeconds = $state(5);

 // Events
 let eventListeners: (() => void)[] = [];
 let autoScroll = $state(true);
 let showDhtSection = $state(true);
 let showBootstrapSection = $state(true);
 let showGethSection = $state(true);
 let showLogsSection = $state(true);

 function isTauri(): boolean {
 return typeof window !=='undefined' &&'__TAURI_INTERNALS__' in window;
 }

 function addLog(level: LogEntry['level'], source: string, message: string) {
 const entry: LogEntry = {
 id: nextLogId++,
 timestamp: new Date(),
 level,
 source,
 message
 };
 logEntries = [...logEntries.slice(-(maxLogEntries - 1)), entry];
 }

 // Filtered logs
 let filteredLogs = $derived(
 logEntries.filter(entry => {
 if (logFilter !=='all' && entry.level !== logFilter) return false;
 if (sourceFilter !=='all' && entry.source.toLowerCase() !== sourceFilter) return false;
 return true;
 })
 );

 onMount(async () => {
 addLog('info','system','Diagnostics page opened');

 if (isTauri()) {
 // Listen for various events
 const events = [
'peer-discovered',
'ping-sent',
'ping-received',
'pong-received',
'geth-download-progress',
'file-download-started',
'file-download-progress',
'file-download-completed',
'file-download-failed',
'file-upload-started',
'file-payment-processing'
 ];

 for (const eventName of events) {
 try {
 const unlisten = await listen(eventName, (event) => {
 const payload = typeof event.payload ==='string'
 ? event.payload
 : JSON.stringify(event.payload, null, 0).slice(0, 200);
 const source = eventName.startsWith('geth') ?'geth'
 : eventName.startsWith('peer') || eventName.startsWith('ping') || eventName.startsWith('pong') ?'dht'
 :'system';
 addLog('info', source, `Event: ${eventName} — ${payload}`);
 });
 eventListeners.push(unlisten);
 } catch {}
 }

 // Initial data load
 await Promise.all([loadDhtHealth(), loadBootstrapHealth(), loadGethStatus(), loadMiningStatus(), loadGethLog()]);

 // Start auto-refresh
 startAutoRefresh();
 }
 });

 onDestroy(() => {
 for (const unlisten of eventListeners) {
 unlisten();
 }
 stopAutoRefresh();
 });

 function startAutoRefresh() {
 stopAutoRefresh();
 if (autoRefreshEnabled) {
 autoRefreshInterval = setInterval(async () => {
 if (!isTauri()) return;
 await Promise.all([loadGethStatus(), loadMiningStatus(), loadGethLog(), loadDhtHealth()]);
 }, autoRefreshSeconds * 1000);
 }
 }

 function stopAutoRefresh() {
 if (autoRefreshInterval) {
 clearInterval(autoRefreshInterval);
 autoRefreshInterval = null;
 }
 }

 function toggleAutoRefresh() {
 autoRefreshEnabled = !autoRefreshEnabled;
 if (autoRefreshEnabled) {
 startAutoRefresh();
 addLog('info','system', `Auto-refresh enabled (${autoRefreshSeconds}s)`);
 } else {
 stopAutoRefresh();
 addLog('info','system','Auto-refresh disabled');
 }
 }

 async function loadDhtHealth() {
 isLoadingDht = true;
 try {
 dhtHealth = await dhtService.getHealth();
 addLog('debug','dht', `DHT health: ${dhtHealth.running ?'Running' :'Stopped'}, ${dhtHealth.connectedPeerCount} peers`);
 } catch (err) {
 addLog('error','dht', `Failed to get DHT health: ${err}`);
 } finally {
 isLoadingDht = false;
 }
 }

 async function loadBootstrapHealth() {
 isLoadingBootstrap = true;
 try {
 const cached = await invoke<BootstrapHealthReport | null>('get_bootstrap_health');
 if (cached) {
 bootstrapHealth = cached;
 addLog('debug','bootstrap', `Bootstrap: ${cached.healthyNodes}/${cached.totalNodes} healthy nodes`);
 }
 } catch (err) {
 addLog('warn','bootstrap', `No cached bootstrap health: ${err}`);
 } finally {
 isLoadingBootstrap = false;
 }
 }

 async function runBootstrapCheck() {
 isLoadingBootstrap = true;
 addLog('info','bootstrap','Running bootstrap health check...');
 try {
 bootstrapHealth = await invoke<BootstrapHealthReport>('check_bootstrap_health');
 addLog('info','bootstrap', `Bootstrap check complete: ${bootstrapHealth.healthyNodes}/${bootstrapHealth.totalNodes} healthy`);
 } catch (err) {
 addLog('error','bootstrap', `Bootstrap check failed: ${err}`);
 } finally {
 isLoadingBootstrap = false;
 }
 }

 async function loadGethStatus() {
 isLoadingGeth = true;
 try {
 gethStatus = await invoke<GethStatus>('get_geth_status');
 addLog('debug','geth', `Geth: ${gethStatus.running ?'Running' :'Stopped'}, block ${gethStatus.currentBlock}, ${gethStatus.peerCount} peers`);
 } catch (err) {
 addLog('error','geth', `Failed to get Geth status: ${err}`);
 gethStatus = { installed: false, running: false, syncing: false, currentBlock: 0, highestBlock: 0, peerCount: 0, chainId: 0 };
 } finally {
 isLoadingGeth = false;
 }
 }

 async function loadMiningStatus() {
 isLoadingMining = true;
 try {
 miningStatus = await invoke<MiningStatus>('get_mining_status');
 addLog('debug','mining', `Mining: ${miningStatus.mining ?'Active' :'Inactive'}, hashrate: ${miningStatus.hashRate} H/s, mined: ${miningStatus.totalMinedChi.toFixed(4)} CHI`);
 } catch (err) {
 addLog('error','mining', `Failed to get mining status: ${err}`);
 miningStatus = null;
 } finally {
 isLoadingMining = false;
 }
 }

 async function loadGethLog() {
 isLoadingGethLog = true;
 try {
 gethLogContent = await invoke<string>('read_geth_log', { lines: gethLogLines });
 } catch (err) {
 gethLogContent = `Error reading log: ${err}`;
 } finally {
 isLoadingGethLog = false;
 }
 }

 function formatHashRate(hr: number): string {
 if (hr >= 1_000_000_000) return `${(hr / 1_000_000_000).toFixed(2)} GH/s`;
 if (hr >= 1_000_000) return `${(hr / 1_000_000).toFixed(2)} MH/s`;
 if (hr >= 1_000) return `${(hr / 1_000).toFixed(2)} KH/s`;
 return `${hr} H/s`;
 }

 function clearLogs() {
 logEntries = [];
 addLog('info','system','Logs cleared');
 }

 function copyLogs() {
 const text = filteredLogs.map(e =>
 `[${e.timestamp.toISOString()}] [${e.level.toUpperCase()}] [${e.source}] ${e.message}`
 ).join('\n');
 navigator.clipboard.writeText(text).then(() => {
 toasts.show('Logs copied to clipboard','success');
 }).catch(() => {
 toasts.show('Failed to copy logs','error');
 });
 }

 function exportLogs() {
 const text = logEntries.map(e =>
 `[${e.timestamp.toISOString()}] [${e.level.toUpperCase()}] [${e.source}] ${e.message}`
 ).join('\n');
 const blob = new Blob([text], { type:'text/plain' });
 const url = URL.createObjectURL(blob);
 const a = document.createElement('a');
 a.href = url;
 a.download = `chiral-diagnostics-${new Date().toISOString().slice(0, 19).replace(/:/g,'-')}.log`;
 a.click();
 URL.revokeObjectURL(url);
 toasts.show('Logs exported','success');
 }

 async function refreshAll() {
 addLog('info','system','Refreshing all diagnostics...');
 await Promise.all([loadDhtHealth(), loadBootstrapHealth(), loadGethStatus(), loadMiningStatus(), loadGethLog()]);
 addLog('info','system','All diagnostics refreshed');
 }

 function levelColor(level: string): string {
 switch (level) {
 case'error': return'text-red-500';
 case'warn': return'text-yellow-500';
 case'info': return'text-indigo-500';
 case'debug': return'text-[var(--text-tertiary)]';
 default: return'text-[var(--text-tertiary)]';
 }
 }

 function levelBg(level: string): string {
 switch (level) {
 case'error': return'bg-red-500/[0.08] text-red-400';
 case'warn': return'bg-yellow-900/30 text-yellow-400';
 case'info': return'bg-indigo-900/30 text-indigo-400';
 case'debug': return'bg-[var(--surface-0)]/[0.07] text-[var(--text-secondary)]';
 default: return'bg-[var(--surface-0)]/[0.07]';
 }
 }

 function sourceBg(source: string): string {
 const normalized = source.toLowerCase();
 switch (normalized) {
 case'geth': return'bg-blue-100 text-blue-600';
 case'mining': return'bg-amber-900/30 text-amber-300';
 case'dht': return'bg-emerald-900/30 text-emerald-700';
 case'bootstrap': return'bg-orange-100 text-orange-700';
 case'system': return'bg-violet-600/10 text-violet-400';
 default: return'bg-[var(--surface-0)]/[0.07] text-[var(--text-secondary)]';
 }
 }

 function gethLogLineColor(line: string, level: string | null): string {
 if (level ==='ERROR' || line.includes('Fatal') || line.includes('ERROR') || line.includes('error')) {
 return'text-red-400';
 }
 if (level ==='WARN' || line.includes('WARN') || line.includes('warn')) {
 return'text-yellow-400';
 }
 if (level ==='DEBUG') {
 return'text-slate-400';
 }
 return'text-[var(--text-secondary)]';
 }

 function parseStructuredGethLine(line: string): {
 timestamp: string | null;
 level:'INFO' |'WARN' |'ERROR' |'DEBUG' | null;
 source: string | null;
 message: string;
 } {
 const match = line.match(/^\[(\d+)\]\s+\[(INFO|WARN|ERROR|DEBUG)\]\s+\[([A-Z_]+)\]\s+(.*)$/);
 if (!match) {
 return { timestamp: null, level: null, source: null, message: line };
 }
 const [, ts, level, source, message] = match;
 const date = new Date(Number(ts) * 1000);
 return {
 timestamp: Number.isFinite(date.getTime()) ? date.toLocaleTimeString() : ts,
 level: level as'INFO' |'WARN' |'ERROR' |'DEBUG',
 source,
 message
 };
 }
</script>

<svelte:head><title>Diagnostics | Chiral Network</title></svelte:head>

<div class="p-4 sm:p-6 space-y-6 max-w-6xl mx-auto">
 <div class="flex items-center justify-between">
 <div>
 <h1 class="text-2xl font-light tracking-tight">Diagnostics</h1>
 <p class="text-[var(--text-secondary)] mt-1">Developer tools for debugging and monitoring</p>
 </div>
 <div class="flex items-center gap-3">
 <label class="flex items-center gap-2 text-xs text-[var(--text-secondary)]">
 <input type="checkbox" checked={autoRefreshEnabled} onchange={toggleAutoRefresh} class="rounded" />
 Auto-refresh ({autoRefreshSeconds}s)
 </label>
 <button
 onclick={refreshAll}
 class="p-2 hover:bg-[var(--surface-0)]/[0.05] rounded-lg transition-colors focus:outline-none focus:border-blue-400/40"
 title="Refresh all"
 >
 <RefreshCw class="w-5 h-5" />
 </button>
 </div>
 </div>

 <!-- DHT Diagnostics -->
 <div class="bg-[var(--surface-1)] rounded-xl border border-[var(--border)]">
 <button
 onclick={() => showDhtSection = !showDhtSection}
 class="w-full flex items-center justify-between p-6 text-left"
 >
 <div class="flex items-center gap-3">
 <div class="p-2 {$networkConnected ?'bg-green-500/[0.08]' :'bg-[var(--surface-0)]/[0.07]'} rounded-lg">
 <Globe class="w-6 h-6 {$networkConnected ?'text-green-400' :'text-[var(--text-secondary)]'}" />
 </div>
 <div>
 <h2 class="font-semibold">DHT Diagnostics</h2>
 <p class="text-sm text-[var(--text-secondary)]">P2P network status and peer information</p>
 </div>
 </div>
 {#if showDhtSection}
 <ChevronUp class="w-5 h-5 text-[var(--text-secondary)]" />
 {:else}
 <ChevronDown class="w-5 h-5 text-[var(--text-secondary)]" />
 {/if}
 </button>

 {#if showDhtSection}
 <div class="px-6 pb-6 space-y-4">
 <div class="flex justify-end">
 <button
 onclick={loadDhtHealth}
 disabled={isLoadingDht}
 class="text-xs px-3 py-1.5 bg-[var(--surface-0)]/[0.07] hover:bg-[var(--surface-0)]/[0.1] rounded transition-colors flex items-center gap-1 disabled:opacity-50"
 >
 {#if isLoadingDht}
 <Loader2 class="w-3 h-3 animate-spin" />
 {:else}
 <RefreshCw class="w-3 h-3" />
 {/if}
 Refresh
 </button>
 </div>

 {#if dhtHealth}
 <div class="grid grid-cols-2 md:grid-cols-4 gap-3">
 <div class="bg-[var(--surface-0)]/[0.07] rounded-lg p-3">
 <p class="text-xs text-[var(--text-secondary)]">Status</p>
 <p class="text-sm font-bold {dhtHealth.running ?'text-green-400' :'text-red-400'}">
 {dhtHealth.running ?'Running' :'Stopped'}
 </p>
 </div>
 <div class="bg-[var(--surface-0)]/[0.07] rounded-lg p-3">
 <p class="text-xs text-[var(--text-secondary)]">Connected Peers</p>
 <p class="text-sm font-bold tabular-nums font-mono text-violet-400/90">{dhtHealth.connectedPeerCount}</p>
 </div>
 <div class="bg-[var(--surface-0)]/[0.07] rounded-lg p-3">
 <p class="text-xs text-[var(--text-secondary)]">Kademlia Peers</p>
 <p class="text-sm font-bold tabular-nums font-mono text-violet-400/90">{dhtHealth.kademliaPeers}</p>
 </div>
 <div class="bg-[var(--surface-0)]/[0.07] rounded-lg p-3">
 <p class="text-xs text-[var(--text-secondary)]">Shared Files</p>
 <p class="text-sm font-bold tabular-nums font-mono text-violet-400/90">{dhtHealth.sharedFiles}</p>
 </div>
 </div>

 {#if dhtHealth.peerId}
 <div class="p-3 bg-[var(--surface-0)]/[0.07] rounded-lg">
 <p class="text-xs text-[var(--text-secondary)] mb-1">Peer ID</p>
 <p class="font-mono text-xs break-all">{dhtHealth.peerId}</p>
 </div>
 {/if}

 {#if dhtHealth.listeningAddresses.length > 0}
 <div class="p-3 bg-[var(--surface-0)]/[0.07] rounded-lg">
 <p class="text-xs text-[var(--text-secondary)] mb-1">Listening Addresses ({dhtHealth.listeningAddresses.length})</p>
 <div class="space-y-1">
 {#each dhtHealth.listeningAddresses as addr}
 <p class="font-mono text-xs break-all">{addr}</p>
 {/each}
 </div>
 </div>
 {/if}

 {#if dhtHealth.protocols.length > 0}
 <div class="p-3 bg-[var(--surface-0)]/[0.07] rounded-lg">
 <p class="text-xs text-[var(--text-secondary)] mb-2">Active Protocols ({dhtHealth.protocols.length})</p>
 <div class="flex flex-wrap gap-1.5">
 {#each dhtHealth.protocols as protocol}
 <span class="px-2 py-0.5 bg-indigo-900/30 text-indigo-400 text-xs rounded-full font-mono">
 {protocol}
 </span>
 {/each}
 </div>
 </div>
 {/if}

 {#if dhtHealth.bootstrapNodes.length > 0}
 <div class="p-3 bg-[var(--surface-0)]/[0.07] rounded-lg">
 <p class="text-xs text-[var(--text-secondary)] mb-2">DHT Bootstrap Nodes</p>
 <div class="space-y-1.5">
 {#each dhtHealth.bootstrapNodes as node}
 <div class="flex items-center gap-2 text-xs">
 <div class="w-2 h-2 rounded-full {node.reachable ?'bg-green-500' :'bg-red-500'} shrink-0"></div>
 <span class="font-mono break-all">{node.address}</span>
 <span class="{node.reachable ?'text-green-400' :'text-red-400'} shrink-0">
 {node.reachable ?'Reachable' :'Unreachable'}
 </span>
 </div>
 {/each}
 </div>
 </div>
 {/if}
 {:else}
 <p class="text-sm text-[var(--text-secondary)] text-center py-4">
 Click"Refresh" to load DHT diagnostics
 </p>
 {/if}
 </div>
 {/if}
 </div>

 <!-- Bootstrap Diagnostics -->
 <div class="bg-[var(--surface-1)] rounded-xl border border-[var(--border)]">
 <button
 onclick={() => showBootstrapSection = !showBootstrapSection}
 class="w-full flex items-center justify-between p-6 text-left"
 >
 <div class="flex items-center gap-3">
 <div class="p-2 bg-orange-100 rounded-lg">
 <Activity class="w-6 h-6 text-orange-600" />
 </div>
 <div>
 <h2 class="font-semibold">Bootstrap Diagnostics</h2>
 <p class="text-sm text-[var(--text-secondary)]">Bootstrap node connectivity and latency</p>
 </div>
 </div>
 {#if showBootstrapSection}
 <ChevronUp class="w-5 h-5 text-[var(--text-secondary)]" />
 {:else}
 <ChevronDown class="w-5 h-5 text-[var(--text-secondary)]" />
 {/if}
 </button>

 {#if showBootstrapSection}
 <div class="px-6 pb-6 space-y-4">
 <div class="flex justify-end">
 <button
 onclick={runBootstrapCheck}
 disabled={isLoadingBootstrap}
 class="text-xs px-3 py-1.5 bg-[var(--surface-0)]/[0.07] hover:bg-[var(--surface-0)]/[0.1] rounded transition-colors flex items-center gap-1 disabled:opacity-50"
 >
 {#if isLoadingBootstrap}
 <Loader2 class="w-3 h-3 animate-spin" />
 {:else}
 <Activity class="w-3 h-3" />
 {/if}
 Run Check
 </button>
 </div>

 {#if bootstrapHealth}
 <div class="grid grid-cols-3 gap-3">
 <div class="bg-[var(--surface-0)]/[0.07] rounded-lg p-3">
 <p class="text-xs text-[var(--text-secondary)]">Status</p>
 <p class="text-sm font-bold {bootstrapHealth.isHealthy ?'text-green-400' :'text-red-400'}">
 {bootstrapHealth.isHealthy ?'Healthy' :'Degraded'}
 </p>
 </div>
 <div class="bg-[var(--surface-0)]/[0.07] rounded-lg p-3">
 <p class="text-xs text-[var(--text-secondary)]">Healthy Nodes</p>
 <p class="text-sm font-bold tabular-nums font-mono text-violet-400/90">{bootstrapHealth.healthyNodes} / {bootstrapHealth.totalNodes}</p>
 </div>
 <div class="bg-[var(--surface-0)]/[0.07] rounded-lg p-3">
 <p class="text-xs text-[var(--text-secondary)]">Last Checked</p>
 <p class="text-sm font-bold">{new Date(bootstrapHealth.timestamp).toLocaleTimeString()}</p>
 </div>
 </div>

 <div class="space-y-2">
 {#each bootstrapHealth.nodes as node}
 <div class="flex items-center justify-between p-3 bg-[var(--surface-0)]/[0.07] rounded-lg text-xs">
 <div class="flex items-center gap-2">
 <div class="w-2 h-2 rounded-full {node.reachable ?'bg-green-500' :'bg-red-500'} shrink-0"></div>
 <div>
 <span class="font-medium text-sm">{node.name}</span>
 <span class="text-[var(--text-secondary)] ml-1">({node.region})</span>
 </div>
 </div>
 <div class="text-right shrink-0">
 {#if node.reachable && node.latencyMs}
 <span class="text-green-400 tabular-nums">{node.latencyMs}ms</span>
 {:else if node.error}
 <span class="text-red-500">{node.error}</span>
 {:else}
 <span class="{node.reachable ?'text-green-400' :'text-red-400'}">
 {node.reachable ?'Reachable' :'Unreachable'}
 </span>
 {/if}
 </div>
 </div>
 {/each}
 </div>
 {:else}
 <p class="text-sm text-[var(--text-secondary)] text-center py-4">
 Click"Run Check" to test bootstrap node connectivity
 </p>
 {/if}
 </div>
 {/if}
 </div>

 <!-- Geth Diagnostics -->
 <div class="bg-[var(--surface-1)] rounded-xl border border-[var(--border)]">
 <button
 onclick={() => showGethSection = !showGethSection}
 class="w-full flex items-center justify-between p-6 text-left"
 >
 <div class="flex items-center gap-3">
 <div class="p-2 {gethStatus?.running ?'bg-green-500/[0.08]' :'bg-[var(--surface-0)]/[0.07]'} rounded-lg">
 <Server class="w-6 h-6 {gethStatus?.running ?'text-green-400' :'text-[var(--text-secondary)]'}" />
 </div>
 <div>
 <h2 class="font-semibold">Geth Diagnostics</h2>
 <p class="text-sm text-[var(--text-secondary)]">Blockchain node status and sync info</p>
 </div>
 </div>
 {#if showGethSection}
 <ChevronUp class="w-5 h-5 text-[var(--text-secondary)]" />
 {:else}
 <ChevronDown class="w-5 h-5 text-[var(--text-secondary)]" />
 {/if}
 </button>

 {#if showGethSection}
 <div class="px-6 pb-6 space-y-4">
 <div class="flex justify-end">
 <button
 onclick={loadGethStatus}
 disabled={isLoadingGeth}
 class="text-xs px-3 py-1.5 bg-[var(--surface-0)]/[0.07] hover:bg-[var(--surface-0)]/[0.1] rounded transition-colors flex items-center gap-1 disabled:opacity-50"
 >
 {#if isLoadingGeth}
 <Loader2 class="w-3 h-3 animate-spin" />
 {:else}
 <RefreshCw class="w-3 h-3" />
 {/if}
 Refresh
 </button>
 </div>

 {#if gethStatus}
 <div class="grid grid-cols-2 md:grid-cols-4 gap-3">
 <div class="bg-[var(--surface-0)]/[0.07] rounded-lg p-3">
 <p class="text-xs text-[var(--text-secondary)]">Installed</p>
 <p class="text-sm font-bold {gethStatus.installed ?'text-green-400' :'text-red-400'}">
 {gethStatus.installed ?'Yes' :'No'}
 </p>
 </div>
 <div class="bg-[var(--surface-0)]/[0.07] rounded-lg p-3">
 <p class="text-xs text-[var(--text-secondary)]">Status</p>
 <p class="text-sm font-bold {gethStatus.running ?'text-green-400' :'text-[var(--text-secondary)]'}">
 {gethStatus.running ?'Running' :'Stopped'}
 </p>
 </div>
 <div class="bg-[var(--surface-0)]/[0.07] rounded-lg p-3">
 <p class="text-xs text-[var(--text-secondary)]">Syncing</p>
 <p class="text-sm font-bold">{gethStatus.syncing ?'Yes' :'No'}</p>
 </div>
 <div class="bg-[var(--surface-0)]/[0.07] rounded-lg p-3">
 <p class="text-xs text-[var(--text-secondary)]">Chain ID</p>
 <p class="text-sm font-bold tabular-nums font-mono text-violet-400/90">{gethStatus.chainId ||'N/A'}</p>
 </div>
 <div class="bg-[var(--surface-0)]/[0.07] rounded-lg p-3">
 <p class="text-xs text-[var(--text-secondary)]">Current Block</p>
 <p class="text-sm font-bold tabular-nums font-mono text-violet-400/90">{gethStatus.currentBlock.toLocaleString()}</p>
 </div>
 <div class="bg-[var(--surface-0)]/[0.07] rounded-lg p-3">
 <p class="text-xs text-[var(--text-secondary)]">Highest Block</p>
 <p class="text-sm font-bold tabular-nums font-mono text-violet-400/90">{gethStatus.highestBlock.toLocaleString()}</p>
 </div>
 <div class="bg-[var(--surface-0)]/[0.07] rounded-lg p-3">
 <p class="text-xs text-[var(--text-secondary)]">Blockchain Peers</p>
 <p class="text-sm font-bold tabular-nums font-mono text-violet-400/90">{gethStatus.peerCount}</p>
 </div>
 <div class="bg-[var(--surface-0)]/[0.07] rounded-lg p-3">
 <p class="text-xs text-[var(--text-secondary)]">Sync Progress</p>
 <p class="text-sm font-bold tabular-nums font-mono text-violet-400/90">
 {#if gethStatus.syncing && gethStatus.highestBlock > 0}
 {((gethStatus.currentBlock / gethStatus.highestBlock) * 100).toFixed(1)}%
 {:else if gethStatus.running}
 Synced
 {:else}
 N/A
 {/if}
 </p>
 </div>
 </div>
 {:else}
 <p class="text-sm text-[var(--text-secondary)] text-center py-4">
 Click"Refresh" to load Geth diagnostics
 </p>
 {/if}
 </div>
 {/if}
 </div>

 <!-- Mining Diagnostics -->
 <div class="bg-[var(--surface-1)] rounded-xl border border-[var(--border)]">
 <button
 onclick={() => showMiningSection = !showMiningSection}
 class="w-full flex items-center justify-between p-6 text-left"
 >
 <div class="flex items-center gap-3">
 <div class="p-2 {miningStatus?.mining ?'bg-amber-900/30' :'bg-[var(--surface-0)]/[0.07]'} rounded-lg">
 <Pickaxe class="w-6 h-6 {miningStatus?.mining ?'text-amber-400' :'text-[var(--text-secondary)]'}" />
 </div>
 <div>
 <h2 class="font-semibold">Mining Diagnostics</h2>
 <p class="text-sm text-[var(--text-secondary)]">Mining status, hashrate, and rewards</p>
 </div>
 </div>
 {#if showMiningSection}
 <ChevronUp class="w-5 h-5 text-[var(--text-secondary)]" />
 {:else}
 <ChevronDown class="w-5 h-5 text-[var(--text-secondary)]" />
 {/if}
 </button>

 {#if showMiningSection}
 <div class="px-6 pb-6 space-y-4">
 <div class="flex justify-end">
 <button
 onclick={loadMiningStatus}
 disabled={isLoadingMining}
 class="text-xs px-3 py-1.5 bg-[var(--surface-0)]/[0.07] hover:bg-[var(--surface-0)]/[0.1] rounded transition-colors flex items-center gap-1 disabled:opacity-50"
 >
 {#if isLoadingMining}
 <Loader2 class="w-3 h-3 animate-spin" />
 {:else}
 <RefreshCw class="w-3 h-3" />
 {/if}
 Refresh
 </button>
 </div>

 {#if miningStatus}
 <div class="grid grid-cols-2 md:grid-cols-4 gap-3">
 <div class="bg-[var(--surface-0)]/[0.07] rounded-lg p-3">
 <p class="text-xs text-[var(--text-secondary)]">Status</p>
 <p class="text-sm font-bold {miningStatus.mining ?'text-amber-400' :'text-[var(--text-secondary)]'}">
 {miningStatus.mining ?'Mining' :'Inactive'}
 </p>
 </div>
 <div class="bg-[var(--surface-0)]/[0.07] rounded-lg p-3">
 <p class="text-xs text-[var(--text-secondary)]">Hash Rate</p>
 <p class="text-sm font-bold tabular-nums font-mono text-violet-400/90">{formatHashRate(miningStatus.hashRate)}</p>
 </div>
 <div class="bg-[var(--surface-0)]/[0.07] rounded-lg p-3">
 <p class="text-xs text-[var(--text-secondary)]">Total Mined</p>
 <p class="text-sm font-bold text-amber-400 tabular-nums">{miningStatus.totalMinedChi.toFixed(4)} CHI</p>
 </div>
 <div class="bg-[var(--surface-0)]/[0.07] rounded-lg p-3">
 <p class="text-xs text-[var(--text-secondary)]">Total Mined (Wei)</p>
 <p class="text-sm font-bold font-mono tabular-nums">{miningStatus.totalMinedWei}</p>
 </div>
 </div>

 {#if miningStatus.minerAddress}
 <div class="p-3 bg-[var(--surface-0)]/[0.07] rounded-lg">
 <p class="text-xs text-[var(--text-secondary)] mb-1">Miner Address (Coinbase)</p>
 <p class="font-mono text-xs break-all">{miningStatus.minerAddress}</p>
 </div>
 {:else}
 <div class="p-3 bg-yellow-50 rounded-lg">
 <p class="text-xs text-yellow-400">No miner address set. Set your wallet address to receive mining rewards.</p>
 </div>
 {/if}
 {:else}
 <p class="text-sm text-[var(--text-secondary)] text-center py-4">
 Click"Refresh" to load mining diagnostics
 </p>
 {/if}
 </div>
 {/if}
 </div>

 <!-- Geth Log Viewer -->
 <div class="bg-[var(--surface-1)] rounded-xl border border-[var(--border)]">
 <button
 onclick={() => showGethLogSection = !showGethLogSection}
 class="w-full flex items-center justify-between p-6 text-left"
 >
 <div class="flex items-center gap-3">
 <div class="p-2 bg-blue-100 rounded-lg">
 <FileText class="w-6 h-6 text-violet-400" />
 </div>
 <div>
 <h2 class="font-semibold">Geth Log</h2>
 <p class="text-sm text-[var(--text-secondary)]">Live Geth process output (geth.log)</p>
 </div>
 </div>
 {#if showGethLogSection}
 <ChevronUp class="w-5 h-5 text-[var(--text-secondary)]" />
 {:else}
 <ChevronDown class="w-5 h-5 text-[var(--text-secondary)]" />
 {/if}
 </button>

 {#if showGethLogSection}
 <div class="px-6 pb-6 space-y-4">
 <div class="flex items-center justify-between">
 <div class="flex items-center gap-2">
 <label for="geth-log-lines" class="text-xs text-[var(--text-secondary)]">Lines:</label>
 <select
 id="geth-log-lines"
 bind:value={gethLogLines}
 onchange={() => loadGethLog()}
 class="text-xs px-2 py-1 bg-[var(--surface-0)]/[0.07] border border-[var(--border)]/60 rounded"
 >
 <option value={50}>50</option>
 <option value={100}>100</option>
 <option value={200}>200</option>
 <option value={500}>500</option>
 </select>
 </div>
 <div class="flex items-center gap-2">
 <button
 onclick={() => {
 if (gethLogContent) {
 navigator.clipboard.writeText(gethLogContent).then(() => {
 toasts.show('Geth log copied','success');
 });
 }
 }}
 class="text-xs px-2 py-1 bg-[var(--surface-0)]/[0.07] hover:bg-[var(--surface-0)]/[0.1] rounded transition-colors flex items-center gap-1"
 >
 <Copy class="w-3 h-3" />
 Copy
 </button>
 <button
 onclick={loadGethLog}
 disabled={isLoadingGethLog}
 class="text-xs px-3 py-1.5 bg-[var(--surface-0)]/[0.07] hover:bg-[var(--surface-0)]/[0.1] rounded transition-colors flex items-center gap-1 disabled:opacity-50"
 >
 {#if isLoadingGethLog}
 <Loader2 class="w-3 h-3 animate-spin" />
 {:else}
 <RefreshCw class="w-3 h-3" />
 {/if}
 Refresh
 </button>
 </div>
 </div>

 <div class="bg-[var(--surface-0)]/[0.07] rounded-lg p-4 font-mono text-xs max-h-96 overflow-y-auto whitespace-pre-wrap">
 {#if gethLogContent}
 {#each gethLogContent.split('\n') as line}
 {@const parsed = parseStructuredGethLine(line)}
 <div class="flex gap-2 py-0.5 hover:bg-[var(--surface-0)]/[0.06] px-1 rounded">
 {#if parsed.timestamp}
 <span class="text-[var(--text-tertiary)] shrink-0">{parsed.timestamp}</span>
 {/if}
 {#if parsed.level}
 <span class="shrink-0 px-1 rounded text-[10px] uppercase font-bold {levelBg(parsed.level.toLowerCase())}">
 {parsed.level}
 </span>
 {/if}
 {#if parsed.source}
 <span class="shrink-0 px-1 rounded text-[10px] uppercase font-bold {sourceBg(parsed.source)}">
 {parsed.source}
 </span>
 {/if}
 <span class="{gethLogLineColor(parsed.message, parsed.level)} break-all">
 {parsed.message}
 </span>
 </div>
 {/each}
 {:else}
 <p class="text-[var(--text-tertiary)] text-center py-8">No Geth log available. Start the Geth node to generate logs.</p>
 {/if}
 </div>
 </div>
 {/if}
 </div>

 <!-- Event Logs -->
 <div class="bg-[var(--surface-1)] rounded-xl border border-[var(--border)]">
 <button
 onclick={() => showLogsSection = !showLogsSection}
 class="w-full flex items-center justify-between p-6 text-left"
 >
 <div class="flex items-center gap-3">
 <div class="p-2 bg-purple-900/30 rounded-lg">
 <Terminal class="w-6 h-6 text-purple-400" />
 </div>
 <div>
 <h2 class="font-semibold">Event Logs</h2>
 <p class="text-sm text-[var(--text-secondary)]">Real-time events and log messages ({logEntries.length} entries)</p>
 </div>
 </div>
 {#if showLogsSection}
 <ChevronUp class="w-5 h-5 text-[var(--text-secondary)]" />
 {:else}
 <ChevronDown class="w-5 h-5 text-[var(--text-secondary)]" />
 {/if}
 </button>

 {#if showLogsSection}
 <div class="px-6 pb-6 space-y-4">
 <!-- Log Controls -->
 <div class="flex flex-wrap items-center gap-3">
 <div class="flex items-center gap-2">
 <Filter class="w-4 h-4 text-[var(--text-secondary)]" />
 <select
 bind:value={logFilter}
 class="text-xs px-2 py-1 bg-[var(--surface-0)]/[0.07] border border-[var(--border)]/60 rounded"
 >
 <option value="all">All Levels</option>
 <option value="info">Info</option>
 <option value="warn">Warning</option>
 <option value="error">Error</option>
 <option value="debug">Debug</option>
 </select>
 <select
 bind:value={sourceFilter}
 class="text-xs px-2 py-1 bg-[var(--surface-0)]/[0.07] border border-[var(--border)]/60 rounded"
 >
 <option value="all">All Sources</option>
 <option value="dht">DHT</option>
 <option value="bootstrap">Bootstrap</option>
 <option value="geth">Geth</option>
 <option value="mining">Mining</option>
 <option value="system">System</option>
 </select>
 </div>
 <div class="flex items-center gap-2 ml-auto">
 <label class="flex items-center gap-1.5 text-xs text-[var(--text-secondary)]">
 <input type="checkbox" bind:checked={autoScroll} class="rounded" />
 Auto-scroll
 </label>
 <button
 onclick={copyLogs}
 class="text-xs px-2 py-1 bg-[var(--surface-0)]/[0.07] hover:bg-[var(--surface-0)]/[0.1] rounded transition-colors flex items-center gap-1"
 title="Copy logs to clipboard"
 >
 <Copy class="w-3 h-3" />
 Copy
 </button>
 <button
 onclick={exportLogs}
 class="text-xs px-2 py-1 bg-[var(--surface-0)]/[0.07] hover:bg-[var(--surface-0)]/[0.1] rounded transition-colors flex items-center gap-1"
 title="Export logs as file"
 >
 <Download class="w-3 h-3" />
 Export
 </button>
 <button
 onclick={clearLogs}
 class="text-xs px-2 py-1 bg-red-500/[0.08] hover:bg-red-200 text-red-400 rounded transition-colors flex items-center gap-1"
 title="Clear logs"
 >
 <Trash2 class="w-3 h-3" />
 Clear
 </button>
 </div>
 </div>

 <!-- Log Output -->
 <div class="bg-[var(--surface-0)]/[0.07] rounded-lg p-4 font-mono text-xs max-h-96 overflow-y-auto" id="log-output">
 {#if filteredLogs.length === 0}
 <p class="text-[var(--text-tertiary)] text-center py-8">No log entries{logFilter !=='all' || sourceFilter !=='all' ?' matching filters' :''}</p>
 {:else}
 {#each filteredLogs as entry (entry.id)}
 <div class="flex gap-2 py-0.5 hover:bg-[var(--surface-0)]/[0.06] px-1 rounded">
 <span class="text-[var(--text-tertiary)] shrink-0">{entry.timestamp.toLocaleTimeString()}</span>
 <span class="shrink-0 px-1 rounded {levelBg(entry.level)} text-[10px] uppercase font-bold">{entry.level}</span>
 <span class="shrink-0 px-1 rounded text-[10px] uppercase font-bold {sourceBg(entry.source)}">{entry.source}</span>
 <span class="text-[var(--text-secondary)] break-all">{entry.message}</span>
 </div>
 {/each}
 {/if}
 </div>
 </div>
 {/if}
 </div>
</div>
