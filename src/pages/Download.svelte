<script lang="ts">
  import Button from '$lib/components/ui/button.svelte'
  import Card from '$lib/components/ui/card.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import Progress from '$lib/components/ui/progress.svelte'
  import { Search, Pause, Play, X, ChevronUp, ChevronDown, Settings, Clock } from 'lucide-svelte'
  import { files, downloadQueue } from '$lib/stores'
  
  let searchHash = ''
  let maxConcurrentDownloads = 3
  let autoStartQueue = true
  let filterStatus = 'all' // 'all', 'active', 'queued', 'completed'

  // Ensure maxConcurrentDownloads is always a valid number (minimum 1)
  $: maxConcurrentDownloads = Math.max(1, Number(maxConcurrentDownloads) || 3)
  
  // Combine all files and queue into single list
  $: allDownloads = [...$files, ...$downloadQueue].sort((a, b) => {
    // Sort by status priority: downloading > queued > paused > completed > failed
    const statusOrder = {
      'downloading': 0,
      'queued': 1,
      'paused': 2,
      'completed': 3,
      'failed': 4,
      'uploaded': 5,
      'seeding': 6
    }
    return (statusOrder[a.status] || 999) - (statusOrder[b.status] || 999)
  })
  
  // Filter downloads based on selected status
  $: filteredDownloads = filterStatus === 'all' 
    ? allDownloads.filter(f => f.status !== 'uploaded' && f.status !== 'seeding')
    : filterStatus === 'active'
    ? allDownloads.filter(f => f.status === 'downloading' || f.status === 'paused')
    : filterStatus === 'queued'
    ? allDownloads.filter(f => f.status === 'queued')
    : allDownloads.filter(f => f.status === 'completed')
  
  $: activeCount = allDownloads.filter(f => f.status === 'downloading').length
  $: queuedCount = allDownloads.filter(f => f.status === 'queued').length
  $: completedCount = allDownloads.filter(f => f.status === 'completed').length
  
  // Process download queue
  $: {
    if (autoStartQueue) {
      const activeDownloads = $files.filter(f => f.status === 'downloading').length
      const queued = $downloadQueue.filter(f => f.status === 'queued')
      
      if (activeDownloads < maxConcurrentDownloads && queued.length > 0) {
        // Start next queued download
        const nextFile = queued.sort((a, b) => {
          // Priority order: high > normal > low
          const priorityOrder = { high: 3, normal: 2, low: 1 }
          return (priorityOrder[b.priority || 'normal'] - priorityOrder[a.priority || 'normal'])
        })[0]
        
        if (nextFile) {
          startQueuedDownload(nextFile.id)
        }
      }
    }
  }
  
  function startDownload() {
    if (!searchHash) return
    
    const newFile = {
      id: `download-${Date.now()}`,
      name: 'File_' + searchHash.substring(0, 8) + '.dat',
      hash: searchHash,
      size: Math.floor(Math.random() * 100000000),
      price: Math.random() * 5,
      status: 'downloading' as const,
      progress: 0
    }
    
    files.update(f => [...f, newFile])
    searchHash = ''
    
    // Add to queue if max concurrent downloads reached
    if ($files.filter(f => f.status === 'downloading').length >= maxConcurrentDownloads) {
      downloadQueue.update(q => [...q, { ...newFile, status: 'queued', priority: 'normal' }])
    } else {
      // Start download immediately
      simulateDownloadProgress(newFile.id)
    }
  }
  
  function togglePause(fileId: string) {
    files.update(f => f.map(file => {
      if (file.id === fileId) {
        return { 
          ...file, 
          status: file.status === 'downloading' ? 'paused' : 'downloading'
        }
      }
      return file
    }))
  }
  
  function cancelDownload(fileId: string) {
    files.update(f => f.filter(file => file.id !== fileId))
    downloadQueue.update(q => q.filter(file => file.id !== fileId))
  }
  
  function startQueuedDownload(fileId: string) {
    downloadQueue.update(queue => {
      const file = queue.find(f => f.id === fileId)
      if (file) {
        files.update(f => [...f, { ...file, status: 'downloading', progress: 0 }])
        simulateDownloadProgress(fileId)
      }
      return queue.filter(f => f.id !== fileId)
    })
  }
  
  function simulateDownloadProgress(fileId: string) {
    const interval = setInterval(() => {
      files.update(f => f.map(file => {
        if (file.id === fileId && file.status === 'downloading') {
          const speed = file.priority === 'high' ? 15 : file.priority === 'low' ? 5 : 10
          const newProgress = Math.min(100, (file.progress || 0) + Math.random() * speed)
          if (newProgress >= 100) {
            clearInterval(interval)
            return { ...file, progress: 100, status: 'completed' }
          }
          return { ...file, progress: newProgress }
        }
        return file
      }))
    }, 1000)
  }
  
  function changePriority(fileId: string, priority: 'low' | 'normal' | 'high') {
    downloadQueue.update(queue => queue.map(file => 
      file.id === fileId ? { ...file, priority } : file
    ))
  }
  
  function moveInQueue(fileId: string, direction: 'up' | 'down') {
    downloadQueue.update(queue => {
      const index = queue.findIndex(f => f.id === fileId)
      if (index === -1) return queue
      
      const newIndex = direction === 'up' ? Math.max(0, index - 1) : Math.min(queue.length - 1, index + 1)
      if (index === newIndex) return queue
      
      const newQueue = [...queue]
      const [removed] = newQueue.splice(index, 1)
      newQueue.splice(newIndex, 0, removed)
      return newQueue
    })
  }
  
  function formatFileSize(bytes: number): string {
    if (bytes < 1024) return bytes + ' B'
    if (bytes < 1048576) return (bytes / 1024).toFixed(2) + ' KB'
    return (bytes / 1048576).toFixed(2) + ' MB'
  }
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-3xl font-bold">Download Files</h1>
    <p class="text-muted-foreground mt-2">Download files from the network using their hash</p>
  </div>
  
  <Card class="p-6">
    <div class="space-y-4">
      <div>
        <Label for="hash-input">File Hash</Label>
        <div class="flex gap-2 mt-2">
          <Input
            id="hash-input"
            bind:value={searchHash}
            placeholder="Enter file hash (e.g., QmZ4tDuvesekqMD...)"
            class="flex-1"
          />
          <Button on:click={startDownload} disabled={!searchHash}>
            <Search class="h-4 w-4 mr-2" />
            Search & Download
          </Button>
        </div>
      </div>
    </div>
  </Card>
  
  <!-- Unified Downloads List -->
  <Card class="p-6">
    <div class="flex items-center justify-between mb-4">
      <div class="flex items-center gap-4">
        <h2 class="text-lg font-semibold">Downloads</h2>
        <div class="flex gap-2">
          <Button
            size="sm"
            variant={filterStatus === 'all' ? 'default' : 'outline'}
            on:click={() => filterStatus = 'all'}
          >
            All ({allDownloads.filter(f => f.status !== 'uploaded' && f.status !== 'seeding').length})
          </Button>
          <Button
            size="sm"
            variant={filterStatus === 'active' ? 'default' : 'outline'}
            on:click={() => filterStatus = 'active'}
          >
            Active ({activeCount})
          </Button>
          <Button
            size="sm"
            variant={filterStatus === 'queued' ? 'default' : 'outline'}
            on:click={() => filterStatus = 'queued'}
          >
            Queued ({queuedCount})
          </Button>
          <Button
            size="sm"
            variant={filterStatus === 'completed' ? 'default' : 'outline'}
            on:click={() => filterStatus = 'completed'}
          >
            Completed ({completedCount})
          </Button>
        </div>
      </div>
      
      <div class="flex items-center gap-2">
        <div class="flex items-center gap-2 text-sm">
          <Settings class="h-4 w-4" />
          <Label>Max Concurrent:</Label>
          <Input
            type="number"
            bind:value={maxConcurrentDownloads}
            min="1"
            step="1"
            class="w-16 pl-2 pr-2 py-1 text-center"
            placeholder="3"
          />
        </div>
        <Button
          size="sm"
          variant="outline"
          on:click={() => autoStartQueue = !autoStartQueue}
        >
          {autoStartQueue ? 'Auto-Start ON' : 'Auto-Start OFF'}
        </Button>
      </div>
    </div>
    
    {#if filteredDownloads.length === 0}
      <p class="text-sm text-muted-foreground text-center py-8">
        {#if filterStatus === 'all'}
          No downloads yet. Enter a file hash above to start downloading.
        {:else if filterStatus === 'active'}
          No active downloads.
        {:else if filterStatus === 'queued'}
          No files in queue.
        {:else}
          No completed downloads.
        {/if}
      </p>
    {:else}
      <div class="space-y-3">
        {#each filteredDownloads as file, index}
          <div class="p-4 border rounded-lg">
            <div class="flex items-center justify-between mb-2">
              <div class="flex items-center gap-3">
                {#if file.status === 'queued'}
                  <div class="flex flex-col gap-1">
                    <button
                      on:click={() => moveInQueue(file.id, 'up')}
                      disabled={index === 0}
                      class="p-0.5 hover:bg-accent rounded disabled:opacity-30"
                    >
                      <ChevronUp class="h-3 w-3" />
                    </button>
                    <button
                      on:click={() => moveInQueue(file.id, 'down')}
                      disabled={index === filteredDownloads.filter(f => f.status === 'queued').length - 1}
                      class="p-0.5 hover:bg-accent rounded disabled:opacity-30"
                    >
                      <ChevronDown class="h-3 w-3" />
                    </button>
                  </div>
                {/if}
                <div class="flex-1">
                  <p class="font-medium">{file.name}</p>
                  <p class="text-xs text-muted-foreground">Hash: {file.hash}</p>
                </div>
              </div>
              
              <div class="flex items-center gap-2">
                {#if file.status === 'queued'}
                  <select
                    value={file.priority || 'normal'}
                    on:change={(e) => {
                      const target = e.target as HTMLSelectElement;
                      if (target) changePriority(file.id, target.value as 'low' | 'normal' | 'high');
                    }}
                    class="text-xs px-2 py-1 border rounded bg-background"
                  >
                    <option value="low">Low Priority</option>
                    <option value="normal">Normal</option>
                    <option value="high">High Priority</option>
                  </select>
                {/if}
                <Badge variant="outline">{formatFileSize(file.size)}</Badge>
                <Badge variant={
                  file.status === 'downloading' ? 'default' : 
                  file.status === 'completed' ? 'secondary' :
                  file.status === 'queued' ? 'outline' :
                  file.status === 'paused' ? 'outline' : 'destructive'
                }>
                  {file.status === 'queued' ? `Queued #${filteredDownloads.filter(f => f.status === 'queued').indexOf(file) + 1}` : file.status}
                </Badge>
              </div>
            </div>
            
            {#if file.status === 'downloading' || file.status === 'paused'}
              <div class="space-y-2 mt-3">
                <div class="flex items-center justify-between text-sm">
                  <span>Progress</span>
                  <span>{(file.progress || 0).toFixed(1)}%</span>
                </div>
                <Progress value={file.progress || 0} max={100} />
              </div>
            {/if}
            
            {#if file.status === 'downloading' || file.status === 'paused' || file.status === 'queued'}
              <div class="flex gap-2 mt-3">
                {#if file.status !== 'queued'}
                  <Button
                    size="sm"
                    variant="outline"
                    on:click={() => togglePause(file.id)}
                  >
                    {#if file.status === 'downloading'}
                      <Pause class="h-3 w-3 mr-1" />
                      Pause
                    {:else}
                      <Play class="h-3 w-3 mr-1" />
                      Resume
                    {/if}
                  </Button>
                {/if}
                <Button
                  size="sm"
                  variant="destructive"
                  on:click={() => cancelDownload(file.id)}
                >
                  <X class="h-3 w-3 mr-1" />
                  {file.status === 'queued' ? 'Remove' : 'Cancel'}
                </Button>
              </div>
            {/if}
            
            {#if file.status === 'completed'}
              <div class="flex items-center justify-between mt-3 text-sm text-muted-foreground">
                <span>Download complete</span>
                <Button size="sm" variant="outline">
                  <Clock class="h-3 w-3 mr-1" />
                  Open File
                </Button>
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {/if}
  </Card>
</div>