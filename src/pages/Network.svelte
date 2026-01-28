<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import Button from '$lib/components/ui/button.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import PeerMetrics from '$lib/components/PeerMetrics.svelte'
  import GeoDistributionCard from '$lib/components/GeoDistributionCard.svelte'
  import GethStatusCard from '$lib/components/GethStatusCard.svelte'
  import { peers, networkStats, userLocation, settings, wallet } from '$lib/stores'
  import type { AppSettings } from '$lib/stores'
  import { normalizeRegion, UNKNOWN_REGION_ID } from '$lib/geo'
  import { Users, HardDrive, Activity, RefreshCw, UserPlus, Signal, Server, Square, Play, Download, AlertCircle, LayoutDashboard, Network, FileText, Wifi, WifiOff } from 'lucide-svelte'
  import { onMount, onDestroy } from 'svelte'
  import { get } from 'svelte/store'
  import { invoke } from '@tauri-apps/api/core'
  import { listen } from '@tauri-apps/api/event'
  import { dhtService, type DhtHealth as DhtHealthSnapshot, type NatConfidence, type NatReachabilityState } from '$lib/dht'
  import {
    getStatus as fetchGethStatus,
    type GethStatus,
    gethTransition,
    markGethStarting,
    markGethStopping
  } from '$lib/services/gethService'
  import { resetConnectionAttempts } from '$lib/dhtHelpers'
  import { relayErrorService } from '$lib/services/relayErrorService'
  import { Clipboard } from "lucide-svelte"
  import { t } from 'svelte-i18n';
  import { showToast } from '$lib/toast';
  import DropDown from '$lib/components/ui/dropDown.svelte'
  import { SignalingService } from '$lib/services/signalingService';
  import { createWebRTCSession } from '$lib/services/webrtcService';
  import { peerDiscoveryStore, startPeerEventStream, type PeerDiscovery } from '$lib/services/peerEventService';
  import RelayErrorMonitor from '$lib/components/RelayErrorMonitor.svelte'
  import NetworkQuickActions from '$lib/components/network/NetworkQuickActions.svelte'
  import type { GeoRegionConfig } from '$lib/geo';
  import { calculateRegionDistance } from '$lib/services/geolocation';
  import { diagnosticLogger, errorLogger, networkLogger } from '$lib/diagnostics/logger';

  // Check if running in Tauri environment
  const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
  const tr = (k: string, params?: Record<string, any>): string => $t(k, params)

  type NatStatusPayload = {
    state: NatReachabilityState
    confidence: NatConfidence
    lastError?: string | null
    summary?: string | null
  }
  
  // Tab State
  let activeTab: 'overview' | 'peers' | 'diagnostics' | 'relay' = 'overview';

  let discoveryRunning = false
  let newPeerAddress = ''
  let sortBy: 'reputation' | 'sharedFiles' | 'totalSize' | 'nickname' | 'location' | 'joinDate' | 'lastSeen' | 'status' = 'reputation'
  let sortDirection: 'asc' | 'desc' = 'desc'

  const UNKNOWN_DISTANCE = 1_000_000;

  $: if (sortBy || sortDirection) {
    // Reset to page 1 when sorting changes
    // currentPage = 1
  }

  let currentUserRegion: GeoRegionConfig = normalizeRegion(undefined);
  $: currentUserRegion = normalizeRegion($userLocation);
  
  // Update sort direction when category changes to match the default
  $: if (sortBy) {
    const defaults: Record<typeof sortBy, 'asc' | 'desc'> = {
      reputation: 'desc',     // Highest first
      sharedFiles: 'desc',    // Most first
      totalSize: 'desc',      // Largest first
      joinDate: 'desc',       // Newest first
      lastSeen: 'desc',       // Most Recent first
      location: 'asc',        // Closest first
      status: 'asc',          // Online first
      nickname: 'asc'         // A â†’ Z first
    }
    sortDirection = defaults[sortBy]
  }
  
  // Chiral Network Node variables (status only)
  let isGethRunning = false
  let isGethInstalled = false
  let isStartingNode = false
  let isDownloading = false
  let isCheckingGeth = false 
  let downloadProgress = {
    downloaded: 0,
    total: 0,
    percentage: 0,
    status: ''
  }
  let downloadError = ''
  let peerCount = 0
  let peerCountInterval: ReturnType<typeof setInterval> | undefined
  let chainId: number | null = 98765; // Default, will be fetched from backend

  type GethDataCompatibilityStatus = 'ok' | 'missing' | 'mismatch' | 'unknown' | 'corrupted'
  type GethDataCompatibility = {
    status: GethDataCompatibilityStatus
    expected_chain_id: number
    expected_genesis_sha256: string
    detected_chain_id: number | null
    detected_genesis_sha256: string | null
    message: string
  }

  let gethDataCompatibility: GethDataCompatibility | null = null
  // Wallet address (account) vs. blockchain node identity (geth node id)
  let walletAddress = ''
  $: walletAddress = $wallet.address || ''
  let nodeAddress = '' // Geth node id (admin_nodeInfo.id)
  let nodeEnode: string | null = null
  // let copiedNodeAddr = false
  
  // DHT variables
  let dhtStatus: 'disconnected' | 'connecting' | 'connected' = 'disconnected'
  let dhtPeerId: string | null = null
  let dhtPort = 4001
  let dhtBootstrapNodes: string[] = []
  let dhtBootstrapNode = 'Loading bootstrap nodes...'
  let dhtEvents: string[] = []
  let dhtPeerCount = 0
  let dhtHealth: DhtHealthSnapshot | null = null
  let dhtError: string | null = null
  let autorelayToggling = false
  let connectionAttempts = 0
  let dhtPollInterval: number | undefined
  let natStatusUnlisten: (() => void) | null = null
  let lastNatState: NatReachabilityState | null = null
  let lastNatConfidence: NatConfidence | null = null
  let cancelConnection = false
  let isConnecting = false  // Prevent multiple simultaneous connection attempts
  let relayServerEnabled = $settings.enableRelayServer
  let relayServerAlias = $settings.relayServerAlias || ''
  let relayServerRunning = false
  let relayServerToggling = false
  let healthCheckInterval = 30 // seconds
  let isHealthCheckRunning = false
  let relayHealthInitialized = false
  const relayErrorLog = relayErrorService.errorLog
  const formatRelayErrorTimestamp = (ms: number) => new Date(ms).toLocaleString()
  let relayErrorClearedAt = 0
  $: filteredRelayErrors = $relayErrorLog.filter((err) => err.timestamp >= relayErrorClearedAt)
  const formatHealthMessage = (value: string | null | undefined) => value ?? $t('network.dht.health.none')
  type SnapshotRelayError = { message: string; type: string; timestamp: number; relayId: string; retryCount?: number }
  const SNAPSHOT_STORAGE_KEY = 'relaySnapshotHistory'
  const DHT_PORT_STORAGE_KEY = 'chiralDhtPort'
  let snapshotHistory: SnapshotRelayError[] = []

  // Always preserve connections - no unreliable time-based detection
  
  // WebRTC and Signaling variables
  let signaling: SignalingService;
  let webrtcSession: ReturnType<typeof createWebRTCSession> | null = null;
  let webDiscoveredPeers: string[] = [];
  let discoveredPeerEntries: PeerDiscovery[] = [];
  let peerDiscoveryUnsub: (() => void) | null = null;
  let stopPeerEvents: (() => void) | null = null;
  let signalingConnected = false;

  $: if ($settings.enableRelayServer !== relayServerEnabled) {
    relayServerEnabled = $settings.enableRelayServer;
  }

  $: if (($settings.relayServerAlias || '') !== relayServerAlias) {
    relayServerAlias = $settings.relayServerAlias || '';
  }

  $: relayServerRunning = relayServerEnabled && dhtStatus !== 'disconnected';

  // Helper: add a connected peer to the central peers store (if not present)
  function addConnectedPeer(address: string) {
    peers.update(list => {
      const exists = list.find(p => p.address === address || p.id === address)
      if (exists) {
        // mark online
        exists.status = 'online'
        exists.lastSeen = new Date()
        return [...list]
      }

      // Minimal PeerInfo; other fields will be filled by DHT metadata when available
      const newPeer = {
        id: address,
        address,
        nickname: undefined,
        status: 'online' as const,
        reputation: 0,
        sharedFiles: 0,
        totalSize: 0,
        joinDate: new Date(),
        lastSeen: new Date(),
        location: undefined,
      }
      return [newPeer, ...list]
    })
  }

  // Helper: mark a peer disconnected (set status offline) or remove
  function markPeerDisconnected(address: string) {
    peers.update(list => {
      const idx = list.findIndex(p => p.address === address || p.id === address)
      if (idx === -1) return list
      const copy = [...list]
      copy[idx] = { ...copy[idx], status: 'offline', lastSeen: new Date() }
      return copy
    })
  }
  
  // UI variables

  // Fetch public multiaddresses (non-loopback)
  /*
  async function fetchPublicMultiaddrs() {
    try {
      const addrs = await invoke<string[]>('get_multiaddresses')
      publicMultiaddrs = addrs
    } catch (e) {
      errorLogger.networkError(`Failed to get multiaddresses: ${e instanceof Error ? e.message : String(e)}`);
      publicMultiaddrs = []
    }
  }
  */

  function formatSize(bytes: number | undefined): string {
    if (bytes === undefined || bytes === null || isNaN(bytes)) {
      return '0 B'
    }

    const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB']
    let size = bytes
    let unitIndex = 0

    while (size >= 1024 && unitIndex < units.length - 1) {
      size /= 1024
      unitIndex++
    }

    return `${size.toFixed(2)} ${units[unitIndex]}`
  }

  function formatPeerDate(date: Date | string | number | null | undefined): string {
    if (!date) {
      return tr('network.connectedPeers.unknown')
    }
    try {
      const d = new Date(date)
      if (isNaN(d.getTime())) return tr('network.connectedPeers.unknown')
      
      // Show year only if different from current year
      const showYear = d.getFullYear() !== new Date().getFullYear()
      
      return d.toLocaleString(undefined, {
        month: 'short',
        day: 'numeric',
        year: showYear ? 'numeric' : undefined,
        hour: 'numeric',
        minute: '2-digit'
      })
    } catch (e) {
      return tr('network.connectedPeers.unknown')
    }
  }

  function formatReachabilityState(state?: NatReachabilityState | null): string {
    switch (state) {
      case 'public':
        return tr('network.dht.reachability.state.public')
      case 'private':
        return tr('network.dht.reachability.state.private')
      default:
        return tr('network.dht.reachability.state.unknown')
    }
  }

  /*
  function getNodeRole(state?: NatReachabilityState | null): { title: string, description: string, color: string } {
    if (state === 'public') {
      return {
        title: 'Participant (Full Node)',
        description: 'Your node is publicly reachable. You are storing records and helping the network.',
        color: 'text-emerald-600 dark:text-emerald-400'
      }
    }
    return {
      title: 'Observer (Client)',
      description: 'Your node is behind a NAT. You can download files, but you are not routing traffic.',
      color: 'text-muted-foreground'
    }
  }
  */

  function formatNatConfidence(confidence?: NatConfidence | null): string {
    switch (confidence) {
      case 'high':
        return tr('network.dht.reachability.confidence.high')
      case 'medium':
        return tr('network.dht.reachability.confidence.medium')
      default:
        return tr('network.dht.reachability.confidence.low')
    }
  }

  function reachabilityBadgeClass(state?: NatReachabilityState | null): string {
    switch (state) {
      case 'public':
        return 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-300'
      case 'private':
        return 'bg-amber-500/10 text-amber-600 dark:text-amber-300'
      default:
        return 'bg-muted text-muted-foreground'
    }
  }

  function formatNatTimestamp(epoch?: number | null): string {
    if (!epoch) return tr('network.dht.health.never')
    return new Date(epoch * 1000).toLocaleString()
  }

  function persistSettingsPatch(patch: Partial<AppSettings>): AppSettings {
    let storedSettings: Partial<AppSettings> = {}
    try {
      storedSettings = JSON.parse(localStorage.getItem('chiralSettings') || '{}')
    } catch (error) {
      diagnosticLogger.debug('Network', 'Failed to parse stored settings', { error: error instanceof Error ? error.message : String(error) })
    }

    const merged = { ...get(settings), ...storedSettings, ...patch } as AppSettings
    localStorage.setItem('chiralSettings', JSON.stringify(merged))
    settings.set(merged)
    return merged
  }

  function persistRelayServerSettings(patch: Partial<AppSettings> = {}): AppSettings {
    const merged = persistSettingsPatch({
      enableRelayServer: relayServerEnabled,
      relayServerAlias: relayServerAlias.trim(),
      ...patch,
    })

    relayServerEnabled = merged.enableRelayServer ?? false
    relayServerAlias = merged.relayServerAlias || ''
    return merged
  }

  async function toggleRelayServer() {
    if (relayServerToggling) return
    relayServerToggling = true

    const desiredState = !relayServerEnabled

    try {
      relayServerEnabled = desiredState
      const merged = persistRelayServerSettings({ enableRelayServer: desiredState })

      const dhtRunning = isTauri
        ? await invoke<boolean>('is_dht_running').catch(() => false)
        : dhtStatus !== 'disconnected'

      if (dhtRunning) {
        if (dhtPollInterval) {
          clearInterval(dhtPollInterval)
          dhtPollInterval = undefined
        }
        await stopDht()
        if (!dhtBootstrapNodes.length) {
          await fetchBootstrapNodes()
        }
        await startDht()
      } else {
        relayServerRunning = false
      }

      relayServerRunning = merged.enableRelayServer && dhtStatus !== 'disconnected'
      showToast(desiredState ? 'Relay server enabled' : 'Relay server disabled', 'success')
    } catch (error) {
      relayServerEnabled = !desiredState
      persistRelayServerSettings({ enableRelayServer: relayServerEnabled })
      errorLogger.networkError(`Failed to toggle relay server: ${error instanceof Error ? error.message : String(error)}`)
      showToast('Failed to update relay server setting', 'error')
    } finally {
      relayServerToggling = false
    }
  }

  function saveRelayServerAlias() {
    persistRelayServerSettings()
  }

  async function setAutorelay(enabled: boolean) {
    if (autorelayToggling) return
    autorelayToggling = true
    try {
      persistSettingsPatch({ enableAutorelay: enabled })
      await initRelayHealthChecks()
      if (isTauri) {
        const isRunning = await invoke<boolean>('is_dht_running').catch(() => false)
        if (isRunning) {
          if (dhtPollInterval) {
            clearInterval(dhtPollInterval)
            dhtPollInterval = undefined
          }
          await stopDht()
          if (!dhtBootstrapNodes.length) {
            await fetchBootstrapNodes()
          }
          await startDht()
        }
      }
      showToast(enabled ? 'AutoRelay enabled' : 'AutoRelay disabled', 'success')
    } catch (error) {
      errorLogger.networkError(`Failed to toggle AutoRelay: ${error instanceof Error ? error.message : String(error)}`)
      showToast('Failed to update AutoRelay setting', 'error')
    } finally {
      autorelayToggling = false
    }
  }

  function handleAutorelayToggle(event: Event) {
    const target = event.target as HTMLInputElement
    setAutorelay(!!target.checked)
  }

  function clampHealthInterval(value: number) {
    if (value < 10) return 10
    if (value > 300) return 300
    return value
  }

  function loadSnapshotHistory(): SnapshotRelayError[] {
    try {
      const raw = localStorage.getItem(SNAPSHOT_STORAGE_KEY)
      if (!raw) return []
      const parsed = JSON.parse(raw)
      if (Array.isArray(parsed)) return parsed
    } catch (error) {
      diagnosticLogger.debug('Network', 'Failed to load relay snapshot history', { error: error instanceof Error ? error.message : String(error) })
    }
    return []
  }

  function persistSnapshotHistory(history: SnapshotRelayError[]) {
    try {
      localStorage.setItem(SNAPSHOT_STORAGE_KEY, JSON.stringify(history))
    } catch (error) {
      diagnosticLogger.debug('Network', 'Failed to persist relay snapshot history', { error: error instanceof Error ? error.message : String(error) })
    }
  }

  snapshotHistory = loadSnapshotHistory()

  function loadDhtPort() {
    try {
      const raw = localStorage.getItem(DHT_PORT_STORAGE_KEY)
      if (!raw) return
      const parsed = parseInt(raw, 10)
      if (Number.isNaN(parsed)) return
      if (parsed < 1 || parsed > 65535) return
      dhtPort = parsed
    } catch (error) {
      diagnosticLogger.debug('Network', 'Failed to load DHT port', { error: error instanceof Error ? error.message : String(error) })
    }
  }

  function persistDhtPort(value: number) {
    try {
      localStorage.setItem(DHT_PORT_STORAGE_KEY, value.toString())
    } catch (error) {
      diagnosticLogger.debug('Network', 'Failed to persist DHT port', { error: error instanceof Error ? error.message : String(error) })
    }
  }

  function handleDhtPortChange() {
    if (!Number.isFinite(dhtPort)) return
    const next = Math.trunc(dhtPort)
    if (next < 1 || next > 65535) return
    if (next !== dhtPort) {
      dhtPort = next
    }
    persistDhtPort(next)
  }

  function loadHealthCheckInterval() {
    try {
      const saved = localStorage.getItem('relayHealthCheckInterval')
      if (saved) {
        const parsed = parseInt(saved)
        if (!Number.isNaN(parsed)) {
          healthCheckInterval = clampHealthInterval(parsed)
        }
      }
    } catch (error) {
      diagnosticLogger.debug('Network', 'Failed to load relay health interval', { error: error instanceof Error ? error.message : String(error) })
    }
    relayErrorService.setHealthCheckInterval(healthCheckInterval)
  }

  function updateHealthCheckInterval() {
    healthCheckInterval = clampHealthInterval(healthCheckInterval)
    relayErrorService.setHealthCheckInterval(healthCheckInterval)

    try {
      localStorage.setItem('relayHealthCheckInterval', healthCheckInterval.toString())
      showToast(`Health check interval updated to ${healthCheckInterval}s`, 'success')
    } catch (error) {
      diagnosticLogger.debug('Network', 'Failed to save relay health interval', { error: error instanceof Error ? error.message : String(error) })
    }
  }

  function toggleHealthChecks() {
    if (isHealthCheckRunning) {
      relayErrorService.stopHealthChecks()
      isHealthCheckRunning = false
      showToast('Health checks stopped', 'info')
    } else {
      relayErrorService.startHealthChecks()
      isHealthCheckRunning = true
      showToast('Health checks started', 'success')
    }
  }

  async function initRelayHealthChecks() {
    if (relayHealthInitialized) return

    const preferredRelays = get(settings).preferredRelays || []
    const autorelay = $settings.enableAutorelay

    if (preferredRelays.length === 0 && !autorelay) {
      return
    }

    try {
      await relayErrorService.initialize(preferredRelays, autorelay)
      loadHealthCheckInterval()
      relayErrorService.startHealthChecks()
      isHealthCheckRunning = true
      relayHealthInitialized = true
    } catch (error) {
      diagnosticLogger.debug('Network', 'Failed to initialize relay health checks', { error: error instanceof Error ? error.message : String(error) })
    }
  }

  $: snapshotRelayError = (() => {
    if (!dhtHealth) return null
    const message = formatHealthMessage(dhtHealth.lastRelayError || dhtHealth.lastError)
    if (!message || message === $t('network.dht.health.none')) return null

    const atMs = (dhtHealth.lastRelayErrorAt ?? dhtHealth.lastErrorAt ?? 0) * 1000
    if (relayErrorClearedAt && atMs < relayErrorClearedAt) return null
    return {
      message,
      type: dhtHealth.lastRelayErrorType ?? 'relay_error',
      timestamp: atMs || Date.now(),
      relayId: dhtHealth.activeRelayPeerId ?? 'unknown'
    } as SnapshotRelayError
  })()

  $: {
    if (snapshotRelayError && snapshotRelayError.timestamp >= relayErrorClearedAt) {
      const exists = snapshotHistory.some(
        (e) =>
          e.timestamp === snapshotRelayError.timestamp &&
          e.message === snapshotRelayError.message &&
          e.relayId === snapshotRelayError.relayId
      )
      if (!exists) {
        snapshotHistory = [snapshotRelayError, ...snapshotHistory].slice(0, 100)
        persistSnapshotHistory(snapshotHistory)
      }
    }
  }

  $: combinedRelayErrors = [...snapshotHistory, ...filteredRelayErrors]
  $: dedupRelayErrors = (() => {
    const seen = new Set<string>()
    const out: typeof combinedRelayErrors = []
    for (const err of combinedRelayErrors) {
      const key = `${err.relayId}-${err.type}-${err.message}`
      if (seen.has(key)) continue
      seen.add(key)
      out.push(err)
    }
    return out
  })()

  function clearRelayErrors() {
    relayErrorClearedAt = Date.now()
    relayErrorService.clearErrorLog()
    snapshotHistory = []
    persistSnapshotHistory(snapshotHistory)
  }

  async function copyObservedAddr(addr: string) {
    try {
      await navigator.clipboard.writeText(addr)
      showToast(tr('network.dht.reachability.copySuccess'), 'success')
    } catch (error) {
      errorLogger.networkError(`Failed to copy observed address: ${error instanceof Error ? error.message : String(error)}`);
      showToast(tr('network.dht.reachability.copyError'), 'error')
    }
  }

  function showNatToast(payload: NatStatusPayload) {
    if (lastNatState === null) {
      lastNatState = payload.state
      lastNatConfidence = payload.confidence
      return
    }

    if (payload.state === lastNatState && payload.confidence === lastNatConfidence) {
      lastNatState = payload.state
      lastNatConfidence = payload.confidence
      return
    }

    lastNatState = payload.state
    lastNatConfidence = payload.confidence

    const rawSummary = payload.summary ?? payload.lastError ?? ''
    const summaryText = rawSummary.trim().length > 0
      ? rawSummary
      : tr('network.dht.reachability.genericSummary')

    let toastKey = 'network.dht.reachability.toast.unknown'
    let tone: 'success' | 'warning' | 'info' = 'info'

    if (payload.state === 'public') {
      toastKey = 'network.dht.reachability.toast.public'
      tone = 'success'
    } else if (payload.state === 'private') {
      toastKey = 'network.dht.reachability.toast.private'
      tone = 'warning'
    }

    showToast(tr(toastKey, { values: { summary: summaryText } }), tone)
  }

  async function fetchBootstrapNodes() {
    try {
      // Use custom bootstrap nodes if configured, otherwise use defaults
      if ($settings.customBootstrapNodes && $settings.customBootstrapNodes.length > 0) {
        dhtBootstrapNodes = $settings.customBootstrapNodes
        dhtBootstrapNode = dhtBootstrapNodes[0] || 'No bootstrap nodes configured'
      } else {
        dhtBootstrapNodes = await invoke<string[]>("get_bootstrap_nodes_command")
        dhtBootstrapNode = dhtBootstrapNodes[0] || 'No bootstrap nodes configured'
      }
    } catch (error) {
      errorLogger.networkError(`Failed to fetch bootstrap nodes: ${error instanceof Error ? error.message : String(error)}`);
      dhtBootstrapNodes = []
      dhtBootstrapNode = 'Failed to load bootstrap nodes'
    }
  }
  async function registerNatListener() {
    if (!isTauri || natStatusUnlisten) return
    try {
      natStatusUnlisten = await listen('nat_status_update', async (event) => {
        const payload = event.payload as NatStatusPayload
        if (!payload) return
        showNatToast(payload)
      try {
        const snapshot = await dhtService.getHealth()
        if (snapshot) {
          dhtHealth = snapshot
          lastNatState = snapshot.reachability
          lastNatConfidence = snapshot.reachabilityConfidence
          relayErrorService.syncFromHealthSnapshot(snapshot)
        }
      } catch (error) {
        errorLogger.networkError(`Failed to refresh NAT status: ${error instanceof Error ? error.message : String(error)}`);
      }
      })
    } catch (error) {
      errorLogger.networkError(`Failed to subscribe to NAT status updates: ${error instanceof Error ? error.message : String(error)}`);
    }
  }
  
  // Listen for low peer count warnings from backend
  let lowPeerCountUnlisten: (() => void) | null = null;
  
  async function registerLowPeerCountListener() {
    if (!isTauri || lowPeerCountUnlisten) return;
    try {
      lowPeerCountUnlisten = await listen('dht_low_peer_count', (event) => {
        const payload = event.payload as { peer_count: number; minimum: number; message: string };
        if (payload && payload.message) {
          dhtEvents = [...dhtEvents, `âš ï¸ ${payload.message}`];
          showToast(payload.message, 'warning');
          diagnosticLogger.debug('Network', payload.message, { peerCount: payload.peer_count, minimum: payload.minimum });
        }
      });
    } catch (error) {
      errorLogger.networkError(`Failed to subscribe to low peer count warnings: ${error instanceof Error ? error.message : String(error)}`);
    }
  }
  
  async function startDht() {
    if (!isTauri) {
      // Mock DHT connection for web
      dhtStatus = 'connecting'
      cancelConnection = false
      setTimeout(() => {
        if (cancelConnection) {
          dhtStatus = 'disconnected'
          return
        }
        dhtStatus = 'connected'
        dhtPeerId = '12D3KooWMockPeerIdForWebDemo123456789'
      }, 1000)
      return
    }
    
    // Prevent multiple simultaneous connection attempts
    if (isConnecting) {
      diagnosticLogger.debug('Network', 'Connection attempt already in progress, ignoring');
      return;
    }
    
    try {
      isConnecting = true;
      dhtError = null
      cancelConnection = false
      
      // Check if DHT is already running in backend (with retry for timing issues)
      let isRunning = await invoke<boolean>('is_dht_running').catch(() => false)
      
      // If not running on first check, wait a bit and check again (in case auto-start is in progress)
      if (!isRunning) {
        await new Promise(resolve => setTimeout(resolve, 500))
        isRunning = await invoke<boolean>('is_dht_running').catch(() => false)
      }
      
      if (isRunning) {
        // DHT is already running in backend, sync the frontend state immediately
        const backendPeerId = await invoke<string | null>('get_dht_peer_id')
        const peerCount = await invoke<number>('get_dht_peer_count').catch(() => 0)
        
        if (backendPeerId) {
          dhtPeerId = backendPeerId
          dhtService.setPeerId(backendPeerId)
          dhtPeerCount = peerCount
          dhtEvents = [...dhtEvents, `âœ“ DHT already running with peer ID: ${backendPeerId.slice(0, 16)}...`]
          
          // Get health snapshot
          const health = await dhtService.getHealth()
          if (health) {
            dhtHealth = health
            dhtPeerCount = health.peerCount
            relayErrorService.syncFromHealthSnapshot(health)
          }

          // Set status based on peer count
          dhtStatus = dhtPeerCount > 0 ? 'connected' : 'connecting'
          if (dhtPeerCount > 0) {
            dhtEvents = [...dhtEvents, `âœ“ Connected to ${dhtPeerCount} peer(s)`]
          }
          startDhtPolling()
          return
        }
      }
      
      // DHT not running, start it
      dhtStatus = 'connecting'
      connectionAttempts++
      
      // Add a small delay to show the connecting state
      await new Promise(resolve => setTimeout(resolve, 500))
      
      // Check if user cancelled during the delay
      if (cancelConnection) {
        dhtStatus = 'disconnected'
        dhtEvents = [...dhtEvents, 'âš  Connection cancelled by user']
        return
      }
      
      const peerId = await dhtService.start({
        port: dhtPort,
        bootstrapNodes: dhtBootstrapNodes,
        enableAutonat: $settings.enableAutonat,
        autonatProbeIntervalSeconds: $settings.autonatProbeInterval,
        autonatServers: $settings.autonatServers,
        enableAutorelay: $settings.enableAutorelay,
        preferredRelays: $settings.preferredRelays || [],
        enableRelayServer: $settings.enableRelayServer,
        relayServerAlias: $settings.relayServerAlias || '',
        chunkSizeKb: $settings.chunkSize,
        cacheSizeMb: $settings.cacheSize,
        enableUpnp: $settings.enableUPnP,
        pureClientMode: $settings.pureClientMode,
        forceServerMode: $settings.forceServerMode,
      })
      dhtPeerId = peerId
      dhtService.setPeerId(peerId)
      dhtEvents = [...dhtEvents, `âœ“ DHT started with peer ID: ${peerId.slice(0, 16)}...`]
      
      // Try to connect to bootstrap nodes
      let connectionSuccessful = false

      if (dhtBootstrapNodes.length > 0) {
        dhtEvents = [...dhtEvents, `[Attempt ${connectionAttempts}] Connecting to ${dhtBootstrapNodes.length} bootstrap node(s)...`]
        
        // Add another small delay to show the connection attempt
        await new Promise(resolve => setTimeout(resolve, 1000))
        
        // Check if user cancelled during connection attempt
        if (cancelConnection) {
          await stopDht()
          dhtEvents = [...dhtEvents, 'âš  Connection cancelled by user']
          return
        }
        
        try {
          // Try connecting to the first available bootstrap node
          await dhtService.connectPeer(dhtBootstrapNodes[0])
          connectionSuccessful = true
          dhtEvents = [...dhtEvents, `âœ“ Connection initiated to bootstrap nodes (waiting for handshake...)`]
          
          // Poll for actual connection after a delay
          setTimeout(async () => {
            const dhtPeerCountResult = await invoke('get_dht_peer_count') as number
            if (dhtPeerCountResult > 0) {
              dhtEvents = [...dhtEvents, `âœ“ Successfully connected! Peers: ${dhtPeerCountResult}`]
            } else {
              dhtEvents = [...dhtEvents, `âš  Connection pending... (bootstrap nodes may be unreachable)`]
            }
          }, 3000)
        } catch (error: any) {
          diagnosticLogger.warn('Network', 'Cannot connect to bootstrap nodes', { error: error?.message || String(error) });
          
          // Parse and improve error messages
          let errorMessage = error.toString ? error.toString() : String(error)
          
          if (errorMessage.includes('DHT not started')) {
            errorMessage = 'DHT service not initialized properly. Try stopping and restarting.'
            connectionSuccessful = false
          } else if (errorMessage.includes('DHT networking not implemented')) {
            errorMessage = 'P2P networking not available (requires libp2p implementation)'
            connectionSuccessful = false
          } else if (errorMessage.includes('already running')) {
            errorMessage = 'DHT already running on this port'
            connectionSuccessful = true
          } else if (errorMessage.includes('Connection refused') || errorMessage.includes('timeout') || errorMessage.includes('rsa') || errorMessage.includes('Transport')) {
            // These are expected bootstrap connection failures - DHT can still work
            errorMessage = 'Bootstrap nodes unreachable - running in standalone mode'
            connectionSuccessful = true
            dhtEvents = [...dhtEvents, `âš  Bootstrap connection failed but DHT is operational`]
            dhtEvents = [...dhtEvents, `â„¹ Other nodes can connect to you at: /ip4/YOUR_IP/tcp/${dhtPort}/p2p/${dhtPeerId?.slice(0, 16)}...`]
            dhtEvents = [...dhtEvents, `ðŸ’¡ To connect with others, share your connection address above`]
          } else {
            errorMessage = 'Unknown connection error - running in standalone mode'
            connectionSuccessful = true
          }
          
          if (!connectionSuccessful) {
            dhtError = errorMessage
            dhtEvents = [...dhtEvents, `âœ— Connection failed: ${errorMessage}`]
          } else {
            dhtEvents = [...dhtEvents, `âš  ${errorMessage}`]
          }
        }
      }
      
      // Set status based on connection result
      dhtStatus = connectionSuccessful ? 'connected' : 'disconnected'
      connectionAttempts = resetConnectionAttempts(connectionAttempts, connectionSuccessful)
      
      // Start polling for DHT events and peer count
      const snapshot = await dhtService.getHealth()
      if (snapshot) {
        dhtHealth = snapshot
        dhtPeerCount = snapshot.peerCount
        lastNatState = snapshot.reachability
        lastNatConfidence = snapshot.reachabilityConfidence
      }
      startDhtPolling()
    } catch (error: any) {
      errorLogger.dhtInitError(`Failed to start DHT: ${error?.message || String(error)}`);
      dhtStatus = 'disconnected'
      let errorMessage = error.toString ? error.toString() : String(error)
      
      // Handle port already in use error (Windows error 10048)
      if (errorMessage.includes('10048') || errorMessage.includes('address already in use') || errorMessage.includes('Address in use')) {
        errorMessage = `Port ${dhtPort} is already in use. Try stopping the DHT first, or choose a different port.`
        dhtEvents = [...dhtEvents, `âœ— Port conflict detected on ${dhtPort}`]
        dhtEvents = [...dhtEvents, `ðŸ’¡ Try clicking "Stop DHT" first, or change the port number`]
      } else if (errorMessage.includes('already running')) {
        errorMessage = 'DHT is already running. Try stopping it first.'
        dhtEvents = [...dhtEvents, `âš  DHT already running - click "Stop DHT" to restart`]
      }
      
      dhtError = errorMessage
      dhtEvents = [...dhtEvents, `âœ— Failed to start DHT: ${errorMessage}`]
    } finally {
      isConnecting = false;
    }
  }

  
  let peerRefreshCounter = 0;

  function startDhtPolling() {
    // If already polling, don't start another one
    if (dhtPollInterval !== undefined) {
      return
    }

    const applyHealth = (health: DhtHealthSnapshot) => {
      dhtHealth = health
      dhtPeerCount = health.peerCount
      lastNatState = health.reachability
      lastNatConfidence = health.reachabilityConfidence
      relayErrorService.syncFromHealthSnapshot(health)
    }

    dhtPollInterval = setInterval(async () => {
      try {
        // Only call getEvents if running in Tauri mode
        // Note: getEvents is not available in the current DhtService implementation
        const events: any[] = []
        if (events.length > 0) {
          const formattedEvents = events.map(event => {
            if (event.peerDisconnected) {
              return `âœ— Peer disconnected: ${event.peerDisconnected.peer_id.slice(0, 12)}... (Reason: ${event.peerDisconnected.cause})`
            } else if (event.peerConnected) {
              return `âœ“ Peer connected: ${event.peerConnected.slice(0, 12)}...`
            } else if (event.peerDiscovered) {
              return `â„¹ Peer discovered: ${event.peerDiscovered.slice(0, 12)}...`
            } else if (event.error) {
              return `âœ— Error: ${event.error}`
            }
            return JSON.stringify(event) // Fallback for other event types
          })
          dhtEvents = [...dhtEvents, ...formattedEvents].slice(-10)
        }

        let peerCount = dhtPeerCount
        const health = await dhtService.getHealth()
        if (health) {
          applyHealth(health)
          peerCount = health.peerCount
          // Fetch public multiaddresses
          // await fetchPublicMultiaddrs() // Disabled for now to remove unused variable warning
        } else {
          peerCount = await dhtService.getPeerCount()
          dhtPeerCount = peerCount
          lastNatState = null
          lastNatConfidence = null
        }

        // Update connection status based on peer count
        // IMPORTANT: Never set to 'disconnected' while backend is running
        if (peerCount === 0) {
          // If backend is running but no peers, show 'connecting' not 'disconnected'
          if (dhtStatus === 'connected') {
            dhtStatus = 'connecting'
            dhtEvents = [...dhtEvents, 'âš  Lost connection to all peers']
          }
        } else {
          if (dhtStatus !== 'connected') {
            dhtStatus = 'connected'
            dhtEvents = [...dhtEvents, `âœ“ Reconnected to ${peerCount} peer(s)`]
          }
        }

        // Auto-refresh connected peers list every 5 seconds (every ~2.5 poll cycles)
        peerRefreshCounter++;
        if (peerRefreshCounter >= 3 && isTauri && peerCount > 0) {
          peerRefreshCounter = 0;
          // Silently refresh peer list in background
          try {
            const { peerService } = await import('$lib/services/peerService');
            const connectedPeers = await peerService.getConnectedPeers();
            peers.set(connectedPeers);
          } catch (error) {
            diagnosticLogger.debug('Network', 'Background peer refresh failed', { error: error instanceof Error ? error.message : String(error) });
          }
        }
      } catch (error) {
        errorLogger.networkError(`Failed to poll DHT status: ${error instanceof Error ? error.message : String(error)}`);
      }
    }, 2000) as unknown as number
  }
  
  function cancelDhtConnection() {
    cancelConnection = true
    dhtStatus = 'disconnected'
    dhtEvents = [...dhtEvents, 'âš  Connection cancelled by user']
    showToast($t('network.dht.connectionCancelled'), 'info')
  }

  async function stopDht() {
    if (!isTauri) {
      dhtStatus = 'disconnected'
      dhtPeerId = null
      dhtError = null
      connectionAttempts = 0
      dhtHealth = null
      // copiedListenAddr = null
      lastNatState = null
      lastNatConfidence = null
      cancelConnection = false
      return
    }
    
    try {
      // Stop polling first to prevent race conditions
      if (dhtPollInterval) {
        clearInterval(dhtPollInterval)
        dhtPollInterval = undefined
      }
      
      await dhtService.stop()
      dhtStatus = 'disconnected'
      dhtPeerId = null
      dhtError = null
      connectionAttempts = 0
      dhtEvents = [...dhtEvents, `âœ“ DHT stopped - port ${dhtPort} released`]
      dhtHealth = null
      // copiedListenAddr = null
      lastNatState = null
      lastNatConfidence = null
      cancelConnection = false
      
      // Small delay to ensure port is fully released
      await new Promise(resolve => setTimeout(resolve, 500))
    } catch (error) {
      errorLogger.dhtInitError(`Failed to stop DHT: ${error instanceof Error ? error.message : String(error)}`);
      dhtEvents = [...dhtEvents, `âœ— Failed to stop DHT: ${error}`]
      // Even if stop failed, clear local state
      dhtStatus = 'disconnected'
      dhtPeerId = null
    }
  }

  // Sync DHT status with backend state on page navigation (preserves connections)
  async function syncDhtStatusOnPageLoad() {
    if (!isTauri) {
      dhtStatus = 'disconnected'
      return
    }
    
    try {
      // Check current DHT status without resetting connections
      let isRunning = await invoke<boolean>('is_dht_running').catch(() => false)
      
      // If not running, retry after a short delay (DHT might be starting up)
      if (!isRunning) {
        await new Promise(resolve => setTimeout(resolve, 500))
        isRunning = await invoke<boolean>('is_dht_running').catch(() => false)
      }
      
      const peerCount = await invoke<number>('get_dht_peer_count').catch(() => 0)
      let peerId = await invoke<string | null>('get_dht_peer_id').catch(() => null)
      
      // If DHT is running but peer ID is not yet available, retry
      if (isRunning && !peerId) {
        await new Promise(resolve => setTimeout(resolve, 500))
        peerId = await invoke<string | null>('get_dht_peer_id').catch(() => null)
      }

      // If DHT is running in backend, sync status and start polling
      if (isRunning) {
        // DHT is running even if peerId isn't available yet (startup race condition)
        if (peerId) {
          dhtPeerId = peerId
          dhtService.setPeerId(peerId)
        }
        
        dhtPeerCount = peerCount
        
        // Also restore health snapshot
          try {
            const health = await dhtService.getHealth()
            if (health) {
              dhtHealth = health
              lastNatState = health.reachability
              lastNatConfidence = health.reachabilityConfidence
              relayErrorService.syncFromHealthSnapshot(health)
            }
          } catch (healthError) {
            diagnosticLogger.debug('Network', 'Could not fetch health snapshot', { error: healthError instanceof Error ? healthError.message : String(healthError) });
          }
        
        // Set status based on peer count - polling will handle dynamic updates
        dhtStatus = peerCount > 0 ? 'connected' : 'connecting'
        dhtEvents = [...dhtEvents, `âœ“ DHT restored (${peerCount} peer${peerCount !== 1 ? 's' : ''} connected)`]
        startDhtPolling() // Always start polling when DHT is running
      } else {
        dhtStatus = 'disconnected'
        dhtPeerId = null
        dhtPeerCount = 0
        dhtHealth = null
        lastNatState = null
        lastNatConfidence = null
      }
    } catch (error) {
      errorLogger.networkError(`Failed to sync DHT status: ${error instanceof Error ? error.message : String(error)}`);
      dhtStatus = 'disconnected'
      dhtPeerId = null
      dhtPeerCount = 0
      dhtHealth = null
      lastNatState = null
      lastNatConfidence = null
      dhtEvents = [...dhtEvents, 'âš  Error checking network status']
    }
  }

  async function runDiscovery() {
    if (dhtStatus !== 'connected') {
      showToast($t('network.errors.dhtNotConnected'), 'error');
      return;
    }

    // In Tauri mode, peer discovery happens automatically via DHT events
    // This button just shows the current count
    if (isTauri) {
      const discoveryCount = discoveredPeerEntries.length;
      showToast(tr('network.peerDiscovery.discoveryStarted', { values: { count: discoveryCount } }), 'info');
      return;
    }

    // In web mode, use WebRTC signaling for testing
    if (!signalingConnected) {
      try {
        if (!signaling) {
          signaling = new SignalingService();
        }
        await signaling.connect();
        signalingConnected = true;
        const myClientId = signaling.getClientId();
        signaling.peers.subscribe(peers => {
          // Filter out own client ID from discovered peers
          // discoveredPeers = peers.filter(p => p !== myClientId);
          webDiscoveredPeers = peers.filter(p => p !== myClientId);
          diagnosticLogger.debug('Network', 'Updated discovered peers', { peerCount: webDiscoveredPeers.length });
        });

        // Register signaling message handler for WebRTC
        signaling.setOnMessage((msg) => {
          if (webrtcSession && msg.from === webrtcSession.peerId) {
            if (msg.type === "offer") {
              webrtcSession.acceptOfferCreateAnswer(msg.sdp).then(answer => {
                signaling.send({ type: "answer", sdp: answer, to: msg.from });
              });
            } else if (msg.type === "answer") {
              webrtcSession.acceptAnswer(msg.sdp);
            } else if (msg.type === "candidate") {
              webrtcSession.addRemoteIceCandidate(msg.candidate);
            }
          }
        });
        // showToast('Connected to signaling server', 'success');
        showToast(tr('toasts.network.signalingConnected'), 'success');
      } catch (error) {
        errorLogger.networkError(`Failed to connect to signaling server: ${error instanceof Error ? error.message : String(error)}`);
        // showToast('Failed to connect to signaling server for web mode testing', 'error');
        showToast(
          tr('toasts.network.signalingError'),
          'error'
        );
        return;
      }
    }

    // discoveredPeers will update automatically
    // showToast(tr('network.peerDiscovery.discoveryStarted', { values: { count: discoveredPeers.length } }), 'info');
    const discoveryCount = isTauri ? discoveredPeerEntries.length : webDiscoveredPeers.length;
    showToast(tr('network.peerDiscovery.discoveryStarted', { values: { count: discoveryCount } }), 'info');
  }
  
  async function connectToPeer() {
    if (!newPeerAddress.trim()) {
      // showToast('Please enter a peer address', 'error');
      showToast(tr('toasts.network.peerAddressRequired'), 'error');
      return;
    }

    const peerAddress = newPeerAddress.trim();

    // In Tauri mode, use DHT backend for P2P connections
    if (isTauri) {
      if (dhtStatus !== 'connected') {
        // showToast('DHT not connected. Please start DHT first.', 'error');
        showToast(tr('toasts.network.dhtRequired'), 'error');
        return;
      }

      // Check if peer is already connected
      const isAlreadyConnected = $peers.some(peer =>
        peer.id === peerAddress ||
        peer.address === peerAddress ||
        peer.address.includes(peerAddress) ||
        peerAddress.includes(peer.id)
      );

      if (isAlreadyConnected) {
        // showToast('Peer is already connected', 'info');
        showToast(tr('toasts.network.alreadyConnected'), 'info');
        newPeerAddress = '';
        return;
      }

      try {
        // showToast('Connecting to peer via DHT...', 'info');
        showToast(tr('toasts.network.connecting'), 'info');
        const currentPeerCount = $peers.length;
        await invoke('connect_to_peer', { peerAddress });

        // Clear input
        newPeerAddress = '';

        // Wait a moment and check if the peer was actually added
        setTimeout(async () => {
          await refreshConnectedPeers();
          if ($peers.length > currentPeerCount) {
            // showToast('Connection Success!', 'success');
            showToast(tr('toasts.network.connectionSuccess'), 'success')
          } else {
            // showToast('Connection failed. Peer may be unreachable or address invalid.', 'error');
            showToast(tr('toasts.network.connectionFailed'), 'error');
          }
        }, 2000);
      } catch (error) {
        errorLogger.networkError(`Failed to connect to peer: ${error instanceof Error ? error.message : String(error)}`);
        // showToast('Failed to connect to peer: ' + error, 'error');
        showToast(
          tr('toasts.network.connectError', { values: { error: String(error) } }),
          'error'
        );
      }
      return;
    }

    // In web mode, use WebRTC for testing
    if (!signalingConnected) {
      // showToast('Signaling server not connected. Please start DHT first.', 'error');
      showToast(tr('toasts.network.signalingMissing'), 'error');
      return;
    }

    const peerId = peerAddress;

    // Check if peer exists in discovered peers
    // if (!discoveredPeers.includes(peerId)) {
    if (!webDiscoveredPeers.includes(peerId)) {
      // showToast(`Peer ${peerId} not found in discovered peers`, 'warning');
      showToast(
        tr('toasts.network.peerNotFound', { values: { peer: peerId } }),
        'warning'
      );
      // Still attempt connection in case peer was discovered recently
    }

    try {
      webrtcSession = createWebRTCSession({
        peerId,
        signaling,
        isInitiator: true,
        onMessage: (data) => {
          // showToast('Received from peer: ' + data, 'info');
          showToast(
            tr('toasts.network.messageReceived', { values: { message: String(data) } }),
            'info'
          )
        },
        onConnectionStateChange: (state) => {
          // Only log connected/disconnected states for network logger
          if (state === 'connected' || state === 'disconnected') {
            networkLogger.statusChanged(state, 1);
          }

          // Only show toasts for important states (not every intermediate state)
          if (state === 'connected') {
            // showToast('Successfully connected to peer!', 'success');
            showToast(tr('toasts.network.webrtcConnected'), 'success');
            // Add minimal PeerInfo to peers store if not present
            addConnectedPeer(peerId);
          } else if (state === 'failed') {
            // showToast('Connection to peer failed', 'error');
            showToast(tr('toasts.network.webrtcFailed'), 'error');
            // Mark peer as offline / remove from peers list
            markPeerDisconnected(peerId);
          } else if (state === 'disconnected' || state === 'closed') {
            diagnosticLogger.debug('Network', 'WebRTC peer disconnected', { peerId });
            // Mark peer as offline / remove from peers list
            markPeerDisconnected(peerId);
          }
        },
        onDataChannelOpen: () => {
          // showToast('Data channel open - you can now send messages!', 'success');
          showToast(tr('toasts.network.dataChannelOpen'), 'success');
          // Ensure peer is listed as connected when data channel opens
          addConnectedPeer(peerId);
        },
        onDataChannelClose: () => {
          // showToast('Data channel closed', 'warning');
          showToast(tr('toasts.network.dataChannelClosed'), 'warning');
          markPeerDisconnected(peerId);
        },
        onError: (e) => {
          // showToast('WebRTC error: ' + e, 'error');
          showToast(
            tr('toasts.network.webrtcError', { values: { error: String(e) } }),
            'error'
          );
          errorLogger.networkError(`WebRTC error: ${e instanceof Error ? e.message : String(e)}`);
        }
      });
      // Optimistically add the peer as 'connecting' so it appears in UI while the handshake occurs
      peers.update(list => {
        const exists = list.find(p => p.address === peerId || p.id === peerId)
        if (exists) {
          exists.status = 'away'
          exists.lastSeen = new Date()
          return [...list]
        }
        const pending = {
          id: peerId,
          address: peerId,
          nickname: undefined,
          status: 'away' as const, // using 'away' to indicate in-progress
          reputation: 0,
          sharedFiles: 0,
          totalSize: 0,
          joinDate: new Date(),
          lastSeen: new Date(),
          location: undefined,
        }
        return [pending, ...list]
      })

      // Create offer asynchronously (don't await to avoid freezing UI)
      webrtcSession.createOffer();
      // showToast('Connecting to peer: ' + peerId, 'success');
      showToast(
        tr('toasts.network.webrtcConnecting', { values: { peer: peerId } }),
        'success'
      );

      // Clear input on successful connection attempt
      newPeerAddress = '';

    } catch (error) {
      errorLogger.networkError(`Failed to create WebRTC session: ${error instanceof Error ? error.message : String(error)}`);
      // showToast('Failed to create connection: ' + error, 'error');
      showToast(
        tr('toasts.network.webrtcCreateError', { values: { error: String(error) } }),
        'error'
      );
    }
  }
  
  async function refreshConnectedPeers() {
    if (!isTauri) {
      return;
    }

    try {
      const { peerService } = await import('$lib/services/peerService');
      const connectedPeers = await peerService.getConnectedPeers();
      peers.set(connectedPeers);
    } catch (error) {
      diagnosticLogger.debug('Network', 'Failed to refresh peers', { error: error instanceof Error ? error.message : String(error) });
    }
  }

  async function disconnectFromPeer(peerId: string) {
    if (!isTauri) {
      // Mock disconnection in web mode
      peers.update(p => p.filter(peer => peer.address !== peerId))
      showToast($t('network.connectedPeers.disconnected'), 'success')
      return
    }

    try {
      await invoke('disconnect_from_peer', { peerId })
      // Remove peer from local store
      peers.update(p => p.filter(peer => peer.address !== peerId))
      showToast($t('network.connectedPeers.disconnected'), 'success')
    } catch (error) {
      errorLogger.networkError(`Failed to disconnect from peer: ${error instanceof Error ? error.message : String(error)}`);
      showToast($t('network.connectedPeers.disconnectError') + ': ' + error, 'error')
    }
  }
  
  function refreshStats() {
    networkStats.update(s => ({
      ...s,
      avgDownloadSpeed: 5 + Math.random() * 20,
      avgUploadSpeed: 3 + Math.random() * 15,
      onlinePeers: Math.floor(s.totalPeers * (0.6 + Math.random() * 0.3))
    }))
  }

  function applyGethStatus(status: GethStatus) {
    const wasRunning = isGethRunning
    isGethInstalled = status.installed
    isGethRunning = status.running

    if (status.running && !wasRunning) {
      startPolling()
    } else if (!status.running && wasRunning) {
      if (peerCountInterval) {
        clearInterval(peerCountInterval)
        peerCountInterval = undefined
      }
      peerCount = 0
      nodeAddress = ''
      nodeEnode = null
    }

    if (status.running && !nodeAddress) {
      void refreshGethNodeInfo()
    }
  }

  
  async function checkGethStatus() {
    if (!isTauri) {
      // In web mode, simulate that geth is not installed
      isGethInstalled = false
      isGethRunning = false
      return
    }

    isCheckingGeth = true
    try {
      const status = await fetchGethStatus('./bin/geth-data', 1)
      // Preserve the running state - don't stop the node if it's already running
      applyGethStatus(status)
      await refreshGethDataCompatibility()
    } catch (error) {
      errorLogger.networkError(`Failed to check geth status: ${error instanceof Error ? error.message : String(error)}`);
    } finally {
      isCheckingGeth = false
    }
  }

  async function refreshGethDataCompatibility() {
    if (!isTauri) return
    try {
      gethDataCompatibility = await invoke<GethDataCompatibility>('check_geth_data_compatibility', {
        dataDir: './bin/geth-data'
      })
    } catch (error) {
      diagnosticLogger.debug('Network', 'Failed to check geth data compatibility', { error: error instanceof Error ? error.message : String(error) })
      gethDataCompatibility = null
    }
  }

  async function downloadGeth() {
    if (!isTauri) {
      downloadError = $t('network.errors.downloadOnlyTauri')
      return
    }

    // First check if Geth is already installed
    isCheckingGeth = true
    try {
      const status = await fetchGethStatus('./bin/geth-data', 1)
      if (status.installed) {
        // Geth is already installed, update state and return
        applyGethStatus(status)
        isCheckingGeth = false
        // showToast('Geth is already installed', 'info')
        showToast(tr('toasts.network.gethInstalled'), 'info')
        return
      }
    } catch (error) {
      errorLogger.networkError(`Failed to check geth status before download: ${error instanceof Error ? error.message : String(error)}`);
      // Continue with download attempt
    }
    isCheckingGeth = false

    isDownloading = true
    downloadError = ''
    downloadProgress = {
      downloaded: 0,
      total: 0,
      percentage: 0,
      status: $t('network.download.starting')
    }

    try {
      await invoke('download_geth_binary')
      isGethInstalled = true
      isDownloading = false
      // Download completed successfully - UI will update to show start button
    } catch (e) {
      downloadError = String(e)
      isDownloading = false
      // showToast('Failed to download Geth: ' + e, 'error')
      showToast(
        tr('toasts.network.gethDownloadError', { values: { error: String(e) } }),
        'error'
      )
    }
  }

  async function startGethNode() {
    if (!isTauri) {
      diagnosticLogger.info('Network', 'Cannot start Chiral Node in web mode - desktop app required');
      return
    }

    isStartingNode = true
    markGethStarting()
    try {
      // Check if in client mode (forced OR NAT-based)
      let isClientMode = $settings.pureClientMode;
      if (!isClientMode) {
        // Check DHT reachability to detect NAT-based client mode
        try {
          const health = await dhtService.getHealth();
          if (health && health.reachability === 'private') {
            isClientMode = true;
          }
        } catch (err) {
          console.warn('Failed to check DHT reachability for client mode:', err);
        }
      }

      await invoke('start_geth_node', {
        dataDir: './bin/geth-data',
        pureClientMode: isClientMode,  // Combined: forced OR NAT-based
        allowStartAnyway: true
      })
      isGethRunning = true
      startPolling()
      await refreshGethNodeInfo()
      await refreshGethDataCompatibility()
    } catch (error) {
      errorLogger.networkError(`Failed to start Chiral node: ${error instanceof Error ? error.message : String(error)}`);
    } finally {
      isStartingNode = false
    }
  }

  async function reinitAndStartGethNode() {
    if (!isTauri) return

    isStartingNode = true
    markGethStarting()
    try {
      if (isGethRunning) {
        await stopGethNode()
      }

      // Check if in client mode (forced OR NAT-based)
      let isClientMode = $settings.pureClientMode;
      if (!isClientMode) {
        try {
          const health = await dhtService.getHealth();
          if (health && health.reachability === 'private') {
            isClientMode = true;
          }
        } catch {
          // ignore
        }
      }

      await invoke('start_geth_node', {
        dataDir: './bin/geth-data',
        pureClientMode: isClientMode,
        allowReinit: true,
        allowStartAnyway: false,
      })

      isGethRunning = true
      startPolling()
      await refreshGethNodeInfo()
      await refreshGethDataCompatibility()
      showToast(tr('toasts.network.gethReinitSuccess'), 'success')
    } catch (error) {
      errorLogger.networkError(`Failed to reinitialize Chiral node: ${error instanceof Error ? error.message : String(error)}`);
      showToast(tr('toasts.network.gethReinitError', { values: { error: String(error) } }), 'error')
    } finally {
      isStartingNode = false
    }
  }

  async function stopGethNode() {
    if (!isTauri) {
      diagnosticLogger.info('Network', 'Cannot stop Chiral Node in web mode - desktop app required');
      return
    }

    markGethStopping()
    try {
      await invoke('stop_geth_node')
      isGethRunning = false
      if (peerCountInterval) {
        clearInterval(peerCountInterval)
        peerCountInterval = undefined
      }
      peerCount = 0
      nodeAddress = ''
      nodeEnode = null
    } catch (error) {
      errorLogger.networkError(`Failed to stop Chiral node: ${error instanceof Error ? error.message : String(error)}`);
    }
  }
  

  function startPolling() {
    if (peerCountInterval) {
      clearInterval(peerCountInterval)
    }
    fetchPeerCount()
    fetchChainId()  // Fetch chain ID when node starts
    peerCountInterval = setInterval(fetchPeerCount, 5000)
  }


  // Copy Helper
  async function copy(text: string | null | undefined) {
    if (!text) return
    try {
      await navigator.clipboard.writeText(text)
    } catch (e) {
      errorLogger.networkError(`Copy failed: ${e instanceof Error ? e.message : String(e)}`);
    }
  }

  function extractNodeIdFromEnode(enode: string | null | undefined): string | null {
    if (!enode) return null
    const prefix = 'enode://'
    if (!enode.startsWith(prefix)) return null
    const rest = enode.slice(prefix.length)
    const at = rest.indexOf('@')
    if (at === -1) return null
    const id = rest.slice(0, at).trim()
    return id.length ? id : null
  }

  async function refreshGethNodeInfo() {
    if (!isTauri || !isGethRunning) {
      nodeAddress = ''
      nodeEnode = null
      return
    }

    try {
      const info = await invoke<any>('get_geth_node_info')
      const id = (typeof info?.id === 'string' && info.id.trim().length) ? info.id.trim() : null
      const enode = (typeof info?.enode === 'string' && info.enode.trim().length) ? info.enode.trim() : null

      nodeEnode = enode
      nodeAddress = id ?? extractNodeIdFromEnode(enode) ?? ''
    } catch (error) {
      errorLogger.networkError(`Failed to fetch geth node info: ${error instanceof Error ? error.message : String(error)}`)
      if (!nodeAddress) {
        nodeEnode = null
      }
    }
  }

  async function fetchChainId() {
    if (!isGethRunning) return
    if (!isTauri) {
      // Default chain ID for web mode
      chainId = 98765
      return
    }
    
    try {
      chainId = await invoke('get_network_chain_id') as number
    } catch (error) {
      console.error('Failed to fetch chain ID:', error)
      // Keep the default value on error
    }
  }

  async function fetchPeerCount() {
    if (!isGethRunning) return
    if (!isTauri) {
      // Simulate peer count in web mode
      peerCount = Math.floor(Math.random() * 10) + 5
      return
    }
    
    try {
      peerCount = await invoke('get_network_peer_count') as number
    } catch (error) {
      errorLogger.networkError(`Failed to fetch peer count: ${error instanceof Error ? error.message : String(error)}`);
      peerCount = 0
    }
  }

  onMount(() => {
    const interval = setInterval(refreshStats, 5000)
    let unlistenProgress: (() => void) | null = null
    
    // Initialize signaling service (web preview only) and DHT integrations
    ;(async () => {
      if (!isTauri) {
        try {
          signaling = new SignalingService();
          await signaling.connect();
          signalingConnected = true;
          const myClientId = signaling.getClientId();
          signaling.peers.subscribe(peers => {
            // Filter out own client ID from discovered peers
            webDiscoveredPeers = peers.filter(p => p !== myClientId);
          });

          // Register signaling message handler for WebRTC
          signaling.setOnMessage((msg) => {
            if (webrtcSession && msg.from === webrtcSession.peerId) {
              if (msg.type === "offer") {
                webrtcSession.acceptOfferCreateAnswer(msg.sdp).then(answer => {
                  signaling.send({ type: "answer", sdp: answer, to: msg.from });
                });
              } else if (msg.type === "answer") {
                webrtcSession.acceptAnswer(msg.sdp);
              } else if (msg.type === "candidate") {
                webrtcSession.addRemoteIceCandidate(msg.candidate);
              }
            }
          });
        } catch (error) {
          // Signaling service not available (DHT not running) - this is normal
          signalingConnected = false;
        }
      }
      
      // Fetch chain ID from backend
      const fetchChainId = async () => {
        if (isTauri) {
          try {
            chainId = await invoke<number>('get_chain_id')
          } catch (error) {
            console.warn('Failed to fetch chain ID from backend, using default:', error)
          }
        }
      }
      
      loadDhtPort()

      // Initialize async operations (preserves connections)
      const initAsync = async () => {
        // Run ALL independent checks in parallel for better performance
        await Promise.all([
          fetchBootstrapNodes(),
          checkGethStatus(),
          syncDhtStatusOnPageLoad(), // DHT check is independent from Geth check
          fetchChainId()
        ])
        await initRelayHealthChecks()

        // Listen for download progress updates (only in Tauri)
        if (isTauri) {
          await registerNatListener()
          unlistenProgress = await listen('geth-download-progress', (event) => {
            downloadProgress = event.payload as typeof downloadProgress
          })
        }
      }     

      // Always preserve existing connections
      await initAsync()

      if (isTauri) {
        if (!peerDiscoveryUnsub) {
          peerDiscoveryUnsub = peerDiscoveryStore.subscribe((entries) => {
            discoveredPeerEntries = entries;
          });
        }
        if (!stopPeerEvents) {
          try {
            stopPeerEvents = await startPeerEventStream();
          } catch (error) {
            errorLogger.networkError(`Failed to start peer event stream: ${error instanceof Error ? error.message : String(error)}`);
          }
        }
        await refreshConnectedPeers();
        await registerNatListener()
        await registerLowPeerCountListener()

        // Listen for download progress updates
        unlistenProgress = await listen('geth-download-progress', (event) => {
          downloadProgress = event.payload as typeof downloadProgress
        })
      }

      // initAsync()
    })()
    
    return () => {
      clearInterval(interval)
      if (peerCountInterval) {
        clearInterval(peerCountInterval)
      }
      if (unlistenProgress) {
        unlistenProgress()
      }
      if (natStatusUnlisten) {
        natStatusUnlisten()
        natStatusUnlisten = null
      }
      if (lowPeerCountUnlisten) {
        lowPeerCountUnlisten()
        lowPeerCountUnlisten = null
      }
      if (stopPeerEvents) {
        stopPeerEvents()
        stopPeerEvents = null
      }
      if (peerDiscoveryUnsub) {
        peerDiscoveryUnsub()
        peerDiscoveryUnsub = null
      }
      relayErrorService.stopHealthChecks()
      // Note: We do NOT disconnect the signaling service here
      // It should persist across page navigations to maintain peer connections
    }
  })

  onDestroy(() => {
    if (peerCountInterval) {
      clearInterval(peerCountInterval)
      peerCountInterval = undefined
    }
    if (dhtPollInterval) {
      clearInterval(dhtPollInterval)
      dhtPollInterval = undefined
    }
    if (natStatusUnlisten) {
      natStatusUnlisten()
      natStatusUnlisten = null
    }
    if (lowPeerCountUnlisten) {
      lowPeerCountUnlisten()
      lowPeerCountUnlisten = null
    }
    if (stopPeerEvents) {
      stopPeerEvents()
      stopPeerEvents = null
    }
    if (peerDiscoveryUnsub) {
      peerDiscoveryUnsub()
      peerDiscoveryUnsub = null
    }
    relayErrorService.stopHealthChecks()
    // Note: We do NOT stop the DHT service here
    // The DHT should persist across page navigations
  })
