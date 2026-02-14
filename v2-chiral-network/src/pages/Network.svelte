<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { peers, networkStats, networkConnected, walletAccount } from '$lib/stores';
  import { dhtService, type DhtHealthInfo } from '$lib/dhtService';
  import { toasts } from '$lib/toastStore';
  import {
    Play,
    Square,
    Radio,
    Server,
    Download,
    Upload,
    RefreshCw,
    AlertTriangle,
    Check,
    Loader2,
    Globe,
    Activity,
    HeartPulse,
    ChevronDown,
    ChevronUp,
    ArrowDownToLine,
    ArrowUpFromLine
  } from 'lucide-svelte';
  import { logger } from '$lib/logger';
  const log = logger('Network');

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

  interface DownloadProgress {
    downloaded: number;
    total: number;
    percentage: number;
    status: string;
  }

  interface NodeHealth {
    enode: string;
    name: string;
    region: string;
    reachable: boolean;
    latencyMs: number | null;
    error: string | null;
    lastChecked: number;
  }

  interface BootstrapHealthReport {
    totalNodes: number;
    healthyNodes: number;
    nodes: NodeHealth[];
    timestamp: number;
    isHealthy: boolean;
    healthyEnodeString: string;
  }

  // DHT State
  let isConnecting = $state(false);
  let error = $state('');
  let localPeerId = $state('');

  // Geth State
  let gethStatus = $state<GethStatus | null>(null);
  let isLoadingGeth = $state(true);
  let isStartingGeth = $state(false);
  let isDownloading = $state(false);
  let downloadProgress = $state<DownloadProgress | null>(null);
  let refreshInterval: ReturnType<typeof setInterval> | null = null;
  let unlistenDownload: (() => void) | null = null;

  // Bootstrap Health State
  let bootstrapHealth = $state<BootstrapHealthReport | null>(null);
  let isCheckingBootstrap = $state(false);
  let showBootstrapDetails = $state(false);

  // "Connecting to network" message auto-dismiss
  let showGethConnectingMsg = $state(false);
  let gethConnectingTimeout: ReturnType<typeof setTimeout> | null = null;

  // DHT Health State
  let dhtHealth = $state<DhtHealthInfo | null>(null);
  let isCheckingDhtHealth = $state(false);
  let showDhtHealthDetails = $state(false);

  // Traffic Statistics State
  let trafficStats = $state({
    totalDownloaded: 0,
    totalUploaded: 0,
    downloadSpeed: 0,
    uploadSpeed: 0,
    sessionStart: Date.now()
  });
  let trafficInterval: ReturnType<typeof setInterval> | null = null;

  // Load traffic stats from localStorage
  function loadTrafficStats() {
    if (typeof window === 'undefined') return;
    const saved = localStorage.getItem('chiral-traffic-stats');
    if (saved) {
      try {
        const parsed = JSON.parse(saved);
        trafficStats = { ...trafficStats, ...parsed, sessionStart: Date.now() };
      } catch {}
    }
  }

  function saveTrafficStats() {
    if (typeof window === 'undefined') return;
    localStorage.setItem('chiral-traffic-stats', JSON.stringify({
      totalDownloaded: trafficStats.totalDownloaded,
      totalUploaded: trafficStats.totalUploaded
    }));
  }

  function formatSpeed(bytesPerSec: number): string {
    if (bytesPerSec >= 1e9) return `${(bytesPerSec / 1e9).toFixed(2)} GB/s`;
    if (bytesPerSec >= 1e6) return `${(bytesPerSec / 1e6).toFixed(2)} MB/s`;
    if (bytesPerSec >= 1e3) return `${(bytesPerSec / 1e3).toFixed(2)} KB/s`;
    return `${bytesPerSec.toFixed(0)} B/s`;
  }

  // Show "connecting" message only when Geth is running with 0 peers, auto-dismiss after 30s
  $effect(() => {
    if (gethStatus?.running && gethStatus?.peerCount === 0) {
      showGethConnectingMsg = true;
      if (gethConnectingTimeout) clearTimeout(gethConnectingTimeout);
      gethConnectingTimeout = setTimeout(() => {
        showGethConnectingMsg = false;
      }, 30000);
    } else {
      showGethConnectingMsg = false;
      if (gethConnectingTimeout) {
        clearTimeout(gethConnectingTimeout);
        gethConnectingTimeout = null;
      }
    }
  });

  // Check if Tauri is available
  function isTauri(): boolean {
    return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
  }

  // Classify a multiaddr as IPv4, IPv6, or other
  function addrType(addr: string): 'IPv4' | 'IPv6' | 'other' {
    if (addr.startsWith('/ip4/')) return 'IPv4';
    if (addr.startsWith('/ip6/')) return 'IPv6';
    return 'other';
  }

  // Extract the IP address and port from a multiaddr like /ip4/1.2.3.4/tcp/4001/...
  function extractIpPort(addr: string): string {
    const parts = addr.split('/').filter(Boolean);
    const ipIdx = parts.findIndex(p => p === 'ip4' || p === 'ip6');
    if (ipIdx === -1 || ipIdx + 1 >= parts.length) return addr;
    const ip = parts[ipIdx + 1];
    const tcpIdx = parts.indexOf('tcp', ipIdx);
    const port = tcpIdx !== -1 && tcpIdx + 1 < parts.length ? parts[tcpIdx + 1] : null;
    return port ? `${ip}:${port}` : ip;
  }

  onMount(async () => {
    loadTrafficStats();

    if (isTauri()) {
      await loadGethStatus();
      await loadBootstrapHealth();

      // Set up download progress listener
      unlistenDownload = await listen<DownloadProgress>('geth-download-progress', (event) => {
        downloadProgress = event.payload;
        if (event.payload.percentage >= 100) {
          isDownloading = false;
          loadGethStatus();
        }
      });

      // Refresh status every 10 seconds
      refreshInterval = setInterval(loadGethStatus, 10000);
    }
    isLoadingGeth = false;
  });

  onDestroy(() => {
    if (refreshInterval) {
      clearInterval(refreshInterval);
    }
    if (unlistenDownload) {
      unlistenDownload();
    }
    if (gethConnectingTimeout) {
      clearTimeout(gethConnectingTimeout);
    }
    if (trafficInterval) {
      clearInterval(trafficInterval);
    }
    saveTrafficStats();
  });

  // Load Geth status
  async function loadGethStatus() {
    if (!isTauri()) {
      // In non-Tauri mode, set a default status
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
      return;
    }

    try {
      gethStatus = await invoke<GethStatus>('get_geth_status');
    } catch (err) {
      // If we can't get status, set installed to false
      log.error('Geth status check failed:', err);
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

  // Download Geth
  async function handleDownloadGeth() {
    if (!isTauri()) {
      toasts.show('Geth download requires desktop app', 'error');
      return;
    }

    isDownloading = true;
    downloadProgress = { downloaded: 0, total: 0, percentage: 0, status: 'Starting download...' };

    try {
      await invoke('download_geth');
      toasts.show('Geth downloaded successfully!', 'success');
      await loadGethStatus();
    } catch (err) {
      log.error('Failed to download Geth:', err);
      toasts.show(`Download failed: ${err}`, 'error');
    } finally {
      isDownloading = false;
    }
  }

  // Start Geth
  async function handleStartGeth() {
    if (!isTauri()) return;

    isStartingGeth = true;
    try {
      await invoke('start_geth', { minerAddress: $walletAccount?.address || null });
      toasts.show('Blockchain node started!', 'success');
      await loadGethStatus();
    } catch (err) {
      log.error('Failed to start Geth:', err);
      toasts.show(`Failed to start node: ${err}`, 'error');
    } finally {
      isStartingGeth = false;
    }
  }

  // Stop Geth
  async function handleStopGeth() {
    if (!isTauri()) return;

    try {
      await invoke('stop_geth');
      toasts.show('Blockchain node stopped', 'info');
      await loadGethStatus();
    } catch (err) {
      log.error('Failed to stop Geth:', err);
      toasts.show(`Failed to stop node: ${err}`, 'error');
    }
  }

  // Check bootstrap node health
  async function checkBootstrapHealth() {
    if (!isTauri()) return;

    isCheckingBootstrap = true;
    try {
      bootstrapHealth = await invoke<BootstrapHealthReport>('check_bootstrap_health');
    } catch (err) {
      log.error('Failed to check bootstrap health:', err);
    } finally {
      isCheckingBootstrap = false;
    }
  }

  // Load cached bootstrap health (fast, no network calls)
  async function loadBootstrapHealth() {
    if (!isTauri()) return;

    try {
      const cached = await invoke<BootstrapHealthReport | null>('get_bootstrap_health');
      if (cached) {
        bootstrapHealth = cached;
      }
    } catch (err) {
      log.debug('No cached bootstrap health available');
    }
  }

  // DHT Health Check
  async function checkDhtHealth() {
    isCheckingDhtHealth = true;
    try {
      dhtHealth = await dhtService.getHealth();
    } catch (err) {
      log.error('Failed to check DHT health:', err);
      toasts.show('Failed to check DHT health', 'error');
    } finally {
      isCheckingDhtHealth = false;
    }
  }

  // DHT Functions
  async function connectToNetwork() {
    isConnecting = true;
    error = '';
    try {
      await dhtService.start();
      const peerId = await dhtService.getPeerId();
      if (peerId) {
        localPeerId = peerId;
      }
      toasts.show('Connected to P2P network!', 'success');
    } catch (err) {
      const errMsg = err instanceof Error ? err.message : String(err);
      // If DHT is already running (e.g. stale state after logout), sync the UI
      if (errMsg.includes('already running')) {
        networkConnected.set(true);
        const peerId = await dhtService.getPeerId();
        if (peerId) localPeerId = peerId;
        toasts.show('Reconnected to P2P network', 'success');
      } else {
        error = errMsg;
        log.error('Failed to connect:', err);
        toasts.show(`Connection failed: ${error}`, 'error');
      }
    } finally {
      isConnecting = false;
    }
  }

  async function disconnectFromNetwork() {
    try {
      await dhtService.stop();
      localPeerId = '';
      toasts.show('Disconnected from P2P network', 'info');
    } catch (err) {
      error = err instanceof Error ? err.message : 'Failed to disconnect';
      log.error('Failed to disconnect:', err);
    }
  }

  async function pingPeer(peerId: string) {
    try {
      const result = await dhtService.pingPeer(peerId);
      toasts.show('Ping sent!', 'success');
      log.info('Ping successful:', result);
    } catch (err) {
      toasts.show('Ping failed', 'error');
      log.error('Ping failed:', err);
    }
  }

  function formatDate(date: Date | number): string {
    const d = typeof date === 'number' ? new Date(date) : date;
    return d.toLocaleString();
  }

  function formatBytes(bytes: number): string {
    if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(2)} GB`;
    if (bytes >= 1e6) return `${(bytes / 1e6).toFixed(2)} MB`;
    if (bytes >= 1e3) return `${(bytes / 1e3).toFixed(2)} KB`;
    return `${bytes} B`;
  }
</script>

<div class="p-6">
  <div class="flex items-center justify-between mb-6">
    <div>
      <h1 class="text-3xl font-bold dark:text-white">Network</h1>
      <p class="text-gray-600 dark:text-gray-400 mt-1">Manage blockchain and P2P network connections</p>
    </div>
    <button
      onclick={loadGethStatus}
      disabled={isLoadingGeth}
      class="p-2 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition-colors disabled:opacity-50 dark:text-gray-300"
      title="Refresh status"
    >
      <RefreshCw class="w-5 h-5 {isLoadingGeth ? 'animate-spin' : ''}" />
    </button>
  </div>

  {#if error}
    <div class="bg-red-50 dark:bg-red-900/30 border-l-4 border-red-400 p-4 mb-6 rounded-r-lg">
      <div class="flex items-center gap-2">
        <AlertTriangle class="w-5 h-5 text-red-600 dark:text-red-400" />
        <p class="text-sm text-red-800 dark:text-red-300">{error}</p>
      </div>
    </div>
  {/if}

  <!-- Blockchain Node Section -->
  <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6 mb-6">
    <div class="flex items-center justify-between mb-4">
      <div class="flex items-center gap-3">
        <div class="p-2 {gethStatus?.running ? 'bg-green-100 dark:bg-green-900/30' : 'bg-gray-100 dark:bg-gray-700'} rounded-lg">
          <Server class="w-6 h-6 {gethStatus?.running ? 'text-green-600 dark:text-green-400' : 'text-gray-600 dark:text-gray-400'}" />
        </div>
        <div>
          <h2 class="font-semibold dark:text-white">Blockchain Node (Geth)</h2>
          <p class="text-sm text-gray-500 dark:text-gray-400">Chiral Network blockchain connection</p>
        </div>
      </div>
      <div class="flex items-center gap-2">
        {#if gethStatus?.running}
          <span class="flex items-center gap-2 px-3 py-1 bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400 rounded-full text-sm">
            <span class="w-2 h-2 bg-green-500 rounded-full animate-pulse"></span>
            Running
          </span>
        {:else if gethStatus?.installed}
          <span class="flex items-center gap-2 px-3 py-1 bg-gray-100 dark:bg-gray-700 text-gray-700 dark:text-gray-300 rounded-full text-sm">
            <span class="w-2 h-2 bg-gray-400 rounded-full"></span>
            Stopped
          </span>
        {:else}
          <span class="flex items-center gap-2 px-3 py-1 bg-yellow-100 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-400 rounded-full text-sm">
            <AlertTriangle class="w-4 h-4" />
            Not Installed
          </span>
        {/if}
      </div>
    </div>

    {#if !gethStatus?.installed}
      <!-- Download Geth Section -->
      <div class="bg-yellow-50 dark:bg-yellow-900/30 border border-yellow-200 dark:border-yellow-800 rounded-lg p-4 mb-4">
        <div class="flex items-start gap-3">
          <AlertTriangle class="w-5 h-5 text-yellow-600 dark:text-yellow-400 flex-shrink-0 mt-0.5" />
          <div>
            <p class="font-medium text-yellow-800 dark:text-yellow-300">Geth Not Installed</p>
            <p class="text-sm text-yellow-700 dark:text-yellow-400 mt-1">
              Download Core-Geth to connect to the Chiral Network blockchain.
              This is required for wallet balance, transactions, and mining.
            </p>
          </div>
        </div>
      </div>

      {#if isDownloading && downloadProgress}
        <div class="space-y-2">
          <div class="flex justify-between text-sm dark:text-gray-300">
            <span>{downloadProgress.status}</span>
            <span>{downloadProgress.percentage.toFixed(1)}%</span>
          </div>
          <div class="w-full bg-gray-200 dark:bg-gray-700 rounded-full h-2">
            <div
              class="bg-blue-600 h-2 rounded-full transition-all"
              style="width: {downloadProgress.percentage}%"
            ></div>
          </div>
          {#if downloadProgress.total > 0}
            <p class="text-xs text-gray-500 dark:text-gray-400 text-right">
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
        <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-3">
          <p class="text-xs text-gray-500 dark:text-gray-400">Block Height</p>
          <p class="text-lg font-bold dark:text-white">{gethStatus?.currentBlock?.toLocaleString() || 0}</p>
        </div>
        <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-3">
          <p class="text-xs text-gray-500 dark:text-gray-400">Blockchain Peers</p>
          <p class="text-lg font-bold dark:text-white">{gethStatus?.peerCount || 0}</p>
        </div>
        <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-3">
          <p class="text-xs text-gray-500 dark:text-gray-400">Chain ID</p>
          <p class="text-lg font-bold dark:text-white">{gethStatus?.chainId || 'N/A'}</p>
        </div>
        <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-3">
          <p class="text-xs text-gray-500 dark:text-gray-400">Sync Status</p>
          <p class="text-lg font-bold dark:text-white">{gethStatus?.syncing ? 'Syncing' : gethStatus?.running ? 'Synced' : gethStatus?.chainId ? 'Remote' : 'Offline'}</p>
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
            Stop Node
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
              Start Node
            {/if}
          </button>
        {/if}
      </div>

      <!-- Connecting Info -->
      {#if showGethConnectingMsg}
        <div class="mt-4 p-3 bg-blue-50 dark:bg-blue-900/30 border border-blue-200 dark:border-blue-800 rounded-lg">
          <p class="text-sm text-blue-800 dark:text-blue-300">
            <strong>Connecting to network...</strong> The node is discovering peers via bootstrap nodes.
            This may take a moment. Peer count will update automatically.
          </p>
        </div>
      {/if}

      <!-- Bootstrap Health Check -->
      <div class="mt-4 border-t border-gray-200 dark:border-gray-700 pt-4">
        <div class="flex items-center justify-between mb-3">
          <div class="flex items-center gap-2">
            <Activity class="w-4 h-4 text-gray-500 dark:text-gray-400" />
            <span class="text-sm font-medium text-gray-700 dark:text-gray-300">Bootstrap Health Check</span>
          </div>
          <button
            onclick={checkBootstrapHealth}
            disabled={isCheckingBootstrap}
            class="text-xs px-2 py-1 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 rounded transition-colors flex items-center gap-1 disabled:opacity-50 dark:text-gray-300"
          >
            {#if isCheckingBootstrap}
              <Loader2 class="w-3 h-3 animate-spin" />
            {:else}
              <Activity class="w-3 h-3" />
            {/if}
            Run Check
          </button>
        </div>

        {#if bootstrapHealth}
          <div class="grid grid-cols-2 md:grid-cols-3 gap-3 mb-3">
            <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-2.5">
              <p class="text-xs text-gray-500 dark:text-gray-400">Status</p>
              <p class="text-sm font-bold {bootstrapHealth.isHealthy ? 'text-green-600 dark:text-green-400' : 'text-red-600 dark:text-red-400'}">
                {bootstrapHealth.isHealthy ? 'Healthy' : 'Degraded'}
              </p>
            </div>
            <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-2.5">
              <p class="text-xs text-gray-500 dark:text-gray-400">Healthy Nodes</p>
              <p class="text-sm font-bold dark:text-white">{bootstrapHealth.healthyNodes} / {bootstrapHealth.totalNodes}</p>
            </div>
            <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-2.5">
              <p class="text-xs text-gray-500 dark:text-gray-400">Last Checked</p>
              <p class="text-sm font-bold dark:text-white">{new Date(bootstrapHealth.timestamp).toLocaleTimeString()}</p>
            </div>
          </div>

          <!-- Expandable Node Details -->
          <button
            onclick={() => showBootstrapDetails = !showBootstrapDetails}
            class="w-full flex items-center justify-between text-left py-2"
          >
            <span class="text-xs text-gray-500 dark:text-gray-400">Node Details</span>
            {#if showBootstrapDetails}
              <ChevronUp class="w-4 h-4 text-gray-400" />
            {:else}
              <ChevronDown class="w-4 h-4 text-gray-400" />
            {/if}
          </button>

          {#if showBootstrapDetails}
            <div class="space-y-2">
              {#each bootstrapHealth.nodes as node}
                <div class="flex items-center justify-between p-2.5 bg-gray-50 dark:bg-gray-700 rounded-lg text-xs">
                  <div class="flex items-center gap-2">
                    <div class="w-2 h-2 rounded-full {node.reachable ? 'bg-green-500' : 'bg-red-500'} shrink-0"></div>
                    <div>
                      <span class="font-medium dark:text-white text-sm">{node.name}</span>
                      <span class="text-gray-500 dark:text-gray-400 ml-1">({node.region})</span>
                    </div>
                  </div>
                  <div class="text-right shrink-0">
                    {#if node.reachable && node.latencyMs}
                      <span class="text-green-600 dark:text-green-400">{node.latencyMs}ms</span>
                    {:else if node.error}
                      <span class="text-red-500 dark:text-red-400">{node.error}</span>
                    {:else}
                      <span class="{node.reachable ? 'text-green-600 dark:text-green-400' : 'text-red-600 dark:text-red-400'}">
                        {node.reachable ? 'Reachable' : 'Unreachable'}
                      </span>
                    {/if}
                  </div>
                </div>
              {/each}

              {#if !bootstrapHealth.isHealthy}
                <div class="p-2 bg-red-50 dark:bg-red-900/30 border border-red-200 dark:border-red-800 rounded-lg">
                  <p class="text-xs text-red-700 dark:text-red-300">
                    <strong>Warning:</strong> Not enough bootstrap nodes are reachable.
                    Peer discovery may be limited.
                  </p>
                </div>
              {/if}
            </div>
          {/if}
        {:else}
          <p class="text-xs text-gray-500 dark:text-gray-400 text-center py-2">
            Click "Run Check" to test bootstrap node connectivity
          </p>
        {/if}
      </div>
    {/if}
  </div>

  <!-- P2P Network (DHT) Section -->
  <div class="bg-white dark:bg-gray-800 rounded-xl shadow-sm border border-gray-200 dark:border-gray-700 p-6 mb-6">
    <!-- Header with status and controls -->
    <div class="flex items-center justify-between mb-4">
      <div class="flex items-center gap-3">
        <div class="p-2 {$networkConnected ? 'bg-green-100 dark:bg-green-900/30' : 'bg-gray-100 dark:bg-gray-700'} rounded-lg">
          <Globe class="w-6 h-6 {$networkConnected ? 'text-green-600 dark:text-green-400' : 'text-gray-600 dark:text-gray-400'}" />
        </div>
        <div>
          <h2 class="font-semibold dark:text-white">P2P Network (DHT)</h2>
          <p class="text-sm text-gray-500 dark:text-gray-400">Kademlia DHT file sharing and peer discovery</p>
        </div>
      </div>
      <div class="flex items-center gap-2">
        <span class="flex items-center gap-2 px-3 py-1 {$networkConnected ? 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400' : 'bg-gray-100 dark:bg-gray-700 text-gray-700 dark:text-gray-300'} rounded-full text-sm">
          <span class="w-2 h-2 rounded-full {$networkConnected ? 'bg-green-500 animate-pulse' : 'bg-gray-400'}"></span>
          {$networkConnected ? 'Connected' : 'Disconnected'}
        </span>
      </div>
    </div>

    <!-- Stats Grid -->
    <div class="grid grid-cols-2 md:grid-cols-4 gap-3 mb-4">
      <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-3">
        <p class="text-xs text-gray-500 dark:text-gray-400">DHT Peers</p>
        <p class="text-lg font-bold dark:text-white">{$networkStats.connectedPeers}</p>
      </div>
      <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-3">
        <p class="text-xs text-gray-500 dark:text-gray-400">Discovered Peers</p>
        <p class="text-lg font-bold dark:text-white">{$networkStats.totalPeers}</p>
      </div>
      <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-3">
        <p class="text-xs text-gray-500 dark:text-gray-400">Blockchain Peers</p>
        <p class="text-lg font-bold dark:text-white">{gethStatus?.peerCount || 0}</p>
      </div>
      <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-3">
        <p class="text-xs text-gray-500 dark:text-gray-400">Block Height</p>
        <p class="text-lg font-bold dark:text-white">{gethStatus?.currentBlock?.toLocaleString() || 0}</p>
      </div>
    </div>

    <!-- Peer ID -->
    {#if localPeerId}
      <div class="mb-4 p-3 bg-gray-50 dark:bg-gray-700 rounded-lg">
        <div class="text-xs text-gray-500 dark:text-gray-400 mb-1">Your Peer ID</div>
        <div class="font-mono text-xs break-all dark:text-gray-300">{localPeerId}</div>
      </div>
    {/if}

    <!-- Connect/Disconnect -->
    <div class="mb-4">
      {#if $networkConnected}
        <button
          onclick={disconnectFromNetwork}
          class="w-full flex items-center justify-center gap-2 px-4 py-2 bg-red-600 text-white rounded-lg hover:bg-red-700 transition"
        >
          <Square class="w-4 h-4" />
          <span>Disconnect</span>
        </button>
      {:else}
        <button
          onclick={connectToNetwork}
          disabled={isConnecting}
          class="w-full flex items-center justify-center gap-2 px-4 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition disabled:opacity-50"
        >
          {#if isConnecting}
            <Loader2 class="w-4 h-4 animate-spin" />
            <span>Connecting...</span>
          {:else}
            <Play class="w-4 h-4" />
            <span>Connect</span>
          {/if}
        </button>
      {/if}
    </div>

    <!-- Health Check -->
    <div class="border-t border-gray-200 dark:border-gray-700 pt-4">
      <div class="flex items-center justify-between mb-3">
        <div class="flex items-center gap-2">
          <HeartPulse class="w-4 h-4 text-gray-500 dark:text-gray-400" />
          <span class="text-sm font-medium text-gray-700 dark:text-gray-300">Health Check</span>
        </div>
        <button
          onclick={checkDhtHealth}
          disabled={isCheckingDhtHealth}
          class="text-xs px-2 py-1 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 rounded transition-colors flex items-center gap-1 disabled:opacity-50 dark:text-gray-300"
        >
          {#if isCheckingDhtHealth}
            <Loader2 class="w-3 h-3 animate-spin" />
          {:else}
            <HeartPulse class="w-3 h-3" />
          {/if}
          Run Check
        </button>
      </div>

      {#if dhtHealth}
        <div class="grid grid-cols-2 md:grid-cols-4 gap-3 mb-3">
          <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-2.5">
            <p class="text-xs text-gray-500 dark:text-gray-400">Status</p>
            <p class="text-sm font-bold {dhtHealth.running ? 'text-green-600 dark:text-green-400' : 'text-red-600 dark:text-red-400'}">
              {dhtHealth.running ? 'Running' : 'Stopped'}
            </p>
          </div>
          <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-2.5">
            <p class="text-xs text-gray-500 dark:text-gray-400">Connected Peers</p>
            <p class="text-sm font-bold dark:text-white">{dhtHealth.connectedPeerCount}</p>
          </div>
          <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-2.5">
            <p class="text-xs text-gray-500 dark:text-gray-400">Kademlia Peers</p>
            <p class="text-sm font-bold dark:text-white">{dhtHealth.kademliaPeers}</p>
          </div>
          <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-2.5">
            <p class="text-xs text-gray-500 dark:text-gray-400">Shared Files</p>
            <p class="text-sm font-bold dark:text-white">{dhtHealth.sharedFiles}</p>
          </div>
        </div>

        <!-- Expandable Details -->
        <button
          onclick={() => showDhtHealthDetails = !showDhtHealthDetails}
          class="w-full flex items-center justify-between text-left py-2"
        >
          <span class="text-xs text-gray-500 dark:text-gray-400">Advanced Details</span>
          {#if showDhtHealthDetails}
            <ChevronUp class="w-4 h-4 text-gray-400" />
          {:else}
            <ChevronDown class="w-4 h-4 text-gray-400" />
          {/if}
        </button>

        {#if showDhtHealthDetails}
          <div class="space-y-2">
            {#if dhtHealth.peerId}
              <div class="p-2.5 bg-gray-50 dark:bg-gray-700 rounded-lg">
                <p class="text-xs text-gray-500 dark:text-gray-400 mb-1">Peer ID</p>
                <p class="font-mono text-xs break-all dark:text-gray-300">{dhtHealth.peerId}</p>
              </div>
            {/if}

            {#if dhtHealth.listeningAddresses.length > 0}
              <div class="p-2.5 bg-gray-50 dark:bg-gray-700 rounded-lg">
                <p class="text-xs text-gray-500 dark:text-gray-400 mb-2">Listening Addresses ({dhtHealth.listeningAddresses.length})</p>
                <div class="space-y-1.5">
                  {#each dhtHealth.listeningAddresses as addr}
                    <div class="flex items-start gap-2 text-xs">
                      <span class="shrink-0 px-1.5 py-0.5 rounded text-[10px] font-semibold {addrType(addr) === 'IPv6' ? 'bg-purple-100 dark:bg-purple-900/40 text-purple-700 dark:text-purple-300' : addrType(addr) === 'IPv4' ? 'bg-blue-100 dark:bg-blue-900/40 text-blue-700 dark:text-blue-300' : 'bg-gray-100 dark:bg-gray-600 text-gray-600 dark:text-gray-300'}">
                        {addrType(addr)}
                      </span>
                      <span class="font-mono break-all dark:text-gray-300">{extractIpPort(addr)}</span>
                    </div>
                  {/each}
                </div>
              </div>
            {/if}

            {#if dhtHealth.bootstrapNodes.length > 0}
              <div class="p-2.5 bg-gray-50 dark:bg-gray-700 rounded-lg">
                <p class="text-xs text-gray-500 dark:text-gray-400 mb-2">DHT Bootstrap Nodes</p>
                <div class="space-y-1.5">
                  {#each dhtHealth.bootstrapNodes as node}
                    <div class="flex items-start gap-2 text-xs">
                      <div class="w-2 h-2 rounded-full mt-1 {node.reachable ? 'bg-green-500' : 'bg-red-500'} shrink-0"></div>
                      <span class="shrink-0 px-1.5 py-0.5 rounded text-[10px] font-semibold {addrType(node.address) === 'IPv6' ? 'bg-purple-100 dark:bg-purple-900/40 text-purple-700 dark:text-purple-300' : addrType(node.address) === 'IPv4' ? 'bg-blue-100 dark:bg-blue-900/40 text-blue-700 dark:text-blue-300' : 'bg-gray-100 dark:bg-gray-600 text-gray-600 dark:text-gray-300'}">
                        {addrType(node.address)}
                      </span>
                      <span class="font-mono break-all dark:text-gray-300">{extractIpPort(node.address)}</span>
                      <span class="{node.reachable ? 'text-green-600 dark:text-green-400' : 'text-red-600 dark:text-red-400'} shrink-0">
                        {node.reachable ? 'Reachable' : 'Unreachable'}
                      </span>
                    </div>
                  {/each}
                </div>
              </div>
            {/if}

            {#if dhtHealth.protocols.length > 0}
              <div class="p-2.5 bg-gray-50 dark:bg-gray-700 rounded-lg">
                <p class="text-xs text-gray-500 dark:text-gray-400 mb-1">Active Protocols ({dhtHealth.protocols.length})</p>
                <div class="flex flex-wrap gap-1.5">
                  {#each dhtHealth.protocols as protocol}
                    <span class="px-2 py-0.5 bg-blue-100 dark:bg-blue-900/30 text-blue-700 dark:text-blue-400 text-xs rounded-full font-mono">
                      {protocol}
                    </span>
                  {/each}
                </div>
              </div>
            {/if}
          </div>
        {/if}
      {:else}
        <p class="text-xs text-gray-500 dark:text-gray-400 text-center py-2">
          Click "Run Check" to view DHT health diagnostics
        </p>
      {/if}
    </div>

    <!-- Traffic Statistics -->
    <div class="border-t border-gray-200 dark:border-gray-700 pt-4 mt-4">
      <div class="flex items-center gap-2 mb-3">
        <Activity class="w-4 h-4 text-gray-500 dark:text-gray-400" />
        <span class="text-sm font-medium text-gray-700 dark:text-gray-300">Traffic Statistics</span>
      </div>
      <div class="grid grid-cols-2 md:grid-cols-4 gap-3">
        <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-3">
          <div class="flex items-center gap-2 mb-1">
            <ArrowDownToLine class="w-3.5 h-3.5 text-green-500" />
            <p class="text-xs text-gray-500 dark:text-gray-400">Download Speed</p>
          </div>
          <p class="text-lg font-bold dark:text-white">{formatSpeed(trafficStats.downloadSpeed)}</p>
        </div>
        <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-3">
          <div class="flex items-center gap-2 mb-1">
            <ArrowUpFromLine class="w-3.5 h-3.5 text-blue-500" />
            <p class="text-xs text-gray-500 dark:text-gray-400">Upload Speed</p>
          </div>
          <p class="text-lg font-bold dark:text-white">{formatSpeed(trafficStats.uploadSpeed)}</p>
        </div>
        <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-3">
          <div class="flex items-center gap-2 mb-1">
            <Download class="w-3.5 h-3.5 text-green-500" />
            <p class="text-xs text-gray-500 dark:text-gray-400">Total Downloaded</p>
          </div>
          <p class="text-lg font-bold dark:text-white">{formatBytes(trafficStats.totalDownloaded)}</p>
        </div>
        <div class="bg-gray-50 dark:bg-gray-700 rounded-lg p-3">
          <div class="flex items-center gap-2 mb-1">
            <Upload class="w-3.5 h-3.5 text-blue-500" />
            <p class="text-xs text-gray-500 dark:text-gray-400">Total Uploaded</p>
          </div>
          <p class="text-lg font-bold dark:text-white">{formatBytes(trafficStats.totalUploaded)}</p>
        </div>
      </div>
    </div>

    <!-- Connected Peers -->
    <div class="border-t border-gray-200 dark:border-gray-700 pt-4 mt-4">
      <div class="flex items-center gap-2 mb-3">
        <Radio class="w-4 h-4 text-gray-500 dark:text-gray-400" />
        <span class="text-sm font-medium text-gray-700 dark:text-gray-300">Connected Peers</span>
        <span class="px-2 py-0.5 text-xs rounded-full bg-gray-100 dark:bg-gray-700 text-gray-600 dark:text-gray-400">
          {$peers.length}
        </span>
      </div>

      {#if $peers.length === 0}
        <div class="text-center py-4 text-gray-500 dark:text-gray-400">
          <p class="text-sm">No peers connected</p>
          <p class="text-xs">Connect to the P2P network to discover peers</p>
        </div>
      {:else}
        <div class="space-y-2">
          {#each $peers as peer}
            <div class="p-3 bg-gray-50 dark:bg-gray-700 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-600 transition-colors">
              <div class="flex items-start justify-between gap-3">
                <div class="flex-1 min-w-0">
                  <div class="font-mono text-sm break-all dark:text-gray-200">{peer.id}</div>
                  {#if peer.address}
                    <div class="text-xs text-gray-500 dark:text-gray-400 mt-1">Address: {peer.address}</div>
                  {/if}
                  <div class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                    Last seen: {formatDate(peer.lastSeen)}
                  </div>
                </div>
                <button
                  onclick={() => pingPeer(peer.id)}
                  class="flex items-center gap-1 px-3 py-1.5 bg-blue-600 text-white text-sm rounded hover:bg-blue-700 transition shrink-0"
                  title="Ping this peer"
                >
                  <Radio class="w-3 h-3" />
                  <span>Ping</span>
                </button>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  </div>
</div>
