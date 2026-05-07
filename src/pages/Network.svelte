<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { listen } from '@tauri-apps/api/event';
  import { peers, networkStats, networkConnected, walletAccount, blacklist } from '$lib/stores';
  import { formatBytes } from '$lib/utils';
  import { dhtService, type DhtHealthInfo } from '$lib/dhtService';
  import { toasts } from '$lib/toastStore';
  import type { HostAdvertisement } from '$lib/types/hosting';
  import {
    Play,
    Square,
    Radio,
    Server,
    Download,
    RefreshCw,
    AlertTriangle,
    Loader2,
    Globe,
    Activity,
    HeartPulse,
    ShieldBan,
    Trash2,
    Plus,
    Users,
    Cloud,
  } from 'lucide-svelte';
  import { logger } from '$lib/logger';
  const log = logger('Network');

  // ---------- Types ----------

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

  interface HostRegistryEntry {
    peerId: string;
    walletAddress: string;
    updatedAt: number;
  }

  interface AdvertisedHostRow {
    peerId: string;
    walletAddress: string;
    updatedAt: number | null;
    publishedAt: number | null;
    lastHeartbeatAt: number | null;
  }

  type Health = 'good' | 'warn' | 'bad' | 'idle';
  type Tab = 'overview' | 'peers' | 'hosts' | 'blacklist';

  // ---------- State ----------

  let activeTab = $state<Tab>('overview');

  // DHT
  let isConnecting = $state(false);
  let error = $state('');
  let localPeerId = $state('');

  // Geth
  let gethStatus = $state<GethStatus | null>(null);
  let isLoadingGeth = $state(true);
  let isStartingGeth = $state(false);
  let isDownloading = $state(false);
  let downloadProgress = $state<DownloadProgress | null>(null);
  let refreshInterval: ReturnType<typeof setInterval> | null = null;
  let unlistenDownload: (() => void) | null = null;

  // Bootstrap
  let bootstrapHealth = $state<BootstrapHealthReport | null>(null);
  let isCheckingBootstrap = $state(false);

  // DHT health
  let dhtHealth = $state<DhtHealthInfo | null>(null);
  let isCheckingDhtHealth = $state(false);

  // "Connecting to network" message auto-dismiss
  let showGethConnectingMsg = $state(false);
  let gethConnectingTimeout: ReturnType<typeof setTimeout> | null = null;

  // Peer list filtering / pagination
  let bootstrapPeerIds = $state<Set<string>>(new Set());
  let showBootstrapPeers = $state(true);
  let visiblePeers = $derived(
    showBootstrapPeers ? $peers : $peers.filter((peer) => !bootstrapPeerIds.has(peer.id))
  );
  const PEERS_PER_PAGE = 10;
  let peerPage = $state(0);
  let peerTotalPages = $derived(Math.max(1, Math.ceil(visiblePeers.length / PEERS_PER_PAGE)));
  let paginatedPeers = $derived(
    visiblePeers.slice(peerPage * PEERS_PER_PAGE, (peerPage + 1) * PEERS_PER_PAGE)
  );

  // Advertised hosts
  let advertisedHosts = $state<AdvertisedHostRow[]>([]);
  let isLoadingAdvertisedHosts = $state(false);
  let advertisedHostsError = $state('');
  let connectedPeerIds = $derived(new Set($peers.map((peer) => peer.id)));
  let advertisedHostsWithStatus = $derived(
    advertisedHosts.map((host) => ({ ...host, isOnline: connectedPeerIds.has(host.peerId) }))
  );

  // Blacklist form
  let blacklistAddress = $state('');
  let blacklistReason = $state('');

  // Reset peer page when peer list shrinks
  $effect(() => {
    if (peerPage >= peerTotalPages) peerPage = Math.max(0, peerTotalPages - 1);
  });

  // Show "connecting" message when Geth is running with 0 peers, auto-dismiss after 30s
  $effect(() => {
    if (gethStatus?.running && gethStatus?.peerCount === 0) {
      showGethConnectingMsg = true;
      if (gethConnectingTimeout) clearTimeout(gethConnectingTimeout);
      gethConnectingTimeout = setTimeout(() => (showGethConnectingMsg = false), 30000);
    } else {
      showGethConnectingMsg = false;
      if (gethConnectingTimeout) {
        clearTimeout(gethConnectingTimeout);
        gethConnectingTimeout = null;
      }
    }
  });

  // ---------- Health derivations (drive the status strip) ----------

  let gethHealthState: Health = $derived(
    !gethStatus
      ? 'idle'
      : !gethStatus.installed
        ? 'bad'
        : gethStatus.running && !gethStatus.syncing
          ? 'good'
          : gethStatus.running
            ? 'warn'
            : 'bad'
  );
  let gethHeadline: string = $derived(
    !gethStatus
      ? '—'
      : !gethStatus.installed
        ? 'Not installed'
        : !gethStatus.running
          ? 'Stopped'
          : gethStatus.syncing && gethStatus.highestBlock > 0
            ? `Syncing ${((gethStatus.currentBlock / gethStatus.highestBlock) * 100).toFixed(1)}%`
            : `Block ${gethStatus.currentBlock.toLocaleString()}`
  );

  let dhtHealthState: Health = $derived(
    !$networkConnected
      ? 'bad'
      : $networkStats.connectedPeers > 0
        ? 'good'
        : 'warn'
  );
  let dhtHeadline: string = $derived(
    !$networkConnected
      ? 'Disconnected'
      : `${$networkStats.connectedPeers} peer${$networkStats.connectedPeers === 1 ? '' : 's'}`
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

  let relayListeningCount = $derived(
    dhtHealth ? dhtHealth.listeningAddresses.filter((addr) => isRelayCircuitAddress(addr)).length : 0
  );

  // ---------- Lifecycle ----------

  function isTauri(): boolean {
    return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
  }

  onMount(async () => {
    if (isTauri()) {
      try {
        const ids: string[] = await invoke('get_bootstrap_peer_ids');
        bootstrapPeerIds = new Set(ids);
      } catch {
        /* bootstrap IDs unavailable — show all peers */
      }

      await loadGethStatus();
      await loadBootstrapHealth();
      await loadAdvertisedHosts();

      unlistenDownload = await listen<DownloadProgress>('geth-download-progress', (event) => {
        downloadProgress = event.payload;
        if (event.payload.percentage >= 100) {
          isDownloading = false;
          loadGethStatus();
        }
      });

      refreshInterval = setInterval(loadGethStatus, 10000);
    }
    isLoadingGeth = false;
  });

  onDestroy(() => {
    if (refreshInterval) clearInterval(refreshInterval);
    if (unlistenDownload) unlistenDownload();
    if (gethConnectingTimeout) clearTimeout(gethConnectingTimeout);
  });

  // ---------- Helpers ----------

  function addrType(addr: string): 'IPv4' | 'IPv6' | 'other' {
    if (addr.startsWith('/ip4/')) return 'IPv4';
    if (addr.startsWith('/ip6/')) return 'IPv6';
    return 'other';
  }

  function isRelayCircuitAddress(addr: string): boolean {
    return addr.includes('/p2p-circuit');
  }

  function extractIpPort(addr: string): string {
    const parts = addr.split('/').filter(Boolean);
    const ipIdx = parts.findIndex((p) => p === 'ip4' || p === 'ip6');
    if (ipIdx === -1 || ipIdx + 1 >= parts.length) return addr;
    const ip = parts[ipIdx + 1];
    const tcpIdx = parts.indexOf('tcp', ipIdx);
    const port = tcpIdx !== -1 && tcpIdx + 1 < parts.length ? parts[tcpIdx + 1] : null;
    return port ? `${ip}:${port}` : ip;
  }

  function formatUnixSeconds(timestamp: number): string {
    return new Date(timestamp * 1000).toLocaleTimeString();
  }

  function parseUnixSeconds(value: unknown): number | null {
    if (typeof value === 'number' && Number.isFinite(value)) return Math.floor(value);
    if (typeof value === 'string' && value.trim().length > 0) {
      const parsed = Number(value);
      if (Number.isFinite(parsed)) return Math.floor(parsed);
    }
    return null;
  }

  function formatUnixDateTime(timestamp: number | null): string {
    return timestamp ? new Date(timestamp * 1000).toLocaleString() : 'Unknown';
  }

  function truncateAddress(addr: string): string {
    if (addr.length <= 16) return addr;
    return `${addr.slice(0, 8)}...${addr.slice(-6)}`;
  }

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

  // ---------- Loaders ----------

  async function loadGethStatus(notify = false) {
    if (!isTauri()) {
      gethStatus = {
        installed: false,
        running: false,
        localRunning: false,
        syncing: false,
        currentBlock: 0,
        highestBlock: 0,
        peerCount: 0,
        chainId: 0,
      };
      return;
    }
    try {
      gethStatus = await invoke<GethStatus>('get_geth_status');
      if (notify) toasts.show('Node status refreshed', 'success');
    } catch (err) {
      log.error('Geth status check failed:', err);
      if (notify) toasts.detail('Failed to refresh node status', String(err), 'error');
      gethStatus = {
        installed: false,
        running: false,
        localRunning: false,
        syncing: false,
        currentBlock: 0,
        highestBlock: 0,
        peerCount: 0,
        chainId: 0,
      };
    }
  }

  async function handleDownloadGeth() {
    if (!isTauri()) {
      toasts.show('Geth download requires the desktop app', 'warning');
      return;
    }
    isDownloading = true;
    downloadProgress = { downloaded: 0, total: 0, percentage: 0, status: 'Starting download...' };
    try {
      await invoke('download_geth');
      toasts.show('Geth installed', 'success');
      await loadGethStatus();
    } catch (err) {
      log.error('Failed to download Geth:', err);
      toasts.detail('Download failed', String(err), 'error');
    } finally {
      isDownloading = false;
    }
  }

  async function handleStartGeth() {
    if (!isTauri()) return;
    isStartingGeth = true;
    try {
      await invoke('start_geth', { minerAddress: $walletAccount?.address || null });
      toasts.show('Blockchain node started', 'success');
      await loadGethStatus();
    } catch (err) {
      log.error('Failed to start Geth:', err);
      toasts.detail('Failed to start node', String(err), 'error');
    } finally {
      isStartingGeth = false;
    }
  }

  async function handleStopGeth() {
    if (!isTauri()) return;
    try {
      await invoke('stop_geth');
      toasts.show('Blockchain node stopped', 'info');
      await loadGethStatus();
    } catch (err) {
      log.error('Failed to stop Geth:', err);
      toasts.detail('Failed to stop node', String(err), 'error');
    }
  }

  async function checkBootstrapHealth(notify = false) {
    if (!isTauri()) return;
    isCheckingBootstrap = true;
    try {
      bootstrapHealth = await invoke<BootstrapHealthReport>('check_bootstrap_health');
      if (notify) toasts.show('Bootstrap health refreshed', 'success');
    } catch (err) {
      log.error('Failed to check bootstrap health:', err);
      if (notify) toasts.detail('Failed to refresh bootstrap health', String(err), 'error');
    } finally {
      isCheckingBootstrap = false;
    }
  }

  async function loadBootstrapHealth() {
    if (!isTauri()) return;
    try {
      const cached = await invoke<BootstrapHealthReport | null>('get_bootstrap_health');
      if (cached) bootstrapHealth = cached;
    } catch {
      log.debug('No cached bootstrap health available');
    }
  }

  async function loadAdvertisedHosts(notify = false) {
    if (!isTauri()) {
      advertisedHosts = [];
      return;
    }
    isLoadingAdvertisedHosts = true;
    advertisedHostsError = '';
    try {
      const registryJson = await invoke<string>('get_host_registry');
      const parsed = JSON.parse(registryJson) as unknown;
      const registry: HostRegistryEntry[] = Array.isArray(parsed)
        ? parsed
            .map((entry) => {
              if (!entry || typeof entry !== 'object') return null;
              const me = entry as Partial<HostRegistryEntry>;
              if (typeof me.peerId !== 'string' || me.peerId.trim().length === 0) return null;
              return {
                peerId: me.peerId,
                walletAddress: typeof me.walletAddress === 'string' ? me.walletAddress : '',
                updatedAt: parseUnixSeconds(me.updatedAt) ?? 0,
              };
            })
            .filter((e): e is HostRegistryEntry => e !== null)
        : [];

      const rows = await Promise.all(
        registry.map(async (entry): Promise<AdvertisedHostRow> => {
          let ad: Partial<HostAdvertisement> | null = null;
          try {
            const adJson = await invoke<string | null>('get_host_advertisement', { peerId: entry.peerId });
            if (adJson) {
              const adValue = JSON.parse(adJson) as unknown;
              if (adValue && typeof adValue === 'object') ad = adValue as Partial<HostAdvertisement>;
            }
          } catch {
            /* ignore individual ad failures and still show registry entry */
          }
          const walletAddress =
            typeof ad?.walletAddress === 'string' && ad.walletAddress.trim().length > 0
              ? ad.walletAddress
              : entry.walletAddress || '(unknown)';
          return {
            peerId: entry.peerId,
            walletAddress,
            updatedAt: parseUnixSeconds(entry.updatedAt),
            publishedAt: parseUnixSeconds(ad?.publishedAt),
            lastHeartbeatAt: parseUnixSeconds(ad?.lastHeartbeatAt),
          };
        })
      );

      advertisedHosts = rows.sort((a, b) => {
        const aTs = a.lastHeartbeatAt ?? a.updatedAt ?? 0;
        const bTs = b.lastHeartbeatAt ?? b.updatedAt ?? 0;
        return bTs - aTs;
      });
      if (notify) toasts.show('Host advertisements refreshed', 'success');
    } catch (err) {
      const errMsg = err instanceof Error ? err.message : String(err);
      if (errMsg.includes('DHT not running')) {
        advertisedHosts = [];
      } else {
        advertisedHostsError = errMsg;
        log.error('Failed to load host advertisements:', err);
        if (notify) toasts.detail('Failed to refresh host advertisements', errMsg, 'error');
      }
    } finally {
      isLoadingAdvertisedHosts = false;
    }
  }

  async function checkDhtHealth(notify = false) {
    isCheckingDhtHealth = true;
    try {
      dhtHealth = await dhtService.getHealth();
      if (notify) toasts.show('DHT health refreshed', 'success');
    } catch (err) {
      log.error('Failed to check DHT health:', err);
      if (notify) toasts.detail('Failed to refresh DHT health', String(err), 'error');
      else toasts.show('Failed to check DHT health', 'error');
    } finally {
      isCheckingDhtHealth = false;
    }
  }

  // ---------- DHT connect / disconnect ----------

  async function connectToNetwork() {
    isConnecting = true;
    error = '';
    try {
      await dhtService.start();
      const peerId = await dhtService.getPeerId();
      if (peerId) localPeerId = peerId;
      await loadAdvertisedHosts();
      toasts.notify('networkStatus', 'Connected to P2P network', 'success');
    } catch (err) {
      const errMsg = err instanceof Error ? err.message : String(err);
      if (errMsg.includes('already running')) {
        networkConnected.set(true);
        const peerId = await dhtService.getPeerId();
        if (peerId) localPeerId = peerId;
        await loadAdvertisedHosts();
        toasts.notify('networkStatus', 'Reconnected to P2P network', 'info');
      } else {
        error = errMsg;
        log.error('Failed to connect:', err);
        toasts.notifyDetail('networkStatus', 'Connection failed', error, 'error');
      }
    } finally {
      isConnecting = false;
    }
  }

  async function disconnectFromNetwork() {
    try {
      await dhtService.stop();
      localPeerId = '';
      advertisedHosts = [];
      advertisedHostsError = '';
      toasts.notify('networkStatus', 'Disconnected from P2P network', 'info');
    } catch (err) {
      error = err instanceof Error ? err.message : 'Failed to disconnect';
      log.error('Failed to disconnect:', err);
    }
  }

  async function pingPeer(peerId: string) {
    try {
      const result = await dhtService.pingPeer(peerId);
      log.info('Ping successful:', result);
      toasts.show('Pong', 'success');
    } catch (err) {
      toasts.show('Ping failed', 'error');
      log.error('Ping failed:', err);
    }
  }

  // ---------- Blacklist ----------

  function addToBlacklist() {
    const addr = blacklistAddress.trim();
    if (!addr) {
      toasts.show('Enter an address first', 'warning');
      return;
    }
    if ($blacklist.some((e) => e.address.toLowerCase() === addr.toLowerCase())) {
      toasts.show('Address is already blacklisted', 'warning');
      return;
    }
    blacklist.add(addr, blacklistReason.trim() || 'No reason given');
    blacklistAddress = '';
    blacklistReason = '';
  }

  function removeFromBlacklist(address: string) {
    blacklist.remove(address);
  }
</script>

<svelte:head><title>Network | Chiral Network</title></svelte:head>

<div class="max-w-[1400px] mx-auto p-4 sm:p-6 space-y-4">
  <!-- Header -->
  <div class="flex items-center justify-between gap-3">
    <div>
      <h1 class="text-2xl font-bold dark:text-white">Network</h1>
      <p class="text-sm text-gray-500 dark:text-gray-400">
        Blockchain node, peer-to-peer network, and connection health.
      </p>
    </div>
    <button
      onclick={() => loadGethStatus(true)}
      disabled={isLoadingGeth}
      class="p-2 rounded-lg bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 disabled:opacity-50"
      title="Refresh status"
    >
      <RefreshCw class="w-4 h-4 {isLoadingGeth ? 'animate-spin' : ''}" />
    </button>
  </div>

  {#if error}
    <div class="bg-red-50 dark:bg-red-900/30 border-l-4 border-red-400 p-4 rounded-r-lg">
      <div class="flex items-center gap-2">
        <AlertTriangle class="w-5 h-5 text-red-600 dark:text-red-400" />
        <p class="text-sm text-red-800 dark:text-red-300">{error}</p>
      </div>
    </div>
  {/if}

  <!-- Sticky status strip -->
  <div
    class="sticky top-0 z-10 grid grid-cols-1 sm:grid-cols-3 gap-2 p-2 rounded-2xl bg-white/80 dark:bg-gray-800/80 backdrop-blur border border-gray-200 dark:border-gray-700 shadow-sm"
  >
    {#each [{ icon: Server, label: 'Blockchain', state: gethHealthState, headline: gethHeadline, onclick: () => (activeTab = 'overview') }, { icon: Globe, label: 'P2P network', state: dhtHealthState, headline: dhtHeadline, onclick: () => (activeTab = 'overview') }, { icon: Activity, label: 'Bootstrap', state: bootstrapHealthState, headline: bootstrapHeadline, onclick: () => checkBootstrapHealth(true) }] as tile}
      <button
        onclick={tile.onclick}
        class="flex items-center gap-3 px-3 py-2 rounded-xl ring-1 {healthRing(tile.state)} bg-white dark:bg-gray-800 hover:bg-gray-50 dark:hover:bg-gray-700/60 transition-all text-left"
      >
        <div class="relative shrink-0">
          <tile.icon class="w-5 h-5 text-gray-600 dark:text-gray-300" />
          <span
            class="absolute -top-0.5 -right-0.5 w-2 h-2 rounded-full {healthDot(tile.state)} ring-2 ring-white dark:ring-gray-800"
          ></span>
        </div>
        <div class="min-w-0">
          <div class="text-[11px] uppercase tracking-wider text-gray-500 dark:text-gray-400">{tile.label}</div>
          <div class="text-sm font-semibold text-gray-900 dark:text-white truncate tabular-nums">
            {tile.headline}
          </div>
        </div>
      </button>
    {/each}
  </div>

  <!-- Tabs -->
  <div class="flex border-b border-gray-200 dark:border-gray-700 overflow-x-auto">
    {#each [
      { id: 'overview' as Tab, label: 'Overview', icon: Globe, count: 0 },
      { id: 'peers' as Tab, label: 'Peers', icon: Users, count: visiblePeers.length },
      { id: 'hosts' as Tab, label: 'Hosts', icon: Cloud, count: advertisedHostsWithStatus.length },
      { id: 'blacklist' as Tab, label: 'Blacklist', icon: ShieldBan, count: $blacklist.length },
    ] as tab}
      <button
        onclick={() => (activeTab = tab.id)}
        class="px-4 py-2.5 text-sm font-medium border-b-2 transition-colors flex items-center gap-2 whitespace-nowrap shrink-0
          {activeTab === tab.id
            ? 'border-primary-500 text-primary-700 dark:text-primary-400'
            : 'border-transparent text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-200'}"
      >
        <tab.icon class="w-4 h-4" />
        <span>{tab.label}</span>
        {#if tab.count > 0}
          <span
            class="px-1.5 py-0.5 text-[10px] tabular-nums rounded {activeTab === tab.id
              ? 'bg-primary-100 dark:bg-primary-900/30'
              : 'bg-gray-100 dark:bg-gray-700'}"
          >
            {tab.count}
          </span>
        {/if}
      </button>
    {/each}
  </div>

  <!-- Tab content -->
  {#if activeTab === 'overview'}
    <div class="grid grid-cols-1 lg:grid-cols-2 gap-4">
      <!-- Blockchain Node Card -->
      <section class="bg-white dark:bg-gray-800 rounded-2xl shadow-sm border border-gray-200 dark:border-gray-700 p-5 space-y-4">
        <header class="flex items-center justify-between">
          <div class="flex items-center gap-3">
            <div class="p-2 rounded-lg {gethStatus?.running ? 'bg-emerald-100 dark:bg-emerald-900/30' : 'bg-gray-100 dark:bg-gray-700'}">
              <Server class="w-5 h-5 {gethStatus?.running ? 'text-emerald-600 dark:text-emerald-400' : 'text-gray-600 dark:text-gray-400'}" />
            </div>
            <div>
              <h2 class="font-semibold dark:text-white">Blockchain Node</h2>
              <p class="text-xs text-gray-500 dark:text-gray-400">Geth — Chiral chain</p>
            </div>
          </div>
          {#if gethStatus?.running}
            <span class="flex items-center gap-1.5 px-2 py-1 bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400 rounded-full text-xs font-medium">
              <span class="w-1.5 h-1.5 bg-green-500 rounded-full animate-pulse"></span>
              Running
            </span>
          {:else if gethStatus?.installed}
            <span class="px-2 py-1 bg-gray-100 dark:bg-gray-700 text-gray-700 dark:text-gray-300 rounded-full text-xs font-medium">
              Stopped
            </span>
          {:else}
            <span class="flex items-center gap-1 px-2 py-1 bg-yellow-100 dark:bg-yellow-900/30 text-yellow-700 dark:text-yellow-400 rounded-full text-xs font-medium">
              <AlertTriangle class="w-3 h-3" />
              Not installed
            </span>
          {/if}
        </header>

        {#if !gethStatus?.installed}
          <div class="rounded-lg bg-yellow-50 dark:bg-yellow-900/20 border border-yellow-200 dark:border-yellow-800 p-3 text-sm text-yellow-800 dark:text-yellow-200">
            Geth is required for wallet balance, transactions, and mining. Download to get started.
          </div>
          {#if isDownloading && downloadProgress}
            <div class="space-y-2">
              <div class="flex justify-between text-sm dark:text-gray-300">
                <span>{downloadProgress.status}</span>
                <span class="tabular-nums">{downloadProgress.percentage.toFixed(1)}%</span>
              </div>
              <div class="w-full bg-gray-200 dark:bg-gray-700 rounded-full h-2 overflow-hidden">
                <div
                  class="bg-primary-600 h-full transition-all"
                  style:width="{downloadProgress.percentage}%"
                ></div>
              </div>
              {#if downloadProgress.total > 0}
                <p class="text-xs text-gray-500 dark:text-gray-400 text-right tabular-nums">
                  {formatBytes(downloadProgress.downloaded)} / {formatBytes(downloadProgress.total)}
                </p>
              {/if}
            </div>
          {:else}
            <button
              onclick={handleDownloadGeth}
              disabled={isDownloading}
              class="w-full px-4 py-2.5 bg-primary-600 text-white rounded-lg hover:bg-primary-700 transition-colors flex items-center justify-center gap-2 disabled:opacity-50 text-sm font-medium"
            >
              <Download class="w-4 h-4" />
              Download Geth
            </button>
          {/if}
        {:else}
          <!-- Stat tiles -->
          <div class="grid grid-cols-2 gap-2">
            <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2.5">
              <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Block</p>
              <p class="text-sm font-semibold dark:text-white tabular-nums">{(gethStatus?.currentBlock ?? 0).toLocaleString()}</p>
            </div>
            <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2.5">
              <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Peers</p>
              <p class="text-sm font-semibold dark:text-white tabular-nums">{gethStatus?.peerCount ?? 0}</p>
            </div>
            <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2.5">
              <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Chain ID</p>
              <p class="text-sm font-semibold dark:text-white tabular-nums">{gethStatus?.chainId || '—'}</p>
            </div>
            <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2.5">
              <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Sync</p>
              <p class="text-sm font-semibold dark:text-white">
                {gethStatus?.syncing
                  ? 'Syncing'
                  : gethStatus?.running
                    ? 'Synced'
                    : gethStatus?.chainId
                      ? 'Remote'
                      : 'Offline'}
              </p>
            </div>
          </div>

          {#if gethStatus.syncing && gethStatus.highestBlock > 0}
            <div class="space-y-1">
              <div class="flex justify-between text-xs text-gray-500 dark:text-gray-400 tabular-nums">
                <span>{gethStatus.currentBlock.toLocaleString()}</span>
                <span>{((gethStatus.currentBlock / gethStatus.highestBlock) * 100).toFixed(1)}%</span>
                <span>{gethStatus.highestBlock.toLocaleString()}</span>
              </div>
              <div class="w-full bg-gray-200 dark:bg-gray-700 rounded-full h-1.5 overflow-hidden">
                <div
                  class="bg-blue-500 h-full transition-all"
                  style:width="{(gethStatus.currentBlock / gethStatus.highestBlock) * 100}%"
                ></div>
              </div>
            </div>
          {/if}

          {#if showGethConnectingMsg}
            <div class="rounded-lg bg-primary-50 dark:bg-primary-900/20 border border-primary-200 dark:border-primary-800 p-3 text-sm text-primary-800 dark:text-primary-300">
              <strong>Connecting…</strong> The node is discovering peers via bootstrap. Peer count will update automatically.
            </div>
          {/if}

          <!-- Action -->
          {#if gethStatus?.running}
            <button
              onclick={handleStopGeth}
              class="w-full px-4 py-2 bg-red-600 text-white rounded-lg hover:bg-red-700 transition-colors flex items-center justify-center gap-2 text-sm font-medium"
            >
              <Square class="w-4 h-4" />
              Stop Node
            </button>
          {:else}
            <button
              onclick={handleStartGeth}
              disabled={isStartingGeth}
              class="w-full px-4 py-2 bg-green-600 text-white rounded-lg hover:bg-green-700 transition-colors flex items-center justify-center gap-2 disabled:opacity-50 text-sm font-medium"
            >
              {#if isStartingGeth}
                <Loader2 class="w-4 h-4 animate-spin" />
                Starting…
              {:else}
                <Play class="w-4 h-4" />
                Start Node
              {/if}
            </button>
          {/if}

          <!-- Bootstrap health (collapsed) -->
          <details class="border-t border-gray-200 dark:border-gray-700 pt-3">
            <summary class="flex items-center justify-between cursor-pointer">
              <span class="flex items-center gap-2 text-xs font-medium text-gray-700 dark:text-gray-300">
                <Activity class="w-3.5 h-3.5" />
                Bootstrap health
                {#if bootstrapHealth}
                  <span class="text-gray-500 dark:text-gray-400 tabular-nums">{bootstrapHealth.healthyNodes}/{bootstrapHealth.totalNodes}</span>
                {/if}
              </span>
              <button
                onclick={(e) => {
                  e.preventDefault();
                  e.stopPropagation();
                  checkBootstrapHealth(true);
                }}
                disabled={isCheckingBootstrap}
                class="text-xs px-2 py-1 rounded hover:bg-gray-100 dark:hover:bg-gray-700 disabled:opacity-50 text-gray-600 dark:text-gray-300 flex items-center gap-1"
              >
                {#if isCheckingBootstrap}
                  <Loader2 class="w-3 h-3 animate-spin" />
                {:else}
                  <Activity class="w-3 h-3" />
                {/if}
                Check
              </button>
            </summary>
            <div class="mt-3 space-y-1.5">
              {#if bootstrapHealth}
                {#each bootstrapHealth.nodes as node}
                  <div class="flex items-center justify-between gap-2 p-2 bg-gray-50 dark:bg-gray-700/50 rounded text-xs">
                    <div class="flex items-center gap-2 min-w-0">
                      <div class="w-1.5 h-1.5 rounded-full {node.reachable ? 'bg-green-500' : 'bg-red-500'} shrink-0"></div>
                      <span class="font-medium dark:text-white truncate">{node.name}</span>
                      <span class="text-[10px] text-gray-500 dark:text-gray-400">{node.region}</span>
                    </div>
                    <span class="shrink-0 tabular-nums {node.reachable ? 'text-green-600 dark:text-green-400' : 'text-red-500 dark:text-red-400'}">
                      {#if node.reachable && node.latencyMs}
                        {node.latencyMs}ms
                      {:else if node.error}
                        error
                      {:else}
                        down
                      {/if}
                    </span>
                  </div>
                {/each}
                <p class="text-[10px] text-gray-400 dark:text-gray-500 text-right">
                  Last checked {formatUnixSeconds(bootstrapHealth.timestamp)}
                </p>
              {:else}
                <p class="text-xs text-gray-500 dark:text-gray-400">Click "Check" to test connectivity.</p>
              {/if}
            </div>
          </details>
        {/if}
      </section>

      <!-- P2P Network Card -->
      <section class="bg-white dark:bg-gray-800 rounded-2xl shadow-sm border border-gray-200 dark:border-gray-700 p-5 space-y-4">
        <header class="flex items-center justify-between">
          <div class="flex items-center gap-3">
            <div class="p-2 rounded-lg {$networkConnected ? 'bg-emerald-100 dark:bg-emerald-900/30' : 'bg-gray-100 dark:bg-gray-700'}">
              <Globe class="w-5 h-5 {$networkConnected ? 'text-emerald-600 dark:text-emerald-400' : 'text-gray-600 dark:text-gray-400'}" />
            </div>
            <div>
              <h2 class="font-semibold dark:text-white">P2P Network</h2>
              <p class="text-xs text-gray-500 dark:text-gray-400">Kademlia DHT</p>
            </div>
          </div>
          <span
            class="flex items-center gap-1.5 px-2 py-1 rounded-full text-xs font-medium
              {$networkConnected
                ? 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400'
                : 'bg-gray-100 dark:bg-gray-700 text-gray-700 dark:text-gray-300'}"
          >
            <span class="w-1.5 h-1.5 rounded-full {$networkConnected ? 'bg-green-500 animate-pulse' : 'bg-gray-400'}"></span>
            {$networkConnected ? 'Connected' : 'Disconnected'}
          </span>
        </header>

        <!-- Stat tiles -->
        <div class="grid grid-cols-2 gap-2">
          <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2.5">
            <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Connected peers</p>
            <p class="text-sm font-semibold dark:text-white tabular-nums">{$networkStats.connectedPeers}</p>
          </div>
          <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-2.5">
            <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Discovered</p>
            <p class="text-sm font-semibold dark:text-white tabular-nums">{$networkStats.totalPeers}</p>
          </div>
        </div>

        <!-- Action -->
        {#if $networkConnected}
          <button
            onclick={disconnectFromNetwork}
            class="w-full px-4 py-2 bg-red-600 text-white rounded-lg hover:bg-red-700 transition-colors flex items-center justify-center gap-2 text-sm font-medium"
          >
            <Square class="w-4 h-4" />
            Disconnect
          </button>
        {:else}
          <button
            onclick={connectToNetwork}
            disabled={isConnecting}
            class="w-full px-4 py-2 bg-primary-600 text-white rounded-lg hover:bg-primary-700 transition-colors flex items-center justify-center gap-2 disabled:opacity-50 text-sm font-medium"
          >
            {#if isConnecting}
              <Loader2 class="w-4 h-4 animate-spin" />
              Connecting…
            {:else}
              <Play class="w-4 h-4" />
              Connect
            {/if}
          </button>
        {/if}

        {#if localPeerId}
          <button
            class="w-full p-2.5 bg-gray-50 dark:bg-gray-700/50 rounded-lg text-left hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
            onclick={() => {
              navigator.clipboard.writeText(localPeerId);
              toasts.show('Peer ID copied', 'success');
            }}
            title="Click to copy"
          >
            <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400 mb-0.5">Your peer ID</p>
            <p class="font-mono text-[11px] break-all dark:text-gray-300">{localPeerId}</p>
          </button>
        {/if}

        <!-- DHT health (collapsed) -->
        <details class="border-t border-gray-200 dark:border-gray-700 pt-3">
          <summary class="flex items-center justify-between cursor-pointer">
            <span class="flex items-center gap-2 text-xs font-medium text-gray-700 dark:text-gray-300">
              <HeartPulse class="w-3.5 h-3.5" />
              DHT health
              {#if dhtHealth}
                <span class="text-gray-500 dark:text-gray-400 tabular-nums">{dhtHealth.connectedPeerCount} peers · {dhtHealth.kademliaPeers} kad</span>
              {/if}
            </span>
            <button
              onclick={(e) => {
                e.preventDefault();
                e.stopPropagation();
                checkDhtHealth(true);
              }}
              disabled={isCheckingDhtHealth}
              class="text-xs px-2 py-1 rounded hover:bg-gray-100 dark:hover:bg-gray-700 disabled:opacity-50 text-gray-600 dark:text-gray-300 flex items-center gap-1"
            >
              {#if isCheckingDhtHealth}
                <Loader2 class="w-3 h-3 animate-spin" />
              {:else}
                <HeartPulse class="w-3 h-3" />
              {/if}
              Check
            </button>
          </summary>
          {#if dhtHealth}
            <div class="mt-3 grid grid-cols-2 gap-2">
              <div class="bg-gray-50 dark:bg-gray-700/50 rounded p-2">
                <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Shared files</p>
                <p class="text-sm font-semibold dark:text-white tabular-nums">{dhtHealth.sharedFiles}</p>
              </div>
              <div class="bg-gray-50 dark:bg-gray-700/50 rounded p-2">
                <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400">Relay listeners</p>
                <p
                  class="text-sm font-semibold tabular-nums {relayListeningCount > 0
                    ? 'text-green-600 dark:text-green-400'
                    : 'text-yellow-600 dark:text-yellow-400'}"
                >
                  {relayListeningCount}
                </p>
              </div>
            </div>
            {#if dhtHealth.listeningAddresses.length > 0}
              <div class="mt-2 p-2 bg-gray-50 dark:bg-gray-700/50 rounded">
                <p class="text-[10px] uppercase tracking-wider text-gray-500 dark:text-gray-400 mb-1">Listening ({dhtHealth.listeningAddresses.length})</p>
                <div class="space-y-1 max-h-32 overflow-y-auto">
                  {#each dhtHealth.listeningAddresses as addr}
                    <div class="flex items-start gap-1.5 text-[11px]">
                      <span class="shrink-0 px-1 py-0.5 rounded text-[9px] font-semibold {addrType(addr) === 'IPv6' ? 'bg-purple-100 dark:bg-purple-900/40 text-purple-700 dark:text-purple-300' : 'bg-blue-100 dark:bg-blue-900/40 text-blue-700 dark:text-blue-300'}">
                        {addrType(addr)}
                      </span>
                      {#if isRelayCircuitAddress(addr)}
                        <span class="shrink-0 px-1 py-0.5 rounded text-[9px] font-semibold bg-green-100 dark:bg-green-900/40 text-green-700 dark:text-green-300">
                          Relay
                        </span>
                      {/if}
                      <span class="font-mono break-all dark:text-gray-300" title={addr}>
                        {isRelayCircuitAddress(addr) ? addr : extractIpPort(addr)}
                      </span>
                    </div>
                  {/each}
                </div>
              </div>
            {/if}
          {:else}
            <p class="mt-3 text-xs text-gray-500 dark:text-gray-400">Click "Check" to load DHT health.</p>
          {/if}
        </details>
      </section>
    </div>

  {:else if activeTab === 'peers'}
    <section class="bg-white dark:bg-gray-800 rounded-2xl shadow-sm border border-gray-200 dark:border-gray-700 p-5">
      <header class="flex items-center justify-between mb-4">
        <div class="flex items-center gap-2">
          <Users class="w-5 h-5 text-gray-600 dark:text-gray-300" />
          <h2 class="font-semibold dark:text-white">Connected Peers</h2>
          <span class="px-2 py-0.5 text-xs rounded-full bg-gray-100 dark:bg-gray-700 text-gray-600 dark:text-gray-400 tabular-nums">
            {visiblePeers.length}
          </span>
        </div>
        <label class="inline-flex items-center gap-2 text-xs text-gray-600 dark:text-gray-400 select-none">
          <input
            type="checkbox"
            bind:checked={showBootstrapPeers}
            class="h-3.5 w-3.5 rounded border-gray-300 text-primary-600 focus:ring-primary-500"
          />
          Show bootstrap peers
        </label>
      </header>

      {#if visiblePeers.length === 0}
        <div class="text-center py-8 text-gray-500 dark:text-gray-400">
          {#if !showBootstrapPeers && $peers.length > 0}
            <p class="text-sm">Only bootstrap peers are connected</p>
            <p class="text-xs mt-1">Toggle "Show bootstrap peers" above to include them</p>
          {:else}
            <p class="text-sm">No peers connected</p>
            <p class="text-xs mt-1">Connect to the P2P network on the Overview tab</p>
          {/if}
        </div>
      {:else}
        <div class="space-y-2">
          {#each paginatedPeers as peer (peer.id)}
            <div class="flex items-start justify-between gap-3 p-3 bg-gray-50 dark:bg-gray-700/50 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors">
              <div class="flex-1 min-w-0">
                <button
                  class="font-mono text-sm break-all dark:text-gray-200 text-left hover:text-primary-600 dark:hover:text-primary-400"
                  title="Click to copy peer ID"
                  onclick={() => {
                    navigator.clipboard.writeText(peer.id);
                    toasts.show('Peer ID copied', 'success');
                  }}
                >
                  {peer.id}
                </button>
                {#if peer.address}
                  <div class="text-xs text-gray-500 dark:text-gray-400 mt-1 truncate" title={peer.address}>
                    {peer.address}
                  </div>
                {/if}
              </div>
              <button
                onclick={() => pingPeer(peer.id)}
                class="flex items-center gap-1 px-3 py-1.5 bg-primary-600 text-white text-xs rounded hover:bg-primary-700 transition shrink-0 font-medium"
                title="Ping this peer"
              >
                <Radio class="w-3 h-3" />
                Ping
              </button>
            </div>
          {/each}
        </div>

        {#if peerTotalPages > 1}
          <div class="flex items-center justify-between mt-4 pt-3 border-t border-gray-200 dark:border-gray-700">
            <span class="text-xs text-gray-500 dark:text-gray-400">
              {peerPage * PEERS_PER_PAGE + 1}–{Math.min((peerPage + 1) * PEERS_PER_PAGE, visiblePeers.length)} of {visiblePeers.length}
            </span>
            <div class="flex items-center gap-1">
              <button
                onclick={() => (peerPage = 0)}
                disabled={peerPage === 0}
                class="px-2 py-1 text-xs rounded hover:bg-gray-200 dark:hover:bg-gray-600 disabled:opacity-30 dark:text-gray-300"
              >First</button>
              <button
                onclick={() => (peerPage = Math.max(0, peerPage - 1))}
                disabled={peerPage === 0}
                class="px-2 py-1 text-xs rounded hover:bg-gray-200 dark:hover:bg-gray-600 disabled:opacity-30 dark:text-gray-300"
              >‹</button>
              <span class="px-2 py-1 text-xs text-gray-600 dark:text-gray-400 tabular-nums">
                {peerPage + 1} / {peerTotalPages}
              </span>
              <button
                onclick={() => (peerPage = Math.min(peerTotalPages - 1, peerPage + 1))}
                disabled={peerPage >= peerTotalPages - 1}
                class="px-2 py-1 text-xs rounded hover:bg-gray-200 dark:hover:bg-gray-600 disabled:opacity-30 dark:text-gray-300"
              >›</button>
              <button
                onclick={() => (peerPage = peerTotalPages - 1)}
                disabled={peerPage >= peerTotalPages - 1}
                class="px-2 py-1 text-xs rounded hover:bg-gray-200 dark:hover:bg-gray-600 disabled:opacity-30 dark:text-gray-300"
              >Last</button>
            </div>
          </div>
        {/if}
      {/if}
    </section>

  {:else if activeTab === 'hosts'}
    <section class="bg-white dark:bg-gray-800 rounded-2xl shadow-sm border border-gray-200 dark:border-gray-700 p-5">
      <header class="flex items-center justify-between mb-4">
        <div class="flex items-center gap-2">
          <Cloud class="w-5 h-5 text-gray-600 dark:text-gray-300" />
          <h2 class="font-semibold dark:text-white">Advertised Hosts</h2>
          <span class="px-2 py-0.5 text-xs rounded-full bg-gray-100 dark:bg-gray-700 text-gray-600 dark:text-gray-400 tabular-nums">
            {advertisedHostsWithStatus.length}
          </span>
        </div>
        <button
          onclick={() => loadAdvertisedHosts(true)}
          disabled={isLoadingAdvertisedHosts}
          class="text-xs px-3 py-1.5 rounded-lg bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 dark:text-gray-300 flex items-center gap-1 disabled:opacity-50"
        >
          {#if isLoadingAdvertisedHosts}
            <Loader2 class="w-3 h-3 animate-spin" />
          {:else}
            <RefreshCw class="w-3 h-3" />
          {/if}
          Refresh
        </button>
      </header>

      {#if advertisedHostsError}
        <div class="mb-3 p-2.5 bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg">
          <p class="text-xs text-red-700 dark:text-red-300">{advertisedHostsError}</p>
        </div>
      {/if}

      {#if advertisedHostsWithStatus.length === 0}
        <div class="text-center py-8 text-gray-500 dark:text-gray-400">
          {#if isLoadingAdvertisedHosts}
            <Loader2 class="w-6 h-6 mx-auto mb-2 animate-spin" />
            <p class="text-sm">Loading…</p>
          {:else}
            <Cloud class="w-8 h-8 mx-auto mb-2 opacity-40" />
            <p class="text-sm">No host advertisements</p>
            <p class="text-xs mt-1">Other peers' published hosting offers appear here.</p>
          {/if}
        </div>
      {:else}
        <div class="space-y-2 max-h-[60vh] overflow-y-auto pr-1">
          {#each advertisedHostsWithStatus as host (host.peerId)}
            <div class="p-3 bg-gray-50 dark:bg-gray-700/50 rounded-lg">
              <div class="flex items-start justify-between gap-3">
                <div class="flex-1 min-w-0">
                  <div class="flex items-center gap-2">
                    <span class="w-1.5 h-1.5 rounded-full {host.isOnline ? 'bg-green-500' : 'bg-gray-400'} shrink-0"></span>
                    <button
                      class="font-mono text-xs break-all dark:text-gray-200 text-left hover:text-primary-600 dark:hover:text-primary-400"
                      title="Click to copy peer ID"
                      onclick={() => {
                        navigator.clipboard.writeText(host.peerId);
                        toasts.show('Peer ID copied', 'success');
                      }}
                    >{host.peerId}</button>
                  </div>
                  <p class="text-[11px] text-gray-500 dark:text-gray-400 mt-1 break-all">
                    Wallet: <span class="font-mono">{host.walletAddress}</span>
                  </p>
                  <p class="text-[11px] text-gray-500 dark:text-gray-400 mt-0.5">
                    Updated {formatUnixDateTime(host.updatedAt)} · Heartbeat {formatUnixDateTime(host.lastHeartbeatAt)}
                  </p>
                </div>
                <span
                  class="px-2 py-0.5 text-[10px] uppercase tracking-wider rounded-full shrink-0 font-semibold {host.isOnline
                    ? 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400'
                    : 'bg-gray-100 dark:bg-gray-600 text-gray-600 dark:text-gray-300'}"
                >
                  {host.isOnline ? 'Online' : 'Offline'}
                </span>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </section>

  {:else if activeTab === 'blacklist'}
    <section class="bg-white dark:bg-gray-800 rounded-2xl shadow-sm border border-gray-200 dark:border-gray-700 p-5">
      <header class="flex items-center gap-2 mb-4">
        <ShieldBan class="w-5 h-5 text-red-600 dark:text-red-400" />
        <h2 class="font-semibold dark:text-white">Blacklist</h2>
        <span class="text-xs text-gray-500 dark:text-gray-400">Block addresses from file transfers</span>
        {#if $blacklist.length > 0}
          <span class="ml-auto px-2 py-0.5 text-xs rounded-full bg-red-100 dark:bg-red-900/30 text-red-600 dark:text-red-400 font-medium tabular-nums">
            {$blacklist.length}
          </span>
        {/if}
      </header>

      <div class="flex flex-wrap gap-2 mb-4">
        <input
          type="text"
          bind:value={blacklistAddress}
          placeholder="Wallet or peer address"
          class="flex-1 min-w-[12rem] px-3 py-2 text-sm border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-white placeholder-gray-400 focus:ring-2 focus:ring-primary-500 focus:border-primary-500"
          onkeydown={(e: KeyboardEvent) => {
            if (e.key === 'Enter') addToBlacklist();
          }}
        />
        <input
          type="text"
          bind:value={blacklistReason}
          placeholder="Reason (optional)"
          class="w-48 px-3 py-2 text-sm border border-gray-300 dark:border-gray-600 rounded-lg bg-white dark:bg-gray-700 text-gray-900 dark:text-white placeholder-gray-400 focus:ring-2 focus:ring-primary-500 focus:border-primary-500"
          onkeydown={(e: KeyboardEvent) => {
            if (e.key === 'Enter') addToBlacklist();
          }}
        />
        <button
          onclick={addToBlacklist}
          class="flex items-center gap-1.5 px-4 py-2 bg-red-600 text-white text-sm rounded-lg hover:bg-red-700 transition-colors shrink-0 font-medium"
        >
          <Plus class="w-4 h-4" />
          Add
        </button>
      </div>

      {#if $blacklist.length === 0}
        <div class="text-center py-8 text-gray-500 dark:text-gray-400">
          <ShieldBan class="w-8 h-8 mx-auto mb-2 opacity-40" />
          <p class="text-sm">No blacklisted addresses</p>
          <p class="text-xs mt-1">Add addresses above to refuse file transfers from them.</p>
        </div>
      {:else}
        <div class="space-y-2 max-h-[60vh] overflow-y-auto">
          {#each $blacklist as entry (entry.address)}
            <div class="flex items-center justify-between gap-3 p-3 bg-gray-50 dark:bg-gray-700/50 rounded-lg group">
              <div class="flex-1 min-w-0">
                <button
                  class="font-mono text-sm dark:text-gray-200 truncate text-left hover:text-primary-600 dark:hover:text-primary-400"
                  title="Click to copy: {entry.address}"
                  onclick={() => {
                    navigator.clipboard.writeText(entry.address);
                    toasts.show('Address copied', 'success');
                  }}
                >
                  {truncateAddress(entry.address)}
                </button>
                <div class="flex items-center gap-2 mt-0.5 text-xs text-gray-500 dark:text-gray-400">
                  <span>{entry.reason}</span>
                  <span class="text-gray-400 dark:text-gray-500">·</span>
                  <span>{new Date(entry.addedAt).toLocaleDateString()}</span>
                </div>
              </div>
              <button
                onclick={() => removeFromBlacklist(entry.address)}
                class="p-1.5 text-gray-400 hover:text-red-500 hover:bg-red-50 dark:hover:bg-red-900/30 rounded-lg transition-colors opacity-0 group-hover:opacity-100"
                title="Remove from blacklist"
              >
                <Trash2 class="w-4 h-4" />
              </button>
            </div>
          {/each}
        </div>
      {/if}
    </section>
  {/if}
</div>
