<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { peers, networkStats, networkConnected, walletAccount } from '$lib/stores';
  import { dhtService } from '$lib/dhtService';
  import { toasts } from '$lib/toastStore';
  import {
    Play,
    Square,
    Radio,
    Server,
    Download,
    RefreshCw,
    AlertTriangle,
    Check,
    Loader2,
    Globe,
    Zap,
    Activity,
    ChevronDown,
    ChevronUp
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

  // Check if Tauri is available
  function isTauri(): boolean {
    return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
  }

  onMount(async () => {
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
  });

  // Load Geth status
  async function loadGethStatus() {
    if (!isTauri()) {
      // In non-Tauri mode, set a default status
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
      gethStatus = await invoke<GethStatus>('get_geth_status');
    } catch (err) {
      // If we can't get status, set installed to false
      console.error('Geth status check failed:', err);
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
      console.error('Failed to download Geth:', err);
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
      console.error('Failed to start Geth:', err);
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
      console.error('Failed to stop Geth:', err);
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
      console.error('Failed to check bootstrap health:', err);
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
      console.debug('No cached bootstrap health available');
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
      error = err instanceof Error ? err.message : 'Failed to connect';
      console.error('Failed to connect:', err);
      toasts.show(`Connection failed: ${error}`, 'error');
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
      console.error('Failed to disconnect:', err);
    }
  }

  async function pingPeer(peerId: string) {
    try {
      const result = await dhtService.pingPeer(peerId);
      toasts.show('Ping sent!', 'success');
      console.log('Ping successful:', result);
    } catch (err) {
      toasts.show('Ping failed', 'error');
      console.error('Ping failed:', err);
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

<div class="p-6 max-w-6xl mx-auto">
  <div class="flex items-center justify-between mb-6">
    <div>
      <h1 class="text-3xl font-bold">Network</h1>
      <p class="text-gray-600 mt-1">Manage blockchain and P2P network connections</p>
    </div>
    <button
      onclick={loadGethStatus}
      disabled={isLoadingGeth}
      class="p-2 hover:bg-gray-100 rounded-lg transition-colors disabled:opacity-50"
      title="Refresh status"
    >
      <RefreshCw class="w-5 h-5 {isLoadingGeth ? 'animate-spin' : ''}" />
    </button>
  </div>

  {#if error}
    <div class="bg-red-50 border-l-4 border-red-400 p-4 mb-6 rounded-r-lg">
      <div class="flex items-center gap-2">
        <AlertTriangle class="w-5 h-5 text-red-600" />
        <p class="text-sm text-red-800">{error}</p>
      </div>
    </div>
  {/if}

  <!-- Blockchain Node Section -->
  <div class="bg-white rounded-xl shadow-sm border border-gray-200 p-6 mb-6">
    <div class="flex items-center justify-between mb-4">
      <div class="flex items-center gap-3">
        <div class="p-2 {gethStatus?.running ? 'bg-green-100' : 'bg-gray-100'} rounded-lg">
          <Server class="w-6 h-6 {gethStatus?.running ? 'text-green-600' : 'text-gray-600'}" />
        </div>
        <div>
          <h2 class="font-semibold">Blockchain Node (Geth)</h2>
          <p class="text-sm text-gray-500">Chiral Network blockchain connection</p>
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
              Download Core-Geth to connect to the Chiral Network blockchain.
              This is required for wallet balance, transactions, and mining.
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
          <p class="text-xs text-gray-500">Blockchain Peers</p>
          <p class="text-lg font-bold">{gethStatus?.peerCount || 0}</p>
        </div>
        <div class="bg-gray-50 rounded-lg p-3">
          <p class="text-xs text-gray-500">Chain ID</p>
          <p class="text-lg font-bold">{gethStatus?.chainId || 'N/A'}</p>
        </div>
        <div class="bg-gray-50 rounded-lg p-3">
          <p class="text-xs text-gray-500">Sync Status</p>
          <p class="text-lg font-bold">{gethStatus?.syncing ? 'Syncing' : gethStatus?.running ? 'Synced' : 'Offline'}</p>
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

      <!-- Bootstrap Info -->
      {#if gethStatus?.running && gethStatus?.peerCount === 0}
        <div class="mt-4 p-3 bg-blue-50 border border-blue-200 rounded-lg">
          <p class="text-sm text-blue-800">
            <strong>Connecting to network...</strong> The node is discovering peers via bootstrap nodes.
            This may take a moment. Peer count will update automatically.
          </p>
        </div>
      {/if}

      <!-- Bootstrap Node Health -->
      <div class="mt-4 border-t border-gray-200 pt-4">
        <button
          onclick={() => showBootstrapDetails = !showBootstrapDetails}
          class="w-full flex items-center justify-between text-left"
        >
          <div class="flex items-center gap-2">
            <Activity class="w-4 h-4 text-gray-500" />
            <span class="text-sm font-medium text-gray-700">Bootstrap Nodes</span>
            {#if bootstrapHealth}
              <span class="px-2 py-0.5 text-xs rounded-full {bootstrapHealth.isHealthy ? 'bg-green-100 text-green-700' : 'bg-red-100 text-red-700'}">
                {bootstrapHealth.healthyNodes}/{bootstrapHealth.totalNodes} healthy
              </span>
            {/if}
          </div>
          {#if showBootstrapDetails}
            <ChevronUp class="w-4 h-4 text-gray-400" />
          {:else}
            <ChevronDown class="w-4 h-4 text-gray-400" />
          {/if}
        </button>

        {#if showBootstrapDetails}
          <div class="mt-3 space-y-2">
            <div class="flex justify-end mb-2">
              <button
                onclick={checkBootstrapHealth}
                disabled={isCheckingBootstrap}
                class="text-xs px-2 py-1 bg-gray-100 hover:bg-gray-200 rounded transition-colors flex items-center gap-1 disabled:opacity-50"
              >
                {#if isCheckingBootstrap}
                  <Loader2 class="w-3 h-3 animate-spin" />
                {:else}
                  <RefreshCw class="w-3 h-3" />
                {/if}
                Check Health
              </button>
            </div>

            {#if bootstrapHealth}
              {#each bootstrapHealth.nodes as node}
                <div class="flex items-center justify-between p-2 bg-gray-50 rounded-lg text-sm">
                  <div class="flex items-center gap-2">
                    <div class="w-2 h-2 rounded-full {node.reachable ? 'bg-green-500' : 'bg-red-500'}"></div>
                    <div>
                      <span class="font-medium">{node.name}</span>
                      <span class="text-gray-500 text-xs ml-1">({node.region})</span>
                    </div>
                  </div>
                  <div class="text-right">
                    {#if node.reachable && node.latencyMs}
                      <span class="text-green-600">{node.latencyMs}ms</span>
                    {:else if node.error}
                      <span class="text-red-500 text-xs">{node.error}</span>
                    {:else}
                      <span class="text-gray-400">—</span>
                    {/if}
                  </div>
                </div>
              {/each}

              {#if !bootstrapHealth.isHealthy}
                <div class="p-2 bg-red-50 border border-red-200 rounded-lg">
                  <p class="text-xs text-red-700">
                    <strong>Warning:</strong> Not enough bootstrap nodes are reachable.
                    Peer discovery may be limited.
                  </p>
                </div>
              {/if}
            {:else}
              <p class="text-xs text-gray-500 text-center py-2">
                Click "Check Health" to test bootstrap node connectivity
              </p>
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  </div>

  <!-- P2P Network Section -->
  <div class="grid grid-cols-1 md:grid-cols-2 gap-6 mb-6">
    <div class="bg-white rounded-xl shadow-sm border border-gray-200 p-6">
      <div class="flex items-center gap-3 mb-4">
        <div class="p-2 {$networkConnected ? 'bg-green-100' : 'bg-gray-100'} rounded-lg">
          <Globe class="w-6 h-6 {$networkConnected ? 'text-green-600' : 'text-gray-600'}" />
        </div>
        <div>
          <h2 class="font-semibold">P2P Network (DHT)</h2>
          <p class="text-sm text-gray-500">File sharing peer discovery</p>
        </div>
      </div>

      <div class="flex items-center gap-3 mb-4">
        <div class="w-3 h-3 rounded-full {$networkConnected ? 'bg-green-500 animate-pulse' : 'bg-gray-400'}"></div>
        <span class="font-medium">{$networkConnected ? 'Connected' : 'Disconnected'}</span>
      </div>

      {#if localPeerId}
        <div class="mb-4 p-3 bg-gray-50 rounded-lg border border-gray-200">
          <div class="text-xs text-gray-500 mb-1">Your Peer ID:</div>
          <div class="font-mono text-xs break-all">{localPeerId}</div>
        </div>
      {/if}

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

    <div class="bg-white rounded-xl shadow-sm border border-gray-200 p-6">
      <div class="flex items-center gap-3 mb-4">
        <div class="p-2 bg-purple-100 rounded-lg">
          <Zap class="w-6 h-6 text-purple-600" />
        </div>
        <div>
          <h2 class="font-semibold">Network Statistics</h2>
          <p class="text-sm text-gray-500">Current network status</p>
        </div>
      </div>

      <div class="space-y-3">
        <div class="flex justify-between items-center py-2 border-b border-gray-100">
          <span class="text-sm text-gray-600">DHT Peers</span>
          <span class="font-medium">{$networkStats.connectedPeers}</span>
        </div>
        <div class="flex justify-between items-center py-2 border-b border-gray-100">
          <span class="text-sm text-gray-600">Discovered Peers</span>
          <span class="font-medium">{$networkStats.totalPeers}</span>
        </div>
        <div class="flex justify-between items-center py-2 border-b border-gray-100">
          <span class="text-sm text-gray-600">Blockchain Peers</span>
          <span class="font-medium">{gethStatus?.peerCount || 0}</span>
        </div>
        <div class="flex justify-between items-center py-2">
          <span class="text-sm text-gray-600">Block Height</span>
          <span class="font-medium">{gethStatus?.currentBlock?.toLocaleString() || 0}</span>
        </div>
      </div>
    </div>
  </div>

  <!-- Connected Peers -->
  <div class="bg-white rounded-xl shadow-sm border border-gray-200 p-6">
    <div class="flex items-center gap-3 mb-4">
      <div class="p-2 bg-blue-100 rounded-lg">
        <Radio class="w-6 h-6 text-blue-600" />
      </div>
      <div>
        <h2 class="font-semibold">Connected DHT Peers</h2>
        <p class="text-sm text-gray-500">Peers discovered via mDNS</p>
      </div>
    </div>

    {#if $peers.length === 0}
      <div class="text-center py-8 text-gray-500">
        <Globe class="w-12 h-12 mx-auto mb-2 opacity-50" />
        <p>No peers connected</p>
        <p class="text-sm">Connect to the P2P network to discover peers</p>
      </div>
    {:else}
      <div class="space-y-2">
        {#each $peers as peer}
          <div class="p-3 bg-gray-50 rounded-lg border border-gray-200 hover:bg-gray-100 transition-colors">
            <div class="flex items-start justify-between gap-3">
              <div class="flex-1 min-w-0">
                <div class="font-mono text-sm break-all">{peer.id}</div>
                {#if peer.address}
                  <div class="text-xs text-gray-500 mt-1">Address: {peer.address}</div>
                {/if}
                <div class="text-xs text-gray-500 mt-1">
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
