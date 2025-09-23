<script lang="ts">
  import Button from '$lib/components/ui/button.svelte'
  import Card from '$lib/components/ui/card.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import Progress from '$lib/components/ui/progress.svelte'
  import { Search, Pause, Play, X, ChevronUp, ChevronDown, Settings, FolderOpen, Star, Zap, File as FileIcon, FileText, FileImage, FileVideo, FileAudio, Archive, Code, FileSpreadsheet, Presentation } from 'lucide-svelte'
  import { files, downloadQueue } from '$lib/stores'
  import { t } from 'svelte-i18n'
  import { get } from 'svelte/store'
  const tr = (k: string, params?: Record<string, any>) => get(t)(k, params)
  
  let searchHash = ''  // For downloading new files
  let searchFilter = ''  // For searching existing downloads
  let maxConcurrentDownloads: string | number = 3
  let lastValidMaxConcurrent = 3 // Store the last valid value
  let autoStartQueue = true
  let filterStatus = 'all' // 'all', 'active', 'paused', 'queued', 'completed', 'failed'
  let activeSimulations = new Set<string>() // Track files with active progress simulations

  // New state for search results
  let searchResults: Array<{
    fileHash: string;
    fileName: string;
    fileSize: number;
    seeders: number;
    leechers: number;
    peers: Array<{
      id: string;
      nickname: string;
      reputation: number;
      status: string;
      connection: string;
    }>;
  }> = []
  let isSearching = false
  let hasSearched = false

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
  
  // New search function that only searches without downloading
  async function searchForFile() {
    if (!searchHash) {
      showNotification(tr('download.notifications.enterHash'), 'warning')
      return
    }

    isSearching = true
    searchResults = []
    hasSearched = false

    try {
      // Show search start notification
      showNotification('Searching for file in network...', 'info', 2000)

      // Simulate search delay for realistic UX
      await new Promise(resolve => setTimeout(resolve, 1500));

      // Mock file database with predefined hashes for testing
      // TODO: Replace with actual DHT search results
      const mockFileDatabase: Record<string, {
        fileName: string;
        fileSize: number;
        peers: Array<{
          id: string;
          nickname: string;
          reputation: number;
          status: string;
          connection: string;
        }>;
      }> = {
        // Existing files (already in downloads list)
        'QmZ4tDuvesekqMF': {
          fileName: 'Video.mp4',
          fileSize: 50331648, // 48.00 MB
          peers: [
            { id: 'peer1', nickname: 'AliceNode', reputation: 4.8, status: 'online', connection: 'fast' },
            { id: 'peer2', nickname: 'BobStorage', reputation: 4.5, status: 'online', connection: 'average' }
          ]
        },
        'QmZ4tDuvesekqMD': {
          fileName: 'Document.pdf',
          fileSize: 2048576, // 2.00 MB
          peers: [
            { id: 'peer3', nickname: 'CharlieShare', reputation: 4.2, status: 'away', connection: 'slow' },
            { id: 'peer4', nickname: 'DataVault', reputation: 4.6, status: 'online', connection: 'fast' }
          ]
        },
        'QmZ4tDuvesekqMG': {
          fileName: 'Archive.zip',
          fileSize: 10485760, // 10.00 MB
          peers: [
            { id: 'peer1', nickname: 'AliceNode', reputation: 4.8, status: 'online', connection: 'fast' },
            { id: 'peer5', nickname: 'CloudSync', reputation: 3.9, status: 'online', connection: 'average' }
          ]
        },
        // New files (not in downloads list yet)
        'QmZ4tDuvesekqMA': {
          fileName: 'music_album.zip',
          fileSize: 89234567,
          peers: [
            { id: 'peer2', nickname: 'BobStorage', reputation: 4.5, status: 'online', connection: 'average' },
            { id: 'peer5', nickname: 'CloudSync', reputation: 3.9, status: 'online', connection: 'average' }
          ]
        },
        'QmZ4tDuvesekqMB': {
          fileName: 'software_package.deb',
          fileSize: 12456789,
          peers: [
            { id: 'peer1', nickname: 'AliceNode', reputation: 4.8, status: 'online', connection: 'fast' },
            { id: 'peer3', nickname: 'CharlieShare', reputation: 4.2, status: 'away', connection: 'slow' },
            { id: 'peer4', nickname: 'DataVault', reputation: 4.6, status: 'online', connection: 'fast' }
          ]
        },
        'QmZ4tDuvesekqMC': {
          fileName: 'presentation.pptx',
          fileSize: 5242880, // 5.00 MB
          peers: [
            { id: 'peer2', nickname: 'BobStorage', reputation: 4.5, status: 'online', connection: 'average' },
            { id: 'peer4', nickname: 'DataVault', reputation: 4.6, status: 'online', connection: 'fast' }
          ]
        },
        'QmZ4tDuvesekqME': {
          fileName: 'game_assets.tar.gz',
          fileSize: 156789123,
          peers: [
            { id: 'peer1', nickname: 'AliceNode', reputation: 4.8, status: 'online', connection: 'fast' },
            { id: 'peer2', nickname: 'BobStorage', reputation: 4.5, status: 'online', connection: 'average' },
            { id: 'peer3', nickname: 'CharlieShare', reputation: 4.2, status: 'away', connection: 'slow' },
            { id: 'peer5', nickname: 'CloudSync', reputation: 3.9, status: 'online', connection: 'average' }
          ]
        }
      }

      // Check if the searched hash exists in our mock database
      const fileData = mockFileDatabase[searchHash]

      let mockResults: typeof searchResults = []
      if (fileData) {
        // File found - create mock result
        const seeders = fileData.peers.length
        const leechers = Math.floor(Math.random() * 3) // Random leechers

        mockResults = [{
          fileHash: searchHash,
          fileName: fileData.fileName,
          fileSize: fileData.fileSize,
          seeders: seeders,
          leechers: leechers,
          peers: fileData.peers
        }]
      }
      // If fileData is undefined, mockResults stays empty (no files found)

      searchResults = mockResults
      hasSearched = true
      showNotification(`Found ${mockResults.length} result(s) for your search`, 'success')

    } catch (error) {
      console.error('Search failed:', error)
      searchResults = []
      hasSearched = true
      const errorMessage = error instanceof Error ? error.message : 'Unknown error'
      showNotification(`Search failed: ${errorMessage}`, 'error', 6000)
    } finally {
      isSearching = false
    }
  }

  // New function to download from search results
  async function downloadFromSearchResult(result: typeof searchResults[0]) {
    // Check for duplicates using the existing logic
    const allFiles = [...$files, ...$downloadQueue]
    const existingFile = allFiles.find(f => f.hash === result.fileHash)

    if (existingFile) {
      // Provide detailed status information
      let statusMessage = ''
      switch (existingFile.status) {
        case 'completed':
          statusMessage = `File download already completed`
          break
        case 'downloading':
          statusMessage = `File is currently downloading (${existingFile.progress || 0}% complete)`
          break
        case 'paused':
          statusMessage = `File download is paused at ${existingFile.progress || 0}%`
          break
        case 'queued':
          statusMessage = `File is already in download queue`
          break
        case 'failed':
          statusMessage = `File download previously failed. Try again?`
          break
        case 'seeding':
        case 'uploaded':
          statusMessage = `You are already sharing this file`
          break
        default:
          statusMessage = `File is already in your downloads (${existingFile.status})`
      }

      showNotification(statusMessage, 'warning', 4000)

      // For failed or cancelled downloads, allow retry
      if (existingFile.status !== 'failed' && existingFile.status !== 'canceled') {
        return
      }
    }

    // Create new download item
    const newFile = {
      id: `download-${Date.now()}`,
      name: result.fileName,
      hash: result.fileHash,
      size: result.fileSize,
      status: 'queued' as const,
      priority: 'normal' as const
    }

    downloadQueue.update(q => [...q, newFile])
    showNotification(`Added "${result.fileName}" to download queue`, 'success')

    if (autoStartQueue) {
      processQueue()
    }

    // Clear search results after successful download initiation
    searchResults = []
    hasSearched = false
    searchHash = ''
  }

  // Enhanced startDownload function with real P2P download (kept for backward compatibility)
  // @ts-ignore - Function kept for backward compatibility
  async function startDownload() {
    if (!searchHash) {
      showNotification(tr('download.notifications.enterHash'), 'warning')
      return
    }
    
    // Check for ALL duplicates (including uploaded files)
    const allFiles = [...$files, ...$downloadQueue]
    const existingFile = allFiles.find(f => f.hash === searchHash)

    if (existingFile) {
      // Handle different scenarios
      if (existingFile.status === 'seeding' || existingFile.status === 'uploaded') {
        // User is trying to download a file they're already sharing
        // Show warning but proceed anyway
        showNotification('Downloading file that you are already sharing', 'warning', 3000);
        
      } else {
        // File is already in download queue/completed/etc.
        showNotification('File is already in your download list', 'warning')
        return
      }
    }
    
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      
      // Step 1: Show search start notification
      showNotification(tr('download.notifications.searching'), 'info', 2000)
      
      // Step 2: Start file transfer service if not already running
      try {
        await invoke('start_file_transfer_service');
      } catch (e) {
        console.log('File transfer service already running or error:', e);
      }
      
      // Step 3: Search for file in DHT
      try {
        await invoke('search_file_metadata', { fileHash: searchHash });
        // Wait a moment for DHT search to complete
        await new Promise(resolve => setTimeout(resolve, 2000));
      } catch (e) {
        console.log('DHT search failed:', e);
      }
      
      // Step 4: Skip validation for now - let download fail naturally
      // This avoids Tauri initialization issues
      // The download will fail later in simulateDownloadProgress if needed
      
      // Step 5: Create new download item and add to queue
      const newFile = {
        id: `download-${Date.now()}`,
        name: 'File_' + searchHash.substring(0, 8) + '.dat',
        hash: searchHash,
        size: 0, // Will be updated when we get file info
        price: 0, // No price in this implementation
        status: 'queued' as const,
        priority: 'normal' as const
      }
      
      downloadQueue.update(q => [...q, newFile])
      
      // Step 6: Show download start notification
      showNotification(tr('download.notifications.addedToQueue'), 'success')
      
      if (autoStartQueue) {
        processQueue()
        // Don't show "automatically started" message immediately
        // It will be shown in simulateDownloadProgress if download actually starts
      }
      
      // Clear input
      searchHash = ''
      
    } catch (error) {
      // Error handling
      console.error('Search download failed:', error)
      const errorMessage = error instanceof Error ? error.message : 'Unknown error'
      showNotification(tr('download.notifications.searchFailed', { values: { error: errorMessage } }), 'error', 6000)
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
        return { ...file, status: newStatus }
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
      const [, { save }] = await Promise.all([
        import('@tauri-apps/api/core'),
        import('@tauri-apps/plugin-dialog')
      ]);
      
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
      
      // Start the actual download
      files.update(f => f.map(file => 
        file.id === fileId ? { ...file, progress: 10 } : file
      ));
      
      // Simulate progress while downloading - complete when progress reaches 100%
      const progressInterval = setInterval(() => {
        files.update(f => f.map(file => {
          if (file.id === fileId && file.status === 'downloading') {
            const currentProgress = file.progress || 10;
            const newProgress = Math.min(100, currentProgress + Math.random() * 8 + 2); // 2-10% increment

            // Check if download just completed
            if (newProgress >= 100) {
              // Complete the download
              clearInterval(progressInterval);
              activeSimulations.delete(fileId);
              showNotification(`Download completed: ${fileToDownload.name} saved to ${outputPath}`, 'success');

              return { ...file, progress: 100, status: 'completed', downloadPath: outputPath };
            }

            return { ...file, progress: newProgress };
          }
          return file;
        }));

        // Check if download was cancelled (cleanup if needed)
        const currentFile = $files.find(f => f.id === fileId);
        if (!currentFile || currentFile.status === 'canceled') {
          clearInterval(progressInterval);
          activeSimulations.delete(fileId);
        }
      }, 500);
      
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
    <h1 class="text-3xl font-bold">{$t('download.title')}</h1>
    <p class="text-muted-foreground mt-2">{$t('download.subtitle')}</p>
  </div>
  
  <Card class="p-6">
    <div class="space-y-4">
      <div>
        <Label for="hash-input" class="text-base font-medium">{$t('download.addNew')}</Label>
        <p class="text-sm text-muted-foreground mt-1 mb-3">
          {$t('download.addNewSubtitle')}
        </p>
        <div class="flex flex-col sm:flex-row gap-3">
          <div class="relative flex-1">
            <Input
              id="hash-input"
              bind:value={searchHash}
              placeholder={$t('download.placeholder')}
              class="pr-10 h-10"
            />
            {#if searchHash}
              <button
                on:click={clearSearch}
                class="absolute right-2 top-1/2 transform -translate-y-1/2 p-1 hover:bg-muted rounded-full transition-colors"
                type="button"
                aria-label={$t('download.clearInput')}
              >
                <X class="h-4 w-4 text-muted-foreground hover:text-foreground" />
              </button>
            {/if}
          </div>
          <Button
            on:click={searchForFile}
            disabled={!searchHash.trim() || isSearching}
            class="h-10 px-6"
          >
            <Search class="h-4 w-4 mr-2" />
            {isSearching ? 'Searching...' : 'Search'}
          </Button>
        </div>

        <!-- Search Results Section (within the same card) -->
        {#if hasSearched}
          <div class="pt-6 border-t">
            <h3 class="text-lg font-semibold mb-4">Search Results</h3>

            {#if searchResults.length === 0}
              <div class="text-center py-6">
                <div class="text-muted-foreground">
                  <Search class="h-8 w-8 mx-auto mb-3 opacity-50" />
                  <p class="text-base mb-1">No files found</p>
                  <p class="text-sm">The file hash was not found in the network.</p>
                </div>
              </div>
            {:else}
              <div class="space-y-4">
                {#each searchResults as result}
                  <div class="p-3 bg-secondary rounded-lg hover:bg-secondary/80 transition-colors">
                    <div class="flex flex-col sm:flex-row sm:items-start sm:justify-between gap-4">
                      <div class="flex items-start gap-3 flex-1">
                        <svelte:component this={getFileIcon(result.fileName)} class="h-4 w-4 text-muted-foreground mt-0.5" />
                        <div class="flex-1 min-w-0">
                          <div class="flex items-center gap-3 mb-1">
                            <h4 class="font-semibold text-sm truncate">{result.fileName}</h4>
                            <Badge class="text-xs font-semibold bg-muted-foreground/20 text-foreground border-0 px-2 py-0.5">{(result.fileSize / 1024 / 1024).toFixed(1)} MB</Badge>
                          </div>
                          <div class="flex items-center gap-x-3 gap-y-1 mt-1">
                            <p class="text-xs text-muted-foreground truncate">Hash: {result.fileHash}</p>
                            <span class="text-xs text-muted-foreground">•</span>
                            <span class="flex items-center gap-1">
                              <span class="w-2 h-2 bg-green-500 rounded-full"></span>
                              <span class="text-xs text-muted-foreground">{result.seeders} seeders</span>
                            </span>
                            <span class="text-xs text-muted-foreground">•</span>
                            <span class="flex items-center gap-1">
                              <span class="w-2 h-2 bg-blue-500 rounded-full"></span>
                              <span class="text-xs text-muted-foreground">{result.leechers} leechers</span>
                            </span>
                          </div>

                        </div>
                      </div>

                      <div class="flex items-center gap-2">
                        <Button
                          on:click={() => downloadFromSearchResult(result)}
                          size="sm"
                        >
                          Download
                        </Button>
                      </div>
                    </div>

                    <!-- Peer list spanning full width -->
                    <details class="text-xs mt-2 ml-7">
                      <summary class="cursor-pointer text-muted-foreground hover:text-foreground">
                        View available peers
                      </summary>
                      <div class="mt-2 space-y-1">
                        {#each result.peers as peer}
                          <div class="flex items-center py-1">
                            <span class="text-sm text-foreground mr-1">•</span>
                            <span class="text-sm text-foreground">{peer.nickname}</span>
                            <div class="flex items-center gap-2 ml-3">
                              <Badge variant="outline" class="text-xs border-yellow-400 text-yellow-600">
                                <Star class="h-3 w-3 mr-1 fill-yellow-400 text-yellow-400" />
                                {peer.reputation}
                              </Badge>
                              <Badge variant="outline" class="text-xs {peer.connection === 'fast' ? 'border-green-500 text-green-600' : peer.connection === 'average' ? 'border-yellow-400 text-yellow-600' : 'border-red-500 text-red-600'}">
                                <Zap class="h-3 w-3 mr-1 {peer.connection === 'fast' ? 'fill-green-500 text-green-500' : peer.connection === 'average' ? 'fill-yellow-400 text-yellow-400' : 'text-red-500 fill-red-500'}" />
                                {peer.connection}
                              </Badge>
                            </div>
                          </div>
                        {/each}
                      </div>
                    </details>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {/if}
      </div>
    </div>
  </Card>

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
                  <span class="text-foreground">{$t('download.file.progress')}</span>
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
                    Show in Folder
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
