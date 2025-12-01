
<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import Button from '$lib/components/ui/button.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import PeerMetrics from '$lib/components/PeerMetrics.svelte'
  import GeoDistributionCard from '$lib/components/GeoDistributionCard.svelte'
  import GethStatusCard from '$lib/components/GethStatusCard.svelte'
  import { peers, networkStats, networkStatus, userLocation, settings } from '$lib/stores'
  import { normalizeRegion, UNKNOWN_REGION_ID } from '$lib/geo'
  import { Users, HardDrive, Activity, RefreshCw, UserPlus, Signal, Server, Wifi, UserMinus, Square, Play, Download, AlertCircle } from 'lucide-svelte'
  import { onMount, onDestroy } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import { listen } from '@tauri-apps/api/event'
  import { dhtService } from '$lib/dht'
  import { getStatus as fetchGethStatus, type GethStatus } from '$lib/services/gethService'
  import { resetConnectionAttempts } from '$lib/dhtHelpers'
  import type { DhtHealth, NatConfidence, NatReachabilityState } from '$lib/dht'
  import { Clipboard } from "lucide-svelte"
  import { t } from 'svelte-i18n';
  import { showToast } from '$lib/toast';
  import DropDown from '$lib/components/ui/dropDown.svelte'
  import { SignalingService } from '$lib/services/signalingService';
  import { createWebRTCSession } from '$lib/services/webrtcService';
  import { peerDiscoveryStore, startPeerEventStream, type PeerDiscovery } from '$lib/services/peerEventService';
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
  
  let discoveryRunning = false
  let newPeerAddress = ''
  let sortBy: 'reputation' | 'sharedFiles' | 'totalSize' | 'nickname' | 'location' | 'joinDate' | 'lastSeen' | 'status' = 'reputation'
  let sortDirection: 'asc' | 'desc' = 'desc'
  let currentPage = 1
  let peersPerPage = 5
  let discoveryCurrentPage = 1
  let discoveryPerPage = 5

  const UNKNOWN_DISTANCE = 1_000_000;

  $: if (sortBy || sortDirection) {
    // Reset to page 1 when sorting changes
    currentPage = 1
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
      nickname: 'asc'         // A → Z first
    }
    sortDirection = defaults[sortBy]
  }
  
  // Chiral Network Node variables (status only)
  let isGethRunning = false
  let isGethInstalled = false
  let isStartingNode = false
  let isDownloading = false
  let isCheckingGeth = false  // Start as false, will be set to true only when actually checking
  let downloadProgress = {
    downloaded: 0,
    total: 0,
    percentage: 0,
    status: ''
  }
  let downloadError = ''
  let peerCount = 0
  let peerCountInterval: ReturnType<typeof setInterval> | undefined
  let chainId = 98765 // Default, will be fetched from backend
  let nodeAddress = ''
  let copiedNodeAddr = false
  
  // DHT variables
  let dhtStatus: 'disconnected' | 'connecting' | 'connected' = 'disconnected'
  let dhtPeerId: string | null = null
  let dhtPort = 4001
  let dhtBootstrapNodes: string[] = []
  let dhtBootstrapNode = 'Loading bootstrap nodes...'
  let dhtEvents: string[] = []
  let dhtPeerCount = 0
  let dhtHealth: DhtHealth | null = null
  let dhtError: string | null = null
  let connectionAttempts = 0
  let dhtPollInterval: number | undefined
  let natStatusUnlisten: (() => void) | null = null
  let lastNatState: NatReachabilityState | null = null
  let lastNatConfidence: NatConfidence | null = null
  let cancelConnection = false

  // Always preserve connections - no unreliable time-based detection
  
  // WebRTC and Signaling variables
  let signaling: SignalingService;
  let webrtcSession: ReturnType<typeof createWebRTCSession> | null = null;
  // let discoveredPeers: string[] = [];
  let webDiscoveredPeers: string[] = [];
  let discoveredPeerEntries: PeerDiscovery[] = [];
  let peerDiscoveryUnsub: (() => void) | null = null;
  let stopPeerEvents: (() => void) | null = null;
  let signalingConnected = false;

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
  let copiedPeerId = false
  let copiedBootstrap = false
  let copiedListenAddr: string | null = null
  let publicMultiaddrs: string[] = []

  // Fetch public multiaddresses (non-loopback)
  async function fetchPublicMultiaddrs() {
    try {
      const addrs = await invoke<string[]>('get_multiaddresses')
      publicMultiaddrs = addrs
    } catch (e) {
      errorLogger.networkError(`Failed to get multiaddresses: ${e instanceof Error ? e.message : String(e)}`);
      publicMultiaddrs = []
    }
  }

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

  function formatPeerTimestamp(ms?: number): string {
    if (!ms) return tr('network.dht.health.never')
    return new Date(ms).toLocaleString()
  }

  function formatHealthTimestamp(epoch: number | null): string {
    if (!epoch) return tr('network.dht.health.never')
    return new Date(epoch * 1000).toLocaleString()
  }

  function formatHealthMessage(value: string | null): string {
    return value ?? tr('network.dht.health.none')
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
          }
        } catch (error) {
          errorLogger.networkError(`Failed to refresh NAT status: ${error instanceof Error ? error.message : String(error)}`);
        }
      })
    } catch (error) {
      errorLogger.networkError(`Failed to subscribe to NAT status updates: ${error instanceof Error ? error.message : String(error)}`);
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
    
    try {
      dhtError = null
      cancelConnection = false
      
      // Check if DHT is already running in backend
      const isRunning = await invoke<boolean>('is_dht_running').catch(() => false)
      
      if (isRunning) {
        // DHT is already running in backend, sync the frontend state immediately
        const backendPeerId = await invoke<string | null>('get_dht_peer_id')
        const peerCount = await invoke<number>('get_dht_peer_count').catch(() => 0)
        
        if (backendPeerId) {
          dhtPeerId = backendPeerId
          dhtService.setPeerId(backendPeerId)
          dhtPeerCount = peerCount
          dhtEvents = [...dhtEvents, `✓ DHT already running with peer ID: ${backendPeerId.slice(0, 16)}...`]
          
          // Get health snapshot
          const health = await dhtService.getHealth()
          if (health) {
            dhtHealth = health
            dhtPeerCount = health.peerCount
          }

          // Set status based on peer count
          dhtStatus = dhtPeerCount > 0 ? 'connected' : 'connecting'
          if (dhtPeerCount > 0) {
            dhtEvents = [...dhtEvents, `✓ Connected to ${dhtPeerCount} peer(s)`]
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
        dhtEvents = [...dhtEvents, '⚠ Connection cancelled by user']
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
      })
      dhtPeerId = peerId
      dhtService.setPeerId(peerId)
      dhtEvents = [...dhtEvents, `✓ DHT started with peer ID: ${peerId.slice(0, 16)}...`]
      
      // Try to connect to bootstrap nodes
      let connectionSuccessful = false

      if (dhtBootstrapNodes.length > 0) {
        dhtEvents = [...dhtEvents, `[Attempt ${connectionAttempts}] Connecting to ${dhtBootstrapNodes.length} bootstrap node(s)...`]
        
        // Add another small delay to show the connection attempt
        await new Promise(resolve => setTimeout(resolve, 1000))
        
        // Check if user cancelled during connection attempt
        if (cancelConnection) {
          await stopDht()
          dhtEvents = [...dhtEvents, '⚠ Connection cancelled by user']
          return
        }
        
        try {
          // Try connecting to the first available bootstrap node
          await dhtService.connectPeer(dhtBootstrapNodes[0])
          connectionSuccessful = true
          dhtEvents = [...dhtEvents, `✓ Connection initiated to bootstrap nodes (waiting for handshake...)`]
          
          // Poll for actual connection after a delay
          setTimeout(async () => {
            const dhtPeerCountResult = await invoke('get_dht_peer_count') as number
            if (dhtPeerCountResult > 0) {
              dhtEvents = [...dhtEvents, `✓ Successfully connected! Peers: ${dhtPeerCountResult}`]
            } else {
              dhtEvents = [...dhtEvents, `⚠ Connection pending... (bootstrap nodes may be unreachable)`]
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
            dhtEvents = [...dhtEvents, `⚠ Bootstrap connection failed but DHT is operational`]
            dhtEvents = [...dhtEvents, `ℹ Other nodes can connect to you at: /ip4/YOUR_IP/tcp/${dhtPort}/p2p/${dhtPeerId?.slice(0, 16)}...`]
            dhtEvents = [...dhtEvents, `💡 To connect with others, share your connection address above`]
          } else {
            errorMessage = 'Unknown connection error - running in standalone mode'
            connectionSuccessful = true
          }
          
          if (!connectionSuccessful) {
            dhtError = errorMessage
            dhtEvents = [...dhtEvents, `✗ Connection failed: ${errorMessage}`]
          } else {
            dhtEvents = [...dhtEvents, `⚠ ${errorMessage}`]
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
        dhtEvents = [...dhtEvents, `✗ Port conflict detected on ${dhtPort}`]
        dhtEvents = [...dhtEvents, `💡 Try clicking "Stop DHT" first, or change the port number`]
      } else if (errorMessage.includes('already running')) {
        errorMessage = 'DHT is already running. Try stopping it first.'
        dhtEvents = [...dhtEvents, `⚠ DHT already running - click "Stop DHT" to restart`]
      }
      
      dhtError = errorMessage
      dhtEvents = [...dhtEvents, `✗ Failed to start DHT: ${errorMessage}`]
    }
  }

  
  let peerRefreshCounter = 0;

  function startDhtPolling() {
    // If already polling, don't start another one
    if (dhtPollInterval !== undefined) {
      return
    }

    dhtPollInterval = setInterval(async () => {
      try {
        // Only call getEvents if running in Tauri mode
        // Note: getEvents is not available in the current DhtService implementation
        const events: any[] = []
        if (events.length > 0) {
          const formattedEvents = events.map(event => {
            if (event.peerDisconnected) {
              return `✗ Peer disconnected: ${event.peerDisconnected.peer_id.slice(0, 12)}... (Reason: ${event.peerDisconnected.cause})`
            } else if (event.peerConnected) {
              return `✓ Peer connected: ${event.peerConnected.slice(0, 12)}...`
            } else if (event.peerDiscovered) {
              return `ℹ Peer discovered: ${event.peerDiscovered.slice(0, 12)}...`
            } else if (event.error) {
              return `✗ Error: ${event.error}`
            }
            return JSON.stringify(event) // Fallback for other event types
          })
          dhtEvents = [...dhtEvents, ...formattedEvents].slice(-10)
        }

        let peerCount = dhtPeerCount
        const health = await dhtService.getHealth()
        if (health) {
          dhtHealth = health
          peerCount = health.peerCount
          // Fetch public multiaddresses
          await fetchPublicMultiaddrs()
          dhtPeerCount = peerCount
          lastNatState = health.reachability
          lastNatConfidence = health.reachabilityConfidence
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
            dhtEvents = [...dhtEvents, '⚠ Lost connection to all peers']
          }
        } else {
          if (dhtStatus !== 'connected') {
            dhtStatus = 'connected'
            dhtEvents = [...dhtEvents, `✓ Reconnected to ${peerCount} peer(s)`]
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
    dhtEvents = [...dhtEvents, '⚠ Connection cancelled by user']
    showToast($t('network.dht.connectionCancelled'), 'info')
  }

  async function stopDht() {
    if (!isTauri) {
      dhtStatus = 'disconnected'
      dhtPeerId = null
      dhtError = null
      connectionAttempts = 0
      dhtHealth = null
      copiedListenAddr = null
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
      dhtEvents = [...dhtEvents, `✓ DHT stopped - port ${dhtPort} released`]
      dhtHealth = null
      copiedListenAddr = null
      lastNatState = null
      lastNatConfidence = null
      cancelConnection = false
      
      // Small delay to ensure port is fully released
      await new Promise(resolve => setTimeout(resolve, 500))
    } catch (error) {
      errorLogger.dhtInitError(`Failed to stop DHT: ${error instanceof Error ? error.message : String(error)}`);
      dhtEvents = [...dhtEvents, `✗ Failed to stop DHT: ${error}`]
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
      const isRunning = await invoke<boolean>('is_dht_running').catch(() => false)
      const peerCount = await invoke<number>('get_dht_peer_count').catch(() => 0)
      const peerId = await invoke<string | null>('get_dht_peer_id').catch(() => null)

      // If DHT is running in backend, sync status and start polling
      if (isRunning && peerId) {
        dhtPeerId = peerId
        dhtPeerCount = peerCount
        
        // Update dhtService with the peer ID
        dhtService.setPeerId(peerId)
        
        // Also restore health snapshot
        try {
          const health = await dhtService.getHealth()
          if (health) {
            dhtHealth = health
            lastNatState = health.reachability
            lastNatConfidence = health.reachabilityConfidence
          }
        } catch (healthError) {
          diagnosticLogger.debug('Network', 'Could not fetch health snapshot', { error: healthError instanceof Error ? healthError.message : String(healthError) });
        }
        
        // Set status based on peer count - polling will handle dynamic updates
        dhtStatus = peerCount > 0 ? 'connected' : 'connecting'
        dhtEvents = [...dhtEvents, `✓ DHT restored (${peerCount} peer${peerCount !== 1 ? 's' : ''} connected)`]
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
      dhtEvents = [...dhtEvents, '⚠ Error checking network status']
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
  
  function sendTestMessage() {
    if (!webrtcSession || !webrtcSession.channel || webrtcSession.channel.readyState !== 'open') {
      // showToast('No active WebRTC connection', 'error');
      showToast(tr('toasts.network.noConnection'), 'error');
      return;
    }
    
    const testMessage = `Hello from ${signaling.getClientId()} at ${new Date().toLocaleTimeString()}`;
    try {
      webrtcSession.send(testMessage);
      // showToast('Test message sent: ' + testMessage, 'success');
      showToast(
        tr('toasts.network.messageSent', { values: { message: testMessage } }),
        'success'
      );
    } catch (error) {
      // showToast('Failed to send message: ' + error, 'error');
      showToast(
        tr('toasts.network.sendError', { values: { error: String(error) } }),
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
    } catch (error) {
      errorLogger.networkError(`Failed to check geth status: ${error instanceof Error ? error.message : String(error)}`);
    } finally {
      isCheckingGeth = false
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
    try {
      await invoke('start_geth_node', { dataDir: './bin/geth-data' })
      isGethRunning = true
      startPolling()
    } catch (error) {
      errorLogger.networkError(`Failed to start Chiral node: ${error instanceof Error ? error.message : String(error)}`);
    } finally {
      isStartingNode = false
    }
  }

  async function stopGethNode() {
    if (!isTauri) {
      diagnosticLogger.info('Network', 'Cannot stop Chiral Node in web mode - desktop app required');
      return
    }

    try {
      await invoke('stop_geth_node')
      isGethRunning = false
      if (peerCountInterval) {
        clearInterval(peerCountInterval)
        peerCountInterval = undefined
      }
      peerCount = 0
    } catch (error) {
      errorLogger.networkError(`Failed to stop Chiral node: ${error instanceof Error ? error.message : String(error)}`);
    }
  }
  

  function startPolling() {
    if (peerCountInterval) {
      clearInterval(peerCountInterval)
    }
    fetchPeerCount()
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
      
      // Initialize async operations (preserves connections)
      const initAsync = async () => {
        // Run ALL independent checks in parallel for better performance
        await Promise.all([
          fetchBootstrapNodes(),
          checkGethStatus(),
          syncDhtStatusOnPageLoad(), // DHT check is independent from Geth check
          fetchChainId()
        ])

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
      if (stopPeerEvents) {
        stopPeerEvents()
        stopPeerEvents = null
      }
      if (peerDiscoveryUnsub) {
        peerDiscoveryUnsub()
        peerDiscoveryUnsub = null
      }
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
    if (stopPeerEvents) {
      stopPeerEvents()
      stopPeerEvents = null
    }
    if (peerDiscoveryUnsub) {
      peerDiscoveryUnsub()
      peerDiscoveryUnsub = null
    }
    // Note: We do NOT stop the DHT service here
    // The DHT should persist across page navigations
  })
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-3xl font-bold">{$t('network.title')}</h1>
    <p class="text-muted-foreground mt-2">{$t('network.subtitle')}</p>
  </div>

  <!-- Quick Actions -->
<Card class="p-6 bg-muted/60 border border-muted-foreground/10 rounded-xl shadow-sm mb-6">
  <div class="flex items-center justify-between mb-4">
    <h2 class="text-lg font-semibold text-foreground tracking-tight">{$t('network.quickActions.title')}</h2>
    <Badge variant="outline" class="text-xs text-muted-foreground border-muted-foreground/20 bg-transparent">
      {$t('network.quickActions.badge')}
    </Badge>
  </div>
  <div class="flex flex-wrap gap-4 justify-start items-center">
    <!-- Discover Peers -->
    <Button
      size="lg"
      variant="secondary"
      class="flex items-center gap-2 px-6 py-3 font-semibold text-base rounded-lg shadow-sm border border-primary/10 bg-background hover:bg-secondary/80"
      title={$t('network.quickActions.discoverPeers.tooltip')}
      on:click={async () => {
        if (dhtStatus !== 'connected') await startDht();
        await runDiscovery();
      }}
      disabled={dhtStatus === 'connecting'}
    >
      <RefreshCw class={`h-5 w-5${dhtStatus === 'connecting' ? ' animate-spin' : ''}`} />
      {dhtStatus === 'connecting' ? $t('network.quickActions.discoverPeers.discovering') : $t('network.quickActions.discoverPeers.button')}
    </Button>

    <!-- Add Peer by Address (inline input, condensed) -->
    <div class="flex items-center gap-2 bg-muted/80 border border-muted-foreground/10 rounded-lg px-3 py-2">
      <Input placeholder={$t('network.quickActions.addPeer.placeholder')} bind:value={newPeerAddress} class="w-32 text-sm bg-background border border-muted-foreground/10 rounded" />
      <Button size="sm" variant="secondary" class="rounded" title={$t('network.quickActions.addPeer.tooltip')} on:click={async () => { if (newPeerAddress) { await addConnectedPeer(newPeerAddress); showToast($t('network.quickActions.addPeer.success'), 'success'); newPeerAddress = ''; }}} disabled={!newPeerAddress}>
        <UserPlus class="h-4 w-4" />
      </Button>
    </div>

    <!-- Copy Peer ID -->
    <Button
      size="lg"
      variant="secondary"
      class="flex items-center gap-2 px-6 py-3 font-semibold text-base rounded-lg shadow-sm border border-primary/10 bg-background hover:bg-secondary/80"
      title={dhtPeerId ? $t('network.quickActions.copyPeerId.tooltip') : $t('network.quickActions.copyPeerId.tooltipUnavailable')}
      on:click={async () => {
        if (dhtPeerId) {
          await copy(dhtPeerId);
          showToast($t('network.quickActions.copyPeerId.success'), 'success');
        }
      }}
      disabled={!dhtPeerId}
    >
      <Users class="h-5 w-5" />
      {$t('network.quickActions.copyPeerId.button')}
    </Button>

    <!-- Refresh Status -->
    <Button
      size="lg"
      variant="secondary"
      class="flex items-center gap-2 px-6 py-3 font-semibold text-base rounded-lg shadow-sm border border-primary/10 bg-background hover:bg-secondary/80"
      title={$t('network.quickActions.refreshStatus.tooltip')}
      on:click={async () => {
        await checkGethStatus();
        showToast($t('network.quickActions.refreshStatus.success'), 'success');
      }}
    >
      <Activity class="h-5 w-5" />
      {$t('network.quickActions.refreshStatus.button')}
    </Button>


  </div>
</Card>

  <!-- Chiral Network Node Status Card -->
  <Card class="p-6">
    <div class="flex items-center justify-between mb-4">
      <h2 class="text-lg font-semibold">{$t('network.nodeStatus')}</h2>
      <div class="flex items-center gap-2">
        {#if !isGethInstalled}
          <div class="h-2 w-2 bg-yellow-500 rounded-full"></div>
          <span class="text-sm text-yellow-600">{$t('network.status.notInstalled')}</span>
        {:else if isGethRunning}
          <div class="h-2 w-2 bg-green-500 rounded-full animate-pulse"></div>
          <span class="text-sm text-green-600">{$t('network.status.connected')}</span>
        {:else}
          <div class="h-2 w-2 bg-red-500 rounded-full"></div>
          <span class="text-sm text-red-600">{$t('network.status.disconnected')}</span>
        {/if}
      </div>
    </div>

    <div class="space-y-3">
      {#if !isGethInstalled && !isGethRunning}
        <div class="text-center py-4">
          <Server class="h-12 w-12 text-muted-foreground mx-auto mb-2" />
          <p class="text-sm text-muted-foreground mb-1">
            {isCheckingGeth ? 'Checking...' : 'Geth not installed'}
          </p>
          {#if !isCheckingGeth}
            <p class="text-xs text-muted-foreground mb-3">Download and install the Chiral Network node</p>
          {/if}
          {#if downloadError}
            <div class="bg-red-500/10 border border-red-500/20 rounded-lg p-2 mb-3">
              <div class="flex items-center gap-2 justify-center">
                <AlertCircle class="h-4 w-4 text-red-500 flex-shrink-0" />
                <p class="text-xs text-red-500">{downloadError}</p>
              </div>
            </div>
          {/if}
          {#if !isCheckingGeth}
            <Button on:click={downloadGeth} disabled={isDownloading}>
              <Download class="h-4 w-4 mr-2" />
              Download Geth
            </Button>
          {/if}
        </div>
      {:else if isGethRunning}
        <div class="grid grid-cols-2 gap-4">
          <div class="bg-secondary rounded-lg p-3">
            <p class="text-sm text-muted-foreground">{$t('network.chiralPeers')}</p>
            <p class="text-2xl font-bold">{peerCount}</p>
          </div>
          <div class="bg-secondary rounded-lg p-3">
            <p class="text-sm text-muted-foreground">{$t('network.chainId')}</p>
            <p class="text-2xl font-bold">{chainId}</p>
          </div>
        </div>
        <div class="pt-2">
          <div class="flex items-center justify-between mb-1 gap-2">
            <p class="text-sm text-muted-foreground">{$t('network.nodeAddress')}</p>
            <Button
              variant="outline"
              size="sm"
              class="h-7 px-2"
              on:click={async () => {
                await copy(nodeAddress);
                copiedNodeAddr = true;
                setTimeout(() => (copiedNodeAddr = false), 1200);
              }}
            >
              <Clipboard class="h-3.5 w-3.5 mr-1" />
              {copiedNodeAddr ? $t('network.copied') : $t('network.copy')}
            </Button>
          </div>
          <p class="text-xs font-mono break-all">{nodeAddress}</p>
        </div>
        <Button class="w-full mt-4" variant="outline" on:click={stopGethNode}>
          <Square class="h-4 w-4 mr-2" />
          Stop Node
        </Button>
      {:else}
        <div class="text-center py-8">
          <Server class="h-12 w-12 text-muted-foreground mx-auto mb-2" />
          <p class="text-sm text-muted-foreground mb-3">Chiral Node not running</p>
          <Button on:click={startGethNode} disabled={isStartingNode || isCheckingGeth}>
            {#if isStartingNode}
              <RefreshCw class="h-4 w-4 mr-2 animate-spin" />
              Starting Node...
            {:else}
              <Play class="h-4 w-4 mr-2" />
              Start Node
            {/if}
          </Button>
        </div>
      {/if}
    </div>
  </Card>

  <!-- Geth Node Lifecycle & Bootstrap Health -->
  <GethStatusCard dataDir="./bin/geth-data" logLines={40} refreshIntervalMs={10000} />


  <!-- DHT Network Status Card -->
  <Card class="p-6">
    <div class="flex items-center justify-between mb-4">
      <h2 class="text-lg font-semibold">{$t('network.dht.title')}</h2>
      <div class="flex items-center gap-2">
        {#if dhtStatus === 'connected'}
          <div class="h-2 w-2 bg-green-500 rounded-full animate-pulse"></div>
          <span class="text-sm text-green-600">{$t('network.status.connected')}</span>
        {:else if dhtStatus === 'connecting'}
          <div class="h-2 w-2 bg-yellow-500 rounded-full animate-pulse"></div>
          <span class="text-sm text-yellow-600">{$t('network.status.connecting')}</span>
        {:else}
          <div class="h-2 w-2 bg-red-500 rounded-full"></div>
          <span class="text-sm text-red-600">{$t('network.status.disconnected')}</span>
        {/if}
      </div>
    </div>
    
    <div class="space-y-3">
      {#if dhtStatus === 'disconnected'}
        <div class="text-center py-4">
          <Wifi class="h-12 w-12 text-muted-foreground mx-auto mb-2" />
          <p class="text-sm text-muted-foreground mb-3">{$t('network.dht.notConnected')}</p>
          <div class="px-8 my-4 text-left">
            <Label for="dht-port" class="text-sm">{$t('network.dht.port')}</Label>
            <Input id="dht-port" type="number" bind:value={dhtPort} class="mt-1" />
          </div>
          {#if dhtError}
            <div class="bg-red-500/10 border border-red-500/20 rounded-lg p-3 mb-3 mx-4">
              <p class="text-xs text-red-400 font-medium mb-1">{$t('network.dht.connectionError')}:</p>
              <p class="text-xs text-red-300 font-mono">{dhtError}</p>
            </div>
          {/if}
          <div class="flex gap-2 justify-center">
            <Button on:click={startDht}>
              <Wifi class="h-4 w-4 mr-2" />
              {connectionAttempts > 0 ? $t('network.dht.retry') : $t('network.dht.connect')}
            </Button>
            {#if dhtPeerId}
              <Button variant="outline" on:click={stopDht}>
                <Wifi class="h-4 w-4 mr-2" />
                {$t('network.dht.stop')}
              </Button>
            {/if}
          </div>
          
          {#if dhtEvents.length > 0}
            <div class="mt-4 mx-4">
              <p class="text-xs text-muted-foreground mb-2">{$t('network.dht.log')}:</p>
              <div class="bg-secondary/50 rounded-lg p-2 max-h-32 overflow-y-auto text-left">
                {#each dhtEvents.slice(-5) as event}
                  <p class="text-xs font-mono text-muted-foreground">{event}</p>
                {/each}
              </div>
            </div>
          {/if}
        </div>
      {:else if dhtStatus === 'connecting'}
        <div class="text-center py-4">
          <Wifi class="h-12 w-12 text-yellow-500 mx-auto mb-2 animate-pulse" />
          <p class="text-sm text-muted-foreground">{$t('network.dht.connectingToBootstrap')}</p>
          <p class="text-xs text-muted-foreground mt-1">{dhtBootstrapNode}</p>
          <p class="text-xs text-yellow-500 mt-2">{$t('network.dht.attempt', { values: { connectionAttempts: connectionAttempts } })}</p>
          <div class="mt-4">
            <Button variant="outline" size="sm" on:click={cancelDhtConnection}>
              <Square class="h-4 w-4 mr-2" />
              {$t('network.dht.cancel')}
            </Button>
          </div>
        </div>
      {:else}
        <div class="space-y-3">
          <div class="grid grid-cols-2 gap-4">
            <div class="bg-secondary rounded-lg p-3">
              <p class="text-sm text-muted-foreground">{$t('network.dht.port')}</p>
              <p class="text-2xl font-bold">{dhtPort}</p>
            </div>
            <div class="bg-secondary rounded-lg p-3">
              <p class="text-sm text-muted-foreground">{$t('network.dht.peers')}</p>
              <p class="text-2xl font-bold">{dhtPeerCount}</p>
            </div>
          </div>
          
          <div class="pt-2">
            <div class="flex items-center justify-between mb-1 gap-2">
              <p class="text-sm text-muted-foreground">{$t('network.dht.peerId')}</p>
              <Button
                variant="outline"
                size="sm"
                class="h-7 px-2"
                on:click={async () => {
                  await copy(dhtPeerId);
                  copiedPeerId = true;
                  setTimeout(() => (copiedPeerId = false), 1200);
                }}
                disabled={!dhtPeerId}
              >
                <Clipboard class="h-3.5 w-3.5 mr-1" />
                {copiedPeerId ? $t('network.copied') : $t('network.copy')}
              </Button>
            </div>
            <p class="text-xs font-mono break-all">{dhtPeerId}</p>
          </div>
          
          <div class="pt-2">
            <div class="flex items-center justify-between mb-1 gap-2">
              <p class="text-sm text-muted-foreground">{$t('network.dht.bootstrapNode')}</p>
              <Button
                variant="outline"
                size="sm"
                class="h-7 px-2"
                on:click={async () => {
                  await copy(dhtBootstrapNode);
                  copiedBootstrap = true;
                  setTimeout(() => (copiedBootstrap = false), 1200);
                }}
                disabled={!dhtBootstrapNode}
              >
                <Clipboard class="h-3.5 w-3.5 mr-1" />
                {copiedBootstrap ? $t('network.copied') : $t('network.copy')}
              </Button>
            </div>
            <p class="text-xs font-mono break-all">{dhtBootstrapNode}</p>
          </div>

          {#if publicMultiaddrs && publicMultiaddrs.length > 0}
            <div class="pt-2 space-y-2">
              <p class="text-sm text-muted-foreground">{$t('network.dht.listenAddresses')}</p>
              {#each publicMultiaddrs as fullAddr}
                <div class="bg-muted/40 rounded-lg px-3 py-2">
                  <div class="flex items-center justify-between gap-2">
                    <p class="text-xs font-mono break-all flex-1">{fullAddr}</p>
                    <Button
                      variant="outline"
                      size="sm"
                      class="h-7 px-2 flex-shrink-0"
                      on:click={async () => {
                        await copy(fullAddr)
                        copiedListenAddr = fullAddr
                        setTimeout(() => (copiedListenAddr = null), 1200)
                      }}
                    >
                      <Clipboard class="h-3.5 w-3.5 mr-1" />
                      {copiedListenAddr === fullAddr ? $t('network.copied') : $t('network.copy')}
                    </Button>
                  </div>
                </div>
              {/each}
            </div>
          {:else if dhtStatus === 'connected'}
            <div class="pt-2">
              <p class="text-xs text-muted-foreground">{$t('network.dht.noListenAddresses')}</p>
            </div>
          {/if}

          <div class="pt-4 space-y-4">
            <div class="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
              <div>
                <p class="text-xs uppercase text-muted-foreground">{$t('network.dht.reachability.title')}</p>
                <div class="mt-2 flex items-center gap-2">
                  <Badge class={reachabilityBadgeClass(dhtHealth?.reachability)}>
                    {formatReachabilityState(dhtHealth?.reachability)}
                  </Badge>
                  <span class="text-sm text-muted-foreground">
                    {formatNatConfidence(dhtHealth?.reachabilityConfidence)}
                  </span>
                </div>
              </div>
              <div class="text-sm text-muted-foreground space-y-1 text-right">
                <p>{$t('network.dht.reachability.lastProbe')}: {formatNatTimestamp(dhtHealth?.lastProbeAt ?? null)}</p>
                <p>{$t('network.dht.reachability.lastChange')}: {formatNatTimestamp(dhtHealth?.lastReachabilityChange ?? null)}</p>
                {#if dhtHealth && !dhtHealth.autonatEnabled}
                  <p class="text-xs text-yellow-600">{$t('network.dht.reachability.autonatDisabled')}</p>
                {/if}
              </div>
            </div>

            <div class="grid gap-4 md:grid-cols-2">
              <div>
                <p class="text-xs uppercase text-muted-foreground">{$t('network.dht.reachability.observedAddrs')}</p>
                {#if dhtHealth?.observedAddrs && dhtHealth.observedAddrs.length > 0}
                  <div class="mt-2 flex flex-wrap gap-2">
                    {#each dhtHealth.observedAddrs as addr}
                      <button
                        class="inline-flex max-w-full items-center gap-1 rounded-full bg-muted px-3 py-1 text-xs font-mono hover:bg-muted/80"
                        on:click={() => copyObservedAddr(addr)}
                        type="button"
                      >
                        <span class="break-all text-left">{addr}</span>
                        <Clipboard class="h-3 w-3 flex-shrink-0" />
                      </button>
                    {/each}
                  </div>
                {:else}
                  <p class="mt-2 text-sm text-muted-foreground">{$t('network.dht.reachability.observedEmpty')}</p>
                {/if}
              </div>
              <div>
                <p class="text-xs uppercase text-muted-foreground">{$t('network.dht.reachability.lastError')}</p>
                <p class="mt-2 break-all text-sm text-muted-foreground">{dhtHealth?.lastReachabilityError ?? tr('network.dht.health.none')}</p>
              </div>
            </div>

            <div>
              <p class="text-xs uppercase text-muted-foreground">{$t('network.dht.reachability.history')}</p>
              {#if dhtHealth?.reachabilityHistory && dhtHealth.reachabilityHistory.length > 0}
                <div class="mt-2 overflow-hidden rounded-md border border-muted/40">
                  <table class="min-w-full text-sm">
                    <thead class="bg-muted/50 text-left text-xs uppercase text-muted-foreground">
                      <tr>
                        <th class="px-3 py-2">{$t('network.dht.reachability.timestamp')}</th>
                        <th class="px-3 py-2">{$t('network.dht.reachability.stateLabel')}</th>
                        <th class="px-3 py-2">{$t('network.dht.reachability.summary')}</th>
                      </tr>
                    </thead>
                    <tbody>
                      {#each dhtHealth.reachabilityHistory as item}
                        <tr class="border-t border-muted/30">
                          <td class="px-3 py-2">{formatNatTimestamp(item.timestamp)}</td>
                          <td class="px-3 py-2">{formatReachabilityState(item.state)}</td>
                          <td class="px-3 py-2 break-words text-muted-foreground">{item.summary ?? '—'}</td>
                        </tr>
                      {/each}
                    </tbody>
                  </table>
                </div>
              {:else}
                <p class="mt-2 text-sm text-muted-foreground">{$t('network.dht.reachability.historyEmpty')}</p>
              {/if}
            </div>
          </div>

          <!-- AutoRelay Status -->
          <div class="pt-4 space-y-4 border-t border-muted/40">
            <div class="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
              <div>
                <p class="text-xs uppercase text-muted-foreground">{$t('network.dht.relay.title')}</p>
                <div class="mt-2 flex items-center gap-2">
                  {#if dhtHealth?.autorelayEnabled}
                    <Badge class="bg-green-600">{$t('network.dht.relay.enabled')}</Badge>
                  {:else}
                    <Badge class="bg-gray-500">{$t('network.dht.relay.disabled')}</Badge>
                  {/if}
                  {#if dhtHealth?.activeRelayPeerId}
                    <span class="text-xs font-mono text-muted-foreground">{dhtHealth.activeRelayPeerId.slice(0, 12)}...</span>
                  {/if}
                </div>
              </div>
              {#if dhtHealth?.autorelayEnabled}
                <div class="text-sm text-muted-foreground space-y-1 text-right">
                  {#if dhtHealth?.activeRelayPeerId}
                    <p class="text-green-600">{$t('network.dht.relay.status')}: {dhtHealth.relayReservationStatus ?? $t('network.dht.relay.pending')}</p>
                  {:else}
                    <p class="text-yellow-600">{$t('network.dht.relay.noPeer')}</p>
                  {/if}
                </div>
              {/if}
            </div>

            {#if dhtHealth}
              <div class="bg-muted/40 rounded-lg p-3 border border-dashed border-muted/60">
                <p class="text-xs uppercase text-muted-foreground">{$t('network.dht.relay.timeline')}</p>
                <div class="mt-2 grid gap-3 sm:grid-cols-2">
                  <div>
                    <p class="text-xs text-muted-foreground">{$t('network.dht.relay.enabledAt')}</p>
                    <p class="text-sm font-medium">
                      {dhtHealth.lastAutorelayEnabledAt
                        ? formatHealthTimestamp(dhtHealth.lastAutorelayEnabledAt)
                        : $t('network.dht.relay.neverEnabled')}
                    </p>
                  </div>
                  <div>
                    <p class="text-xs text-muted-foreground">{$t('network.dht.relay.disabledAt')}</p>
                    <p class="text-sm font-medium">
                      {dhtHealth.lastAutorelayDisabledAt
                        ? formatHealthTimestamp(dhtHealth.lastAutorelayDisabledAt)
                        : $t('network.dht.relay.neverDisabled')}
                    </p>
                  </div>
                </div>
              </div>
            {/if}

            {#if dhtHealth?.autorelayEnabled}
              <div class="grid gap-3 md:grid-cols-2">
                <div class="bg-muted/40 rounded-lg p-3">
                  <p class="text-xs uppercase text-muted-foreground">{$t('network.dht.relay.activePeer')}</p>
                  <p class="text-sm font-mono mt-1">{dhtHealth?.activeRelayPeerId ?? $t('network.dht.relay.noPeer')}</p>
                </div>
                <div class="bg-muted/40 rounded-lg p-3">
                  <p class="text-xs uppercase text-muted-foreground">{$t('network.dht.relay.renewals')}</p>
                  <p class="text-sm font-medium mt-1">{dhtHealth?.reservationRenewals ?? 0}</p>
                </div>
                <div class="bg-muted/40 rounded-lg p-3">
                  <p class="text-xs uppercase text-muted-foreground">{$t('network.dht.relay.lastSuccess')}</p>
                  <p class="text-sm font-medium mt-1">{formatNatTimestamp(dhtHealth?.lastReservationSuccess ?? null)}</p>
                </div>
                <div class="bg-muted/40 rounded-lg p-3">
                  <p class="text-xs uppercase text-muted-foreground">{$t('network.dht.relay.evictions')}</p>
                  <p class="text-sm font-medium mt-1">{dhtHealth?.reservationEvictions ?? 0}</p>
                </div>
              </div>
            {/if}
          </div>

          {#if dhtHealth}
            <div class="grid grid-cols-1 md:grid-cols-2 gap-3 pt-3">
              <div class="bg-muted/40 rounded-lg p-3">
                <p class="text-xs uppercase text-muted-foreground">{$t('network.dht.health.lastBootstrap')}</p>
                <p class="text-sm font-medium mt-1">{formatHealthTimestamp(dhtHealth.lastBootstrap)}</p>
              </div>
              <div class="bg-muted/40 rounded-lg p-3">
                <p class="text-xs uppercase text-muted-foreground">{$t('network.dht.health.lastPeer')}</p>
                <p class="text-sm font-medium mt-1">{formatHealthTimestamp(dhtHealth.lastPeerEvent)}</p>
              </div>
              <div class="bg-muted/40 rounded-lg p-3">
                <p class="text-xs uppercase text-muted-foreground">{$t('network.dht.health.lastError')}</p>
                <p class="text-sm font-medium mt-1 break-words w-full">{formatHealthMessage(dhtHealth.lastError)}</p>
                {#if dhtHealth.lastErrorAt}
                  <p class="text-xs text-muted-foreground mt-1">{formatHealthTimestamp(dhtHealth.lastErrorAt)}</p>
                {/if}
              </div>
              <div class="bg-muted/40 rounded-lg p-3">
                <p class="text-xs uppercase text-muted-foreground">{$t('network.dht.health.failures')}</p>
                <p class="text-sm font-medium mt-1">{dhtHealth.bootstrapFailures}</p>
              </div>
            </div>
          {/if}
          
          {#if dhtEvents.length > 0}
            <div class="pt-2">
              <p class="text-sm text-muted-foreground mb-2">{$t('network.dht.recentEvents')}</p>
              <div class="bg-secondary rounded-lg p-2 max-h-32 overflow-y-auto">
                {#each dhtEvents.slice(-5) as event}
                  <p class="text-xs font-mono text-muted-foreground">{event}</p>
                {/each}
              </div>
            </div>
          {/if}
          
          <Button class="w-full" variant="outline" on:click={stopDht}>
            <Square class="h-4 w-4 mr-2" />
            {$t('network.dht.disconnect')}
          </Button>
        </div>
      {/if}
    </div>
  </Card>

  <!-- DCUtR Hole-Punching Card -->
  {#if dhtStatus === 'connected' && dhtHealth}
    <Card class="p-6">
      <h3 class="text-lg font-semibold mb-4">{$t('network.dht.dcutr.title')}</h3>
      <p class="text-sm text-muted-foreground mb-4">{$t('network.dht.dcutr.description')}</p>

      <div class="grid gap-4 md:grid-cols-3">
        <div>
          <p class="text-xs uppercase text-muted-foreground">{$t('network.dht.dcutr.attempts')}</p>
          <p class="mt-1 text-2xl font-bold">{dhtHealth.dcutrHolePunchAttempts ?? 0}</p>
        </div>
        <div>
          <p class="text-xs uppercase text-muted-foreground">{$t('network.dht.dcutr.successes')}</p>
          <p class="mt-1 text-2xl font-bold text-emerald-600 dark:text-emerald-400">{dhtHealth.dcutrHolePunchSuccesses ?? 0}</p>
        </div>
        <div>
          <p class="text-xs uppercase text-muted-foreground">{$t('network.dht.dcutr.failures')}</p>
          <p class="mt-1 text-2xl font-bold text-rose-600 dark:text-rose-400">{dhtHealth.dcutrHolePunchFailures ?? 0}</p>
        </div>
      </div>

      <div class="mt-4 pt-4 border-t border-muted/40">
        <div class="grid gap-4 md:grid-cols-2">
          <div>
            <p class="text-xs uppercase text-muted-foreground">{$t('network.dht.dcutr.successRate')}</p>
            <p class="mt-1 text-lg font-semibold">
              {#if dhtHealth.dcutrHolePunchAttempts > 0}
                {((dhtHealth.dcutrHolePunchSuccesses / dhtHealth.dcutrHolePunchAttempts) * 100).toFixed(1)}%
              {:else}
                —
              {/if}
            </p>
          </div>
          <div>
            <p class="text-xs uppercase text-muted-foreground">{$t('network.dht.dcutr.enabled')}</p>
            <Badge class={dhtHealth.dcutrEnabled ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-300' : 'bg-muted text-muted-foreground'}>
              {dhtHealth.dcutrEnabled ? $t('network.dht.dcutr.enabled') : $t('network.dht.dcutr.disabled')}
            </Badge>
          </div>
        </div>
      </div>

      <div class="mt-4 text-sm text-muted-foreground space-y-1">
        <p>{$t('network.dht.dcutr.lastSuccess')}: {formatNatTimestamp(dhtHealth.lastDcutrSuccess ?? null)}</p>
        <p>{$t('network.dht.dcutr.lastFailure')}: {formatNatTimestamp(dhtHealth.lastDcutrFailure ?? null)}</p>
      </div>
    </Card>
  {/if}

  <!-- Smart Peer Selection Metrics -->
  {#if dhtStatus === 'connected'}
    <PeerMetrics />
  {/if}
  
  <!-- Network Statistics Cards -->
  <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
    <Card class="p-4">
      <div class="flex items-center gap-3">
        <div class="p-2 bg-green-500/10 rounded-lg">
          <Signal class="h-5 w-5 text-green-500" />
        </div>
        <div>
          <p class="text-sm text-muted-foreground">{$t('network.networkStatus')}</p>
          <p class="text-xl font-bold capitalize">{$networkStatus}</p>
        </div>
      </div>
    </Card>
    
    <Card class="p-4">
      <div class="flex items-center gap-3">
        <div class="p-2 bg-blue-500/10 rounded-lg">
          <Users class="h-5 w-5 text-blue-500" />
        </div>
        <div>
          <p class="text-sm text-muted-foreground">{$t('network.dhtPeers')}</p>
          <p class="text-xl font-bold">{dhtStatus === 'connected' ? dhtPeerCount : 0}</p>
        </div>
      </div>
    </Card>
    
    <Card class="p-4">
      <div class="flex items-center gap-3">
        <div class="p-2 bg-purple-500/10 rounded-lg">
          <HardDrive class="h-5 w-5 text-purple-500" />
        </div>
        <div>
          <p class="text-sm text-muted-foreground">{$t('network.networkSize')}</p>
          <p class="text-xl font-bold">{formatSize($networkStats.networkSize)}</p>
        </div>
      </div>
    </Card>
    
    <Card class="p-4">
      <div class="flex items-center gap-3">
        <div class="p-2 bg-orange-500/10 rounded-lg">
          <Activity class="h-5 w-5 text-orange-500" />
        </div>
        <div>
          <p class="text-sm text-muted-foreground">{$t('network.bandwidth')}</p>
          <p class="text-sm font-bold">↓ {dhtStatus === 'connected' ? $networkStats.avgDownloadSpeed.toFixed(1) : '0.0'} MB/s</p>
          <p class="text-sm font-bold">↑ {dhtStatus === 'connected' ? $networkStats.avgUploadSpeed.toFixed(1) : '0.0'} MB/s</p>
        </div>
      </div>
    </Card>
  </div>
  
  <div class="mt-6">
    <GeoDistributionCard />
  </div>
  
  <!-- Peer Discovery -->
  <Card class="p-6">
    <div class="flex flex-wrap items-center justify-between gap-2 mb-4">
      <h2 class="text-lg font-semibold">{$t('network.peerDiscovery.title')}</h2>
      <div class="flex-shrink-0">
        <Button
          size="sm"
          variant="outline"
          on:click={runDiscovery}
          disabled={discoveryRunning}
        >
          <RefreshCw class="h-4 w-4 mr-2 {discoveryRunning ? 'animate-spin' : ''}" />
          {discoveryRunning ? $t('network.peerDiscovery.discovering') : $t('network.peerDiscovery.run')}
        </Button>
      </div>
    </div>
    
    <div class="space-y-4">
      <div>
        <Label for="peer-address">{$t('network.peerDiscovery.directConnect')}</Label>
        <div class="flex flex-wrap gap-2 mt-2">
          <Input
            id="peer-address"
            bind:value={newPeerAddress}
            placeholder={$t('network.peerDiscovery.placeholder')}
            class="flex-1 min-w-0 break-all"
          />
          <Button on:click={connectToPeer} disabled={!newPeerAddress}>
            <UserPlus class="h-4 w-4 mr-2" />
            {$t('network.peerDiscovery.connect')}
          </Button>
          <Button
            on:click={sendTestMessage}
            disabled={!webrtcSession || !webrtcSession.channel || webrtcSession.channel.readyState !== 'open'}
            variant="outline"
          >
            {$t('network.sendTest')}
          </Button>
        </div>
        <!-- {#if discoveredPeers && discoveredPeers.length > 0} -->
         {#if isTauri}
          {@const discoveryTotalPages = Math.ceil(discoveredPeerEntries.length / discoveryPerPage)}
          {@const discoveryStartIndex = (discoveryCurrentPage - 1) * discoveryPerPage}
          {@const discoveryEndIndex = Math.min(discoveryStartIndex + discoveryPerPage, discoveredPeerEntries.length)}
          {@const paginatedDiscoveryPeers = discoveredPeerEntries.slice(discoveryStartIndex, discoveryEndIndex)}

          <div class="mt-4 space-y-3">
            <!-- Controls bar: showing text, pagination, and refresh button all on same line -->
            <div class="flex items-center justify-between gap-4">
              <!-- Left: Showing peers counter -->
              <div class="text-sm text-muted-foreground flex-shrink-0">
                {#if discoveredPeerEntries.length > 0}
                  Showing {discoveryStartIndex + 1}-{discoveryEndIndex} of {discoveredPeerEntries.length} discovered peers
                {:else}
                  No discovered peers
                {/if}
              </div>

              <!-- Center: Pagination Controls -->
              <div class="flex items-center justify-center flex-1">
                {#if discoveredPeerEntries.length > discoveryPerPage}
                  <div class="flex items-center gap-2">
                    <Button
                      size="sm"
                      variant="outline"
                      on:click={() => {
                        if (discoveryCurrentPage > 1) discoveryCurrentPage--
                      }}
                      disabled={discoveryCurrentPage === 1}
                    >
                      Previous
                    </Button>
                    <div class="flex items-center gap-1">
                      {#each Array.from({ length: discoveryTotalPages }, (_, i) => i + 1) as page}
                        {#if page === 1 || page === discoveryTotalPages || (page >= discoveryCurrentPage - 1 && page <= discoveryCurrentPage + 1)}
                          <Button
                            size="sm"
                            variant={page === discoveryCurrentPage ? 'default' : 'outline'}
                            class="w-10"
                            on:click={() => discoveryCurrentPage = page}
                          >
                            {page}
                          </Button>
                        {:else if page === discoveryCurrentPage - 2 || page === discoveryCurrentPage + 2}
                          <span class="px-2 text-muted-foreground">...</span>
                        {/if}
                      {/each}
                    </div>
                    <Button
                      size="sm"
                      variant="outline"
                      on:click={() => {
                        if (discoveryCurrentPage < discoveryTotalPages) discoveryCurrentPage++
                      }}
                      disabled={discoveryCurrentPage === discoveryTotalPages}
                    >
                      Next
                    </Button>
                  </div>
                {/if}
              </div>

              <!-- Right: Run Discovery button -->
              <div class="flex-shrink-0">
                <Button
                  size="sm"
                  variant="outline"
                  on:click={runDiscovery}
                  disabled={discoveryRunning}
                >
                  <RefreshCw class="h-4 w-4 mr-2 {discoveryRunning ? 'animate-spin' : ''}" />
                  {discoveryRunning ? $t('network.peerDiscovery.discovering') : $t('network.peerDiscovery.run')}
                </Button>
              </div>
            </div>

            {#if paginatedDiscoveryPeers.length > 0}
              <ul class="space-y-3">
                {#each paginatedDiscoveryPeers as peer}
                  <li class="border rounded p-3 space-y-2 bg-background/50">
                    <div class="flex items-start justify-between gap-2">
                      <div class="text-sm font-mono break-all">{peer.peerId}</div>
                      <Button
                        size="icon"
                        variant="ghost"
                        class="h-8 w-8"
                        title={$t('network.quickActions.copyPeerId.button')}
                        on:click={async () => {
                          await copy(peer.peerId)
                          showToast($t('network.quickActions.copyPeerId.success'), 'success')
                        }}
                      >
                        <Clipboard class="h-4 w-4" />
                      </Button>
                    </div>
                    {#if peer.addresses.length > 0}
                      <div class="space-y-1">
                        {#each peer.addresses as addr}
                          <div class="flex items-center justify-between gap-2">
                            <span class="text-xs font-mono break-all">{addr}</span>
                            {#if addr.includes('/p2p/')}
                              <Button
                                size="sm"
                                variant="outline"
                                on:click={() => {
                                  newPeerAddress = addr
                                  showToast($t('network.peerDiscovery.peerAddedToInput'), 'success')
                                }}
                              >
                                {$t('network.peerDiscovery.add')}
                              </Button>
                            {/if}
                          </div>
                        {/each}
                      </div>
                    {:else}
                      <p class="text-xs text-muted-foreground">{$t('network.peerMetrics.noPeers')}</p>
                    {/if}
                    <p class="text-xs text-muted-foreground">{$t('network.connectedPeers.lastSeen')}: {formatPeerTimestamp(peer.lastSeen)}</p>
                  </li>
                {/each}
              </ul>
            {/if}
          </div>
        {:else if webDiscoveredPeers.length > 0}
          {@const webDiscoveryTotalPages = Math.ceil(webDiscoveredPeers.length / discoveryPerPage)}
          {@const webDiscoveryStartIndex = (discoveryCurrentPage - 1) * discoveryPerPage}
          {@const webDiscoveryEndIndex = Math.min(webDiscoveryStartIndex + discoveryPerPage, webDiscoveredPeers.length)}
          {@const paginatedWebDiscoveryPeers = webDiscoveredPeers.slice(webDiscoveryStartIndex, webDiscoveryEndIndex)}

          <div class="mt-4 space-y-3">
            <!-- Controls bar: showing text, pagination, and refresh button all on same line -->
            <div class="flex items-center justify-between gap-4">
              <!-- Left: Showing peers counter -->
              <div class="text-sm text-muted-foreground flex-shrink-0">
                {#if webDiscoveredPeers.length > 0}
                  Showing {webDiscoveryStartIndex + 1}-{webDiscoveryEndIndex} of {webDiscoveredPeers.length} discovered peers
                {:else}
                  No discovered peers
                {/if}
              </div>

              <!-- Center: Pagination Controls -->
              <div class="flex items-center justify-center flex-1">
                {#if webDiscoveredPeers.length > discoveryPerPage}
                  <div class="flex items-center gap-2">
                    <Button
                      size="sm"
                      variant="outline"
                      on:click={() => {
                        if (discoveryCurrentPage > 1) discoveryCurrentPage--
                      }}
                      disabled={discoveryCurrentPage === 1}
                    >
                      Previous
                    </Button>
                    <div class="flex items-center gap-1">
                      {#each Array.from({ length: webDiscoveryTotalPages }, (_, i) => i + 1) as page}
                        {#if page === 1 || page === webDiscoveryTotalPages || (page >= discoveryCurrentPage - 1 && page <= discoveryCurrentPage + 1)}
                          <Button
                            size="sm"
                            variant={page === discoveryCurrentPage ? 'default' : 'outline'}
                            class="w-10"
                            on:click={() => discoveryCurrentPage = page}
                          >
                            {page}
                          </Button>
                        {:else if page === discoveryCurrentPage - 2 || page === discoveryCurrentPage + 2}
                          <span class="px-2 text-muted-foreground">...</span>
                        {/if}
                      {/each}
                    </div>
                    <Button
                      size="sm"
                      variant="outline"
                      on:click={() => {
                        if (discoveryCurrentPage < webDiscoveryTotalPages) discoveryCurrentPage++
                      }}
                      disabled={discoveryCurrentPage === webDiscoveryTotalPages}
                    >
                      Next
                    </Button>
                  </div>
                {/if}
              </div>

              <!-- Right: Run Discovery button -->
              <div class="flex-shrink-0">
                <Button
                  size="sm"
                  variant="outline"
                  on:click={runDiscovery}
                  disabled={discoveryRunning}
                >
                  <RefreshCw class="h-4 w-4 mr-2 {discoveryRunning ? 'animate-spin' : ''}" />
                  {discoveryRunning ? $t('network.peerDiscovery.discovering') : $t('network.peerDiscovery.run')}
                </Button>
              </div>
            </div>

            <ul class="space-y-2">
              {#each paginatedWebDiscoveryPeers as p}
                <li class="flex items-center justify-between p-2 border rounded">
                  <div class="truncate mr-4">{p}</div>
                      <!-- <div class="flex items-center gap-2">
                        <Button size="sm" variant="outline" on:click={() => { newPeerAddress = p; showToast($t('network.peerDiscovery.peerAddedToInput'), 'success'); }}>
                          {$t('network.peerDiscovery.add')}
                        </Button>
                      </div> -->
                    <div class="flex items-center gap-2">
                      <Button size="sm" variant="outline" on:click={() => { newPeerAddress = p; showToast($t('network.peerDiscovery.peerAddedToInput'), 'success'); }}>
                        {$t('network.peerDiscovery.add')}
                      </Button>
                    </div>
                </li>
              {/each}
            </ul>
          </div>
        {/if}
      </div>
    </div>
  </Card>
  
  <!-- Connected Peers -->
  <Card class="p-6">
      <div class="flex flex-wrap items-center justify-between gap-2 mb-4">
          <h2 class="text-lg font-semibold">{$t('network.connectedPeers.title', { values: { count: $peers.length } })}</h2>
      <div class="flex items-center gap-2">
        <Label for="sort" class="flex items-center">
    <span class="text-base">{$t('network.connectedPeers.sortBy')}</span>
        </Label>
  <div class="w-40 flex-shrink-0">
  <DropDown
  id="sort"
        options={[
          { value: 'reputation', label: $t('network.connectedPeers.reputation') },
          { value: 'sharedFiles', label: $t('network.connectedPeers.sharedFiles') },
          { value: 'totalSize', label: $t('network.connectedPeers.totalSize') },
          { value: 'nickname', label: $t('network.connectedPeers.name') },
          { value: 'location', label: $t('network.connectedPeers.location') },
          { value: 'joinDate', label: $t('network.connectedPeers.joinDate') },
          { value: 'lastSeen', label: $t('network.connectedPeers.lastSeen') },
          { value: 'status', label: $t('network.connectedPeers.status') }
        ]}
        bind:value={sortBy}
  />
  </div>
  <div class="w-40 flex-shrink-0">
  <DropDown
  id="sort-direction"
        options={
          sortBy === 'reputation'
          ? [ { value: 'desc', label: $t('network.connectedPeers.highest') }, { value: 'asc', label: $t('network.connectedPeers.lowest') } ]
          : sortBy === 'sharedFiles'
          ? [ { value: 'desc', label: $t('network.connectedPeers.most') }, { value: 'asc', label: $t('network.connectedPeers.least') } ]
          : sortBy === 'totalSize'
          ? [ { value: 'desc', label: $t('network.connectedPeers.largest') }, { value: 'asc', label: $t('network.connectedPeers.smallest') } ]
          : sortBy === 'joinDate'
          ? [ { value: 'desc', label: $t('network.connectedPeers.newest') }, { value: 'asc', label: $t('network.connectedPeers.oldest') } ]
          : sortBy === 'lastSeen'
          ? [ { value: 'desc', label: $t('network.connectedPeers.mostRecent') }, { value: 'asc', label: $t('network.connectedPeers.leastRecent') } ]
          : sortBy === 'location'
          ? [ { value: 'asc', label: $t('network.connectedPeers.closest') }, { value: 'desc', label: $t('network.connectedPeers.farthest') } ]
          : sortBy === 'status'
          ? [ { value: 'asc', label: $t('network.connectedPeers.online') }, { value: 'desc', label: $t('network.connectedPeers.offline') } ]
          : sortBy === 'nickname'
          ? [ { value: 'asc', label: $t('network.connectedPeers.aToZ') }, { value: 'desc', label: $t('network.connectedPeers.zToA') } ]
          : [ { value: 'desc', label: 'Desc' }, { value: 'asc', label: 'Asc' } ]
        }
        bind:value={sortDirection}
        />
        </div>
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
                    aVal = (a.nickname || 'zzzzz').toLowerCase() // Put empty names at the end
                    bVal = (b.nickname || 'zzzzz').toLowerCase()
                    break
                case 'location':
                    const getLocationDistance = (peerLocation: string | undefined) => {
                        if (!peerLocation) return UNKNOWN_DISTANCE;

                        const peerRegion = normalizeRegion(peerLocation);

                        if (peerRegion.id === UNKNOWN_REGION_ID) {
                            return UNKNOWN_DISTANCE;
                        }

                        if (currentUserRegion.id === UNKNOWN_REGION_ID) {
                            return peerRegion.id === UNKNOWN_REGION_ID ? 0 : UNKNOWN_DISTANCE;
                        }

                        if (peerRegion.id === currentUserRegion.id) {
                            return 0;
                        }

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
                    // Assign numeric values for logical sorting: online (0) > away (1) > offline (2)
                    aVal = a.status === 'online' ? 0 : a.status === 'away' ? 1 : 2
                    bVal = b.status === 'online' ? 0 : b.status === 'away' ? 1 : 2
                    break
                default:
                    return 0
            }

            // Handle string comparison
            if (typeof aVal === 'string' && typeof bVal === 'string') {
                if (aVal < bVal) {
                    return sortDirection === 'asc' ? -1 : 1
                } else if (aVal > bVal) {
                    return sortDirection === 'asc' ? 1 : -1
                } else {
                    return 0
                }
            }

            // Handle numeric comparison
            if (typeof aVal === 'number' && typeof bVal === 'number') {
                const result = aVal - bVal
                return sortDirection === 'asc' ? result : -result
            }

            return 0
        })}
    {@const totalPages = Math.ceil(sortedPeers.length / peersPerPage)}
    {@const startIndex = (currentPage - 1) * peersPerPage}
    {@const endIndex = Math.min(startIndex + peersPerPage, sortedPeers.length)}
    {@const paginatedPeers = sortedPeers.slice(startIndex, endIndex)}

    <!-- Controls bar: showing text, pagination, and refresh button all on same line -->
    <div class="flex items-center justify-between mb-4 gap-4">
      <!-- Left: Showing peers counter -->
      <div class="text-sm text-muted-foreground flex-shrink-0">
        {#if sortedPeers.length > 0}
          Showing {startIndex + 1}-{endIndex} of {sortedPeers.length} peers
        {:else}
          No peers
        {/if}
      </div>

      <!-- Center: Pagination Controls -->
      <div class="flex items-center justify-center flex-1">
        {#if sortedPeers.length > peersPerPage}
          <div class="flex items-center gap-2">
            <Button
              size="sm"
              variant="outline"
              on:click={() => {
                if (currentPage > 1) currentPage--
              }}
              disabled={currentPage === 1}
            >
              Previous
            </Button>
            <div class="flex items-center gap-1">
              {#each Array.from({ length: totalPages }, (_, i) => i + 1) as page}
                {#if page === 1 || page === totalPages || (page >= currentPage - 1 && page <= currentPage + 1)}
                  <Button
                    size="sm"
                    variant={page === currentPage ? 'default' : 'outline'}
                    class="w-10"
                    on:click={() => currentPage = page}
                  >
                    {page}
                  </Button>
                {:else if page === currentPage - 2 || page === currentPage + 2}
                  <span class="px-2 text-muted-foreground">...</span>
                {/if}
              {/each}
            </div>
            <Button
              size="sm"
              variant="outline"
              on:click={() => {
                if (currentPage < totalPages) currentPage++
              }}
              disabled={currentPage === totalPages}
            >
              Next
            </Button>
          </div>
        {/if}
      </div>

      <!-- Right: Refresh button -->
      <div class="flex-shrink-0">
        <Button
          size="sm"
          variant="outline"
          on:click={refreshConnectedPeers}
          disabled={!isTauri || dhtStatus !== 'connected'}
        >
          <RefreshCw class="h-4 w-4 mr-2" />
          Refresh Peers
        </Button>
      </div>
    </div>

    <!-- Peer list -->
    <div class="space-y-3">
        {#each paginatedPeers as peer}
        <div class="p-4 bg-secondary rounded-lg">
          <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between mb-2 gap-2">
            <div class="flex items-start gap-3 min-w-0">
              <div class="w-2 h-2 rounded-full flex-shrink-0 {
                peer.status === 'online' ? 'bg-green-500' :
                peer.status === 'away' ? 'bg-yellow-500' :
                'bg-red-500'
              }"></div>
              <div>
                <p class="font-medium">{peer.nickname || $t('network.connectedPeers.anonymous')}</p>
                <p class="text-xs text-muted-foreground break-all">{peer.address}</p>
              </div>
            </div>
            <div class="flex flex-wrap items-center gap-2 justify-end">
              <Badge variant="outline" class="flex-shrink-0">
              ⭐ {(peer.reputation ?? 0).toFixed(1)}
              </Badge>
                <Badge variant={peer.status === 'online' ? 'default' : 'secondary'}
                       class={
                          peer.status === 'online' ? 'bg-green-500 text-white' :
                          peer.status === 'away' ? 'bg-yellow-500 text-white' :
                          'bg-red-500 text-white'
                        }
                        style="pointer-events: none;"
                >
                {peer.status}
              </Badge>
              <Button
                size="sm"
                variant="outline"
                class="h-8 px-2"
                on:click={() => disconnectFromPeer(peer.address)}
              >
                <UserMinus class="h-3.5 w-3.5 mr-1" />
                {$t('network.connectedPeers.disconnect')}
              </Button>
            </div>
          </div>
          
          <div class="grid grid-cols-2 md:grid-cols-5 gap-4 text-sm">
            <div>
              <p class="text-xs text-muted-foreground">{$t('network.connectedPeers.sharedFiles')}</p>
              <p class="font-medium">{peer.sharedFiles}</p>
            </div>
            <div>
              <p class="text-xs text-muted-foreground">{$t('network.connectedPeers.totalSize')}</p>
              <p class="font-medium">{formatSize(peer.totalSize)}</p>
            </div>
            <div>
              <p class="text-xs text-muted-foreground">{$t('network.connectedPeers.location')}</p>
              <p class="font-medium">{peer.location || $t('network.connectedPeers.unknown')}</p>
            </div>
            <div>
              <p class="text-xs text-muted-foreground">{$t('network.connectedPeers.joined')}</p>
              <p class="font-medium">{formatPeerDate(peer.joinDate)}</p>
            </div>
            <div>
              <p class="text-xs text-muted-foreground">{$t('network.connectedPeers.lastSeen')}</p>
              <p class="font-medium">
                {#if peer.status === 'online'}
                  {$t('network.connectedPeers.now')}
                {:else}
                  {formatPeerDate(peer.lastSeen)}
                {/if}
              </p>
            </div>
          </div>
        </div>
      {/each}

      {#if $peers.length === 0}
        <p class="text-center text-muted-foreground py-8">{$t('network.connectedPeers.noPeers')}</p>
      {/if}
    </div>
  </Card>
</div>