</script>

<div class="space-y-6">
  
  <!-- Header & Status Bar -->
  <div class="flex flex-col md:flex-row md:items-center justify-between gap-4">
    <div>
      <h1 class="text-3xl font-bold tracking-tight">{$t('network.title')}</h1>
      <p class="text-muted-foreground mt-1">{$t('network.subtitle')}</p>
    </div>
    <div class="flex items-center gap-3">
      <!-- Global status badge -->
      <Badge class="px-3 py-1 text-sm font-medium {dhtStatus === 'connected' ? 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400' : 'bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400'}">
        {dhtStatus === 'connected' ? 'Network Online' : 'Network Offline'}
      </Badge>
    </div>
  </div>

  <!-- Notification Boxes -->
  <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4 mb-6">
    <Card class="p-4 border-l-4 {isGethRunning ? 'border-l-emerald-500' : 'border-l-red-500'}">
      <div class="flex items-start gap-3">
        <div class="p-2 {isGethRunning ? 'bg-emerald-100 dark:bg-emerald-900/20' : 'bg-red-100 dark:bg-red-900/20'} rounded-full">
          <HardDrive class="h-5 w-5 {isGethRunning ? 'text-emerald-600 dark:text-emerald-400' : 'text-red-600 dark:text-red-400'}" />
        </div>
        <div>
          <h4 class="font-medium text-foreground">Blockchain Node</h4>
          <p class="text-sm font-bold mt-1 {isGethRunning ? 'text-emerald-600 dark:text-emerald-400' : 'text-red-600 dark:text-red-400'}">
            {isGethRunning ? 'Running' : 'Stopped'} <span class="text-muted-foreground font-normal ml-1">(Chain ID: {chainId})</span>
          </p>
        </div>
      </div>
    </Card>

    <Card class="p-4 border-l-4 border-l-blue-500">
      <div class="flex items-start gap-3">
        <div class="p-2 bg-blue-100 dark:bg-blue-900/20 rounded-full">
          <Network class="h-5 w-5 text-blue-600 dark:text-blue-400" />
        </div>
        <div>
          <h4 class="font-medium text-foreground">Network Status</h4>
          <p class="text-sm font-bold mt-1 text-blue-600 dark:text-blue-400">
            {dhtStatus === 'connected' ? `Connected (${dhtPeerCount})` : 'Disconnected'}
          </p>
        </div>
      </div>
    </Card>

    <Card class="p-4 border-l-4 border-l-purple-500">
      <div class="flex items-start gap-3">
        <div class="p-2 bg-purple-100 dark:bg-purple-900/20 rounded-full">
          <Activity class="h-5 w-5 text-purple-600 dark:text-purple-400" />
        </div>
        <div>
          <h4 class="font-medium text-foreground">Traffic</h4>
          <div class="text-sm font-bold mt-1 flex gap-3 text-purple-600 dark:text-purple-400">
            <span>â†“ {$networkStats.avgDownloadSpeed.toFixed(1)} MB/s</span>
            <span>â†‘ {$networkStats.avgUploadSpeed.toFixed(1)} MB/s</span>
          </div>
        </div>
      </div>
    </Card>

    <Card class="p-4 border-l-4 {dhtHealth?.reachability === 'public' ? 'border-l-emerald-500' : 'border-l-orange-500'}">
      <div class="flex items-start gap-3">
        <div class="p-2 {dhtHealth?.reachability === 'public' ? 'bg-emerald-100 dark:bg-emerald-900/20' : 'bg-orange-100 dark:bg-orange-900/20'} rounded-full">
          <Signal class="h-5 w-5 {dhtHealth?.reachability === 'public' ? 'text-emerald-600 dark:text-emerald-400' : 'text-orange-600 dark:text-orange-400'}" />
        </div>
        <div>
          <h4 class="font-medium text-foreground">Reachability</h4>
          <p class="text-sm font-bold mt-1 capitalize {dhtHealth?.reachability === 'public' ? 'text-emerald-600 dark:text-emerald-400' : 'text-orange-600 dark:text-orange-400'}">
            {dhtHealth?.reachability ? formatReachabilityState(dhtHealth.reachability) : 'Unknown'}
          </p>
        </div>
      </div>
    </Card>
  </div>

  <!-- Quick Actions Panel -->
  <NetworkQuickActions
    {dhtPeerId}
    {dhtHealth}
    {dhtStatus}
    {discoveryRunning}
    autorelayEnabled={$settings.enableAutorelay}
    on:discover={runDiscovery}
    on:addPeer={(e) => { newPeerAddress = e.detail.address; connectToPeer(); }}
    on:toggleAutorelay={() => setAutorelay(!$settings.enableAutorelay)}
    on:startDht={startDht}
    on:stopDht={stopDht}
  />

  <!-- Tab Navigation -->
  <div class="border-b border-border">
    <nav class="flex space-x-8" aria-label="Tabs">
      <button
        class="group inline-flex items-center py-4 px-1 border-b-2 font-medium text-sm {activeTab === 'overview' ? 'border-primary text-primary' : 'border-transparent text-muted-foreground hover:text-foreground hover:border-muted-foreground'}"
        on:click={() => activeTab = 'overview'}
      >
        <LayoutDashboard class="mr-2 h-4 w-4" />
        Overview
      </button>
      <button
        class="group inline-flex items-center py-4 px-1 border-b-2 font-medium text-sm {activeTab === 'peers' ? 'border-primary text-primary' : 'border-transparent text-muted-foreground hover:text-foreground hover:border-muted-foreground'}"
        on:click={() => activeTab = 'peers'}
      >
        <Users class="mr-2 h-4 w-4" />
        Peers
      </button>
      <button
        class="group inline-flex items-center py-4 px-1 border-b-2 font-medium text-sm {activeTab === 'diagnostics' ? 'border-primary text-primary' : 'border-transparent text-muted-foreground hover:text-foreground hover:border-muted-foreground'}"
        on:click={() => activeTab = 'diagnostics'}
      >
        <FileText class="mr-2 h-4 w-4" />
        Diagnostics
      </button>
      <button
        class="group inline-flex items-center py-4 px-1 border-b-2 font-medium text-sm {activeTab === 'relay' ? 'border-primary text-primary' : 'border-transparent text-muted-foreground hover:text-foreground hover:border-muted-foreground'}"
        on:click={() => activeTab = 'relay'}
      >
        <Signal class="mr-2 h-4 w-4" />
        Relay
      </button>
    </nav>
  </div>

  <!-- Tab Content -->
  <div class="mt-6">
    
    <!-- OVERVIEW TAB -->
    {#if activeTab === 'overview'}
      <div class="space-y-6">

        <div class="grid grid-cols-1 lg:grid-cols-2 gap-6">
          <!-- Blockchain Node Lifecycle -->
          <Card class="p-6">
            <div class="flex items-center justify-between mb-6">
              <h3 class="text-lg font-semibold flex items-center gap-2">
                <Server class="h-5 w-5 text-primary" />
                Blockchain Node
              </h3>
              <Badge
                variant={isGethRunning ? 'default' : 'secondary'}
                class={
                  isGethRunning
                    ? 'bg-emerald-600'
                    : $gethTransition === 'starting'
                      ? 'bg-blue-600'
                      : $gethTransition === 'stopping'
                        ? 'bg-amber-600'
                        : ''
                }
              >
                {#if $gethTransition === 'starting'}
                  Starting…
                {:else if $gethTransition === 'stopping'}
                  Shutting down…
                {:else}
                  {isGethRunning ? 'Running' : !isGethInstalled ? 'Not Installed' : 'Stopped'}
                {/if}
              </Badge>
            </div>

            <div class="space-y-6">
              {#if !isGethInstalled && $gethTransition === 'idle'}
                <div class="text-center py-6 space-y-4">
                  <p class="text-muted-foreground text-sm">The Chiral blockchain node is required for transaction validation and mining.</p>
                  <Button on:click={downloadGeth} disabled={isDownloading}>
                    {#if isDownloading}
                      <RefreshCw class="h-4 w-4 mr-2 animate-spin" /> Downloading...
                    {:else}
                      <Download class="h-4 w-4 mr-2" /> Download Node Software
                    {/if}
                  </Button>
                  {#if downloadError}
                    <p class="text-xs text-red-500 mt-2">{downloadError}</p>
                  {/if}
                </div>
              {:else}
                <div class="space-y-4">
                  <div class="flex gap-3">
                    <Button
                      class="flex-1"
                      variant={isGethRunning ? "secondary" : "default"}
                      disabled={isGethRunning || isStartingNode || $gethTransition !== 'idle'}
                      on:click={startGethNode}
                    >
                      {#if isStartingNode}
                        <RefreshCw class="h-4 w-4 mr-2 animate-spin" /> Starting...
                      {:else}
                        <Play class="h-4 w-4 mr-2" /> Start Node
                      {/if}
                    </Button>
                    <Button 
                      class="flex-1" 
                      variant="destructive"
                      disabled={!isGethRunning || $gethTransition !== 'idle'}
                      on:click={stopGethNode}
                    >
                      {#if $gethTransition === 'stopping'}
                        <RefreshCw class="h-4 w-4 mr-2 animate-spin" /> Stopping…
                      {:else if $gethTransition === 'starting'}
                        <RefreshCw class="h-4 w-4 mr-2 animate-spin" /> Starting…
                      {:else}
                        <Square class="h-4 w-4 mr-2" /> Stop Node
                      {/if}
                    </Button>
                  </div>

                  {#if $gethTransition === 'starting'}
                    <div class="rounded-lg border border-blue-500/20 bg-blue-500/10 p-3 text-sm text-blue-800">
                      The node is starting up. This may take a few seconds while RPC becomes available.
                    </div>
                  {/if}

                  {#if $gethTransition === 'stopping'}
                    <div class="rounded-lg border border-amber-500/20 bg-amber-500/10 p-3 text-sm text-amber-800">
                      The node is shutting down. This may take a few seconds while it flushes chain data.
                    </div>
                  {/if}

                  {#if gethDataCompatibility && ['mismatch', 'unknown', 'corrupted'].includes(gethDataCompatibility.status)}
                    <div
                      class={
                        gethDataCompatibility.status === 'unknown'
                          ? 'rounded-lg border border-sky-500/20 bg-sky-500/10 p-3 text-sm text-sky-950'
                          : 'rounded-lg border border-amber-500/20 bg-amber-500/10 p-3 text-sm text-amber-950'
                      }
                    >
                      <div class="flex items-start gap-2">
                        <AlertCircle
                          class={
                            gethDataCompatibility.status === 'unknown'
                              ? 'h-4 w-4 mt-0.5 flex-shrink-0 text-sky-700'
                              : 'h-4 w-4 mt-0.5 flex-shrink-0 text-amber-700'
                          }
                        />
                        <div class="space-y-1">
                          <div class="font-medium">
                            {gethDataCompatibility.status === 'unknown'
                              ? 'Chain data could not be verified'
                              : 'Network upgrade detected'}
                          </div>
                          <div class={gethDataCompatibility.status === 'unknown' ? 'text-sky-900/90' : 'text-amber-900/90'}>
                            {gethDataCompatibility.message}
                          </div>
                          <div class={gethDataCompatibility.status === 'unknown' ? 'text-xs text-sky-900/80' : 'text-xs text-amber-900/80'}>
                            Expected chain: <span class="font-mono">{gethDataCompatibility.expected_chain_id}</span>
                            {#if gethDataCompatibility.detected_chain_id !== null}
                              · Your data: <span class="font-mono">{gethDataCompatibility.detected_chain_id}</span>
                            {/if}
                          </div>
                        </div>
                      </div>

                      <div class="mt-3 flex flex-col sm:flex-row gap-2">
                        <Button
                          class="sm:flex-1"
                          on:click={reinitAndStartGethNode}
                          disabled={isStartingNode || $gethTransition !== 'idle'}
                        >
                          Reinitialize to new network
                        </Button>
                        {#if !isGethRunning}
                          <Button
                            class="sm:flex-1"
                            variant="secondary"
                            on:click={startGethNode}
                            disabled={isStartingNode || $gethTransition !== 'idle'}
                          >
                            Start anyway (existing data)
                          </Button>
                        {/if}
                      </div>
                    </div>
                  {/if}

                  <div class="pt-2 border-t space-y-3">
                    <div class="flex justify-between text-sm">
                      <span class="text-muted-foreground">Chain ID</span>
                      <span class="font-mono">{chainId}</span>
                    </div>
                    <div class="flex justify-between text-sm">
                      <span class="text-muted-foreground">Peers</span>
                      <span class="font-mono">{peerCount}</span>
                    </div>
                    <div class="space-y-1">
                      <span class="text-xs text-muted-foreground uppercase">Node Address</span>
                      <div class="flex items-center gap-2">
                        <code class="bg-muted px-2 py-1 rounded text-xs font-mono flex-1 truncate" title={nodeEnode ?? nodeAddress}>
                          {nodeAddress || 'Waiting for start...'}
                        </code>
                        {#if nodeAddress}
                          <Button variant="ghost" size="icon" class="h-6 w-6 flex-shrink-0" on:click={() => copy(nodeAddress)}>
                            <Clipboard class="h-3 w-3" />
                          </Button>
                        {/if}
                      </div>
                    </div>
                  </div>
                  
                  <div class="flex justify-end pt-2">
                    <Button variant="ghost" size="sm" class="h-8 text-xs text-muted-foreground" on:click={checkGethStatus} disabled={isCheckingGeth}>
                      <RefreshCw class="h-3 w-3 mr-1 {isCheckingGeth ? 'animate-spin' : ''}" />
                      Refresh Status
                    </Button>
                  </div>
                </div>
              {/if}
            </div>
          </Card>

          <!-- DHT Network Control -->
          <Card class="p-6">
            <div class="flex items-center justify-between mb-6">
              <h3 class="text-lg font-semibold flex items-center gap-2">
                <Network class="h-5 w-5 text-primary" />
                DHT Network
              </h3>
              <Badge variant={dhtStatus === 'connected' ? 'default' : 'secondary'} class={dhtStatus === 'connected' ? 'bg-green-600' : ''}>
                {dhtStatus === 'connected' ? 'Connected' : dhtStatus === 'connecting' ? 'Connecting...' : 'Disconnected'}
              </Badge>
            </div>

            <div class="space-y-6">
              {#if dhtStatus === 'disconnected'}
                <div class="space-y-4">
                  <div class="space-y-2">
                    <Label for="dht-port">Network Port</Label>
                    <div class="flex gap-3">
                      <Input id="dht-port" type="number" bind:value={dhtPort} on:change={handleDhtPortChange} class="max-w-[120px]" />
                      <Button on:click={startDht} class="flex-1" disabled={connectionAttempts > 0}>
                        <Play class="h-4 w-4 mr-2" />
                        Connect Network
                      </Button>
                    </div>
                    <p class="text-xs text-muted-foreground">
                      Port {dhtPort} will be used for P2P connections. Ensure this port is open if you are behind a firewall.
                    </p>
                  </div>
                  {#if dhtError}
                    <div class="p-3 bg-red-100/50 border border-red-200 text-red-700 rounded-md text-sm flex items-start gap-2">
                      <AlertCircle class="h-4 w-4 mt-0.5 flex-shrink-0" />
                      <span>{dhtError}</span>
                    </div>
                  {/if}
                </div>
              {:else if dhtStatus === 'connecting'}
                 <div class="text-center py-8 space-y-3">
                    <RefreshCw class="h-8 w-8 mx-auto animate-spin text-primary" />
                    <p class="text-muted-foreground">Connecting to Chiral Network...</p>
                    <Button variant="outline" size="sm" on:click={cancelDhtConnection}>Cancel</Button>
                 </div>
              {:else}
                <div class="space-y-4">
                  <div class="grid grid-cols-1 md:grid-cols-2 gap-4">
                    <div class="space-y-1">
                      <span class="text-xs font-medium text-muted-foreground uppercase">My Peer ID</span>
                      <div class="flex items-center gap-2">
                        <code class="bg-muted px-2 py-1 rounded text-xs font-mono flex-1 truncate" title={dhtPeerId}>{dhtPeerId}</code>
                        <Button variant="ghost" size="icon" class="h-6 w-6 flex-shrink-0" on:click={() => copy(dhtPeerId)}>
                          <Clipboard class="h-3 w-3" />
                        </Button>
                      </div>
                    </div>
                    <div class="space-y-1">
                      <span class="text-xs font-medium text-muted-foreground uppercase">Port</span>
                      <div class="font-mono text-sm border px-3 py-1 rounded bg-muted/20">{dhtPort}</div>
                    </div>
                  </div>

                  {#if dhtHealth?.observedAddrs?.[0]}
                    <div class="space-y-1">
                      <span class="text-xs font-medium text-muted-foreground uppercase">Multiaddress</span>
                      <div class="flex items-center gap-2">
                        <code class="bg-muted px-2 py-1 rounded text-xs font-mono flex-1 truncate" title={dhtHealth.observedAddrs[0]}>
                          {dhtHealth.observedAddrs[0]}
                        </code>
                        <Button variant="ghost" size="icon" class="h-6 w-6 flex-shrink-0" on:click={() => copy(dhtHealth?.observedAddrs?.[0])}>
                          <Clipboard class="h-3 w-3" />
                        </Button>
                      </div>
                    </div>
                  {/if}

                  {#if dhtBootstrapNode}
                    <div class="space-y-1">
                      <span class="text-xs font-medium text-muted-foreground uppercase">Connected Bootstrap</span>
                      <div class="font-mono text-xs text-muted-foreground truncate" title={dhtBootstrapNode}>
                        {dhtBootstrapNode}
                      </div>
                    </div>
                  {/if}

                  <Button variant="destructive" class="w-full mt-2" on:click={stopDht}>
                    <Square class="h-4 w-4 mr-2" />
                    Disconnect Network
                  </Button>
                </div>
              {/if}
            </div>
          </Card>
        </div>

        <div class="grid grid-cols-1 lg:grid-cols-2 gap-6">
          
          <!-- Hole Punching (DCUtR) -->
          <Card class="p-6">
            <div class="flex items-center justify-between mb-4">
                <h3 class="text-lg font-semibold">Hole Punching (DCUtR)</h3>
              {#if dhtHealth}
                  <Badge variant={dhtHealth.dcutrEnabled ? 'default' : 'secondary'} class={dhtHealth.dcutrEnabled ? 'bg-blue-100 text-blue-800 hover:bg-blue-200' : ''}>
                      {dhtHealth.dcutrEnabled ? 'Enabled' : 'Disabled'}
                  </Badge>
              {/if}
          </div>
          
          {#if dhtHealth}
            <div class="grid grid-cols-3 gap-4 text-center mb-4">
               <div class="p-2 bg-muted/20 rounded-lg">
                  <div class="text-2xl font-bold">{dhtHealth.dcutrHolePunchAttempts || 0}</div>
                  <div class="text-xs text-muted-foreground uppercase tracking-wider">Attempts</div>
               </div>
               <div class="p-2 bg-green-50/50 dark:bg-green-900/10 rounded-lg">
                  <div class="text-2xl font-bold text-green-600 dark:text-green-400">{dhtHealth.dcutrHolePunchSuccesses || 0}</div>
                  <div class="text-xs text-muted-foreground uppercase tracking-wider">Success</div>
               </div>
               <div class="p-2 bg-red-50/50 dark:bg-red-900/10 rounded-lg">
                  <div class="text-2xl font-bold text-red-600 dark:text-red-400">{dhtHealth.dcutrHolePunchFailures || 0}</div>
                  <div class="text-xs text-muted-foreground uppercase tracking-wider">Failed</div>
               </div>
            </div>

            <div class="space-y-3 pt-3 border-t">
               <div class="flex justify-between text-sm">
                  <span class="text-muted-foreground">Success Rate</span>
                  <span class="font-medium">
                      {dhtHealth.dcutrHolePunchAttempts > 0 
                          ? ((dhtHealth.dcutrHolePunchSuccesses / dhtHealth.dcutrHolePunchAttempts) * 100).toFixed(1) 
                          : '0.0'}%
                  </span>
               </div>
               <div class="flex justify-between text-sm">
                  <span class="text-muted-foreground">Last Success</span>
                  <span class="font-mono text-xs">{formatNatTimestamp(dhtHealth.lastDcutrSuccess)}</span>
               </div>
            </div>
          {:else}
            <div class="py-8 text-center">
               <p class="text-sm text-muted-foreground">DHT not connected.</p>
            </div>
          {/if}
        </Card>

          <!-- Geographic Distribution -->
          <GeoDistributionCard />

        </div>
      </div>

    <!-- PEERS TAB -->
    {:else if activeTab === 'peers'}
      <div class="space-y-6">
        
        <!-- Peer Discovery Section -->
        <Card class="p-5 border-2 bg-muted/10">
          <div class="flex items-center justify-between">
            <div>
              <h3 class="font-semibold text-lg">Discovery</h3>
              <p class="text-sm text-muted-foreground">Find new peers to connect with</p>
            </div>
            <div class="flex gap-2">
               <Button variant="secondary" on:click={runDiscovery} disabled={discoveryRunning}>
                 <RefreshCw class="h-4 w-4 mr-2 {discoveryRunning ? 'animate-spin' : ''}" />
                 Run Discovery
               </Button>
               <Button variant="outline" on:click={() => newPeerAddress = ''}>
                 Add Manually
               </Button>
            </div>
          </div>
          
          <div class="mt-4">
             <div class="flex items-center gap-2 max-w-md">
                <Input 
                  placeholder="Peer Address / ID" 
                  class="h-9 text-sm" 
                  bind:value={newPeerAddress} 
                />
                <Button size="sm" variant="secondary" disabled={!newPeerAddress} on:click={connectToPeer}>
                  <UserPlus class="h-4 w-4" />
                </Button>
             </div>
          </div>
          
          {#if discoveredPeerEntries.length > 0}
            <div class="mt-4 pt-4 border-t">
              <p class="text-sm font-medium mb-2">Discovered Peers ({discoveredPeerEntries.length})</p>
              <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-2">
                {#each discoveredPeerEntries.slice(0, 6) as peer}
                  <div class="flex items-center justify-between p-2 bg-background border rounded text-sm">
                    <span class="font-mono truncate w-32">{peer.peerId}</span>
                    <Button size="icon" variant="ghost" class="h-6 w-6" on:click={() => copy(peer.peerId)}>
                      <Clipboard class="h-3 w-3" />
                    </Button>
                  </div>
                {/each}
              </div>
            </div>
          {/if}
        </Card>

        <!-- Smart Peer Connection -->
        <PeerMetrics />

        <!-- Main Connected Peers List -->
        <Card class="p-0 overflow-hidden">
          <div class="p-4 border-b bg-muted/30 flex items-center justify-between">
            <h3 class="font-semibold">Connected Peers ({$peers.length})</h3>
            <div class="flex items-center gap-2">
               <span class="text-sm text-muted-foreground">Sort by:</span>
               <div class="w-32">
                 <DropDown
                  options={[
                    { value: 'reputation', label: $t('network.connectedPeers.reputation') },
                    { value: 'location', label: $t('network.connectedPeers.location') },
                    { value: 'status', label: $t('network.connectedPeers.status') }
                  ]}
                  bind:value={sortBy}
                 />
               </div>
               <Button variant="ghost" size="icon" on:click={refreshConnectedPeers}>
                 <RefreshCw class="h-4 w-4" />
               </Button>
            </div>
          </div>
          
          {@const sortedPeers = [...$peers].sort((a, b) => {
            let aVal: any, bVal: any

            switch (sortBy) {
                case 'reputation':
                    aVal = a.reputation
                    bVal = b.reputation
                    break
                case 'sharedFiles':
                    aVal = a.sharedFiles
                    bVal = b.sharedFiles
                    break
                case 'totalSize':
                    aVal = a.totalSize
                    bVal = b.totalSize
                    break
                case 'nickname':
                    aVal = (a.nickname || 'zzzzz').toLowerCase()
                    bVal = (b.nickname || 'zzzzz').toLowerCase()
                    break
                case 'location':
                    const getLocationDistance = (peerLocation: string | undefined) => {
                        if (!peerLocation) return UNKNOWN_DISTANCE;
                        const peerRegion = normalizeRegion(peerLocation);
                        if (peerRegion.id === UNKNOWN_REGION_ID) return UNKNOWN_DISTANCE;
                        if (currentUserRegion.id === UNKNOWN_REGION_ID) return peerRegion.id === UNKNOWN_REGION_ID ? 0 : UNKNOWN_DISTANCE;
                        if (peerRegion.id === currentUserRegion.id) return 0;
                        return Math.round(calculateRegionDistance(currentUserRegion, peerRegion));
                    };
                    aVal = getLocationDistance(a.location);
                    bVal = getLocationDistance(b.location);
                    break
                case 'joinDate':
                    aVal = new Date(a.joinDate).getTime()
                    bVal = new Date(b.joinDate).getTime()
                    break
                case 'lastSeen':
                    aVal = new Date(a.lastSeen).getTime()
                    bVal = new Date(b.lastSeen).getTime()
                    break
                case 'status':
                    aVal = a.status === 'online' ? 0 : a.status === 'away' ? 1 : 2
                    bVal = b.status === 'online' ? 0 : b.status === 'away' ? 1 : 2
                    break
                default:
                    return 0
            }

            if (typeof aVal === 'string' && typeof bVal === 'string') {
                if (aVal < bVal) return sortDirection === 'asc' ? -1 : 1
                else if (aVal > bVal) return sortDirection === 'asc' ? 1 : -1
                else return 0
            }

            if (typeof aVal === 'number' && typeof bVal === 'number') {
                const result = aVal - bVal
                return sortDirection === 'asc' ? result : -result
            }

            return 0
        })}
          
          <div class="divide-y">
            {#each sortedPeers as peer}
              <div class="p-4 flex flex-col sm:flex-row sm:items-center justify-between gap-4 hover:bg-muted/10 transition-colors">
                 <div class="flex items-center gap-3">
                    <div class="w-2 h-2 rounded-full flex-shrink-0 {peer.status === 'online' ? 'bg-green-500' : 'bg-gray-400'}"></div>
                    <div>
                       <div class="flex items-center gap-2">
                         <span class="font-medium">{peer.nickname || 'Anonymous'}</span>
                         <Badge variant="outline" class="text-xs py-0 h-5">â­ {peer.reputation?.toFixed(1) || '0.0'}</Badge>
                       </div>
                       <p class="text-xs text-muted-foreground font-mono mt-0.5">{peer.address.substring(0, 20)}...</p>
                    </div>
                 </div>
                 
                 <div class="flex items-center gap-6 text-sm text-muted-foreground">
                    <div class="text-right hidden md:block">
                       <p class="text-xs uppercase">Data</p>
                       <p class="font-medium text-foreground">{formatSize(peer.totalSize)}</p>
                    </div>
                    <div class="text-right">
                       <p class="text-xs uppercase">Location</p>
                       <p class="font-medium text-foreground">{peer.location || 'Unknown'}</p>
                    </div>
                    <div class="text-right hidden md:block">
                       <p class="text-xs uppercase">Shared</p>
                       <p class="font-medium text-foreground">{peer.sharedFiles || 0}</p>
                    </div>
                    <div class="text-right hidden md:block">
                       <p class="text-xs uppercase">Last Seen</p>
                       <p class="font-medium text-foreground">{formatPeerDate(peer.lastSeen)}</p>
                    </div>
                    <Button size="sm" variant="ghost" class="text-red-500 hover:text-red-600 hover:bg-red-50" on:click={() => disconnectFromPeer(peer.address)}>
                       Disconnect
                    </Button>
                 </div>
              </div>
            {/each}
            {#if sortedPeers.length === 0}
               <div class="p-8 text-center text-muted-foreground">
                 No peers connected. Try running discovery.
               </div>
            {/if}
          </div>
        </Card>
      </div>

    <!-- DIAGNOSTICS TAB -->
    {:else if activeTab === 'diagnostics'}
      <div class="space-y-6">
        
        <!-- Detailed Node Status (Read-Only) -->
        <Card class="p-6">
          <h3 class="text-lg font-semibold mb-4 flex items-center gap-2">
            <Activity class="h-5 w-5 text-primary" />
            Node Status
          </h3>
          <div class="grid grid-cols-1 md:grid-cols-2 gap-x-8 gap-y-4">
            
            <div class="space-y-1">
              <span class="text-xs text-muted-foreground uppercase">Blockchain Address</span>
              <div class="bg-muted/50 border border-border px-3 py-2 rounded-md text-sm font-mono text-foreground flex items-center">
                <span class="flex-1 truncate" title={walletAddress}>{walletAddress || 'Unknown'}</span>
                {#if walletAddress}
                  <Button variant="outline" size="icon" class="h-8 w-8 ml-2" on:click={() => copy(walletAddress)}>
                    <Clipboard class="h-4 w-4" />
                  </Button>
                {/if}
              </div>
            </div>

            <div class="space-y-1">
              <span class="text-xs text-muted-foreground uppercase">Chain ID</span>
              <div class="bg-muted/50 border border-border px-3 py-2 rounded-md text-sm font-mono text-foreground flex items-center">
                {chainId || 'Unknown'}
              </div>
            </div>

            <div class="space-y-1">
              <span class="text-xs text-muted-foreground uppercase">Network Port</span>
              <div class="bg-muted/50 border border-border px-3 py-2 rounded-md text-sm font-mono text-foreground flex items-center">
                {dhtPort}
              </div>
            </div>

            <div class="space-y-1">
              <span class="text-xs text-muted-foreground uppercase">Connected Peers (Geth / DHT)</span>
              <div class="bg-muted/50 border border-border px-3 py-2 rounded-md text-sm font-mono text-foreground flex items-center">
                {peerCount} / {dhtPeerCount}
              </div>
            </div>

            <div class="space-y-1 md:col-span-2">
              <span class="text-xs text-muted-foreground uppercase">Peer ID</span>
              <div class="bg-muted/50 border border-border px-3 py-2 rounded-md text-sm font-mono text-foreground flex items-center">
                <span class="flex-1 truncate" title={dhtPeerId}>{dhtPeerId || 'Not Connected'}</span>
                {#if dhtPeerId}
                  <Button variant="outline" size="icon" class="h-8 w-8 ml-2" on:click={() => copy(dhtPeerId)}>
                    <Clipboard class="h-4 w-4" />
                  </Button>
                {/if}
              </div>
            </div>

            <div class="space-y-1 md:col-span-2">
              <span class="text-xs text-muted-foreground uppercase">Bootstrap Node</span>
              <div class="bg-muted/50 border border-border px-3 py-2 rounded-md text-sm font-mono text-foreground flex items-center">
                <span class="flex-1 truncate" title={dhtBootstrapNode}>{dhtBootstrapNode || 'None'}</span>
                {#if dhtBootstrapNode}
                  <Button variant="outline" size="icon" class="h-8 w-8 ml-2" on:click={() => copy(dhtBootstrapNode)}>
                    <Clipboard class="h-4 w-4" />
                  </Button>
                {/if}
              </div>
            </div>

            <div class="space-y-1">
              <span class="text-xs text-muted-foreground uppercase">Reachability</span>
              <div class="flex items-center gap-2 h-10">
                <Badge class={reachabilityBadgeClass(dhtHealth?.reachability)}>
                  {formatReachabilityState(dhtHealth?.reachability)}
                </Badge>
                <span class="text-sm text-muted-foreground">
                  ({formatNatConfidence(dhtHealth?.reachabilityConfidence)})
                </span>
              </div>
            </div>

          </div>
        </Card>

        <!-- Blockchain Node Logs Status -->
        <div class="space-y-2">
          <h3 class="text-sm font-medium text-muted-foreground uppercase">Blockchain Logs Status</h3>
          <GethStatusCard dataDir="./bin/geth-data" logLines={20} refreshIntervalMs={10000} />
        </div>

        <!-- Detailed Reachability Info -->
        <Card class="p-5">
           <h3 class="font-semibold mb-4">Network Reachability Details</h3>
           <div class="grid grid-cols-1 md:grid-cols-2 gap-6 text-sm">
             <div class="space-y-2">
               <div class="flex justify-between border-b pb-1">
                 <span class="text-muted-foreground">Current State</span>
                 <span class="font-medium">{formatReachabilityState(dhtHealth?.reachability)}</span>
               </div>
               <div class="flex justify-between border-b pb-1">
                 <span class="text-muted-foreground">Confidence</span>
                 <span class="font-medium">{formatNatConfidence(dhtHealth?.reachabilityConfidence)}</span>
               </div>
               <div class="flex justify-between border-b pb-1">
                 <span class="text-muted-foreground">Public Probe</span>
                 <span class="font-medium">{formatNatTimestamp(dhtHealth?.lastProbeAt)}</span>
               </div>
             </div>
             <div class="space-y-2">
               {#if dhtHealth?.observedAddrs && dhtHealth.observedAddrs.length > 0}
                 <p class="text-xs uppercase text-muted-foreground mb-1">Observed Public Addresses</p>
                 <div class="flex flex-wrap gap-2">
                   {#each dhtHealth.observedAddrs as addr}
                      <button class="font-mono text-xs border rounded px-2 py-1 bg-muted/20 hover:bg-muted/40 text-left truncate max-w-full" on:click={() => copyObservedAddr(addr)}>
                        {addr}
                      </button>
                   {/each}
                 </div>
               {:else}
                 <p class="text-muted-foreground italic">No public addresses observed.</p>
               {/if}
             </div>
           </div>
           
           {#if dhtHealth?.reachabilityHistory}
              <div class="mt-4 pt-4 border-t">
                 <p class="text-xs uppercase text-muted-foreground mb-2">Reachability History</p>
                 <div class="space-y-1">
                    {#each dhtHealth.reachabilityHistory.slice(0, 3) as item}
                       <div class="text-xs flex gap-2">
                          <span class="text-muted-foreground">{formatNatTimestamp(item.timestamp)}</span>
                          <span class="font-medium">{formatReachabilityState(item.state)}</span>
                       </div>
                    {/each}
                 </div>
              </div>
           {/if}
        </Card>

        <!-- DHT Events -->
        <div class="space-y-2">
          <h3 class="font-semibold text-sm uppercase text-muted-foreground">DHT Network Events</h3>
          <div class="h-48 overflow-auto rounded-md border border-border bg-muted/40 p-3 font-mono text-xs">
            {#if dhtEvents.length > 0}
              {#each dhtEvents as event}
                <p class="whitespace-pre-wrap border-b border-border/50 pb-1 mb-1 last:border-0">{event}</p>
              {/each}
            {:else}
               <p class="text-muted-foreground italic">No events recorded.</p>
            {/if}
          </div>
        </div>

      </div>
    <!-- Relay TAB -->
    {:else if activeTab === 'relay'}
      <div class="space-y-6">
        <!-- Relay Server Control -->
        <Card class="p-6">
          <div class="flex items-start justify-between mb-4">
            <div class="flex items-center gap-3">
              <Server class="w-6 h-6 text-blue-600" />
              <div>
                <h3 class="text-lg font-semibold">{$t('relay.server.title')}</h3>
                <p class="text-sm text-muted-foreground">{$t('relay.server.subtitle')}</p>
              </div>
            </div>
            <div
              class="px-3 py-1 rounded-full text-xs font-semibold"
              class:bg-green-100={relayServerRunning}
              class:text-green-800={relayServerRunning}
              class:bg-gray-100={!relayServerRunning}
              class:text-gray-800={!relayServerRunning}
            >
              {relayServerRunning ? $t('relay.server.running') : $t('relay.server.stopped')}
            </div>
          </div>

          <div class="space-y-4">
            <div class="bg-blue-50 border border-blue-200 rounded-lg p-4">
              <p class="text-sm text-blue-900">
                {$t('relay.server.description')}
              </p>
              <ul class="mt-2 text-sm text-blue-800 space-y-1">
                <li>- {$t('relay.server.benefit1')}</li>
                <li>- {$t('relay.server.benefit2')}</li>
                <li>- {$t('relay.server.benefit3')}</li>
              </ul>
            </div>

            <div class="space-y-2">
              <Label for="relay-alias">{$t('relay.server.aliasLabel')}</Label>
              <Input
                id="relay-alias"
                type="text"
                bind:value={relayServerAlias}
                on:blur={saveRelayServerAlias}
                placeholder={$t('relay.server.aliasPlaceholder')}
                maxlength="50"
                class="w-full"
              />
              <p class="text-xs text-muted-foreground">
                {$t('relay.server.aliasHint')}
              </p>
            </div>

            {#if dhtStatus === 'disconnected'}
              <div class="bg-yellow-50 border border-yellow-200 rounded-lg p-3">
                <p class="text-sm font-semibold text-yellow-900">
                  {$t('relay.server.dhtNotRunning')}
                </p>
                <p class="text-xs text-yellow-700 mt-1">
                  {$t('relay.server.dhtNotRunningHint')}
                </p>
              </div>
            {/if}

            <div class="flex items-center justify-between">
              <Button
                on:click={toggleRelayServer}
                disabled={relayServerToggling || dhtStatus === 'disconnected'}
                variant={relayServerEnabled ? 'destructive' : 'default'}
                class="w-full"
              >
                {#if relayServerToggling}
                  {relayServerEnabled ? $t('relay.server.disabling') : $t('relay.server.enabling')}
                {:else if relayServerEnabled}
                  <WifiOff class="w-4 h-4 mr-2" />
                  {$t('relay.server.disable')}
                {:else}
                  <Wifi class="w-4 h-4 mr-2" />
                  {$t('relay.server.enable')}
                {/if}
              </Button>
            </div>

            {#if relayServerRunning}
              <div class="bg-green-50 border border-green-200 rounded-lg p-4">
                <p class="text-sm font-semibold text-green-900">
                  {$t('relay.server.activeMessage')}
                </p>
                {#if relayServerAlias.trim()}
                  <div class="mt-2 flex items-center gap-2">
                    <span class="text-xs text-green-700">{$t('relay.server.broadcastingAs')}</span>
                    <span class="text-sm font-bold text-green-900 bg-green-100 px-2 py-1 rounded">
                      {relayServerAlias}
                    </span>
                  </div>
                {/if}
                <p class="text-xs text-green-700 mt-2">
                  {$t('relay.server.earningReputation')}
                </p>
              </div>
            {/if}
          </div>
        </Card>

        <!-- Relay Status -->
        <Card class="p-6">
          <div class="flex items-start justify-between mb-4">
            <div>
              <h3 class="text-lg font-semibold">Relay Status</h3>
              <p class="text-sm text-muted-foreground">AutoRelay and active relay health</p>
            </div>
            <Badge variant={$settings.enableAutorelay ? 'default' : 'secondary'} class={$settings.enableAutorelay ? 'bg-green-100 text-green-700' : ''}>
              {$settings.enableAutorelay ? $t('network.dht.relay.enabled') : $t('network.dht.relay.disabled')}
            </Badge>
          </div>

          <div class="space-y-4">
            <div class="flex items-center gap-2">
              <input
                type="checkbox"
                id="enable-autorelay-network"
                checked={$settings.enableAutorelay}
                on:change={handleAutorelayToggle}
                disabled={autorelayToggling}
              />
              <Label for="enable-autorelay-network" class="cursor-pointer">
                {$t('relay.client.enableAutorelay')}
              </Label>
            </div>

            {#if $settings.enableAutorelay}
              <div class="bg-purple-50 border border-purple-200 rounded-lg p-3">
                <p class="text-sm text-purple-900">
                  <strong>{$t('relay.client.howItWorks')}</strong>
                </p>
                <p class="text-xs text-purple-700 mt-1">
                  {$t('relay.client.description')}
                </p>
              </div>
            {/if}

            {#if dhtStatus === 'connected' && dhtHealth}
               <div>
                 <RelayErrorMonitor />
               </div>
            {:else}
               <div class="text-xs text-muted-foreground italic">Connect to DHT to view relay status.</div>
            {/if}
          </div>
        </Card>

        <Card class="p-6">
          <h3 class="text-lg font-semibold mb-4 flex items-center gap-2">
            <RefreshCw class="h-5 w-5" />
            Health Check Configuration
          </h3>

          <div class="space-y-4">
            <div class="flex items-center justify-between">
              <div>
                <Label class="text-sm font-medium">Health Check Status</Label>
                <p class="text-xs text-muted-foreground mt-1">
                  {isHealthCheckRunning ? 'Automatically checking relay health' : 'Health checks paused'}
                </p>
              </div>
              <button
                on:click={toggleHealthChecks}
                class="px-4 py-2 rounded-md text-sm font-medium transition-colors {isHealthCheckRunning
                  ? 'bg-green-100 text-green-700 hover:bg-green-200'
                  : 'bg-gray-100 text-gray-700 hover:bg-gray-200'}"
              >
                {isHealthCheckRunning ? 'Running' : 'Stopped'}
              </button>
            </div>

            <div class="space-y-2">
              <Label for="health-check-interval" class="text-sm font-medium">
                Check Interval (seconds)
              </Label>
              <div class="flex items-center gap-3">
                <Input
                  id="health-check-interval"
                  type="number"
                  min="10"
                  max="300"
                  step="5"
                  bind:value={healthCheckInterval}
                  class="flex-1"
                  disabled={!isHealthCheckRunning}
                />
                <button
                  on:click={updateHealthCheckInterval}
                  disabled={!isHealthCheckRunning}
                  class="px-4 py-2 bg-primary text-primary-foreground rounded-md text-sm font-medium hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  Apply
                </button>
              </div>
              <p class="text-xs text-muted-foreground">
                How often to check relay connectivity (10-300 seconds). Lower values detect issues faster but use more resources.
              </p>
            </div>

            <div class="pt-4 border-t border-border">
              <div class="flex items-center justify-between text-sm">
                <span class="text-muted-foreground">Next check in:</span>
                <span class="font-medium">
                  {isHealthCheckRunning ? `~${healthCheckInterval}s` : 'N/A'}
                  </span>
                </div>
              </div>
            </div>
          </Card>

        {#if dhtHealth}
          <Card class="p-6">
            <div class="flex items-center justify-between mb-4">
              <div>
                <p class="text-xs uppercase text-muted-foreground">Relay status</p>
                <h3 class="text-lg font-semibold text-foreground">Active relay snapshot</h3>
              </div>
              <div class="px-3 py-1 rounded-full text-xs font-semibold"
                class:bg-green-100={dhtHealth.autorelayEnabled}
                class:text-green-800={dhtHealth.autorelayEnabled}
                class:bg-gray-100={!dhtHealth.autorelayEnabled}
                class:text-gray-800={!dhtHealth.autorelayEnabled}
              >
                {dhtHealth.autorelayEnabled ? $t('network.dht.relay.enabled') : $t('network.dht.relay.disabled')}
              </div>
            </div>

            <div class="grid gap-3 md:grid-cols-2 lg:grid-cols-3">
              <div class="bg-muted/40 rounded-lg p-3 border border-muted/40">
                <p class="text-xs uppercase text-muted-foreground">Active relay</p>
                <p class="text-sm font-mono mt-1 break-all">{dhtHealth.activeRelayPeerId ?? $t('network.dht.relay.noPeer')}</p>
                <p class="text-xs text-muted-foreground mt-1">
                  Status: {dhtHealth.relayReservationStatus ?? $t('network.dht.relay.pending')}
                </p>
              </div>
              <div class="bg-muted/40 rounded-lg p-3 border border-muted/40">
                <p class="text-xs uppercase text-muted-foreground">Pool</p>
                <p class="text-sm font-medium mt-1">
                  {dhtHealth.totalRelaysInPool ?? 0} total / {dhtHealth.activeRelayCount ?? 0} active
                </p>
                <p class="text-xs text-muted-foreground mt-1">Renewals: {dhtHealth.reservationRenewals ?? 0}</p>
              </div>
              <div class="bg-muted/40 rounded-lg p-3 border border-muted/40">
                <p class="text-xs uppercase text-muted-foreground">Health</p>
                <p class="text-sm font-medium mt-1">
                  {#if typeof dhtHealth.relayHealthScore === 'number'}
                    {(dhtHealth.relayHealthScore * 100).toFixed(0)}%
                  {:else}
                    N/A
                  {/if}
                </p>
                <p class="text-xs text-muted-foreground mt-1">
                  Last renewal: {dhtHealth.lastReservationRenewal ? formatNatTimestamp(dhtHealth.lastReservationRenewal) : $t('network.dht.health.never')}
                </p>
              </div>
            </div>
          </Card>
        {/if}

        <Card class="p-6">
          <div class="flex items-center justify-between mb-4">
            <h3 class="text-lg font-semibold text-foreground">Relay Error Log</h3>
            <Button size="sm" variant="outline" on:click={clearRelayErrors}>
              Clear
            </Button>
          </div>
          {#if dedupRelayErrors.length > 0}
            <div class="max-h-72 overflow-y-auto space-y-2">
              {#each dedupRelayErrors as error}
                <div class="border-l-4 border-red-500 pl-3 py-2 bg-red-50 rounded">
                  <div class="flex items-center justify-between">
                    <span class="text-xs font-semibold text-red-700">{error.type}</span>
                    <span class="text-xs text-muted-foreground">{formatRelayErrorTimestamp(error.timestamp)}</span>
                  </div>
                  <p class="text-sm text-gray-800 break-words">{error.message}</p>
                  <p class="text-xs text-gray-600 mt-1">
                    Relay {error.relayId} {#if typeof error.retryCount === 'number'}Retry {error.retryCount}{/if}
                  </p>
                </div>
              {/each}
            </div>
            {:else}
              <p class="text-sm text-muted-foreground">No relay errors recorded.</p>
            {/if}
        </Card>
      </div>
    {/if}

  </div>
</div>
