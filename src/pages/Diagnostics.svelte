<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { networkConnected } from '$lib/stores';
  import { dhtService, type DhtHealthInfo } from '$lib/dhtService';
  import { toasts } from '$lib/toastStore';
  import {
    RefreshCw,
    Loader2,
    Trash2,
    Copy,
    Download,
    Globe,
    Server,
    Activity,
    Terminal,
    Pickaxe,
    FileText,
    ClipboardList,
  } from 'lucide-svelte';
  import { logger } from '$lib/logger';
  const log = logger('Diagnostics');

  // ---------- Types ----------

  interface LogEntry {
    id: number;
    timestamp: Date;
    level: 'info' | 'warn' | 'error' | 'debug';
    source: string;
    message: string;
  }

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
    totalMinedWei: string;
    totalMinedChi: number;
  }

  type Health = 'good' | 'warn' | 'bad' | 'idle';

  // ---------- State ----------

  let logEntries = $state<LogEntry[]>([]);
  let nextLogId = 0;
  let logFilter = $state<'all' | 'info' | 'warn' | 'error' | 'debug'>('all');
  let sourceFilter = $state<'all' | 'dht' | 'bootstrap' | 'geth' | 'mining' | 'system'>('all');
  const maxLogEntries = 500;

  let dhtHealth = $state<DhtHealthInfo | null>(null);
  let isLoadingDht = $state(false);

  let bootstrapHealth = $state<BootstrapHealthReport | null>(null);
  let isLoadingBootstrap = $state(false);

  let gethStatus = $state<GethStatus | null>(null);
  let isLoadingGeth = $state(false);

  let miningStatus = $state<MiningStatus | null>(null);
  let isLoadingMining = $state(false);

  let gethLogContent = $state('');
  let isLoadingGethLog = $state(false);
  let gethLogLines = $state(100);

  let logTab = $state<'events' | 'geth'>('events');

  let autoRefreshInterval: ReturnType<typeof setInterval> | null = null;
  let autoRefreshEnabled = $state(true);
  let autoRefreshSeconds = $state(5);

  let eventListeners: (() => void)[] = [];
  let autoScroll = $state(true);

  // Auto-scroll the active log view to the bottom on new entries.
  $effect(() => {
    if (!autoScroll) return;
    if (logTab === 'events' && logEntries.length > 0) {
      const el = document.getElementById('event-log-output');
      if (el) el.scrollTop = el.scrollHeight;
    } else if (logTab === 'geth' && gethLogContent) {
      const el = document.getElementById('geth-log-output');
      if (el) el.scrollTop = el.scrollHeight;
    }
  });

  function isTauri(): boolean {
    return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
  }

  function addLog(level: LogEntry['level'], source: string, message: string) {
    const entry: LogEntry = {
      id: nextLogId++,
      timestamp: new Date(),
      level,
      source,
      message,
    };
    logEntries = [...logEntries.slice(-(maxLogEntries - 1)), entry];
  }

  let filteredLogs = $derived(
    logEntries.filter((entry) => {
      if (logFilter !== 'all' && entry.level !== logFilter) return false;
      if (sourceFilter !== 'all' && entry.source.toLowerCase() !== sourceFilter) return false;
      return true;
    })
  );

  // ---------- Health derivations (drives the top status strip) ----------

  let dhtHealthState: Health = $derived(
    !dhtHealth
      ? 'idle'
      : dhtHealth.running && dhtHealth.connectedPeerCount > 0
        ? 'good'
        : dhtHealth.running
          ? 'warn'
          : 'bad'
  );
  let dhtHeadline: string = $derived(
    !dhtHealth
      ? '—'
      : dhtHealth.running
        ? `${dhtHealth.connectedPeerCount} peer${dhtHealth.connectedPeerCount === 1 ? '' : 's'}`
        : 'Stopped'
  );

  let bootstrapHealthState: Health = $derived(
    !bootstrapHealth
      ? 'idle'
      : bootstrapHealth.isHealthy
        ? 'good'
        : bootstrapHealth.healthyNodes > 0
          ? 'warn'
          : 'bad'
  );
  let bootstrapHeadline: string = $derived(
    !bootstrapHealth ? '—' : `${bootstrapHealth.healthyNodes}/${bootstrapHealth.totalNodes} reachable`
  );

  let gethHealthState: Health = $derived(
    !gethStatus
      ? 'idle'
      : gethStatus.running && !gethStatus.syncing
        ? 'good'
        : gethStatus.running
          ? 'warn'
          : 'bad'
  );
  let gethHeadline: string = $derived(
    !gethStatus
      ? '—'
      : !gethStatus.running
        ? 'Stopped'
        : gethStatus.syncing && gethStatus.highestBlock > 0
          ? `Syncing ${((gethStatus.currentBlock / gethStatus.highestBlock) * 100).toFixed(1)}%`
          : `Synced @ ${gethStatus.currentBlock.toLocaleString()}`
  );

  let miningHealthState: Health = $derived(
    !miningStatus ? 'idle' : miningStatus.mining ? 'good' : 'idle'
  );
  let miningHeadline: string = $derived(
    !miningStatus ? '—' : miningStatus.mining ? formatHashRate(miningStatus.hashRate) : 'Inactive'
  );

  // ---------- Lifecycle ----------

  onMount(async () => {
    addLog('info', 'system', 'Diagnostics page opened');

    if (isTauri()) {
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
        'file-payment-processing',
      ];

      for (const eventName of events) {
        try {
          const unlisten = await listen(eventName, (event) => {
            const payload =
              typeof event.payload === 'string'
                ? event.payload
                : JSON.stringify(event.payload, null, 0).slice(0, 200);
            const source = eventName.startsWith('geth')
              ? 'geth'
              : eventName.startsWith('peer') ||
                  eventName.startsWith('ping') ||
                  eventName.startsWith('pong')
                ? 'dht'
                : 'system';
            addLog('info', source, `Event: ${eventName} — ${payload}`);
          });
          eventListeners.push(unlisten);
        } catch {
          /* event listener setup is best-effort */
        }
      }

      await Promise.all([
        loadDhtHealth(),
        loadBootstrapHealth(),
        loadGethStatus(),
        loadMiningStatus(),
        loadGethLog(),
      ]);

      startAutoRefresh();
    }
  });

  // Pause auto-refresh while the page (or tab) is hidden — without this,
  // navigating off Diagnostics keeps firing every Tauri command on the
  // 5s interval forever (the component lives on persistent layouts and
  // doesn't unmount). When the user comes back, the listener kicks the
  // interval back on so they see fresh state.
  function handleVisibilityChange() {
    if (typeof document === 'undefined') return;
    if (document.visibilityState === 'visible') {
      if (autoRefreshEnabled && autoRefreshInterval === null) {
        startAutoRefresh();
      }
    } else {
      // Tear the interval down without flipping `autoRefreshEnabled` so
      // the user's preference survives the tab being away.
      if (autoRefreshInterval !== null) {
        clearInterval(autoRefreshInterval);
        autoRefreshInterval = null;
      }
    }
  }

  onMount(() => {
    if (typeof document !== 'undefined') {
      document.addEventListener('visibilitychange', handleVisibilityChange);
    }
  });

  onDestroy(() => {
    for (const unlisten of eventListeners) unlisten();
    stopAutoRefresh();
    if (typeof document !== 'undefined') {
      document.removeEventListener('visibilitychange', handleVisibilityChange);
    }
  });

  function startAutoRefresh() {
    stopAutoRefresh();
    // Don't start the interval if the page is currently hidden — the
    // visibilitychange handler will kick it on when the user comes back.
    const hidden = typeof document !== 'undefined' && document.visibilityState !== 'visible';
    if (autoRefreshEnabled && !hidden) {
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
      addLog('info', 'system', `Auto-refresh enabled (${autoRefreshSeconds}s)`);
    } else {
      stopAutoRefresh();
      addLog('info', 'system', 'Auto-refresh disabled');
    }
  }

  // ---------- Loaders ----------

  async function loadDhtHealth(notify = false) {
    isLoadingDht = true;
    try {
      dhtHealth = await dhtService.getHealth();
      if (notify) toasts.show('DHT diagnostics refreshed', 'success');
    } catch (err) {
      addLog('error', 'dht', `Failed to get DHT health: ${err}`);
      if (notify) toasts.detail('DHT refresh failed', String(err), 'error');
    } finally {
      isLoadingDht = false;
    }
  }

  async function loadBootstrapHealth(notify = false) {
    isLoadingBootstrap = true;
    try {
      const cached = await invoke<BootstrapHealthReport | null>('get_bootstrap_health');
      if (cached) bootstrapHealth = cached;
      if (notify) toasts.show('Bootstrap diagnostics refreshed', 'success');
    } catch (err) {
      addLog('warn', 'bootstrap', `No cached bootstrap health: ${err}`);
      if (notify) toasts.detail('Bootstrap refresh failed', String(err), 'error');
    } finally {
      isLoadingBootstrap = false;
    }
  }

  async function runBootstrapCheck() {
    isLoadingBootstrap = true;
    addLog('info', 'bootstrap', 'Running bootstrap health check...');
    try {
      bootstrapHealth = await invoke<BootstrapHealthReport>('check_bootstrap_health');
      addLog(
        'info',
        'bootstrap',
        `Bootstrap check complete: ${bootstrapHealth.healthyNodes}/${bootstrapHealth.totalNodes} healthy`
      );
      toasts.show('Bootstrap check complete', 'success');
    } catch (err) {
      addLog('error', 'bootstrap', `Bootstrap check failed: ${err}`);
      toasts.detail('Bootstrap check failed', String(err), 'error');
    } finally {
      isLoadingBootstrap = false;
    }
  }

  async function loadGethStatus(notify = false) {
    isLoadingGeth = true;
    try {
      gethStatus = await invoke<GethStatus>('get_geth_status');
      if (notify) toasts.show('Geth diagnostics refreshed', 'success');
    } catch (err) {
      addLog('error', 'geth', `Failed to get Geth status: ${err}`);
      if (notify) toasts.detail('Geth refresh failed', String(err), 'error');
      gethStatus = {
        installed: false,
        running: false,
        syncing: false,
        currentBlock: 0,
        highestBlock: 0,
        peerCount: 0,
        chainId: 0,
      };
    } finally {
      isLoadingGeth = false;
    }
  }

  async function loadMiningStatus(notify = false) {
    isLoadingMining = true;
    try {
      miningStatus = await invoke<MiningStatus>('get_mining_status');
      if (notify) toasts.show('Mining diagnostics refreshed', 'success');
    } catch (err) {
      addLog('error', 'mining', `Failed to get mining status: ${err}`);
      if (notify) toasts.detail('Mining refresh failed', String(err), 'error');
      miningStatus = null;
    } finally {
      isLoadingMining = false;
    }
  }

  async function loadGethLog(notify = false) {
    isLoadingGethLog = true;
    try {
      gethLogContent = await invoke<string>('read_geth_log', { lines: gethLogLines });
      if (notify) toasts.show('Geth log refreshed', 'success');
    } catch (err) {
      gethLogContent = `Error reading log: ${err}`;
      if (notify) toasts.detail('Geth log refresh failed', String(err), 'error');
    } finally {
      isLoadingGethLog = false;
    }
  }

  async function refreshAll() {
    addLog('info', 'system', 'Refreshing all diagnostics...');
    await Promise.all([
      loadDhtHealth(),
      loadBootstrapHealth(),
      loadGethStatus(),
      loadMiningStatus(),
      loadGethLog(),
    ]);
    toasts.show('All diagnostics refreshed', 'success');
  }

  // ---------- Snapshot for support ----------

  /// Bundle every visible-state value into a single text blob the user can
  /// paste into a bug report. Cuts log entries to the most recent 200 to
  /// keep the clipboard payload manageable.
  function copySnapshot() {
    const lines: string[] = [];
    lines.push(`# Chiral Network diagnostics snapshot`);
    lines.push(`# Captured: ${new Date().toISOString()}`);
    lines.push('');
    lines.push('## DHT');
    if (dhtHealth) {
      lines.push(`  running: ${dhtHealth.running}`);
      lines.push(`  peers: ${dhtHealth.connectedPeerCount}`);
      lines.push(`  kademliaPeers: ${dhtHealth.kademliaPeers}`);
      lines.push(`  sharedFiles: ${dhtHealth.sharedFiles}`);
      lines.push(`  peerId: ${dhtHealth.peerId ?? '(none)'}`);
      lines.push(`  listening:`);
      for (const a of dhtHealth.listeningAddresses) lines.push(`    - ${a}`);
    } else {
      lines.push('  (no data)');
    }
    lines.push('');
    lines.push('## Bootstrap');
    if (bootstrapHealth) {
      lines.push(
        `  ${bootstrapHealth.healthyNodes}/${bootstrapHealth.totalNodes} healthy at ${formatUnixSeconds(bootstrapHealth.timestamp)}`
      );
      for (const n of bootstrapHealth.nodes) {
        lines.push(
          `    - ${n.name} (${n.region}): ${n.reachable ? `${n.latencyMs ?? '?'}ms` : `unreachable${n.error ? ` — ${n.error}` : ''}`}`
        );
      }
    } else {
      lines.push('  (no data)');
    }
    lines.push('');
    lines.push('## Geth');
    if (gethStatus) {
      lines.push(`  running: ${gethStatus.running}`);
      lines.push(`  syncing: ${gethStatus.syncing}`);
      lines.push(`  currentBlock: ${gethStatus.currentBlock}`);
      lines.push(`  highestBlock: ${gethStatus.highestBlock}`);
      lines.push(`  peerCount: ${gethStatus.peerCount}`);
      lines.push(`  chainId: ${gethStatus.chainId}`);
    } else {
      lines.push('  (no data)');
    }
    lines.push('');
    lines.push('## Mining');
    if (miningStatus) {
      lines.push(`  mining: ${miningStatus.mining}`);
      lines.push(`  hashRate: ${formatHashRate(miningStatus.hashRate)}`);
      lines.push(`  minerAddress: ${miningStatus.minerAddress ?? '(none)'}`);
      lines.push(`  totalMined: ${miningStatus.totalMinedChi.toFixed(6)} CHI`);
    } else {
      lines.push('  (no data)');
    }
    lines.push('');
    lines.push('## Recent events (last 200)');
    for (const e of logEntries.slice(-200)) {
      lines.push(
        `  [${e.timestamp.toISOString()}] [${e.level.toUpperCase()}] [${e.source}] ${e.message}`
      );
    }
    const text = lines.join('\n');
    navigator.clipboard
      .writeText(text)
      .then(() => toasts.show('Diagnostics snapshot copied', 'success'))
      .catch(() => toasts.show('Copy failed', 'error'));
  }

  // ---------- Formatters ----------

  function formatHashRate(hr: number): string {
    if (!hr || hr <= 0) return '0 H/s';
    if (hr >= 1_000_000_000) return `${(hr / 1_000_000_000).toFixed(2)} GH/s`;
    if (hr >= 1_000_000) return `${(hr / 1_000_000).toFixed(2)} MH/s`;
    if (hr >= 1_000) return `${(hr / 1_000).toFixed(2)} KH/s`;
    return `${hr} H/s`;
  }

  function formatUnixSeconds(timestamp: number): string {
    return new Date(timestamp * 1000).toLocaleTimeString();
  }

  function clearLogs() {
    logEntries = [];
    addLog('info', 'system', 'Logs cleared');
  }

  function copyLogs() {
    const text = filteredLogs
      .map(
        (e) => `[${e.timestamp.toISOString()}] [${e.level.toUpperCase()}] [${e.source}] ${e.message}`
      )
      .join('\n');
    navigator.clipboard
      .writeText(text)
      .then(() => toasts.show('Logs copied', 'success'))
      .catch(() => toasts.show('Copy failed', 'error'));
  }

  function exportLogs() {
    const text = logEntries
      .map(
        (e) => `[${e.timestamp.toISOString()}] [${e.level.toUpperCase()}] [${e.source}] ${e.message}`
      )
      .join('\n');
    const blob = new Blob([text], { type: 'text/plain' });
    const url = URL.createObjectURL(blob);
    const a = document.createElement('a');
    a.href = url;
    a.download = `chiral-diagnostics-${new Date().toISOString().slice(0, 19).replace(/:/g, '-')}.log`;
    a.click();
    URL.revokeObjectURL(url);
  }

  function levelBg(level: string): string {
    switch (level) {
      case 'error':
        return 'bg-red-100 dark:bg-red-900/30 text-red-700 dark:text-red-400';
      case 'warn':
        return 'bg-yellow-100 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-400';
      case 'info':
        return 'bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-400';
      case 'debug':
        return 'bg-gray-100 dark:bg-gray-700 text-gray-600 dark:text-gray-400';
      default:
        return 'bg-gray-100 dark:bg-gray-700';
    }
  }

  function sourceBg(source: string): string {
    const normalized = source.toLowerCase();
    switch (normalized) {
      case 'geth':
        return 'bg-cyan-100 dark:bg-cyan-900/30 text-cyan-700 dark:text-cyan-300';
      case 'mining':
        return 'bg-amber-100 dark:bg-amber-900/30 text-amber-700 dark:text-amber-300';
      case 'dht':
        return 'bg-emerald-100 dark:bg-emerald-900/30 text-emerald-700 dark:text-emerald-300';
      case 'bootstrap':
        return 'bg-orange-100 dark:bg-orange-900/30 text-orange-700 dark:text-orange-300';
      case 'system':
        return 'bg-violet-100 dark:bg-violet-900/30 text-violet-700 dark:text-violet-300';
      default:
        return 'bg-gray-100 dark:bg-gray-700 text-gray-700 dark:text-gray-300';
    }
  }

  function gethLogLineColor(line: string, level: string | null): string {
    if (
      level === 'ERROR' ||
      line.includes('Fatal') ||
      line.includes('ERROR') ||
      line.includes('error')
    )
      return 'text-red-400';
    if (level === 'WARN' || line.includes('WARN') || line.includes('warn')) return 'text-yellow-400';
    if (level === 'DEBUG') return 'text-slate-400';
    return 'text-gray-300';
  }

  function parseStructuredGethLine(line: string): {
    timestamp: string | null;
    level: 'INFO' | 'WARN' | 'ERROR' | 'DEBUG' | null;
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
      level: level as 'INFO' | 'WARN' | 'ERROR' | 'DEBUG',
      source,
      message,
    };
  }

  // ---------- Status-strip helpers ----------

  function healthDot(state: Health): string {
    switch (state) {
      case 'good':
        return 'bg-green-500';
      case 'warn':
        return 'bg-amber-500';
      case 'bad':
        return 'bg-red-500';
      default:
        return 'bg-gray-300 dark:bg-gray-600';
    }
  }

  function healthRing(state: Health): string {
    switch (state) {
      case 'good':
        return 'ring-green-500/40 hover:ring-green-500/70';
      case 'warn':
        return 'ring-amber-500/40 hover:ring-amber-500/70';
      case 'bad':
        return 'ring-red-500/40 hover:ring-red-500/70';
      default:
        return 'ring-gray-300/40 dark:ring-gray-600/40 hover:ring-gray-400/60';
    }
  }

  function scrollTo(id: string) {
    const el = document.getElementById(id);
    if (el) el.scrollIntoView({ behavior: 'smooth', block: 'start' });
  }
