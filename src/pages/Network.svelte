<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import Button from '$lib/components/ui/button.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import { Users, HardDrive, Activity, RefreshCw, UserPlus, Signal, Server, Play, Square, Download, AlertCircle, Wifi } from 'lucide-svelte'
  import { peers, networkStats, networkStatus, userLocation, etcAccount } from '$lib/stores'
  import { get } from 'svelte/store'
  import { onMount, onDestroy } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import { listen } from '@tauri-apps/api/event'
  import { dhtService, DEFAULT_BOOTSTRAP_NODES } from '$lib/dht'
  import { resetConnectionAttempts } from '$lib/dhtHelpers.js'
  import type { DhtHealth } from '$lib/dht'
  import { Clipboard } from "lucide-svelte"
  import { t } from 'svelte-i18n';
  import { showToast } from '$lib/toast';
  import DropDown from '$lib/components/ui/dropDown.svelte'

  // Check if running in Tauri environment
  const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
  const tr = (k: string, params?: Record<string, any>) => get(t)(k, params)
  
  let discoveryRunning = false
  let newPeerAddress = ''
  let sortBy: 'reputation' | 'sharedFiles' | 'totalSize' | 'nickname' | 'location' | 'joinDate' | 'lastSeen' | 'status' = 'reputation'
  let sortDirection: 'asc' | 'desc' = 'desc'
  
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
  
  // DHT variables
  let dhtStatus: 'disconnected' | 'connecting' | 'connected' = 'disconnected'
  let dhtPeerId: string | null = null
  let dhtPort = 4001
  let dhtBootstrapNode = DEFAULT_BOOTSTRAP_NODES[0] || 'No bootstrap nodes configured'
  let dhtEvents: string[] = []
  let dhtPeerCount = 0
  let dhtHealth: DhtHealth | null = null
  let dhtError: string | null = null
  let connectionAttempts = 0
  let dhtPollInterval: number | undefined
  
  // UI variables
  const nodeAddress = "enode://277ac35977fc0a230e3ca4ccbf6df6da486fd2af9c129925b1193b25da6f013a301788fceed458f03c6c0d289dfcbf7a7ca5c0aef34b680fcbbc8c2ef79c0f71@127.0.0.1:30303"
  let copiedNodeAddr = false
  let copiedPeerId = false
  let copiedBootstrap = false
  let copiedListenAddr: string | null = null

  function formatSize(bytes: number): string {
    const units = ['B', 'KB', 'MB', 'GB', 'TB', 'PB']
    let size = bytes
    let unitIndex = 0
    
    while (size >= 1024 && unitIndex < units.length - 1) {
      size /= 1024
      unitIndex++
    }
    
    return `${size.toFixed(2)} ${units[unitIndex]}`
  }

  function formatHealthTimestamp(epoch: number | null): string {
    if (!epoch) return tr('network.dht.health.never')
    return new Date(epoch * 1000).toLocaleString()
  }

  function formatHealthMessage(value: string | null): string {
    return value ?? tr('network.dht.health.none')
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
            bootstrapNodes: DEFAULT_BOOTSTRAP_NODES
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
      if (DEFAULT_BOOTSTRAP_NODES.length > 0) {
        console.log('Attempting to connect to bootstrap nodes:', DEFAULT_BOOTSTRAP_NODES)
        dhtEvents = [...dhtEvents, `[Attempt ${connectionAttempts}] Connecting to ${DEFAULT_BOOTSTRAP_NODES.length} bootstrap node(s)...`]
        
        // Add another small delay to show the connection attempt
        await new Promise(resolve => setTimeout(resolve, 1000))
        
        try {
          // Try connecting to the first available bootstrap node
          await dhtService.connectPeer(DEFAULT_BOOTSTRAP_NODES[0])
          console.log('Connection initiated to bootstrap nodes')
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

        // Pull health/peers and update UI without attempting a restart
        const health = await dhtService.getHealth()
        if (health) {
          dhtHealth = health
          dhtPeerCount = health.peerCount
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
  
  function startDhtPolling() {
    if (dhtPollInterval) return // Already polling
    
    dhtPollInterval = setInterval(async () => {
      try {
        const events = await dhtService.getEvents() as any[]
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
          dhtPeerCount = peerCount
        } else {
          peerCount = await dhtService.getPeerCount()
          dhtPeerCount = peerCount
        }

        // Update connection status based on peer count
        if (dhtStatus === 'connected' && peerCount === 0) {
          dhtStatus = 'disconnected'
          dhtEvents = [...dhtEvents, '⚠ Lost connection to all peers']
        } else if (dhtStatus === 'disconnected' && peerCount > 0) {
          dhtStatus = 'connected'
          dhtEvents = [...dhtEvents, `✓ Reconnected to ${peerCount} peer(s)`]
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
    } catch (error) {
      console.error('Failed to stop DHT:', error)
      dhtEvents = [...dhtEvents, `✗ Failed to stop DHT: ${error}`]
    }
  }

  function runDiscovery() {
    if (dhtStatus !== 'connected') {
      showToast($t('network.errors.dhtNotConnected'), 'error');
      return;
    }
    discoveryRunning = true
    
    // Simulate discovering new peers
    setTimeout(() => {
      const newPeer = {
        id: `peer-${Date.now()}`,
        address: `${Math.floor(Math.random() * 255)}.${Math.floor(Math.random() * 255)}.${Math.floor(Math.random() * 255)}.${Math.floor(Math.random() * 255)}`,
        nickname: `Node${Math.floor(Math.random() * 1000)}`,
        status: 'online' as const,
        reputation: 3 + Math.random() * 2,
        sharedFiles: Math.floor(Math.random() * 500),
        totalSize: Math.floor(Math.random() * 10737418240),
        joinDate: new Date(),
        lastSeen: new Date(),
        location: ['US-East', 'EU-West', 'Asia-Pacific', 'US-West'][Math.floor(Math.random() * 4)]
      }
      
      peers.update(p => [...p, newPeer])
      networkStats.update(s => ({
        ...s,
        totalPeers: s.totalPeers + 1,
        onlinePeers: s.onlinePeers + 1
      }))
      
      discoveryRunning = false
    }, 2000)
  }
  
  function connectToPeer() {
  if (!newPeerAddress.trim()) return
  
  const trimmedAddress = newPeerAddress.trim()
  
  // Parse IP and port first to check for duplicates properly
  const [ip, portStr] = trimmedAddress.split(':')
  const port = portStr ? parseInt(portStr) : 8080
  const fullAddress = portStr ? trimmedAddress : `${ip}:${port}`
  
  // Check if peer with this exact IP:port combination already exists
  const existingPeer = $peers.find(peer => peer.address === fullAddress)
  if (existingPeer) {
    showToast($t('Peer Already Connected'), 'error')
    return
  }
  
  // Basic IP format validation (supports IP:port format)
  const ipPortRegex = /^\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}(:\d+)?$/
  if (!ipPortRegex.test(trimmedAddress)) {
    showToast($t('Invalid IP Format'), 'error')
    return
  }
  
  // Validate IP ranges (0-255 for each octet)
  const ipParts = ip.split('.').map(Number)
  if (ipParts.some(part => part < 0 || part > 255)) {
    showToast($t('Invalid IP Range'), 'error')
    return
  }
  
  // Validate port range
  if (port < 1 || port > 65535) {
    showToast($t('Invalid Port Number'), 'error')
    return
  }
  
  // Create new peer
  const newPeer = {
    id: `peer-${Date.now()}`,
    address: fullAddress, // Use the normalized IP:port format
    nickname: `DirectPeer${Math.floor(Math.random() * 100)}`,
    status: 'online' as const,
    reputation: 0,
    sharedFiles: 0,
    totalSize: 0,
    joinDate: new Date(),
    lastSeen: new Date(),
    location: 'Unknown'
  }
  
  // Add to peers list
  peers.update(p => [...p, newPeer])
  
  // Update network stats
  networkStats.update(s => ({
    ...s,
    totalPeers: s.totalPeers + 1,
    onlinePeers: s.onlinePeers + 1
  }))
  
  // Clear input and show success
  newPeerAddress = ''
  showToast($t('Peer Connected Successfully'), 'success')
}
  
  function refreshStats() {
    networkStats.update(s => ({
      ...s,
      avgDownloadSpeed: 5 + Math.random() * 20,
      avgUploadSpeed: 3 + Math.random() * 15,
      onlinePeers: Math.floor(s.totalPeers * (0.6 + Math.random() * 0.3))
    }))
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
      // First check if geth is installed
      isGethInstalled = await invoke('check_geth_binary') as boolean
      
      if (isGethInstalled) {
        isGethRunning = await invoke('is_geth_running') as boolean
        if (isGethRunning) {
          isStartingNode = false
          startPolling()
        }
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
    } catch (e) {
      downloadError = String(e)
      isDownloading = false
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
    
    // Initialize async operations
    const initAsync = async () => {
      await checkGethStatus()
      
      // DHT check will happen in startDht()

      // Also passively sync DHT state if it's already running
      await syncDhtStatusOnMount()
      
      // Listen for download progress updates (only in Tauri)
      if (isTauri) {
        unlistenProgress = await listen('geth-download-progress', (event) => {
          downloadProgress = event.payload as typeof downloadProgress
        })
      }
    }
    
    initAsync()
    
    return () => {
      clearInterval(interval)
      if (peerCountInterval) {
        clearInterval(peerCountInterval)
      }
      if (unlistenProgress) {
        unlistenProgress()
      }
    }
  })

  onDestroy(() => {
    if (peerCountInterval) {
      clearInterval(peerCountInterval)
    }
    if (dhtPollInterval) {
      clearInterval(dhtPollInterval)
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
          <p class="text-xs text-yellow-500 mt-2">{$t('network.dht.attempt', { values: { connectionAttempts } })}</p>
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

          {#if dhtHealth?.listenAddrs && dhtHealth.listenAddrs.length > 0}
            <div class="pt-2 space-y-2">
              <p class="text-sm text-muted-foreground">{$t('network.dht.listenAddresses')}</p>
              {#each dhtHealth.listenAddrs as addr}
                <div class="bg-muted/40 rounded-lg px-3 py-2">
                  <div class="flex items-start justify-between gap-2">
                    <p class="text-xs font-mono break-all flex-1">{addr}</p>
                    <Button
                      variant="outline"
                      size="sm"
                      class="h-7 px-2 flex-shrink-0"
                      on:click={async () => {
                        await copy(addr)
                        copiedListenAddr = addr
                        setTimeout(() => (copiedListenAddr = null), 1200)
                      }}
                    >
                      <Clipboard class="h-3.5 w-3.5 mr-1" />
                      {copiedListenAddr === addr ? $t('network.copied') : $t('network.copy')}
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
        </div>
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
                    aVal = (a.location || 'zzzzz').toLowerCase() // Put empty locations at the end
                    bVal = (b.location || 'zzzzz').toLowerCase()
                    // Distance-based sorting: closer peers first
                    const getLocationDistance = (peerLocation: string | undefined) => {
                        if (!peerLocation) return 999; // Unknown locations go to the end
                        
                        // Distance map from user's location to other regions
                        const distanceMap: Record<string, Record<string, number>> = {
                            'US-East': { 'US-East': 0, 'US-West': 1, 'EU-West': 2, 'Asia-Pacific': 3 },
                            'US-West': { 'US-West': 0, 'US-East': 1, 'EU-West': 3, 'Asia-Pacific': 2 },
                            'EU-West': { 'EU-West': 0, 'US-East': 1, 'US-West': 3, 'Asia-Pacific': 2 },
                            'Asia-Pacific': { 'Asia-Pacific': 0, 'US-West': 1, 'EU-West': 2, 'US-East': 3 }
                        };
                        
                        return distanceMap[$userLocation]?.[peerLocation] ?? 999;
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
              ⭐ {peer.reputation.toFixed(1)}
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
  </Card>
</div>
