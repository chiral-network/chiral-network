<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import Button from '$lib/components/ui/button.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import GethStatusCard from '$lib/components/GethStatusCard.svelte'
  import PeerMetrics from '$lib/components/PeerMetrics.svelte'
  import GeoDistributionCard from '$lib/components/GeoDistributionCard.svelte'
  import { peers, networkStats, networkStatus, userLocation, etcAccount, settings } from '$lib/stores'
  import { normalizeRegion, UNKNOWN_REGION_ID } from '$lib/geo'
  import { Users, HardDrive, Activity, RefreshCw, UserPlus, Signal, Server, Play, Square, Download, AlertCircle, Wifi, UserMinus } from 'lucide-svelte'
  import { get } from 'svelte/store'
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

  // Check if running in Tauri environment
  const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
  const tr = (k: string, params?: Record<string, any>): string => (get(t) as (key: string, params?: any) => string)(k, params)

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

  const UNKNOWN_DISTANCE = 1_000_000;

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
  
  // Chiral Network Node variables
  let isGethRunning = false
  let isGethInstalled = false
  let isDownloading = false
  let isStartingNode = false
  let downloadProgress = {
    downloaded: 0,
    total: 0,
    percentage: 0,
    status: ''
  }
  let downloadError = ''
  let dataDir = './bin/geth-data'
  let peerCount = 0
  let peerCountInterval: ReturnType<typeof setInterval> | undefined
  let chainId = 98765
  let gethStatusCardRef: { refresh?: () => Promise<void> } | null = null
  
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
  const nodeAddress = "enode://277ac35977fc0a230e3ca4ccbf6df6da486fd2af9c129925b1193b25da6f013a301788fceed458f03c6c0d289dfcbf7a7ca5c0aef34b680fcbbc8c2ef79c0f71@127.0.0.1:30303"
  let copiedNodeAddr = false
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
      console.error('Failed to get multiaddresses:', e)
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
      console.error('Failed to copy observed address', error)
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
      dhtBootstrapNodes = await invoke<string[]>("get_bootstrap_nodes_command")
      dhtBootstrapNode = dhtBootstrapNodes[0] || 'No bootstrap nodes configured'
    } catch (error) {
      console.error('Failed to fetch bootstrap nodes:', error)
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
          console.error('Failed to refresh NAT status', error)
        }
      })
    } catch (error) {
      console.error('Failed to subscribe to NAT status updates', error)
    }
  }
  
  async function startDht() {
    if (!isTauri) {
      // Mock DHT connection for web
      dhtStatus = 'connecting'
      setTimeout(() => {
        dhtStatus = 'connected'
        dhtPeerId = '12D3KooWMockPeerIdForWebDemo123456789'
      }, 1000)
      return
    }
    
    try {
      dhtError = null
      
      // First check if DHT is already running in backend BEFORE setting any status
      let backendPeerId = null
      try {
        backendPeerId = await invoke<string | null>('get_dht_peer_id')
      } catch (error) {
        console.log('Failed to check backend DHT status:', error)
      }
      
      if (backendPeerId) {
        // DHT is already running in backend, sync the frontend state immediately
        console.log('DHT already running in backend with peer ID:', backendPeerId)
        dhtPeerId = backendPeerId
        dhtService.setPeerId(backendPeerId) // Update frontend service state
        dhtEvents = [...dhtEvents, `✓ DHT already running with peer ID: ${backendPeerId.slice(0, 16)}...`]
        
        // Check connection status immediately
        let currentPeers = 0
        const health = await dhtService.getHealth()
        if (health) {
          dhtHealth = health
          currentPeers = health.peerCount
        } else {
          currentPeers = await dhtService.getPeerCount()
        }
        dhtPeerCount = currentPeers

        if (currentPeers > 0) {
          // Set status directly to connected without showing connecting first
          dhtStatus = 'connected'
          dhtEvents = [...dhtEvents, `✓ Connected to ${currentPeers} peer(s)`]
          startDhtPolling() // Start polling for updates
          return // Already connected, no need to continue
        } else {
          // No peers connected, set to disconnected and try to connect
          dhtStatus = 'disconnected'
          dhtEvents = [...dhtEvents, `⚠ No peers connected, attempting to connect to bootstrap...`]
          startDhtPolling() // Start polling anyway
          connectionAttempts++
          // Continue below to try connecting to bootstrap
        }
      } else {
        // DHT not running, show connecting state and start it
        dhtStatus = 'connecting'
        connectionAttempts++
        
        // Add a small delay to show the connecting state only when starting fresh
        await new Promise(resolve => setTimeout(resolve, 500))
        // DHT not running, start it
        try {
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
          // Also ensure the service knows its own peer ID
          dhtService.setPeerId(peerId)
          dhtEvents = [...dhtEvents, `✓ DHT started with peer ID: ${peerId.slice(0, 16)}...`]
        } catch (error: any) {
          if (error.toString().includes('already running')) {
            // DHT is already running in backend but service doesn't have the peer ID
            // This shouldn't happen with our singleton pattern, but handle it anyway
            console.warn('DHT already running in backend, attempting to retrieve peer ID...')
            dhtEvents = [...dhtEvents, `⚠ DHT already running in backend, retrieving peer ID...`]
            
            // Try to get it from the backend directly
            try {
              const peerId = await invoke('get_dht_peer_id')
              if (peerId) {
                dhtPeerId = peerId as string
                dhtService.setPeerId(dhtPeerId)
                dhtEvents = [...dhtEvents, `✓ Retrieved peer ID: ${dhtPeerId.slice(0, 16)}...`]
              } else {
                throw new Error('Could not retrieve peer ID from backend')
              }
            } catch (retrieveError) {
              console.error('Failed to retrieve peer ID:', retrieveError)
              throw retrieveError
            }
          } else {
            throw error
          }
        }
      }
      
      // Try to connect to bootstrap nodes
      let connectionSuccessful = false

      if (dhtBootstrapNodes.length > 0) {
        dhtEvents = [...dhtEvents, `[Attempt ${connectionAttempts}] Connecting to ${dhtBootstrapNodes.length} bootstrap node(s)...`]
        
        // Add another small delay to show the connection attempt
        await new Promise(resolve => setTimeout(resolve, 1000))
        
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
          console.warn('Cannot connect to bootstrap nodes:', error)
          
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
      console.error('Failed to start DHT:', error)
      dhtStatus = 'disconnected'
      dhtError = error.toString ? error.toString() : String(error)
      dhtEvents = [...dhtEvents, `✗ Failed to start DHT: ${dhtError}`]
    }
  }

  // Ensure UI reflects backend DHT state when returning to this tab
  async function syncDhtStatusOnMount() {
    if (!isTauri) return
    try {
      const backendPeerId = await invoke<string | null>('get_dht_peer_id')
      if (backendPeerId) {
        dhtPeerId = backendPeerId
        dhtService.setPeerId(backendPeerId)

        // Sync the port from the service
        dhtPort = dhtService.getPort()

        // Pull health/peers and update UI without attempting a restart
        const health = await dhtService.getHealth()
        if (health) {
          dhtHealth = health
          dhtPeerCount = health.peerCount
          lastNatState = health.reachability
          lastNatConfidence = health.reachabilityConfidence
        } else {
          dhtPeerCount = await dhtService.getPeerCount()
        }

        // Set status and resume polling if needed
        dhtStatus = dhtPeerCount > 0 ? 'connected' : 'disconnected'
        startDhtPolling()
      }
    } catch (e) {
      console.warn('Failed to sync DHT status on mount:', e)
    }
  }
  
  let peerRefreshCounter = 0;

  function startDhtPolling() {
    if (dhtPollInterval) return // Already polling

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
        if (dhtStatus === 'connected' && peerCount === 0) {
          dhtStatus = 'disconnected'
          dhtEvents = [...dhtEvents, '⚠ Lost connection to all peers']
        } else if (dhtStatus === 'disconnected' && peerCount > 0) {
          dhtStatus = 'connected'
          dhtEvents = [...dhtEvents, `✓ Reconnected to ${peerCount} peer(s)`]
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
            console.debug('Background peer refresh failed:', error);
          }
        }
      } catch (error) {
        console.error('Failed to poll DHT status:', error)
      }
    }, 2000) as unknown as number
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
      return
    }
    
    try {
      await dhtService.stop()
      dhtStatus = 'disconnected'
      dhtPeerId = null
      dhtError = null
      connectionAttempts = 0
      dhtEvents = [...dhtEvents, `✓ DHT stopped`]
      dhtHealth = null
      copiedListenAddr = null
      lastNatState = null
      lastNatConfidence = null
    } catch (error) {
      console.error('Failed to stop DHT:', error)
      dhtEvents = [...dhtEvents, `✗ Failed to stop DHT: ${error}`]
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
          // console.log('Updated discovered peers (excluding self):', discoveredPeers);
          webDiscoveredPeers = peers.filter(p => p !== myClientId);
          console.log('Updated discovered peers (excluding self):', webDiscoveredPeers);
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
        showToast('Connected to signaling server', 'success');
      } catch (error) {
        console.error('Failed to connect to signaling server:', error);
        showToast('Failed to connect to signaling server for web mode testing', 'error');
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
      showToast('Please enter a peer address', 'error');
      return;
    }

    const peerAddress = newPeerAddress.trim();

    // In Tauri mode, use DHT backend for P2P connections
    if (isTauri) {
      if (dhtStatus !== 'connected') {
        showToast('DHT not connected. Please start DHT first.', 'error');
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
        showToast('Peer is already connected', 'info');
        newPeerAddress = '';
        return;
      }

      try {
        showToast('Connecting to peer via DHT...', 'info');
        const currentPeerCount = $peers.length;
        await invoke('connect_to_peer', { peerAddress });

        // Clear input
        newPeerAddress = '';

        // Wait a moment and check if the peer was actually added
        setTimeout(async () => {
          await refreshConnectedPeers();
          if ($peers.length > currentPeerCount) {
            showToast('Connection Success!', 'success');
          } else {
            showToast('Connection failed. Peer may be unreachable or address invalid.', 'error');
          }
        }, 2000);
      } catch (error) {
        console.error('Failed to connect to peer:', error);
        showToast('Failed to connect to peer: ' + error, 'error');
      }
      return;
    }

    // In web mode, use WebRTC for testing
    if (!signalingConnected) {
      showToast('Signaling server not connected. Please start DHT first.', 'error');
      return;
    }

    const peerId = peerAddress;

    // Check if peer exists in discovered peers
    // if (!discoveredPeers.includes(peerId)) {
    if (!webDiscoveredPeers.includes(peerId)) {
      showToast(`Peer ${peerId} not found in discovered peers`, 'warning');
      // Still attempt connection in case peer was discovered recently
    }

    try {
      webrtcSession = createWebRTCSession({
        peerId,
        signaling,
        isInitiator: true,
        onMessage: (data) => {
          showToast('Received from peer: ' + data, 'info');
        },
        onConnectionStateChange: (state) => {
          console.log('[WebRTC] Connection state:', state);

          // Only show toasts for important states (not every intermediate state)
          if (state === 'connected') {
            showToast('Successfully connected to peer!', 'success');
            // Add minimal PeerInfo to peers store if not present
            addConnectedPeer(peerId);
          } else if (state === 'failed') {
            showToast('Connection to peer failed', 'error');
            // Mark peer as offline / remove from peers list
            markPeerDisconnected(peerId);
          } else if (state === 'disconnected' || state === 'closed') {
            console.log('[WebRTC] Peer disconnected');
            // Mark peer as offline / remove from peers list
            markPeerDisconnected(peerId);
          }
        },
        onDataChannelOpen: () => {
          showToast('Data channel open - you can now send messages!', 'success');
          // Ensure peer is listed as connected when data channel opens
          addConnectedPeer(peerId);
        },
        onDataChannelClose: () => {
          showToast('Data channel closed', 'warning');
          markPeerDisconnected(peerId);
        },
        onError: (e) => {
          showToast('WebRTC error: ' + e, 'error');
          console.error('WebRTC error:', e);
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
      showToast('Connecting to peer: ' + peerId, 'success');

      // Clear input on successful connection attempt
      newPeerAddress = '';

    } catch (error) {
      console.error('Failed to create WebRTC session:', error);
      showToast('Failed to create connection: ' + error, 'error');
    }
  }
  
  function sendTestMessage() {
    if (!webrtcSession || !webrtcSession.channel || webrtcSession.channel.readyState !== 'open') {
      showToast('No active WebRTC connection', 'error');
      return;
    }
    
    const testMessage = `Hello from ${signaling.getClientId()} at ${new Date().toLocaleTimeString()}`;
    try {
      webrtcSession.send(testMessage);
      showToast('Test message sent: ' + testMessage, 'success');
    } catch (error) {
      showToast('Failed to send message: ' + error, 'error');
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
      console.debug('Failed to refresh peers:', error);
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
      console.error('Failed to disconnect from peer:', error)
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
    isStartingNode = false

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

  function handleGethStatusChange(event: CustomEvent<GethStatus>) {
    applyGethStatus(event.detail)
  }
  
  async function checkGethStatus() {
    if (!isTauri) {
      // In web mode, simulate that geth is not installed
      isGethInstalled = false
      isGethRunning = false
      isStartingNode = false
      return
    }

    try {
      if (gethStatusCardRef?.refresh) {
        await gethStatusCardRef.refresh()
      } else {
        const status = await fetchGethStatus(dataDir, 1)
        applyGethStatus(status)
      }
    } catch (error) {
      console.error('Failed to check geth status:', error)
    }
  }
  
  async function downloadGeth() {
    if (!isTauri) {
      downloadError = $t('network.errors.downloadOnlyTauri')
      return
    }
    
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
      // Auto-start after download
      await startGethNode()
      if (gethStatusCardRef?.refresh) {
        await gethStatusCardRef.refresh()
      }
    } catch (e) {
      downloadError = String(e)
      isDownloading = false
      if (gethStatusCardRef?.refresh) {
        await gethStatusCardRef.refresh()
      }
    }
  }

  function startPolling() {
    if (peerCountInterval) {
      clearInterval(peerCountInterval)
    }
    fetchPeerCount()
    peerCountInterval = setInterval(fetchPeerCount, 5000)
  }

  async function startGethNode() {
    if (!isTauri) {
      console.log('Cannot start Chiral Node in web mode - desktop app required')
      return
    }
    
    isStartingNode = true
    try {
      // Set miner address if we have an account
      if ($etcAccount) {
        await invoke('set_miner_address', { address: $etcAccount.address })
      }
      await invoke('start_geth_node', { dataDir })
      isGethRunning = true
      startPolling()
      if (gethStatusCardRef?.refresh) {
        await gethStatusCardRef.refresh()
      }
    } catch (error) {
      console.error('Failed to start geth node:', error)
      alert('Failed to start Chiral node: ' + error)
    } finally {
      isStartingNode = false
    }
  }

  async function stopGethNode() {
    if (!isTauri) {
      console.log('Cannot stop Chiral Node in web mode - desktop app required')
      return
    }
    
    try {
      await invoke('stop_geth_node')
      isGethRunning = false
      isStartingNode = false
      peerCount = 0
      if (peerCountInterval) {
        clearInterval(peerCountInterval)
        peerCountInterval = undefined
      }
      if (gethStatusCardRef?.refresh) {
        await gethStatusCardRef.refresh()
      }
    } catch (error) {
      console.error('Failed to stop geth node:', error)
    }
  }

  // Copy Helper
  async function copy(text: string | null | undefined) {
    if (!text) return
    try {
      await navigator.clipboard.writeText(text)
    } catch (e) {
      console.error('Copy failed:', e)
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
      console.error('Failed to fetch peer count:', error)
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
      
      // Initialize async operations
      // const initAsync = async () => {
      //   await fetchBootstrapNodes()
      //   await checkGethStatus()
        
      //   // DHT check will happen in startDht()

      //   // Also passively sync DHT state if it's already running
      //   await syncDhtStatusOnMount()
        
      //   // Listen for download progress updates (only in Tauri)
      //   if (isTauri) {
      //     await registerNatListener()
      //     unlistenProgress = await listen('geth-download-progress', (event) => {
      //       downloadProgress = event.payload as typeof downloadProgress
      //     })
      await fetchBootstrapNodes()
      await checkGethStatus()

      // DHT check will happen in startDht()

      // Also passively sync DHT state if it's already running
      await syncDhtStatusOnMount()

      // Auto-start DHT if enabled in settings
      if (isTauri && $settings.autoStartDht && dhtStatus === 'disconnected') {
        console.log('Auto-starting DHT network...')
        dhtEvents = [...dhtEvents, '🚀 Auto-starting network...']
        await startDht()
      }

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
            console.error('Failed to start peer event stream:', error);
          }
        }
        await refreshConnectedPeers();
        await registerNatListener()
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
    }
    if (dhtPollInterval) {
      clearInterval(dhtPollInterval)
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
        if (gethStatusCardRef?.refresh) await gethStatusCardRef.refresh();
        showToast($t('network.quickActions.refreshStatus.success'), 'success');
      }}
    >
      <Activity class="h-5 w-5" />
      {$t('network.quickActions.refreshStatus.button')}
    </Button>

    <!-- Restart Node -->
    <Button
      size="lg"
      variant="secondary"
      class="flex items-center gap-2 px-6 py-3 font-semibold text-base rounded-lg shadow-sm border border-primary/10 bg-background hover:bg-secondary/80"
      title={$t('network.quickActions.restartNode.tooltip')}
      on:click={async () => {
        if (!isGethRunning) {
          showToast($t('network.quickActions.restartNode.notRunning'), 'error');
          return;
        }
        await stopGethNode();
        await startGethNode();
        showToast($t('network.quickActions.restartNode.success'), 'success');
      }}
      disabled={!isGethInstalled || isStartingNode || !isGethRunning}
    >
      <Square class="h-5 w-5" />
      {$t('network.quickActions.restartNode.button')}
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
        {:else if isDownloading}
          <div class="h-2 w-2 bg-blue-500 rounded-full animate-pulse"></div>
          <span class="text-sm text-blue-600">{$t('network.status.downloading')}</span>
        {:else if isStartingNode}
          <div class="h-2 w-2 bg-yellow-500 rounded-full animate-pulse"></div>
          <span class="text-sm text-yellow-600">{$t('network.status.starting')}</span>
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
      {#if !isGethInstalled}
        {#if isDownloading}
          <div class="space-y-3">
            <div class="text-center py-2">
              <Download class="h-12 w-12 text-blue-500 mx-auto mb-2 animate-pulse" />
              <p class="text-sm font-medium">{downloadProgress.status}</p>
            </div>
            <div class="space-y-2">
              <div class="flex justify-between text-sm">
                <span>{$t('network.download.progress')}</span>
                <span>{downloadProgress.percentage.toFixed(0)}%</span>
              </div>
              <div class="w-full bg-secondary rounded-full h-2 overflow-hidden">
                <div 
                  class="bg-blue-500 h-full transition-all duration-300"
                  style="width: {downloadProgress.percentage}%"
                ></div>
              </div>
              {#if downloadProgress.total > 0}
                <p class="text-xs text-muted-foreground text-center">
                  {(downloadProgress.downloaded / 1024 / 1024).toFixed(1)} MB / 
                  {(downloadProgress.total / 1024 / 1024).toFixed(1)} MB
                </p>
              {/if}
            </div>
          </div>
        {:else}
          <div class="text-center py-4">
            <Server class="h-12 w-12 text-muted-foreground mx-auto mb-2" />
            <p class="text-sm text-muted-foreground mb-1">{$t('network.download.notFound')}</p>
            <p class="text-xs text-muted-foreground mb-3">{$t('network.download.prompt')}</p>
            {#if downloadError}
              <div class="bg-red-500/10 border border-red-500/20 rounded-lg p-2 mb-3">
                <div class="flex items-center gap-2 justify-center">
                  <AlertCircle class="h-4 w-4 text-red-500 flex-shrink-0" />
                  <p class="text-xs text-red-500">{downloadError}</p>
                </div>
              </div>
            {/if}
            <Button on:click={downloadGeth} disabled={isDownloading}>
              <Download class="h-4 w-4 mr-2" />
              {$t('network.download.button')}
            </Button>
          </div>
        {/if}
      {:else if isStartingNode}
        <div class="text-center py-4">
          <Server class="h-12 w-12 text-yellow-500 mx-auto mb-2 animate-pulse" />
          <p class="text-sm text-muted-foreground">{$t('network.startingNode')}</p>
          <p class="text-xs text-muted-foreground mt-1">{$t('network.pleaseWait')}</p>
        </div>
      {:else if !isGethRunning}
        <div class="text-center py-4">
          <Server class="h-12 w-12 text-muted-foreground mx-auto mb-2" />
          <p class="text-sm text-muted-foreground mb-3">{$t('network.notRunning')}</p>
          <Button on:click={startGethNode} disabled={isStartingNode}>
            <Play class="h-4 w-4 mr-2" />
            {$t('network.startNode')}
          </Button>
        </div>
      {:else}
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
        <Button class="w-full" variant="outline" on:click={stopGethNode}>
          <Square class="h-4 w-4 mr-2" />
          {$t('network.stopNode')}
        </Button>
      {/if}
    </div>
  </Card>

  <GethStatusCard
    bind:this={gethStatusCardRef}
    dataDir={dataDir}
    logLines={60}
    refreshIntervalMs={10000}
    on:status={handleGethStatusChange}
  />
  
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
                        class="inline-flex items-center gap-1 rounded-full bg-muted px-3 py-1 text-xs font-mono hover:bg-muted/80"
                        on:click={() => copyObservedAddr(addr)}
                        type="button"
                      >
                        {addr}
                        <Clipboard class="h-3 w-3" />
                      </button>
                    {/each}
                  </div>
                {:else}
                  <p class="mt-2 text-sm text-muted-foreground">{$t('network.dht.reachability.observedEmpty')}</p>
                {/if}
              </div>
              <div>
                <p class="text-xs uppercase text-muted-foreground">{$t('network.dht.reachability.lastError')}</p>
                <p class="mt-2 text-sm text-muted-foreground">{dhtHealth?.lastReachabilityError ?? tr('network.dht.health.none')}</p>
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
                          <td class="px-3 py-2 text-muted-foreground">{item.summary ?? '—'}</td>
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
          <div class="mt-4 space-y-3">
            <p class="text-sm text-muted-foreground">{$t('network.peerDiscovery.foundPeers', { values: { count: discoveredPeerEntries.length } })}</p>
            {#if discoveredPeerEntries.length > 0}
              <ul class="space-y-3">
                {#each discoveredPeerEntries as peer}
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
          <div class="mt-4">
            <!-- <p class="text-sm text-muted-foreground">{$t('network.peerDiscovery.foundPeers', { values: { count: discoveredPeers.length } })}</p> -->
             <p class="text-sm text-muted-foreground">{$t('network.peerDiscovery.foundPeers', { values: { count: webDiscoveredPeers.length } })}</p>
            <ul class="mt-2 space-y-2">
              <!-- {#each discoveredPeers as p} -->
               {#each webDiscoveredPeers as p}
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
    <div class="space-y-3">
        {#each [...$peers].sort((a, b) => {
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
        }) as peer}
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
              <p class="font-medium">{new Date(peer.joinDate).toLocaleString()}</p>
            </div>
            <div>
              <p class="text-xs text-muted-foreground">{$t('network.connectedPeers.lastSeen')}</p>
              <p class="font-medium">
                {#if peer.status === 'online'}
                  {$t('network.connectedPeers.now')}
                {:else}
                  {new Date(peer.lastSeen).toLocaleString()}
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

    <!-- Refresh button at bottom right -->
    <div class="flex justify-end mt-4">
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
  </Card>
</div>