</script>

<svelte:head><title>Diagnostics | Chiral Network</title></svelte:head>

<div class="p-4 sm:p-6 space-y-4 max-w-[1600px] mx-auto">
  <!-- Header -->
  <div class="flex flex-wrap items-center justify-between gap-3">
    <div>
      <h1 class="text-2xl font-bold text-gray-900 dark:text-white">Diagnostics</h1>
      <p class="text-sm text-gray-500 dark:text-gray-400 mt-0.5">
        Live status of every subsystem. Click a tile to jump to its detail panel.
      </p>
    </div>
    <div class="flex items-center gap-2">
      <button
        onclick={toggleAutoRefresh}
        class="text-xs px-3 py-1.5 rounded-full border transition-colors flex items-center gap-1.5 {autoRefreshEnabled
          ? 'border-green-500/40 bg-green-50 dark:bg-green-900/20 text-green-700 dark:text-green-400'
          : 'border-gray-300 dark:border-gray-600 bg-gray-100 dark:bg-gray-700 text-gray-500 dark:text-gray-400'}"
        title={autoRefreshEnabled ? 'Click to pause auto-refresh' : 'Click to resume auto-refresh'}
      >
        <span
          class="w-1.5 h-1.5 rounded-full {autoRefreshEnabled
            ? 'bg-green-500 animate-pulse'
            : 'bg-gray-400'}"
        ></span>
        Auto-refresh {autoRefreshEnabled ? `· ${autoRefreshSeconds}s` : '· off'}
      </button>
      <button
        onclick={copySnapshot}
        class="text-xs px-3 py-1.5 rounded-lg bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 flex items-center gap-1.5"
        title="Copy a one-shot snapshot of every panel + recent events for support"
      >
        <ClipboardList class="w-3.5 h-3.5" />
        Copy snapshot
      </button>
      <button
        onclick={refreshAll}
        class="p-2 rounded-lg bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300"
        title="Refresh every panel"
      >
        <RefreshCw class="w-4 h-4" />
      </button>
    </div>
  </div>

  <!-- Sticky status strip -->
  <div
    class="sticky top-0 z-10 grid grid-cols-2 md:grid-cols-4 gap-2 p-2 rounded-2xl bg-white/80 dark:bg-gray-800/80 backdrop-blur border border-gray-200 dark:border-gray-700 shadow-sm"
  >
    {#each [{ id: 'card-dht', icon: Globe, label: 'DHT', state: dhtHealthState, headline: dhtHeadline }, { id: 'card-bootstrap', icon: Activity, label: 'Bootstrap', state: bootstrapHealthState, headline: bootstrapHeadline }, { id: 'card-geth', icon: Server, label: 'Geth', state: gethHealthState, headline: gethHeadline }, { id: 'card-mining', icon: Pickaxe, label: 'Mining', state: miningHealthState, headline: miningHeadline }] as tile}
      <button
        onclick={() => scrollTo(tile.id)}
        class="flex items-center gap-3 px-3 py-2 rounded-xl ring-1 {healthRing(
          tile.state
        )} bg-white dark:bg-gray-800 hover:bg-gray-50 dark:hover:bg-gray-700/60 transition-all text-left"
      >
        <div class="relative shrink-0">
          <tile.icon class="w-5 h-5 text-gray-600 dark:text-gray-300" />
          <span
            class="absolute -top-0.5 -right-0.5 w-2 h-2 rounded-full {healthDot(
              tile.state
            )} ring-2 ring-white dark:ring-gray-800"
          ></span>
        </div>
        <div class="min-w-0">
          <div class="text-[11px] uppercase tracking-wider text-gray-500 dark:text-gray-400">
            {tile.label}
          </div>
          <div class="text-sm font-semibold text-gray-900 dark:text-white truncate tabular-nums">
            {tile.headline}
          </div>
        </div>
      </button>
    {/each}
  </div>

  <!-- Two-column body -->
  <div class="grid grid-cols-1 lg:grid-cols-5 gap-4">
    <!-- Left column: detail cards (stacked) -->
    <div class="lg:col-span-2 space-y-4">
      <!-- DHT card -->
      <div
        id="card-dht"
        class="bg-white dark:bg-gray-800 rounded-2xl shadow-sm border border-gray-200 dark:border-gray-700"
      >
        <div class="flex items-center justify-between p-4 border-b border-gray-100 dark:border-gray-700">
          <div class="flex items-center gap-2.5 min-w-0">
            <div class="p-1.5 rounded-lg {$networkConnected ? 'bg-emerald-100 dark:bg-emerald-900/30' : 'bg-gray-100 dark:bg-gray-700'}">
              <Globe class="w-4 h-4 {$networkConnected ? 'text-emerald-600 dark:text-emerald-400' : 'text-gray-600 dark:text-gray-400'}" />
            </div>
            <h2 class="text-sm font-semibold text-gray-900 dark:text-white">DHT</h2>
            <span class="text-xs text-gray-400 dark:text-gray-500 truncate">peer network</span>
          </div>
          <button
            onclick={() => loadDhtHealth(true)}
            disabled={isLoadingDht}
            class="p-1.5 rounded hover:bg-gray-100 dark:hover:bg-gray-700 disabled:opacity-50 text-gray-500 dark:text-gray-400"
            title="Refresh DHT"
          >
            {#if isLoadingDht}
              <Loader2 class="w-3.5 h-3.5 animate-spin" />
            {:else}
              <RefreshCw class="w-3.5 h-3.5" />
            {/if}
          </button>
        </div>

        <div class="p-4 space-y-3">
          {#if dhtHealth}
            <div class="grid grid-cols-3 gap-2">
              <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2">
                <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Status</p>
                <p
                  class="text-sm font-semibold {dhtHealth.running
                    ? 'text-green-600 dark:text-green-400'
                    : 'text-red-600 dark:text-red-400'}"
                >
                  {dhtHealth.running ? 'Running' : 'Stopped'}
                </p>
              </div>
              <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2">
                <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Peers</p>
                <p class="text-sm font-semibold dark:text-white tabular-nums">{dhtHealth.connectedPeerCount}</p>
              </div>
              <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2">
                <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Kad table</p>
                <p class="text-sm font-semibold dark:text-white tabular-nums">{dhtHealth.kademliaPeers}</p>
              </div>
              <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2 col-span-3">
                <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Shared files</p>
                <p class="text-sm font-semibold dark:text-white tabular-nums">{dhtHealth.sharedFiles}</p>
              </div>
            </div>

            {#if dhtHealth.peerId}
              <details class="group">
                <summary class="text-xs text-gray-500 dark:text-gray-400 cursor-pointer hover:text-gray-700 dark:hover:text-gray-200">
                  Peer ID + listening addrs ({dhtHealth.listeningAddresses.length})
                </summary>
                <div class="mt-2 space-y-1.5">
                  <p class="font-mono text-[11px] break-all dark:text-gray-300 bg-gray-50 dark:bg-gray-700/50 rounded p-2">
                    {dhtHealth.peerId}
                  </p>
                  {#each dhtHealth.listeningAddresses as addr}
                    <p class="font-mono text-[11px] break-all dark:text-gray-300 bg-gray-50 dark:bg-gray-700/50 rounded p-2">
                      {addr}
                    </p>
                  {/each}
                </div>
              </details>
            {/if}

            {#if dhtHealth.protocols.length > 0}
              <details>
                <summary class="text-xs text-gray-500 dark:text-gray-400 cursor-pointer hover:text-gray-700 dark:hover:text-gray-200">
                  Active protocols ({dhtHealth.protocols.length})
                </summary>
                <div class="mt-2 flex flex-wrap gap-1">
                  {#each dhtHealth.protocols as protocol}
                    <span class="px-1.5 py-0.5 bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-400 text-[10px] rounded font-mono">
                      {protocol}
                    </span>
                  {/each}
                </div>
              </details>
            {/if}

            {#if dhtHealth.bootstrapNodes.length > 0}
              <details>
                <summary class="text-xs text-gray-500 dark:text-gray-400 cursor-pointer hover:text-gray-700 dark:hover:text-gray-200">
                  DHT bootstrap nodes ({dhtHealth.bootstrapNodes.length})
                </summary>
                <div class="mt-2 space-y-1">
                  {#each dhtHealth.bootstrapNodes as node}
                    <div class="flex items-center gap-2 text-[11px]">
                      <div class="w-1.5 h-1.5 rounded-full {node.reachable ? 'bg-green-500' : 'bg-red-500'} shrink-0"></div>
                      <span class="font-mono break-all dark:text-gray-300 flex-1 min-w-0">{node.address}</span>
                      <span class="{node.reachable ? 'text-green-600 dark:text-green-400' : 'text-red-600 dark:text-red-400'} shrink-0">
                        {node.reachable ? 'OK' : 'Unreachable'}
                      </span>
                    </div>
                  {/each}
                </div>
              </details>
            {/if}
          {:else}
            <p class="text-sm text-gray-500 dark:text-gray-400 text-center py-4">No data yet — refresh to load.</p>
          {/if}
        </div>
      </div>

      <!-- Bootstrap card -->
      <div
        id="card-bootstrap"
        class="bg-white dark:bg-gray-800 rounded-2xl shadow-sm border border-gray-200 dark:border-gray-700"
      >
        <div class="flex items-center justify-between p-4 border-b border-gray-100 dark:border-gray-700">
          <div class="flex items-center gap-2.5 min-w-0">
            <div class="p-1.5 rounded-lg bg-orange-100 dark:bg-orange-900/30">
              <Activity class="w-4 h-4 text-orange-600 dark:text-orange-400" />
            </div>
            <h2 class="text-sm font-semibold text-gray-900 dark:text-white">Bootstrap</h2>
            <span class="text-xs text-gray-400 dark:text-gray-500 truncate">enode reachability</span>
          </div>
          <button
            onclick={runBootstrapCheck}
            disabled={isLoadingBootstrap}
            class="p-1.5 rounded hover:bg-gray-100 dark:hover:bg-gray-700 disabled:opacity-50 text-gray-500 dark:text-gray-400"
            title="Run a fresh bootstrap check"
          >
            {#if isLoadingBootstrap}
              <Loader2 class="w-3.5 h-3.5 animate-spin" />
            {:else}
              <Activity class="w-3.5 h-3.5" />
            {/if}
          </button>
        </div>

        <div class="p-4 space-y-3">
          {#if bootstrapHealth}
            <div class="grid grid-cols-2 gap-2">
              <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2">
                <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Healthy</p>
                <p class="text-sm font-semibold dark:text-white tabular-nums">
                  {bootstrapHealth.healthyNodes}/{bootstrapHealth.totalNodes}
                </p>
              </div>
              <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2">
                <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Last check</p>
                <p class="text-sm font-semibold dark:text-white">{formatUnixSeconds(bootstrapHealth.timestamp)}</p>
              </div>
            </div>

            <div class="space-y-1.5">
              {#each bootstrapHealth.nodes as node}
                <div class="flex items-center justify-between gap-2 px-2 py-1.5 bg-gray-50 dark:bg-gray-700/50 rounded">
                  <div class="flex items-center gap-2 min-w-0">
                    <div class="w-1.5 h-1.5 rounded-full {node.reachable ? 'bg-green-500' : 'bg-red-500'} shrink-0"></div>
                    <span class="text-xs font-medium dark:text-white truncate">{node.name}</span>
                    <span class="text-[10px] text-gray-500 dark:text-gray-400 shrink-0">{node.region}</span>
                  </div>
                  <div class="text-[11px] shrink-0 tabular-nums">
                    {#if node.reachable && node.latencyMs}
                      <span class="text-green-600 dark:text-green-400">{node.latencyMs}ms</span>
                    {:else if node.error}
                      <span class="text-red-500 dark:text-red-400 truncate" title={node.error}>error</span>
                    {:else}
                      <span class="text-red-500 dark:text-red-400">down</span>
                    {/if}
                  </div>
                </div>
              {/each}
            </div>
          {:else}
            <p class="text-sm text-gray-500 dark:text-gray-400 text-center py-4">
              No data yet — click the icon above to run a check.
            </p>
          {/if}
        </div>
      </div>

      <!-- Geth card -->
      <div
        id="card-geth"
        class="bg-white dark:bg-gray-800 rounded-2xl shadow-sm border border-gray-200 dark:border-gray-700"
      >
        <div class="flex items-center justify-between p-4 border-b border-gray-100 dark:border-gray-700">
          <div class="flex items-center gap-2.5 min-w-0">
            <div class="p-1.5 rounded-lg {gethStatus?.running ? 'bg-emerald-100 dark:bg-emerald-900/30' : 'bg-gray-100 dark:bg-gray-700'}">
              <Server class="w-4 h-4 {gethStatus?.running ? 'text-emerald-600 dark:text-emerald-400' : 'text-gray-600 dark:text-gray-400'}" />
            </div>
            <h2 class="text-sm font-semibold text-gray-900 dark:text-white">Geth</h2>
            <span class="text-xs text-gray-400 dark:text-gray-500 truncate">blockchain node</span>
          </div>
          <button
            onclick={() => loadGethStatus(true)}
            disabled={isLoadingGeth}
            class="p-1.5 rounded hover:bg-gray-100 dark:hover:bg-gray-700 disabled:opacity-50 text-gray-500 dark:text-gray-400"
            title="Refresh Geth"
          >
            {#if isLoadingGeth}
              <Loader2 class="w-3.5 h-3.5 animate-spin" />
            {:else}
              <RefreshCw class="w-3.5 h-3.5" />
            {/if}
          </button>
        </div>

        <div class="p-4">
          {#if gethStatus}
            <div class="grid grid-cols-2 gap-2">
              <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2">
                <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Status</p>
                <p
                  class="text-sm font-semibold {gethStatus.running
                    ? 'text-green-600 dark:text-green-400'
                    : 'text-red-600 dark:text-red-400'}"
                >
                  {gethStatus.running ? 'Running' : 'Stopped'}
                </p>
              </div>
              <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2">
                <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Sync</p>
                <p class="text-sm font-semibold dark:text-white tabular-nums">
                  {#if gethStatus.syncing && gethStatus.highestBlock > 0}
                    {((gethStatus.currentBlock / gethStatus.highestBlock) * 100).toFixed(1)}%
                  {:else if gethStatus.running}
                    Synced
                  {:else}
                    —
                  {/if}
                </p>
              </div>
              <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2">
                <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Block</p>
                <p class="text-sm font-semibold dark:text-white tabular-nums">{gethStatus.currentBlock.toLocaleString()}</p>
              </div>
              <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2">
                <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Highest</p>
                <p class="text-sm font-semibold dark:text-white tabular-nums">{gethStatus.highestBlock.toLocaleString()}</p>
              </div>
              <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2">
                <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Peers</p>
                <p class="text-sm font-semibold dark:text-white tabular-nums">{gethStatus.peerCount}</p>
              </div>
              <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2">
                <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Chain ID</p>
                <p class="text-sm font-semibold dark:text-white tabular-nums">{gethStatus.chainId || '—'}</p>
              </div>
            </div>

            {#if !gethStatus.installed}
              <p class="mt-3 text-xs text-amber-600 dark:text-amber-400">
                Geth is not installed. Visit the Network page to install it.
              </p>
            {/if}

            {#if gethStatus.syncing && gethStatus.highestBlock > 0}
              <div class="mt-3 h-1.5 bg-gray-100 dark:bg-gray-700 rounded-full overflow-hidden">
                <div
                  class="h-full bg-blue-500 transition-all"
                  style:width="{(gethStatus.currentBlock / gethStatus.highestBlock) * 100}%"
                ></div>
              </div>
            {/if}
          {:else}
            <p class="text-sm text-gray-500 dark:text-gray-400 text-center py-4">No data yet — refresh to load.</p>
          {/if}
        </div>
      </div>

      <!-- Mining card -->
      <div
        id="card-mining"
        class="bg-white dark:bg-gray-800 rounded-2xl shadow-sm border border-gray-200 dark:border-gray-700"
      >
        <div class="flex items-center justify-between p-4 border-b border-gray-100 dark:border-gray-700">
          <div class="flex items-center gap-2.5 min-w-0">
            <div class="p-1.5 rounded-lg {miningStatus?.mining ? 'bg-amber-100 dark:bg-amber-900/30' : 'bg-gray-100 dark:bg-gray-700'}">
              <Pickaxe class="w-4 h-4 {miningStatus?.mining ? 'text-amber-600 dark:text-amber-400' : 'text-gray-600 dark:text-gray-400'}" />
            </div>
            <h2 class="text-sm font-semibold text-gray-900 dark:text-white">Mining</h2>
            <span class="text-xs text-gray-400 dark:text-gray-500 truncate">CHI rewards</span>
          </div>
          <button
            onclick={() => loadMiningStatus(true)}
            disabled={isLoadingMining}
            class="p-1.5 rounded hover:bg-gray-100 dark:hover:bg-gray-700 disabled:opacity-50 text-gray-500 dark:text-gray-400"
            title="Refresh mining"
          >
            {#if isLoadingMining}
              <Loader2 class="w-3.5 h-3.5 animate-spin" />
            {:else}
              <RefreshCw class="w-3.5 h-3.5" />
            {/if}
          </button>
        </div>

        <div class="p-4 space-y-3">
          {#if miningStatus}
            <div class="grid grid-cols-3 gap-2">
              <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2">
                <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400">State</p>
                <p
                  class="text-sm font-semibold {miningStatus.mining
                    ? 'text-amber-600 dark:text-amber-400'
                    : 'text-gray-600 dark:text-gray-400'}"
                >
                  {miningStatus.mining ? 'Mining' : 'Idle'}
                </p>
              </div>
              <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2">
                <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Hashrate</p>
                <p class="text-sm font-semibold dark:text-white tabular-nums">{formatHashRate(miningStatus.hashRate)}</p>
              </div>
              <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2">
                <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Mined</p>
                <p class="text-sm font-semibold text-amber-600 dark:text-amber-400 tabular-nums">
                  {miningStatus.totalMinedChi.toFixed(4)} CHI
                </p>
              </div>
            </div>

            {#if miningStatus.minerAddress}
              <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2">
                <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400 mb-0.5">Coinbase</p>
                <p class="font-mono text-[11px] break-all dark:text-gray-300">{miningStatus.minerAddress}</p>
              </div>
            {:else}
              <div class="p-2 bg-yellow-50 dark:bg-yellow-900/20 rounded-lg">
                <p class="text-xs text-yellow-700 dark:text-yellow-400">
                  No miner address set — set your wallet to receive rewards.
                </p>
              </div>
            {/if}
          {:else}
            <p class="text-sm text-gray-500 dark:text-gray-400 text-center py-4">No data yet — refresh to load.</p>
          {/if}
        </div>
      </div>
    </div>

    <!-- Right column: tabbed log viewer (sticky on desktop so it stays visible) -->
    <div class="lg:col-span-3">
      <div
        class="bg-white dark:bg-gray-800 rounded-2xl shadow-sm border border-gray-200 dark:border-gray-700 lg:sticky lg:top-28"
      >
        <!-- Tab header -->
        <div class="flex items-center justify-between border-b border-gray-100 dark:border-gray-700">
          <div class="flex">
            <button
              onclick={() => (logTab = 'events')}
              class="px-4 py-3 text-sm font-medium border-b-2 transition-colors flex items-center gap-2 {logTab ===
              'events'
                ? 'border-purple-500 text-purple-700 dark:text-purple-400'
                : 'border-transparent text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200'}"
            >
              <Terminal class="w-4 h-4" />
              Events
              <span class="text-[10px] tabular-nums px-1.5 py-0.5 rounded {logTab === 'events' ? 'bg-purple-100 dark:bg-purple-900/30' : 'bg-gray-100 dark:bg-gray-700'}">
                {logEntries.length}
              </span>
            </button>
            <button
              onclick={() => (logTab = 'geth')}
              class="px-4 py-3 text-sm font-medium border-b-2 transition-colors flex items-center gap-2 {logTab ===
              'geth'
                ? 'border-cyan-500 text-cyan-700 dark:text-cyan-400'
                : 'border-transparent text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200'}"
            >
              <FileText class="w-4 h-4" />
              Geth log
            </button>
          </div>

          <div class="flex items-center gap-2 px-3">
            <label class="flex items-center gap-1.5 text-xs text-gray-500 dark:text-gray-400">
              <input type="checkbox" bind:checked={autoScroll} class="rounded" />
              Tail
            </label>
          </div>
        </div>

        <!-- Tab content: Events -->
        {#if logTab === 'events'}
          <div class="p-3 space-y-2">
            <div class="flex flex-wrap items-center gap-2">
              <select
                bind:value={logFilter}
                class="text-xs px-2 py-1 bg-gray-100 dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded dark:text-gray-300"
              >
                <option value="all">All levels</option>
                <option value="info">Info</option>
                <option value="warn">Warning</option>
                <option value="error">Error</option>
                <option value="debug">Debug</option>
              </select>
              <select
                bind:value={sourceFilter}
                class="text-xs px-2 py-1 bg-gray-100 dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded dark:text-gray-300"
              >
                <option value="all">All sources</option>
                <option value="dht">DHT</option>
                <option value="bootstrap">Bootstrap</option>
                <option value="geth">Geth</option>
                <option value="mining">Mining</option>
                <option value="system">System</option>
              </select>
              <span class="text-[11px] text-gray-400 dark:text-gray-500 tabular-nums">
                {filteredLogs.length} / {logEntries.length}
              </span>
              <div class="ml-auto flex items-center gap-1">
                <button
                  onclick={copyLogs}
                  class="text-xs px-2 py-1 rounded hover:bg-gray-100 dark:hover:bg-gray-700 text-gray-600 dark:text-gray-300 flex items-center gap-1"
                  title="Copy filtered logs"
                >
                  <Copy class="w-3 h-3" />
                  Copy
                </button>
                <button
                  onclick={exportLogs}
                  class="text-xs px-2 py-1 rounded hover:bg-gray-100 dark:hover:bg-gray-700 text-gray-600 dark:text-gray-300 flex items-center gap-1"
                  title="Export all logs as a file"
                >
                  <Download class="w-3 h-3" />
                  Export
                </button>
                <button
                  onclick={clearLogs}
                  class="text-xs px-2 py-1 rounded text-red-600 dark:text-red-400 hover:bg-red-50 dark:hover:bg-red-900/30 flex items-center gap-1"
                  title="Clear logs"
                >
                  <Trash2 class="w-3 h-3" />
                  Clear
                </button>
              </div>
            </div>

            <div
              id="event-log-output"
              class="bg-gray-900 rounded-lg p-3 font-mono text-[11px] h-[28rem] lg:h-[36rem] overflow-y-auto"
            >
              {#if filteredLogs.length === 0}
                <p class="text-gray-500 text-center py-8">
                  No log entries{logFilter !== 'all' || sourceFilter !== 'all' ? ' matching filters' : ''}
                </p>
              {:else}
                {#each filteredLogs as entry (entry.id)}
                  <div class="flex gap-2 py-0.5 hover:bg-gray-800 px-1 rounded group">
                    <span class="text-gray-500 shrink-0">{entry.timestamp.toLocaleTimeString()}</span>
                    <span class="shrink-0 px-1 rounded {levelBg(entry.level)} text-[10px] uppercase font-bold">
                      {entry.level}
                    </span>
                    <span class="shrink-0 px-1 rounded text-[10px] uppercase font-bold {sourceBg(entry.source)}">
                      {entry.source}
                    </span>
                    <span class="text-gray-300 break-all flex-1">{entry.message}</span>
                    <button
                      class="shrink-0 px-1 text-gray-600 hover:text-gray-300 opacity-0 group-hover:opacity-100 transition-opacity"
                      title="Copy log entry"
                      onclick={() => {
                        navigator.clipboard.writeText(
                          `[${entry.timestamp.toISOString()}] [${entry.level}] [${entry.source}] ${entry.message}`
                        );
                        toasts.show('Log entry copied', 'success');
                      }}
                    >
                      <Copy class="w-3 h-3" />
                    </button>
                  </div>
                {/each}
              {/if}
            </div>
          </div>
        {:else}
          <!-- Tab content: Geth log -->
          <div class="p-3 space-y-2">
            <div class="flex items-center gap-2">
              <label for="geth-log-lines" class="text-xs text-gray-500 dark:text-gray-400">Lines:</label>
              <select
                id="geth-log-lines"
                bind:value={gethLogLines}
                onchange={() => loadGethLog(true)}
                class="text-xs px-2 py-1 bg-gray-100 dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded dark:text-gray-300"
              >
                <option value={50}>50</option>
                <option value={100}>100</option>
                <option value={200}>200</option>
                <option value={500}>500</option>
              </select>
              <div class="ml-auto flex items-center gap-1">
                <button
                  onclick={() => {
                    if (gethLogContent) {
                      navigator.clipboard.writeText(gethLogContent).then(() => {
                        toasts.show('Log copied', 'success');
                      });
                    }
                  }}
                  class="text-xs px-2 py-1 rounded hover:bg-gray-100 dark:hover:bg-gray-700 text-gray-600 dark:text-gray-300 flex items-center gap-1"
                >
                  <Copy class="w-3 h-3" />
                  Copy
                </button>
                <button
                  onclick={() => loadGethLog(true)}
                  disabled={isLoadingGethLog}
                  class="text-xs px-2 py-1 rounded hover:bg-gray-100 dark:hover:bg-gray-700 text-gray-600 dark:text-gray-300 disabled:opacity-50 flex items-center gap-1"
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

            <div
              id="geth-log-output"
              class="bg-gray-900 rounded-lg p-3 font-mono text-[11px] h-[28rem] lg:h-[36rem] overflow-y-auto whitespace-pre-wrap"
            >
              {#if gethLogContent}
                {#each gethLogContent.split('\n') as line}
                  {@const parsed = parseStructuredGethLine(line)}
                  <div class="flex gap-2 py-0.5 hover:bg-gray-800 px-1 rounded">
                    {#if parsed.timestamp}
                      <span class="text-gray-500 shrink-0">{parsed.timestamp}</span>
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
                <p class="text-gray-500 text-center py-8">
                  No Geth log available. Start the Geth node to generate logs.
                </p>
              {/if}
            </div>
          </div>
        {/if}
      </div>
    </div>
  </div>
</div>
