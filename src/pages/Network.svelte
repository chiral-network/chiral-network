<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import Button from '$lib/components/ui/button.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import { Users, HardDrive, Activity, RefreshCw, UserPlus, Signal } from 'lucide-svelte'
  import { peers, networkStats, networkStatus, userLocation } from '$lib/stores'
  import { onMount } from 'svelte'
  
  let discoveryRunning = false
  let newPeerAddress = ''
  let sortBy: 'reputation' | 'sharedFiles' | 'totalSize' | 'nickname' | 'location' | 'joinDate' | 'lastSeen' | 'status' = 'reputation'
  let sortDirection: 'asc' | 'desc' = 'desc'
  
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
  
  onMount(() => {
    const interval = setInterval(refreshStats, 5000)
    return () => clearInterval(interval)
  })
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-3xl font-bold">Network Overview</h1>
    <p class="text-muted-foreground mt-2">Monitor network health and discover peers</p>
  </div>
  
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
                  <option value="asc">↑ Ascending</option>
                  <option value="desc">↓ Descending</option>
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