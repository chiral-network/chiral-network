<script lang="ts">
  import Button from '$lib/components/ui/button.svelte'
  import Card from '$lib/components/ui/card.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import Progress from '$lib/components/ui/progress.svelte'
  import { Search, Pause, Play, X, ChevronUp, ChevronDown, Settings, File } from 'lucide-svelte'
  import { files, downloadQueue } from '$lib/stores'
  
  let searchHash = ''  // For downloading new files
  let searchFilter = ''  // For searching existing downloads
  let maxConcurrentDownloads: string | number = 3
  let lastValidMaxConcurrent = 3 // Store the last valid value
  let autoStartQueue = true
  let filterStatus = 'all' // 'all', 'active', 'paused', 'queued', 'completed', 'failed'
  let activeSimulations = new Set<string>() // Track files with active progress simulations

  // Add notification related variables
  let currentNotification: HTMLElement | null = null

  // Show notification function
  function showNotification(message: string, type: 'success' | 'error' | 'info' | 'warning' = 'success', duration = 4000) {
    // Remove existing notification
    if (currentNotification) {
      currentNotification.remove()
      currentNotification = null
    }
    
    const colors = {
      success: '#22c55e',
      error: '#ef4444', 
      info: '#3b82f6',
      warning: '#f59e0b'
    }
    
    const notification = document.createElement('div')
    notification.style.cssText = `
      position: fixed;
      top: 20px;
      right: 20px;
      background: ${colors[type]};
      color: white;
      padding: 12px 16px;
      border-radius: 8px;
      box-shadow: 0 4px 12px rgba(0, 0, 0, 0.15);
      z-index: 10000;
      font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
      font-size: 14px;
      font-weight: 500;
      max-width: 320px;
      animation: slideInRight 0.3s ease-out;
      display: flex;
      align-items: center;
      gap: 8px;
    `
    
    // Add CSS animation styles
    if (!document.querySelector('#download-notification-styles')) {
      const style = document.createElement('style')
      style.id = 'download-notification-styles'
      style.textContent = `
        @keyframes slideInRight {
          from { transform: translateX(100%); opacity: 0; }
          to { transform: translateX(0); opacity: 1; }
        }
      `
      document.head.appendChild(style)
    }
    
    notification.innerHTML = `
      <span>${message}</span>
      <button onclick="this.parentElement.remove()" style="
        background: none;
        border: none;
        color: white;
        font-size: 18px;
        cursor: pointer;
        padding: 0;
        margin-left: 8px;
        opacity: 0.8;
      ">×</button>
    `
    
    document.body.appendChild(notification)
    currentNotification = notification
    
    // Auto remove
    setTimeout(() => {
      if (notification.parentNode) {
        notification.remove()
        if (currentNotification === notification) {
          currentNotification = null
        }
      }
    }, duration)
  }

  // Function to validate and correct maxConcurrentDownloads
  function validateMaxConcurrent() {
    // If empty or invalid, revert to last valid value
    if (maxConcurrentDownloads === '' || maxConcurrentDownloads === null || maxConcurrentDownloads === undefined) {
      maxConcurrentDownloads = lastValidMaxConcurrent
      return
    }
    
    const parsed = Number(maxConcurrentDownloads)
    if (isNaN(parsed) || parsed < 1) {
      maxConcurrentDownloads = lastValidMaxConcurrent
    } else {
      const validValue = Math.floor(parsed) // Ensure it's an integer
      maxConcurrentDownloads = validValue
      lastValidMaxConcurrent = validValue // Store as the new last valid value
    }
  }

  // Function to handle input and only allow positive numbers
  function handleMaxConcurrentInput(event: any) {
    const target = event.target as HTMLInputElement
    let value = target.value
    
    // Remove any non-digit characters
    value = value.replace(/\D/g, '')
    
    // Remove leading zeros but allow empty string
    if (value.length > 1 && value.startsWith('0')) {
      value = value.replace(/^0+/, '')
    }
    
    // Update the input value to the cleaned version
    target.value = value
    
    // Update the bound variable (allow empty string during typing)
    if (value === '') {
      maxConcurrentDownloads = '' // Allow empty during typing
    } else {
      maxConcurrentDownloads = parseInt(value)
    }
  }
  
  // Combine all files and queue into single list with stable sorting
  $: allDownloads = (() => {
    const combined = [...$files, ...$downloadQueue]

    // Normal sorting by status
    const statusOrder = {
      'downloading': 0,
      'paused': 1,
      'completed': 2,
      'queued': 3,
      'failed': 4,
      'canceled': 5,
      'uploaded': 6,
      'seeding': 7
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
  
  
  // Filter downloads based on selected status and search
  $: filteredDownloads = (() => {
    let filtered = allDownloads.filter(f => f.status !== 'uploaded' && f.status !== 'seeding')

    // Apply search filter first
    if (searchFilter.trim()) {
      filtered = filtered.filter(f => 
        f.hash.toLowerCase().includes(searchFilter.toLowerCase()) ||
        f.name.toLowerCase().includes(searchFilter.toLowerCase())
      )
    }

    // Then apply status filter
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
  case 'canceled':
    return filtered.filter(f => f.status === 'canceled')
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
      // Handle case where maxConcurrentDownloads might be empty during typing
      const maxConcurrent = Math.max(1, Number(maxConcurrentDownloads) || 3)
      
      if (activeDownloads < maxConcurrent && queued.length > 0) {
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
  
  // Enhanced startDownload function with search and download status notifications
  async function startDownload() {
    if (!searchHash) {
      showNotification("Please enter a file hash", 'warning')
      return
    }
    
    // Check for duplicates
    const exists = [...$files, ...$downloadQueue].some(f => f.hash === searchHash)
    if (exists) {
      showNotification("File already exists in download list", 'warning')
      return
    }
    
    try {
      // Step 1: Show search start notification
      showNotification("🔍 Searching for file...", 'info', 2000)
      
      // Step 2: Simulate search process (replace with actual search API if available)
      await new Promise(resolve => setTimeout(resolve, 1500))
      
      // Step 3: Simulate search results (assumes file can always be found)
      // In actual implementation, you would call real search API
      const fileFound = Math.random() > 0.2 // 80% chance to find file
      
      if (!fileFound) {
        showNotification("❌ File not found, please check the hash", 'error', 5000)
        return
      }
      
      // Step 4: File found, show success notification
      showNotification("✅ File found! Adding to download queue...", 'success')
      
      // Create new download item
      const newFile = {
        id: `download-${Date.now()}`,
        name: 'File_' + searchHash.substring(0, 8) + '.dat',
        hash: searchHash,
        size: Math.floor(Math.random() * 100000000),
        price: Math.random() * 5,
        status: 'queued' as const,
        priority: 'normal' as const
      }
      
      downloadQueue.update(q => [...q, newFile])
      
      // Step 5: Show download start notification
      setTimeout(() => {
        showNotification("📥 File added to download queue", 'success')
      }, 1000)
      
      if (autoStartQueue) {
        processQueue()
        // If auto-start is enabled, show additional notification
        setTimeout(() => {
          showNotification("⚡ Download started automatically", 'info')
        }, 2000)
      }
      
      // Clear input
      searchHash = ''
      
    } catch (error) {
      // Error handling
      console.error('Search download failed:', error)
      showNotification("❌ Search failed: " + (error.message || 'Unknown error'), 'error', 6000)
    }
  }

  // Function to clear search
function clearSearch() {
  searchHash = ''
}

  function processQueue() {
    // Only prevent starting new downloads if we've reached the max concurrent limit
    const activeDownloads = $files.filter(f => f.status === 'downloading').length
    // Handle case where maxConcurrentDownloads might be empty during typing
    const maxConcurrent = Math.max(1, Number(maxConcurrentDownloads) || 3)
    if (activeDownloads >= maxConcurrent) return

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
  files.update(f => f.map(file => 
    file.id === fileId 
      ? { ...file, status: 'canceled' }
      : file
  ))
  downloadQueue.update(q => q.filter(file => file.id !== fileId))
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
            // Download failure notification
            showNotification(`❌ Download failed: ${file.name}`, 'error')
            return { ...file, status: 'failed' }
          }

          if (newProgress >= 100) {
            clearInterval(interval)
            activeSimulations.delete(fileId)
            // Download completion notification
            showNotification(`🎉 Download completed: ${file.name}`, 'success')
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
    <p class="text-muted-foreground mt-2">Download files from the Chiral Network using their hash</p>
  </div>
  
  <Card class="p-6">
    <div class="space-y-4">
      <div>
        <Label for="hash-input" class="text-base font-medium">Add New Download</Label>
        <p class="text-sm text-muted-foreground mt-1 mb-3">
          Enter a file hash to search for and download from the network
        </p>
        <div class="flex flex-col sm:flex-row gap-3">
          <div class="relative flex-1">
            <Input
              id="hash-input"
              bind:value={searchHash}
              placeholder="Enter file hash (e.g., QmZ4tDuvesekqMD...)"
              class="pr-10 h-10"
            />
            {#if searchHash}
              <button
                on:click={clearSearch}
                class="absolute right-2 top-1/2 transform -translate-y-1/2 p-1 hover:bg-muted rounded-full transition-colors"
                type="button"
                aria-label="Clear input"
              >
                <X class="h-4 w-4 text-muted-foreground hover:text-foreground" />
              </button>
            {/if}
          </div>
          <Button 
            on:click={startDownload} 
            disabled={!searchHash.trim()}
            class="h-10 px-6"
          >
            <Search class="h-4 w-4 mr-2" />
            Search & Download
          </Button>
        </div>
      </div>
    </div>
  </Card>
  
  <!-- Unified Downloads List -->
  <Card class="p-6">
    <!-- Header Section -->
    <div class="space-y-4 mb-6">
      <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4">
        <h2 class="text-xl font-semibold">Downloads</h2>
        
        <!-- Search Bar -->
        <div class="relative w-full sm:w-80">
          <Input
            bind:value={searchFilter}
            placeholder="Search downloads..."
            class="pr-8"
          />
          {#if searchFilter}
            <button
              on:click={() => searchFilter = ''}
              class="absolute right-2 top-1/2 transform -translate-y-1/2 text-muted-foreground hover:text-foreground"
              type="button"
              title="Clear search"
            >
              ×
            </button>
          {:else}
            <Search class="absolute right-2 top-1/2 transform -translate-y-1/2 h-4 w-4 text-muted-foreground pointer-events-none" />
          {/if}
        </div>
      </div>
      
      <!-- Filter Buttons and Controls -->
      <div class="flex flex-col lg:flex-row lg:items-center lg:justify-between gap-4">
        <!-- Filter Buttons -->
        <div class="flex flex-wrap gap-2">
          <Button
            size="sm"
            variant={filterStatus === 'all' ? 'default' : 'outline'}
            on:click={() => filterStatus = 'all'}
            class="text-xs"
          >
            All ({allFilteredDownloads.length})
          </Button>
          <Button
            size="sm"
            variant={filterStatus === 'active' ? 'default' : 'outline'}
            on:click={() => filterStatus = 'active'}
            class="text-xs"
          >
            Active ({activeCount})
          </Button>
          <Button
            size="sm"
            variant={filterStatus === 'paused' ? 'default' : 'outline'}
            on:click={() => filterStatus = 'paused'}
            class="text-xs"
          >
            Paused ({pausedCount})
          </Button>
          <Button
            size="sm"
            variant={filterStatus === 'queued' ? 'default' : 'outline'}
            on:click={() => filterStatus = 'queued'}
            class="text-xs"
          >
            Queued ({queuedCount})
          </Button>
          <Button
            size="sm"
            variant={filterStatus === 'completed' ? 'default' : 'outline'}
            on:click={() => filterStatus = 'completed'}
            class="text-xs"
          >
            Completed ({completedCount})
          </Button>
          <Button
            size="sm"
            variant={filterStatus === 'canceled' ? 'default' : 'outline'}
            on:click={() => filterStatus = 'canceled'}
            class="text-xs"
          >
            Canceled ({allFilteredDownloads.filter(f => f.status === 'canceled').length})
          </Button>
          <Button
            size="sm"
            variant={filterStatus === 'failed' ? 'default' : 'outline'}
            on:click={() => filterStatus = 'failed'}
            class="text-xs"
          >
            Failed ({failedCount})
          </Button>
        </div>
        
        <!-- Settings Controls -->
        <div class="flex flex-wrap items-center gap-4 text-sm">
          <div class="flex items-center gap-2">
            <Settings class="h-4 w-4 text-muted-foreground" />
            <Label class="font-medium">Max Concurrent:</Label>
            <input
              type="number"
              bind:value={maxConcurrentDownloads}
              on:input={handleMaxConcurrentInput}
              on:blur={validateMaxConcurrent}
              min="1"
              step="1"
              class="w-14 h-7 text-center text-xs border border-input bg-background px-2 py-1 ring-offset-background file:border-0 file:bg-transparent file:font-medium focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 disabled:cursor-not-allowed disabled:opacity-50 rounded-md"
            />
          </div>
          
          <div class="flex items-center gap-2">
            <Label class="font-medium">Auto-Start:</Label>
            <button
              type="button"
              aria-label="Toggle auto-start queue {autoStartQueue ? 'off' : 'on'}"
              on:click={() => autoStartQueue = !autoStartQueue}
              class="relative inline-flex h-4 w-8 items-center rounded-full transition-colors focus:outline-none"
              class:bg-green-500={autoStartQueue}
              class:bg-muted-foreground={!autoStartQueue}
            >
              <span
                class="inline-block h-3 w-3 rounded-full bg-white transition-transform shadow-sm"
                style="transform: translateX({autoStartQueue ? '18px' : '2px'})"
              ></span>
            </button>
          </div>
        </div>
      </div>
    </div>
    
    {#if filteredDownloads.length === 0}
      <p class="text-sm text-muted-foreground text-center py-8">
        {#if filterStatus === 'all'}
          No downloads yet. Enter a file name or hash above to start downloading.
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
          <div class="bg-card border rounded-lg overflow-hidden hover:shadow-md transition-shadow">
            <!-- File Header -->
            <div class="p-4 pb-2">
              <div class="flex items-start justify-between gap-4">
                <div class="flex items-start gap-3 flex-1 min-w-0">
                  <!-- Queue Controls -->
                  {#if file.status === 'queued'}
                    <div class="flex flex-col gap-1 mt-1">
                      <Button
                        size="sm"
                        variant="ghost"
                        on:click={() => moveInQueue(file.id, 'up')}
                        disabled={index === 0}
                        class="h-6 w-6 p-0 hover:bg-muted"
                      >
                        <ChevronUp class="h-4 w-4" />
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        on:click={() => moveInQueue(file.id, 'down')}
                        disabled={index === filteredDownloads.filter(f => f.status === 'queued').length - 1}
                        class="h-6 w-6 p-0 hover:bg-muted"
                      >
                        <ChevronDown class="h-4 w-4" />
                      </Button>
                    </div>
                  {/if}
                  
                  <!-- File Info -->
                  <div class="flex-1 min-w-0">
                    <h3 class="font-medium text-sm truncate mb-1">{file.name}</h3>
                    <p class="text-xs text-muted-foreground truncate mb-2">
                      Hash: {file.hash}
                    </p>
                    <div class="flex items-center gap-2 flex-wrap">
                      <Badge variant="outline" class="text-xs">
                        {formatFileSize(file.size)}
                      </Badge>
                      {#if file.status === 'queued'}
                        <select
                          value={file.priority || 'normal'}
                          on:change={(e) => {
                            const target = e.target as HTMLSelectElement;
                            if (target) changePriority(file.id, target.value as 'low' | 'normal' | 'high');
                          }}
                          class="text-xs px-2 py-1 border rounded bg-background h-6"
                        >
                          <option value="low">Low</option>
                          <option value="normal">Normal</option>
                          <option value="high">High</option>
                        </select>
                      {/if}
                    </div>
                  </div>
                </div>
                
                <!-- Status Badge -->
                <Badge class={
                  file.status === 'downloading' ? 'bg-blue-500 text-white border-blue-500' :
                  file.status === 'completed' ? 'bg-green-500 text-white border-green-500' :
                  file.status === 'paused' ? 'bg-yellow-400 text-white border-yellow-400' :
                  file.status === 'queued' ? 'bg-gray-500 text-white border-gray-500' :
                  file.status === 'canceled' ? 'bg-red-600 text-white border-red-600' :
                  'bg-red-500 text-white border-red-500'
                }
                >
                  {file.status === 'queued' ? `Queue #${$downloadQueue.indexOf(file) + 1}` : file.status}
                </Badge>
              </div>
            </div>
            
            <!-- Progress Section -->
            {#if file.status === 'downloading' || file.status === 'paused'}
              <div class="px-4 pb-2">
                <div class="bg-muted/50 rounded-lg p-3">
                  <div class="flex items-center justify-between text-sm mb-2">
                    <span class="font-medium">Progress</span>
                    <span class="text-muted-foreground">{(file.progress || 0).toFixed(2)}%</span>
                  </div>
                  <Progress 
                    value={file.progress || 0} 
                    max={100} 
                    class="h-2 bg-muted [&>div]:bg-green-500" 
                  />
                </div>
              </div>
            {/if}
            
            <!-- Action Buttons -->
            <div class="px-4 pb-4">
              <div class="flex flex-wrap gap-2">
                {#if file.status === 'downloading' || file.status === 'paused' || file.status === 'queued'}
                  {#if file.status === 'queued'}
                    <Button
                      size="sm"
                      variant="default"
                      on:click={() => startQueuedDownload(file.id)}
                      class="h-8 px-3"
                    >
                      <Play class="h-3 w-3 mr-1" />
                      Start
                    </Button>
                  {:else}
                    <Button
                      size="sm"
                      variant="outline"
                      on:click={() => togglePause(file.id)}
                      class="h-8 px-3"
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
                    class="h-8 px-3"
                  >
                    <X class="h-3 w-3 mr-1" />
                    {file.status === 'queued' ? 'Remove' : 'Cancel'}
                  </Button>
                {:else if file.status === 'completed'}
                  <Button
                    size="sm"
                    variant="outline"
                    class="h-8 px-3"
                  >
                    <File class="h-3 w-3 mr-1" />
                    Open File
                  </Button>
                {/if}
              </div>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </Card>
</div>