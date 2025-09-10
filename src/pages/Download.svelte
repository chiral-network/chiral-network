B •543 lines
•
Formatting may be inconsistent from source
<script lang="ts">
  import Button from '$lib/components/ui/button.svelte'
  import Card from '$lib/components/ui/card.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import Progress from '$lib/components/ui/progress.svelte'
  import { Search, Pause, Play, X, ChevronUp, ChevronDown, Settings, File } from 'lucide-svelte'
  import { files, downloadQueue } from '$lib/stores'
  
  let searchHash = ''
  let maxConcurrentDownloads = 3
  let autoStartQueue = true
  let filterStatus = 'all' // 'all', 'active', 'paused', 'queued', 'completed', 'failed'
  let activeSimulations = new Set<string>() // Track files with active progress simulations

  // Ensure maxConcurrentDownloads is always a valid number (minimum 1)
  $: maxConcurrentDownloads = Math.max(1, Number(maxConcurrentDownloads) || 3)
  
  // Combine all files and queue into single list with stable sorting
  $: allDownloads = (() => {
    const combined = [...$files, ...$downloadQueue]

    // If there's a search hash, prioritize matching files
    if (searchHash.trim()) {
      const matchingFiles = combined.filter(f => 
        f.hash.toLowerCase().includes(searchHash.toLowerCase()) ||
        f.name.toLowerCase().includes(searchHash.toLowerCase())
      )
      const nonMatchingFiles = combined.filter(f => 
        !f.hash.toLowerCase().includes(searchHash.toLowerCase()) &&
        !f.name.toLowerCase().includes(searchHash.toLowerCase())
      )
      
      // Sort matching files by status first, then non-matching files
      const statusOrder = {
        'downloading': 0,
        'paused': 1,
        'completed': 2,
        'queued': 3,
        'failed': 4,
        'uploaded': 5,
        'seeding': 6
      }
      
      const sortByStatus = (a, b) => {
        const statusA = statusOrder[a.status] ?? 999
        const statusB = statusOrder[b.status] ?? 999
        const statusDiff = statusA - statusB
        return statusDiff === 0 ? a.id.localeCompare(b.id) : statusDiff
      }
      
      return [
        ...matchingFiles.sort(sortByStatus),
        ...nonMatchingFiles.sort(sortByStatus)
      ]
    }

    // Normal sorting when no search hash
    const statusOrder = {
      'downloading': 0,
      'paused': 1,
      'completed': 2,
      'queued': 3,
      'failed': 4,
      'uploaded': 5,
      'seeding': 6
    }

    return combined.sort((a, b) => {
      const statusA = statusOrder[a.status] ?? 999
      const statusB = statusOrder[b.status] ?? 999
      const statusDiff = statusA - statusB

      // If status is the same, sort by ID for stable ordering
      if (statusDiff === 0) {
        return a.id.localeCompare(b.id)
      }

      return statusDiff
    })
  })()
  
  
  // Filter downloads based on selected status
  $: filteredDownloads = (() => {
    let filtered = allDownloads.filter(f => f.status !== 'uploaded' && f.status !== 'seeding')

    switch (filterStatus) {
      case 'active':
        return filtered.filter(f => f.status === 'downloading')
      case 'paused':
        return filtered.filter(f => f.status === 'paused')
      case 'queued':
        return filtered.filter(f => f.status === 'queued')
      case 'completed':
        return filtered.filter(f => f.status === 'completed')
      case 'failed':
        return filtered.filter(f => f.status === 'failed')
      default:
        return filtered
    }
  })()
  
  // Calculate counts from the filtered set (excluding uploaded/seeding)
  $: allFilteredDownloads = allDownloads.filter(f => f.status !== 'uploaded' && f.status !== 'seeding')
  $: activeCount = allFilteredDownloads.filter(f => f.status === 'downloading').length
  $: pausedCount = allFilteredDownloads.filter(f => f.status === 'paused').length
  $: queuedCount = allFilteredDownloads.filter(f => f.status === 'queued').length
  $: completedCount = allFilteredDownloads.filter(f => f.status === 'completed').length
  $: failedCount = allFilteredDownloads.filter(f => f.status === 'failed').length

  // Start progress simulation for any downloading files when component mounts
  $: if ($files.length > 0) {
    $files.forEach(file => {
      if (file.status === 'downloading' && !activeSimulations.has(file.id)) {
        // Start simulation only if not already active
        simulateDownloadProgress(file.id)
      }
    })
  }
  
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
      status: 'queued' as const,
      priority: 'normal' as const
    }
    
    // prevents duplicates in files or queue 
    const exists = [...$files, ...$downloadQueue].some(f => f.hash === searchHash)
    if (exists) {
      // Don't clear searchHash if file exists - keep it for search highlighting
      return
    }
    
    downloadQueue.update(q => [...q, newFile])
    processQueue()
  }

  // Function to clear search
