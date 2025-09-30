<script lang="ts">
  import Button from '$lib/components/ui/button.svelte'
  import Card from '$lib/components/ui/card.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import Progress from '$lib/components/ui/progress.svelte'
  import { Search, Pause, Play, X, ChevronUp, ChevronDown, Settings, FolderOpen, File as FileIcon, FileText, FileImage, FileVideo, FileAudio, Archive, Code, FileSpreadsheet, Presentation } from 'lucide-svelte'
  import { files, downloadQueue, activeTransfers } from '$lib/stores'
  import DownloadSearchSection from '$lib/components/download/DownloadSearchSection.svelte'
  import type { FileMetadata } from '$lib/dht'
  import { onDestroy, onMount } from 'svelte'
  import { t } from 'svelte-i18n'
  import { get } from 'svelte/store'
  import { toHumanReadableSize } from '$lib/utils'
  import { initDownloadTelemetry, disposeDownloadTelemetry } from '$lib/downloadTelemetry'
  
  const tr = (k: string, params?: Record<string, any>) => (get(t) as any)(k, params)

  onMount(() => {
    initDownloadTelemetry()
  })

  onDestroy(() => {
    disposeDownloadTelemetry()
  })
  
  let searchFilter = ''  // For searching existing downloads
  let maxConcurrentDownloads: string | number = 3
  let lastValidMaxConcurrent = 3 // Store the last valid value
  let autoStartQueue = true
  let autoClearCompleted = false // New setting for auto-clearing
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

  function getFileIcon(fileName: string) {
    const extension = fileName.split('.').pop()?.toLowerCase() || '';

    switch (extension) {
      case 'pdf':
      case 'doc':
      case 'docx':
      case 'txt':
      case 'rtf':
        return FileText;

      case 'jpg':
      case 'jpeg':
      case 'png':
      case 'gif':
      case 'bmp':
      case 'svg':
      case 'webp':
        return FileImage;

      case 'mp4':
      case 'avi':
      case 'mov':
      case 'wmv':
      case 'flv':
      case 'webm':
      case 'mkv':
        return FileVideo;

      case 'mp3':
      case 'wav':
      case 'flac':
      case 'aac':
      case 'ogg':
        return FileAudio;

      case 'zip':
      case 'rar':
      case '7z':
      case 'tar':
      case 'gz':
        return Archive;

      case 'js':
      case 'ts':
      case 'html':
      case 'css':
      case 'py':
      case 'java':
      case 'cpp':
      case 'c':
      case 'php':
        return Code;

      case 'xls':
      case 'xlsx':
      case 'csv':
        return FileSpreadsheet;

      case 'ppt':
      case 'pptx':
        return Presentation;

      default:
        return FileIcon;
    }
  }

  function handleSearchMessage(event: CustomEvent<{ message: string; type?: 'success' | 'error' | 'info' | 'warning'; duration?: number }>) {
    const { message, type = 'info', duration = 4000 } = event.detail
    showNotification(message, type, duration)
  }

  async function handleSearchDownload(metadata: FileMetadata) {
    const allFiles = [...$files, ...$downloadQueue]
    const existingFile = allFiles.find((file) => file.hash === metadata.fileHash)

    if (existingFile) {
      let statusMessage = ''
      switch (existingFile.status) {
        case 'completed':
          statusMessage = tr('download.search.queue.status.completed')
          break
        case 'downloading':
          statusMessage = tr('download.search.queue.status.downloading', { values: { progress: existingFile.progress || 0 } })
          break
        case 'paused':
          statusMessage = tr('download.search.queue.status.paused', { values: { progress: existingFile.progress || 0 } })
          break
        case 'queued':
          statusMessage = tr('download.search.queue.status.queued')
          break
        case 'failed':
          statusMessage = tr('download.search.queue.status.failed')
          break
        case 'seeding':
        case 'uploaded':
          statusMessage = tr('download.search.queue.status.seeding')
          break
        default:
          statusMessage = tr('download.search.queue.status.other', { values: { status: existingFile.status } })
      }

      showNotification(statusMessage, 'warning', 4000)

      if (existingFile.status !== 'failed' && existingFile.status !== 'canceled') {
        return
      }
    }

    const newFile = {
      id: `download-${Date.now()}`,
      name: metadata.fileName,
      hash: metadata.fileHash,
      size: metadata.fileSize,
      status: 'queued' as const,
      priority: 'normal' as const,
      version: metadata.version, // Preserve version info if available
      seeders: metadata.seeders.length, // Convert array length to number
      seederAddresses: metadata.seeders, // Store the actual seeder addresses
    }

    downloadQueue.update((queue) => [...queue, newFile])
    showNotification(tr('download.search.status.addedToQueue', { values: { name: metadata.fileName } }), 'success')

    if (autoStartQueue) {
      processQueue()
    }
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
  function handleMaxConcurrentInput(event: Event) {
    const target = (event.target as HTMLInputElement)
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
  
  // New search function that only searches without downloading
  

  // New function to download from search results

  function processQueue() {
    // Only prevent starting new downloads if we've reached the max concurrent limit
    const activeDownloads = $files.filter(f => f.status === 'downloading').length
    // Handle case where maxConcurrentDownloads might be empty during typing
    const maxConcurrent = Math.max(1, Number(maxConcurrentDownloads) || 3)
    if (activeDownloads >= maxConcurrent) return

    const nextFile = $downloadQueue[0]
    if (!nextFile) return
    downloadQueue.update(q => q.filter(f => f.id !== nextFile.id))
    const downloadingFile = { 
      ...nextFile, 
      status: 'downloading' as const, 
      progress: 0,
      speed: '0 B/s', // Ensure speed property exists
      eta: 'N/A'      // Ensure eta property exists
    }
    files.update(f => [...f, downloadingFile])
    simulateDownloadProgress(downloadingFile.id)
  }
  
  function togglePause(fileId: string) {
    files.update(f => f.map(file => {
      if (file.id === fileId) {
        const newStatus = file.status === 'downloading' ? 'paused' as const : 'downloading' as const
        // Ensure speed and eta are always present
        return { 
          ...file, 
          status: newStatus,
          speed: file.speed ?? '0 B/s',
          eta: file.eta ?? 'N/A'
        }
      }
      return file
    }))
  }
  
  async function cancelDownload(fileId: string) {
    files.update(f => f.map(file =>
      file.id === fileId
        ? { ...file, status: 'canceled' }
        : file
    ))
    downloadQueue.update(q => q.filter(file => file.id !== fileId))
    activeSimulations.delete(fileId)

    // Clean up P2P transfer
    const transfer = get(activeTransfers).get(fileId);
    if (transfer && transfer.type === 'p2p') {
      const { p2pFileTransferService } = await import('$lib/services/p2pFileTransfer');
      p2pFileTransferService.cancelTransfer(transfer.transferId);
      activeTransfers.update(transfers => {
        transfers.delete(fileId);
        return transfers;
      });
    }
  }
  
  function startQueuedDownload(fileId: string) {
    downloadQueue.update(queue => {
      const file = queue.find(f => f.id === fileId)
      if (file) {
        files.update(f => [...f, { 
          ...file, 
          status: 'downloading', 
          progress: 0,
          speed: '0 B/s', // Ensure speed property exists
          eta: 'N/A'      // Ensure eta property exists
        }])
        simulateDownloadProgress(fileId)
      }
      return queue.filter(f => f.id !== fileId)
    })
  }
  
  async function simulateDownloadProgress(fileId: string) {
    // Prevent duplicate simulations
    if (activeSimulations.has(fileId)) {
      return
    }

    activeSimulations.add(fileId)

    // Get the file to download
    const fileToDownload = $files.find(f => f.id === fileId);
    if (!fileToDownload) {
      activeSimulations.delete(fileId);
      return;
    }

      // Proceed directly to file dialog
      try {
        const { save } = await import('@tauri-apps/plugin-dialog');
        
        // Show file save dialog
        const outputPath = await save({
          defaultPath: fileToDownload.name,
          filters: [{
            name: 'All Files',
            extensions: ['*']
          }]
        });
        
        if (!outputPath) {
          // User cancelled the save dialog
          activeSimulations.delete(fileId);
          files.update(f => f.map(file => 
            file.id === fileId 
              ? { ...file, status: 'canceled' }
              : file
          ));
          return;
        }
      
      // Show "automatically started" message now that download is proceeding
      showNotification(tr('download.notifications.autostart'), 'info');
      
      // Import P2P file transfer service
      const { p2pFileTransferService } = await import('$lib/services/p2pFileTransfer');

      // Start P2P download using the P2P file transfer service
      try {
        const seeders = fileToDownload.seederAddresses || [];

        if (seeders.length === 0) {
          throw new Error('No seeders available for this file');
        }

        // Create file metadata for P2P transfer
        const fileMetadata = {
          fileHash: fileToDownload.hash,
          fileName: fileToDownload.name,
          fileSize: fileToDownload.size,
          seeders: seeders,
          createdAt: Date.now(),
          isEncrypted: false
        };

        // Initiate P2P download with file saving
        const transferId = await p2pFileTransferService.initiateDownloadWithSave(
          fileMetadata,
          seeders,
          outputPath,
          (transfer) => {
            // Update UI with transfer progress
            files.update(f => f.map(file => {
              if (file.id === fileId) {
                return {
                  ...file,
                  progress: transfer.progress,
                  status: transfer.status === 'completed' ? 'completed' :
                         transfer.status === 'failed' ? 'failed' :
                         transfer.status === 'transferring' ? 'downloading' : file.status,
                  speed: `${Math.round(transfer.speed / 1024)} KB/s`,
                  eta: transfer.eta ? `${Math.round(transfer.eta)}s` : 'N/A',
                  downloadPath: transfer.outputPath // Store the download path
                };
              }
              return file;
            }));

            // Show notification on completion or failure
            if (transfer.status === 'completed') {
              showNotification(tr('download.notifications.downloadComplete', { values: { name: fileToDownload.name } }), 'success');
            } else if (transfer.status === 'failed') {
              showNotification(tr('download.notifications.downloadFailed', { values: { name: fileToDownload.name } }), 'error');
            }
          }
        );

        // Store transfer ID for cleanup
        activeTransfers.update(transfers => {
          transfers.set(fileId, { fileId, transferId, type: 'p2p' });
          return transfers;
        });

        activeSimulations.delete(fileId);

      } catch (error) {
        console.error('P2P download failed:', error);
        activeSimulations.delete(fileId);
        files.update(f => f.map(file =>
          file.id === fileId
            ? { ...file, status: 'failed' }
            : file
        ));
      }
      
    } catch (error) {
      // Download failed
      activeSimulations.delete(fileId);
      
      files.update(f => f.map(file => 
        file.id === fileId 
          ? { ...file, status: 'failed' }
          : file
      ));
      
      console.error('Download failed:', error);
      showNotification(tr('download.notifications.downloadFailed', { values: { name: fileToDownload.name } }), 'error');
    }
  }
  
  function changePriority(fileId: string, priority: 'low' | 'normal' | 'high') {
    downloadQueue.update(queue => queue.map(file => 
      file.id === fileId ? { ...file, priority } : file
    ))
  }

  async function showInFolder(fileId: string) {
    const file = $files.find(f => f.id === fileId);
    if (file && file.downloadPath) {
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        await invoke('show_in_folder', { path: file.downloadPath });
      } catch (error) {
        console.error('Failed to show file in folder:', error);
        showNotification('Failed to open file location', 'error');
      }
    }
  }
  
  function clearDownload(fileId: string) {
    // Remove from both files and downloadQueue for good measure
    files.update(f => f.filter(file => file.id !== fileId));
    downloadQueue.update(q => q.filter(file => file.id !== fileId));
  }

  function clearAllFinished() {
    files.update(f => f.filter(file => 
      file.status !== 'completed' && 
      file.status !== 'failed' && 
      file.status !== 'canceled'
    ));
  }

  function retryDownload(fileId: string) {
    const fileToRetry = filteredDownloads.find(f => f.id === fileId);
    if (!fileToRetry || (fileToRetry.status !== 'failed' && fileToRetry.status !== 'canceled')) {
      return;
    }

    files.update(f => f.filter(file => file.id !== fileId));

    const newFile = {
      ...fileToRetry,
      id: `download-${Date.now()}`,
      status: 'queued' as const,
      progress: 0,
      downloadPath: undefined,
      speed: '0 B/s', // Ensure speed property exists
      eta: 'N/A'      // Ensure eta property exists
    };
    downloadQueue.update(q => [...q, newFile]);
    showNotification(`Retrying download for "${newFile.name}"`, 'info');
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
  
  const formatFileSize = toHumanReadableSize



</script>

<div class="space-y-6">
  <div>
    <h1 class="text-3xl font-bold">{$t('download.title')}</h1>
    <p class="text-muted-foreground mt-2">{$t('download.subtitle')}</p>
  </div>
  
  <DownloadSearchSection
    on:download={(event) => handleSearchDownload(event.detail)}
    on:message={handleSearchMessage}
  />
  <!-- Unified Downloads List -->
  <Card class="p-6">
    <!-- Header Section -->
    <div class="space-y-4 mb-6">
      <div class="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4">
        <h2 class="text-xl font-semibold">{$t('download.downloads')}</h2>
        
        <!-- Search Bar -->
        <div class="relative w-full sm:w-80">
          <Input
            bind:value={searchFilter}
            placeholder={$t('download.searchPlaceholder')}
            class="pr-8"
          />
          {#if searchFilter}
            <button
              on:click={() => searchFilter = ''}
              class="absolute right-2 top-1/2 transform -translate-y-1/2 text-muted-foreground hover:text-foreground"
              type="button"
              title={$t('download.clearSearch')}
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
            variant="outline"
            on:click={clearAllFinished}
            class="text-xs text-destructive border-destructive hover:bg-destructive/10 hover:text-destructive"
            disabled={completedCount === 0 && failedCount === 0 && allFilteredDownloads.filter(f => f.status === 'canceled').length === 0}
          >
            <X class="h-3 w-3 mr-1" />
            {$t('download.clearFinished')}
          </Button>
          <Button
            size="sm"
            variant={filterStatus === 'all' ? 'default' : 'outline'}
            on:click={() => filterStatus = 'all'}
            class="text-xs"
          >
            {$t('download.filters.all')} ({allFilteredDownloads.length})
          </Button>
          <Button
            size="sm"
            variant={filterStatus === 'active' ? 'default' : 'outline'}
            on:click={() => filterStatus = 'active'}
            class="text-xs"
          >
            {$t('download.filters.active')} ({activeCount})
          </Button>
          <Button
            size="sm"
            variant={filterStatus === 'paused' ? 'default' : 'outline'}
            on:click={() => filterStatus = 'paused'}
            class="text-xs"
          >
            {$t('download.filters.paused')} ({pausedCount})
          </Button>
          <Button
            size="sm"
            variant={filterStatus === 'queued' ? 'default' : 'outline'}
            on:click={() => filterStatus = 'queued'}
            class="text-xs"
          >
            {$t('download.filters.queued')} ({queuedCount})
          </Button>
          <Button
            size="sm"
            variant={filterStatus === 'completed' ? 'default' : 'outline'}
            on:click={() => filterStatus = 'completed'}
            class="text-xs"
          >
            {$t('download.filters.completed')} ({completedCount})
          </Button>
          <Button
            size="sm"
            variant={filterStatus === 'canceled' ? 'default' : 'outline'}
            on:click={() => filterStatus = 'canceled'}
            class="text-xs"
          >
            {$t('download.filters.canceled')} ({allFilteredDownloads.filter(f => f.status === 'canceled').length})
          </Button>
          <Button
            size="sm"
            variant={filterStatus === 'failed' ? 'default' : 'outline'}
            on:click={() => filterStatus = 'failed'}
            class="text-xs"
          >
            {$t('download.filters.failed')} ({failedCount})
          </Button>
        </div>
        
        <!-- Settings Controls -->
        <div class="flex flex-wrap items-center gap-4 text-sm">
          <div class="flex items-center gap-2">
            <Settings class="h-4 w-4 text-muted-foreground" />
            <Label class="font-medium">{$t('download.settings.maxConcurrent')}:</Label>
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
            <Label class="font-medium">{$t('download.settings.autoStart')}:</Label>
            <button
              type="button"
              aria-label={$t('download.settings.toggleAutoStart', { values: { status: autoStartQueue ? 'off' : 'on' } })}
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
          <div class="flex items-center gap-2">
            <Label class="font-medium">Auto-clear:</Label>
            <button
              type="button"
              aria-label="Toggle auto-clear completed downloads"
              on:click={() => autoClearCompleted = !autoClearCompleted}
              class="relative inline-flex h-4 w-8 items-center rounded-full transition-colors focus:outline-none"
              class:bg-green-500={autoClearCompleted}
              class:bg-muted-foreground={!autoClearCompleted}
            >
              <span
                class="inline-block h-3 w-3 rounded-full bg-white transition-transform shadow-sm"
                style="transform: translateX({autoClearCompleted ? '18px' : '2px'})"
              ></span>
            </button>
          </div>
        </div>
      </div>
    </div>
    
    {#if filteredDownloads.length === 0}
      <p class="text-sm text-muted-foreground text-center py-8">
        {#if filterStatus === 'all'}
          {$t('download.status.noDownloads')}
        {:else if filterStatus === 'active'}
          {$t('download.status.noActive')}
        {:else if filterStatus === 'paused'}
          {$t('download.status.noPaused')}
        {:else if filterStatus === 'queued'}
          {$t('download.status.noQueued')}
        {:else if filterStatus === 'completed'}
          {$t('download.status.noCompleted')}
        {:else}
          {$t('download.status.noFailed')}
        {/if}
      </p>
    {:else}
      <div class="space-y-3">
        {#each filteredDownloads as file, index}
          <div class="p-3 bg-secondary rounded-lg hover:bg-secondary/80 transition-colors">
            <!-- File Header -->
            <div class="pb-2">
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
                  <div class="flex items-start gap-3 flex-1 min-w-0">
                    <svelte:component this={getFileIcon(file.name)} class="h-4 w-4 text-muted-foreground mt-0.5" />
                    <div class="flex-1 min-w-0">
                      <div class="flex items-center gap-3 mb-1">
                        <h3 class="font-semibold text-sm truncate">{file.name}</h3>
                        {#if file.version}
                          <Badge class="bg-blue-100 text-blue-800 text-xs px-2 py-0.5">
                            v{file.version}
                          </Badge>
                        {/if}
                        <Badge class="text-xs font-semibold bg-muted-foreground/20 text-foreground border-0 px-2 py-0.5">
                          {formatFileSize(file.size)}
                        </Badge>
                      </div>
                      <div class="flex items-center gap-x-3 gap-y-1 mt-1">
                        <p class="text-xs text-muted-foreground truncate">{$t('download.file.hash')}: {file.hash}</p>
                      </div>
                    </div>
                    <div class="flex items-center gap-2 flex-wrap">
                      {#if file.status === 'queued'}
                        <select
                          value={file.priority || 'normal'}
                          on:change={(e) => {
                            const target = e.target as HTMLSelectElement;
                            if (target) changePriority(file.id, target.value as 'low' | 'normal' | 'high');
                          }}
                          class="text-xs px-2 py-1 border rounded bg-background h-6"
                        >
                          <option value="low">{$t('download.priority.low')}</option>
                          <option value="normal">{$t('download.priority.normal')}</option>
                          <option value="high">{$t('download.priority.high')}</option>
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
                  {file.status === 'queued' ? `${$t('download.file.queue')} #${$downloadQueue.indexOf(file) + 1}` : file.status}
                </Badge>
              </div>
            </div>
            
            <!-- Progress Section -->
            {#if file.status === 'downloading' || file.status === 'paused'}
              <div class="pb-2 ml-7">
                <div class="flex items-center justify-between text-sm mb-1">
                  <div class="flex items-center gap-4 text-muted-foreground">
                    <span>Speed: {file.speed || '0 B/s'}</span>
                    <span>ETA: {file.eta || 'N/A'}</span>
                  </div>
                  <span class="text-foreground">{(file.progress || 0).toFixed(2)}%</span>
                </div>
                <Progress
                  value={file.progress || 0}
                  max={100}
                  class="h-2 bg-background [&>div]:bg-green-500 w-full"
                />
              </div>
            {/if}
            
            <!-- Action Buttons -->
            <div class="pt-2 ml-7">
              <div class="flex flex-wrap gap-2">
                {#if file.status === 'downloading' || file.status === 'paused' || file.status === 'queued'}
                  {#if file.status === 'queued'}
                    <Button
                      size="sm"
                      variant="default"
                      on:click={() => startQueuedDownload(file.id)}
                      class="h-7 px-3 text-sm"
                    >
                      <Play class="h-3 w-3 mr-1" />
                      {$t('download.actions.start')}
                    </Button>
                  {:else}
                    <Button
                      size="sm"
                      variant="outline"
                      on:click={() => togglePause(file.id)}
                      class="h-7 px-3 text-sm"
                    >
                      {#if file.status === 'downloading'}
                        <Pause class="h-3 w-3 mr-1" />
                        {$t('download.actions.pause')}
                      {:else}
                        <Play class="h-3 w-3 mr-1" />
                        {$t('download.actions.resume')}
                      {/if}
                    </Button>
                  {/if}
                  <Button
                    size="sm"
                    variant="destructive"
                    on:click={() => cancelDownload(file.id)}
                    class="h-7 px-3 text-sm"
                  >
                    <X class="h-3 w-3 mr-1" />
                    {file.status === 'queued' ? $t('download.actions.remove') : $t('download.actions.cancel')}
                  </Button>
                {:else if file.status === 'completed'}
                  <Button
                    size="sm"
                    variant="outline"
                    on:click={() => showInFolder(file.id)}
                    class="h-7 px-3 text-sm"
                  >
                    <FolderOpen class="h-3 w-3 mr-1" />
                    {$t('download.actions.showInFolder')}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    on:click={() => clearDownload(file.id)}
                    class="h-7 px-3 text-sm text-muted-foreground hover:text-destructive"
                    title={$t('download.actions.remove', { default: 'Remove' })}
                  >
                    <X class="h-3 w-3" />
                  </Button>
                {:else if file.status === 'failed' || file.status === 'canceled'}
                  <Button
                    size="sm"
                    variant="outline"
                    on:click={() => retryDownload(file.id)}
                    class="h-7 px-3 text-sm"
                  >
                    <Play class="h-3 w-3 mr-1" />
                    {$t('download.actions.retry', { default: 'Retry' })}
                  </Button>
                  <!-- You could also add a "Clear" button here to remove it from the list -->
                  <Button
                    size="sm"
                    variant="ghost"
                    on:click={() => clearDownload(file.id)}
                    class="h-7 px-3 text-sm text-muted-foreground hover:text-destructive"
                    title={$t('download.actions.remove', { default: 'Remove' })}
                  >
                    <X class="h-3 w-3" />
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

