<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import Button from '$lib/components/ui/button.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import { Users, HardDrive, Activity, RefreshCw, UserPlus, Signal, Server, Play, Square, Download, AlertCircle } from 'lucide-svelte'
  import { peers, networkStats, networkStatus, userLocation, etcAccount } from '$lib/stores'
  import { onMount, onDestroy } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import { listen } from '@tauri-apps/api/event'
  
  // Check if running in Tauri environment
  const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window
  
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
  let downloadProgress = {
    downloaded: 0,
    total: 0,
    percentage: 0,
    status: ''
  }
  let downloadError = ''
  let dataDir = './geth-data'
  let peerCount = 0
  let peerCountInterval: number | undefined
  let chainId = 98765
  
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
  
  function runDiscovery() {
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
    if (!newPeerAddress) return
    
    const newPeer = {
      id: `peer-${Date.now()}`,
      address: newPeerAddress,
      nickname: `DirectPeer${Math.floor(Math.random() * 100)}`,
      status: 'online' as const,
      reputation: 0,
      sharedFiles: 0,
      totalSize: 0,
      joinDate: new Date(),
      lastSeen: new Date(),
      location: 'Unknown'
    }
    
    peers.update(p => [...p, newPeer])
    newPeerAddress = ''
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
      return
    }
    
    try {
      // First check if geth is installed
      isGethInstalled = await invoke('check_geth_binary') as boolean
      
      if (isGethInstalled) {
        isGethRunning = await invoke('is_geth_running') as boolean
        if (isGethRunning) {
          startPolling()
        }
      }
    } catch (error) {
      console.error('Failed to check geth status:', error)
    }
  }
  
  async function downloadGeth() {
    if (!isTauri) {
      downloadError = 'Chiral Node download is only available in the desktop app. Please download and run the Tauri desktop version.'
      return
    }
    
    isDownloading = true
    downloadError = ''
    downloadProgress = {
      downloaded: 0,
      total: 0,
      percentage: 0,
      status: 'Starting download...'
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
    peerCountInterval = setInterval(fetchPeerCount, 5000) as unknown as number
  }

  async function startGethNode() {
    if (!isTauri) {
      console.log('Cannot start Chiral Node in web mode - desktop app required')
      return
    }
    
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
      peerCount = 0
      if (peerCountInterval) {
        clearInterval(peerCountInterval)
        peerCountInterval = undefined
      }
    } catch (error) {
      console.error('Failed to stop geth node:', error)
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
      
      // Listen for download progress updates (only in Tauri)
      if (isTauri) {
        unlistenProgress = await listen('geth-download-progress', (event) => {
          downloadProgress = event.payload as typeof downloadProgress
        })
      }
      
      // Auto-start geth if installed but not running
      if (isGethInstalled && !isGethRunning) {
        await startGethNode()
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
  })
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-3xl font-bold">Network Overview</h1>
    <p class="text-muted-foreground mt-2">Monitor network health and discover peers</p>
  </div>
  
  <!-- Chiral Network Node Status Card -->
  <Card class="p-6">
    <div class="flex items-center justify-between mb-4">
      <h2 class="text-lg font-semibold">Chiral Node Status</h2>
      <div class="flex items-center gap-2">
        {#if !isGethInstalled}
          <div class="h-2 w-2 bg-yellow-500 rounded-full"></div>
          <span class="text-sm text-yellow-600">Not Installed</span>
        {:else if isDownloading}
          <div class="h-2 w-2 bg-blue-500 rounded-full animate-pulse"></div>
          <span class="text-sm text-blue-600">Downloading...</span>
        {:else if isGethRunning}
          <div class="h-2 w-2 bg-green-500 rounded-full animate-pulse"></div>
          <span class="text-sm text-green-600">Connected</span>
        {:else}
          <div class="h-2 w-2 bg-red-500 rounded-full"></div>
          <span class="text-sm text-red-600">Disconnected</span>
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
                <span>Progress</span>
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
            <p class="text-sm text-muted-foreground mb-1">Chiral node binary not found</p>
            <p class="text-xs text-muted-foreground mb-3">Download the Core-Geth binary to run a local node</p>
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
              Download Chiral Node (~50 MB)
            </Button>
          </div>
        {/if}
      {:else if !isGethRunning}
        <div class="text-center py-4">
          <Server class="h-12 w-12 text-muted-foreground mx-auto mb-2" />
          <p class="text-sm text-muted-foreground mb-3">Chiral node is not running</p>
          <Button on:click={startGethNode}>
            <Play class="h-4 w-4 mr-2" />
            Start Chiral Node
          </Button>
        </div>
      {:else}
        <div class="grid grid-cols-2 gap-4">
          <div class="bg-secondary rounded-lg p-3">
            <p class="text-sm text-muted-foreground">Chiral Peers</p>
            <p class="text-2xl font-bold">{peerCount}</p>
          </div>
          <div class="bg-secondary rounded-lg p-3">
            <p class="text-sm text-muted-foreground">Chain ID</p>
            <p class="text-2xl font-bold">{chainId}</p>
          </div>
        </div>
        <div class="pt-2">
          <p class="text-sm text-muted-foreground mb-1">Node Address</p>
          <p class="text-xs font-mono break-all">enode://277ac35977fc0a230e3ca4ccbf6df6da486fd2af9c129925b1193b25da6f013a301788fceed458f03c6c0d289dfcbf7a7ca5c0aef34b680fcbbc8c2ef79c0f71@127.0.0.1:30303</p>
        </div>
        <Button class="w-full" variant="outline" on:click={stopGethNode}>
          <Square class="h-4 w-4 mr-2" />
          Stop Chiral Node
        </Button>
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
          <p class="text-sm text-muted-foreground">Network Status</p>
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
          <p class="text-sm text-muted-foreground">Active Peers</p>
          <p class="text-xl font-bold">{$networkStats.onlinePeers}/{$networkStats.totalPeers}</p>
        </div>
      </div>
    </Card>
    
    <Card class="p-4">
      <div class="flex items-center gap-3">
        <div class="p-2 bg-purple-500/10 rounded-lg">
          <HardDrive class="h-5 w-5 text-purple-500" />
        </div>
        <div>
          <p class="text-sm text-muted-foreground">Network Size</p>
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
          <p class="text-sm text-muted-foreground">Bandwidth</p>
          <p class="text-sm font-bold">↓ {$networkStats.avgDownloadSpeed.toFixed(1)} MB/s</p>
          <p class="text-sm font-bold">↑ {$networkStats.avgUploadSpeed.toFixed(1)} MB/s</p>
        </div>
      </div>
    </Card>
  </div>
  
  <!-- Peer Discovery -->
  <Card class="p-6">
    <div class="flex flex-wrap items-center justify-between gap-2 mb-4">
      <h2 class="text-lg font-semibold">Peer Discovery</h2>
      <div class="flex-shrink-0">
        <Button
          size="sm"
          variant="outline"
          on:click={runDiscovery}
          disabled={discoveryRunning}
        >
          <RefreshCw class="h-4 w-4 mr-2 {discoveryRunning ? 'animate-spin' : ''}" />
          {discoveryRunning ? 'Discovering...' : 'Run Discovery'}
        </Button>
      </div>
    </div>
    
    <div class="space-y-4">
      <div>
        <Label for="peer-address">Direct Connect</Label>
        <div class="flex flex-wrap gap-2 mt-2">
          <Input
            id="peer-address"
            bind:value={newPeerAddress}
            placeholder="Enter peer address (IP:Port or peer ID)"
            class="flex-1 min-w-0 break-all"
          />
          <Button on:click={connectToPeer} disabled={!newPeerAddress}>
            <UserPlus class="h-4 w-4 mr-2" />
            Connect
          </Button>
        </div>
      </div>
    </div>
  </Card>
  
  <!-- Connected Peers -->
  <Card class="p-6">
      <div class="flex flex-wrap items-center justify-between gap-2 mb-4">
          <h2 class="text-lg font-semibold">Connected Peers ({$peers.length})</h2>
          <div class="flex items-center gap-2">
              <Label for="sort">Sort By</Label>
              <select
                      id="sort"
                      bind:value={sortBy}
                      class="border rounded px-2 py-1 text-sm"
              >
                  <option value="reputation">Reputation</option>
                  <option value="sharedFiles">Shared Files</option>
                  <option value="totalSize">Total Size</option>
                  <option value="nickname">Name</option>
                  <option value="location">Location</option>
                  <option value="joinDate">Join Date</option>
                  <option value="lastSeen">Last Seen</option>
                  <option value="status">Status</option>
              </select>
              <select
                      id="sort-direction"
                      bind:value={sortDirection}
                      class="border rounded px-2 py-1 text-sm"
              >
                  {#if sortBy === 'reputation'}
                      <option value="desc">Highest</option>
                      <option value="asc">Lowest</option>
                  {:else if sortBy === 'sharedFiles'}
                      <option value="desc">Most</option>
                      <option value="asc">Least</option>
                  {:else if sortBy === 'totalSize'}
                      <option value="desc">Largest</option>
                      <option value="asc">Smallest</option>
                  {:else if sortBy === 'joinDate'}
                      <option value="desc">Newest</option>
                      <option value="asc">Oldest</option>
                  {:else if sortBy === 'lastSeen'}
                      <option value="desc">Most Recent</option>
                      <option value="asc">Least Recent</option>
                  {:else if sortBy === 'location'}
                      <option value="asc">Closest</option>
                      <option value="desc">Farthest</option>
                  {:else if sortBy === 'status'}
                      <option value="asc">Online</option>
                      <option value="desc">Offline</option>
                  {:else if sortBy === 'nickname'}
                      <option value="asc">A → Z</option>
                      <option value="desc">Z → A</option>
                  {/if}
              </select>
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
                <p class="font-medium">{peer.nickname || 'Anonymous'}</p>
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
                          peer.status === 'away' ? 'bg-yellow-500 text-black' :
                          'bg-red-500 text-white'
                        }
                >
                {peer.status}
              </Badge>
            </div>
          </div>
          
          <div class="grid grid-cols-2 md:grid-cols-5 gap-4 text-sm">
            <div>
              <p class="text-xs text-muted-foreground">Shared Files</p>
              <p class="font-medium">{peer.sharedFiles}</p>
            </div>
            <div>
              <p class="text-xs text-muted-foreground">Total Size</p>
              <p class="font-medium">{formatSize(peer.totalSize)}</p>
            </div>
            <div>
              <p class="text-xs text-muted-foreground">Location</p>
              <p class="font-medium">{peer.location || 'Unknown'}</p>
            </div>
            <div>
              <p class="text-xs text-muted-foreground">Joined</p>
              <p class="font-medium">{new Date(peer.joinDate).toLocaleString()}</p>
            </div>
            <div>
              <p class="text-xs text-muted-foreground">Last Seen</p>
              <p class="font-medium">
                {#if peer.status === 'online'}
                  Now
                {:else}
                  {new Date(peer.lastSeen).toLocaleString()}
                {/if}
              </p>
            </div>
          </div>
        </div>
      {/each}
      
      {#if $peers.length === 0}
        <p class="text-center text-muted-foreground py-8">No peers connected. Run discovery to find peers.</p>
      {/if}
    </div>
  </Card>
</div>