function clearSearch() {
  searchHash = ''
}

  function processQueue() {
    // Only prevent starting new downloads if we've reached the max concurrent limit
    const activeDownloads = $files.filter(f => f.status === 'downloading').length
    if (activeDownloads >= maxConcurrentDownloads) return

    const nextFile = $downloadQueue[0]
    if (!nextFile) return
    downloadQueue.update(q => q.filter(f => f.id !== nextFile.id))
    const downloadingFile = { ...nextFile, status: 'downloading' as const, progress: 0 }
    files.update(f => [...f, downloadingFile])
    simulateDownloadProgress(downloadingFile.id)
  }
  
  function togglePause(fileId: string) {
    files.update(f => f.map(file => {
      if (file.id === fileId) {
        const newStatus = file.status === 'downloading' ? 'paused' as const : 'downloading' as const
        const updatedFile = { ...file, status: newStatus }

        // If resuming from paused to downloading, restart the simulation
        if (newStatus === 'downloading' && file.status === 'paused') {
          // Small delay to ensure DOM updates first
          setTimeout(() => simulateDownloadProgress(fileId), 100)
        }
        // If pausing from downloading to paused, stop the simulation
        else if (newStatus === 'paused' && file.status === 'downloading') {
          activeSimulations.delete(fileId)
        }

        return updatedFile
      }
      return file
    }))
  }
  
  function cancelDownload(fileId: string) {
    files.update(f => f.filter(file => file.id !== fileId))
    downloadQueue.update(q => q.filter(file => file.id !== fileId))
    // Clean up simulation tracking
    activeSimulations.delete(fileId)
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
    // Prevent duplicate simulations
    if (activeSimulations.has(fileId)) {
      return
    }

    activeSimulations.add(fileId)

    const interval = setInterval(() => {
      files.update(f => f.map(file => {
        if (file.id === fileId && file.status === 'downloading') {
          const speed = file.priority === 'high' ? 15 : file.priority === 'low' ? 5 : 10
          const newProgress = Math.min(100, (file.progress || 0) + Math.random() * speed)

          // 5% chance of failure when progress is between 20-80%
          const currentProgress = file.progress || 0
          if (currentProgress > 20 && currentProgress < 80 && Math.random() < 0.05) {
            clearInterval(interval)
            activeSimulations.delete(fileId)
            return { ...file, status: 'failed' }
          }

          if (newProgress >= 100) {
            clearInterval(interval)
            activeSimulations.delete(fileId)
            return { ...file, progress: 100, status: 'completed' }
          }
          return { ...file, progress: newProgress }
        } else if (file.id === fileId && file.status !== 'downloading') {
          // File is no longer downloading, stop simulation
          clearInterval(interval)
          activeSimulations.delete(fileId)
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
        <div class="flex flex-col sm:flex-row gap-2 mt-2">
            <Input
              id="hash-input"
              bind:value={searchHash}
              placeholder="Enter file hash (e.g., QmZ4tDuvesekqMD...)"
              class="flex-1"
            />
            <div class="flex gap-2">
              <Button on:click={startDownload} disabled={!searchHash}>
                <Search class="h-4 w-4 mr-2" />
                Search & Download
              </Button>
              {#if searchHash}
                <Button variant="outline" on:click={clearSearch}>
                  <X class="h-4 w-4 mr-2" />
                  Clear
                </Button>
              {/if}
            </div>
          </div>
          {#if searchHash}
            <p class="text-xs text-muted-foreground mt-1">
              Searching for: <span class="font-mono">{searchHash}</span>
            </p>
          {/if}
      </div>
    </div>
  </Card>
  
  <!-- Unified Downloads List -->
  <Card class="p-6">
    <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4 mb-4">
      <div class="flex flex-col sm:flex-row sm:items-center gap-2 sm:gap-4">
        <h2 class="text-lg font-semibold">Downloads</h2>
        <div class="flex flex-wrap gap-2">
          <Button
            size="sm"
            variant={filterStatus === 'all' ? 'default' : 'outline'}
            on:click={() => filterStatus = 'all'}
          >
            All ({allFilteredDownloads.length})
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
            variant={filterStatus === 'paused' ? 'default' : 'outline'}
            on:click={() => filterStatus = 'paused'}
          >
            Paused ({pausedCount})
          </Button>
          <Button
            size="sm"
            variant={filterStatus === 'completed' ? 'default' : 'outline'}
            on:click={() => filterStatus = 'completed'}
          >
            Completed ({completedCount})
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
            variant={filterStatus === 'failed' ? 'default' : 'outline'}
            on:click={() => filterStatus = 'failed'}
          >
            Failed ({failedCount})
          </Button>
        </div>
      </div>

      <div class="flex flex-col sm:flex-row sm:items-center gap-2 w-full sm:w-auto">
        <div class="flex items-center gap-2 text-sm flex-wrap">
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
        {:else if filterStatus === 'paused'}
          No paused downloads.
        {:else if filterStatus === 'queued'}
          No files in queue.
        {:else if filterStatus === 'completed'}
          No completed downloads.
        {:else}
          No failed downloads.
        {/if}
      </p>
    {:else}
      <div class="space-y-3">
        {#each filteredDownloads as file, index}

          <div class="p-4 bg-secondary rounded-lg">
            <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-2 sm:gap-4 w-full flex-wrap">
              <div class="flex flex-wrap items-center gap-2">
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
                <div class="flex-1 min-w-0">
                  <p class="font-medium truncate">{file.name}</p>
                  <p class="text-xs text-muted-foreground truncate">Hash: {file.hash}</p>
                </div>
              </div>

              <div class="flex flex-wrap items-center gap-2 mt-2 sm:mt-0">
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
                <Badge class={
                  file.status === 'downloading' ? 'bg-green-500 text-white border-green-500' :
                  file.status === 'completed' ? 'bg-blue-500 text-white border-blue-500' :
                  file.status === 'paused' ? 'bg-yellow-500 text-black border-yellow-500' :
                  file.status === 'queued' ? 'bg-gray-500 text-white border-gray-500' : 'bg-red-500 text-white border-red-500'
                }>
                  {file.status === 'queued' ? `Queued #${filteredDownloads.filter(f => f.status === 'queued').indexOf(file) + 1}` : file.status}
                </Badge>
              </div>
            </div>
            
            {#if file.status === 'downloading' || file.status === 'paused'}
              <div class="space-y-2 mt-3">
                <div class="flex items-center text-sm">
                  <span>Progress: {(file.progress || 0).toFixed(2)}%</span>
                </div>
                <Progress value={file.progress || 0} max={100} class="bg-gray-200" />
              </div>
            {/if}
            
            {#if file.status === 'downloading' || file.status === 'paused' || file.status === 'queued'}
              <div class="flex flex-wrap gap-2 mt-3">
                {#if file.status !== 'queued'}
                  <Button
                    size="sm"
                    variant="outline"
                    on:click={() => togglePause(file.id)}
                    class="flex-1 min-w-[100px] sm:flex-none"
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
                  class="flex-1 min-w-[100px] sm:flex-none"
                >
                  <X class="h-3 w-3 mr-1" />
                  {file.status === 'queued' ? 'Remove' : 'Cancel'}
                </Button>
              </div>
            {/if}
            
            {#if file.status === 'completed'}
              <div class="flex flex-wrap gap-2 mt-3">
                <Button
                        size="sm"
                        variant="outline"
                        class="flex-1 min-w-[100px] sm:flex-none"
                >
                  <File class="h-3 w-3 mr-1" />
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