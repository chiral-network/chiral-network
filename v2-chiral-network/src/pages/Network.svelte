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
    Cpu,
    Download,
    RefreshCw,
    AlertTriangle,
    Check,
    Loader2,
    Globe,
    Zap,
    Copy,
    Link,
    Plus
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
  let localEnode = $state('');
  let peerEnodeInput = $state('');
  let isAddingPeer = $state(false);
  let enodeCopied = $state(false);

  let refreshInterval: ReturnType<typeof setInterval> | null = null;
  let unlistenDownload: (() => void) | null = null;

  // Check if Tauri is available
  function isTauri(): boolean {
    return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
  }

  onMount(async () => {
    if (isTauri()) {
      await loadGethStatus();

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
      localEnode = '';
      await loadGethStatus();
    } catch (err) {
      console.error('Failed to stop Geth:', err);
      toasts.show(`Failed to stop node: ${err}`, 'error');
    }
  }

  // Get local enode URL
  async function loadEnode() {
    if (!isTauri() || !gethStatus?.running) return;

    try {
      localEnode = await invoke<string>('get_enode');
    } catch (err) {
      console.debug('Failed to get enode:', err);
    }
  }

  // Copy enode to clipboard
  async function copyEnode() {
    if (!localEnode) return;
    try {
      await navigator.clipboard.writeText(localEnode);
      enodeCopied = true;
      toasts.show('Enode URL copied to clipboard', 'success');
      setTimeout(() => enodeCopied = false, 2000);
    } catch (err) {
      toasts.show('Failed to copy', 'error');
    }
  }

  // Add peer by enode URL
  async function handleAddPeer() {
    if (!isTauri() || !peerEnodeInput.trim()) return;

    isAddingPeer = true;
    try {
      const success = await invoke<boolean>('add_peer', { enode: peerEnodeInput.trim() });
      if (success) {
        toasts.show('Peer added successfully!', 'success');
        peerEnodeInput = '';
        await loadGethStatus();
      } else {
        toasts.show('Failed to add peer - invalid enode or peer unreachable', 'error');
      }
    } catch (err) {
      console.error('Failed to add peer:', err);
      toasts.show(`Failed to add peer: ${err}`, 'error');
    } finally {
      isAddingPeer = false;
    }
  }

  // Load enode when Geth is running
  $effect(() => {
    if (gethStatus?.running) {
      loadEnode();
    }
  });

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
      <div class="flex gap-3 mb-4">
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

      <!-- Peer Connection Section (only when running) -->
      {#if gethStatus?.running}
        <div class="border-t border-gray-200 pt-4 mt-4">
          <h3 class="text-sm font-semibold text-gray-700 mb-3 flex items-center gap-2">
            <Link class="w-4 h-4" />
            Peer Connection
          </h3>

          <!-- Your Enode URL -->
          {#if localEnode}
            <div class="mb-4">
              <label class="block text-xs text-gray-500 mb-1">Your Enode URL (share with other devices)</label>
              <div class="flex gap-2">
                <input
                  type="text"
                  readonly
                  value={localEnode}
                  class="flex-1 px-3 py-2 bg-gray-50 border border-gray-200 rounded-lg font-mono text-xs"
                />
                <button
                  onclick={copyEnode}
                  class="px-3 py-2 bg-gray-100 hover:bg-gray-200 rounded-lg transition-colors"
                  title="Copy enode URL"
                >
                  {#if enodeCopied}
                    <Check class="w-4 h-4 text-green-600" />
                  {:else}
                    <Copy class="w-4 h-4 text-gray-600" />
                  {/if}
                </button>
              </div>
            </div>
          {/if}

          <!-- Add Peer -->
          <div>
            <label class="block text-xs text-gray-500 mb-1">Add Peer (paste enode URL from another device)</label>
            <div class="flex gap-2">
              <input
                type="text"
                bind:value={peerEnodeInput}
                placeholder="enode://..."
                class="flex-1 px-3 py-2 border border-gray-200 rounded-lg font-mono text-xs focus:outline-none focus:ring-2 focus:ring-blue-500"
              />
              <button
                onclick={handleAddPeer}
                disabled={isAddingPeer || !peerEnodeInput.trim()}
                class="px-3 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700 transition-colors disabled:opacity-50 flex items-center gap-1"
              >
                {#if isAddingPeer}
                  <Loader2 class="w-4 h-4 animate-spin" />
                {:else}
                  <Plus class="w-4 h-4" />
                {/if}
              </button>
            </div>
            <p class="text-xs text-gray-400 mt-1">
              To sync with another device: copy your enode URL and paste it on the other device
            </p>
          </div>
        </div>
      {/if}
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
