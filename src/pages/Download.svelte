<script lang="ts">
  import Button from '$lib/components/ui/button.svelte'
  import Card from '$lib/components/ui/card.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import Progress from '$lib/components/ui/progress.svelte'
  import { Search, Pause, Play, X, ChevronUp, ChevronDown, Settings, FolderOpen, File as FileIcon, FileText, FileImage, FileVideo, FileAudio, Archive, Code, FileSpreadsheet, Presentation, Globe, Blocks } from 'lucide-svelte'
  import { files, downloadQueue, activeTransfers } from '$lib/stores'
  import { dhtService } from '$lib/dht'
  import DownloadSearchSection from '$lib/components/download/DownloadSearchSection.svelte'
  import type { FileMetadata } from '$lib/dht'
  import { onDestroy, onMount } from 'svelte'
  import { t } from 'svelte-i18n'
  import { get } from 'svelte/store'
  import { toHumanReadableSize } from '$lib/utils'
  import { initDownloadTelemetry, disposeDownloadTelemetry } from '$lib/downloadTelemetry'
  import { MultiSourceDownloadService, type MultiSourceProgress } from '$lib/services/multiSourceDownloadService'
  import { listen } from '@tauri-apps/api/event'
  import PeerSelectionService from '$lib/services/peerSelectionService'


  import { invoke }  from '@tauri-apps/api/core';

  const tr = (k: string, params?: Record<string, any>) => (get(t) as any)(k, params)

  // Protocol selection state
  let selectedProtocol: 'WebRTC' | 'Bitswap';
  let hasSelectedProtocol = false

  function handleProtocolSelect(protocol: 'WebRTC' | 'Bitswap') {
    selectedProtocol = protocol
    hasSelectedProtocol = true
  }

  onMount(() => {
    initDownloadTelemetry()

    // Listen for multi-source download events
    const setupEventListeners = async () => {
      try {
        const unlistenProgress = await listen('multi_source_progress_update', (event) => {
          const progress = event.payload as MultiSourceProgress

          // Find the corresponding file and update its progress
          files.update(f => f.map(file => {
            if (file.hash === progress.fileHash) {
              const percentage = MultiSourceDownloadService.getCompletionPercentage(progress);
              return {
                ...file,
                progress: percentage,
                status: 'downloading' as const,
                speed: MultiSourceDownloadService.formatSpeed(progress.downloadSpeedBps),
                eta: MultiSourceDownloadService.formatETA(progress.etaSeconds)
              };
            }
            return file;
          }));

          multiSourceProgress.set(progress.fileHash, progress)
          multiSourceProgress = multiSourceProgress // Trigger reactivity
        })

        const unlistenCompleted = await listen('multi_source_download_completed', (event) => {
          const data = event.payload as any

          // Update file status to completed
          files.update(f => f.map(file => {
            if (file.hash === data.file_hash) {
              return {
                ...file,
                status: 'completed' as const,
                progress: 100,
                downloadPath: data.output_path
              };
            }
            return file;
          }));

          multiSourceProgress.delete(data.file_hash)
          multiSourceProgress = multiSourceProgress
          showNotification(`Multi-source download completed: ${data.file_name}`, 'success')
        })

        const unlistenStarted = await listen('multi_source_download_started', (event) => {
          const data = event.payload as any
          showNotification(`Multi-source download started with ${data.total_peers} peers`, 'info')
        })

        const unlistenFailed = await listen('multi_source_download_failed', (event) => {
          const data = event.payload as any

          // Update file status to failed
          files.update(f => f.map(file => {
            if (file.hash === data.file_hash) {
              return {
                ...file,
                status: 'failed' as const
              };
            }
            return file;
          }));

          multiSourceProgress.delete(data.file_hash)
          multiSourceProgress = multiSourceProgress
          showNotification(`Multi-source download failed: ${data.error}`, 'error')
        })

        // Cleanup listeners on destroy
        return () => {
          unlistenProgress()
          unlistenCompleted()
          unlistenStarted()
          unlistenFailed()
        }
      } catch (error) {
        console.error('Failed to setup event listeners:', error)
        return () => {} // Return empty cleanup function
      }
    }

    setupEventListeners()
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

  // Multi-source download state
  let multiSourceProgress = new Map<string, MultiSourceProgress>()
  let multiSourceEnabled = true
  let maxPeersPerDownload = 3

  // Add notification related variables
  let currentNotification: HTMLElement | null = null
  let showSettings = false // Toggle for settings panel

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
  // Commented out - not currently used but kept for future reference
  // async function saveRawData(fileName: string, data: Uint8Array) {
  //   try {
  //     const { save } = await import('@tauri-apps/plugin-dialog');
  //     const filePath = await save({ defaultPath: fileName });
  //     if (filePath) {
  //       const { writeFile } = await import('@tauri-apps/plugin-fs');
  //       await writeFile(filePath, new Uint8Array(data));
  //       showNotification(`Successfully saved "${fileName}"`, 'success');
  //     }
  //   } catch (error) {
  //     console.error('Failed to save file:', error);
  //     showNotification(`Error saving "${fileName}"`, 'error');
  //   }
  // }

  function handleSearchMessage(event: CustomEvent<{ message: string; type?: 'success' | 'error' | 'info' | 'warning'; duration?: number }>) {
    const { message, type = 'info', duration = 4000 } = event.detail
    showNotification(message, type, duration)
  }

  async function handleSearchDownload(metadata: FileMetadata) {
    const allFiles = [...$downloadQueue]
    const existingFile = allFiles.find((file) => file.hash === metadata.fileHash)
    
    if (selectedProtocol === 'Bitswap') {
      return;
    }

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
      // Pass encryption info to the download item
      isEncrypted: metadata.isEncrypted,
      manifest: metadata.manifest ? JSON.parse(metadata.manifest) : null
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
        console.log("🔍 DEBUG: Starting download for file:", fileToDownload.name);
        const { save } = await import('@tauri-apps/plugin-dialog');

        // Show file save dialog
        console.log("🔍 DEBUG: Opening file save dialog...");
        const outputPath = await save({
          defaultPath: fileToDownload.name,
          filters: [{
            name: 'All Files',
            extensions: ['*']
          }]
        });
        console.log("✅ DEBUG: File save dialog result:", outputPath);

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


      // Determine seeders, prefer local seeder when available
        let seeders = (fileToDownload.seederAddresses || []).slice();
        try {
          const localPeerId = dhtService.getPeerId ? dhtService.getPeerId() : null;
          if ((!seeders || seeders.length === 0) && fileToDownload.status === 'seeding') {
            if (localPeerId) seeders.unshift(localPeerId)
            else seeders.unshift('local_peer')
          }
        } catch (e) {
          // ignore
        }

        // If the local copy is available and we're running in Tauri, copy directly to outputPath
        const localPeerIdNow = dhtService.getPeerId ? dhtService.getPeerId() : null;

        if (outputPath && (localPeerIdNow || seeders.includes('local_peer'))) {
          try {

            let hash = fileToDownload.hash
            console.log("🔍 DEBUG: Attempting to get file data for hash:", hash);
            const base64Data = await invoke('get_file_data', { fileHash: hash }) as string;
            console.log("✅ DEBUG: Retrieved base64 data length:", base64Data.length);

            // Convert base64 to Uint8Array
            let data_ = new Uint8Array(0); // Default empty array
            if (base64Data && base64Data.length > 0) {
              const binaryStr = atob(base64Data);
              data_ = new Uint8Array(binaryStr.length);
              for (let i = 0; i < binaryStr.length; i++) {
                data_[i] = binaryStr.charCodeAt(i);
              }
              console.log("Converted to Uint8Array with length:", data_.length);
            } else {
              console.warn("No file data found for hash:", hash);
            }

            console.log("Final data array length:", data_.length);

            // Write the file data to the output path
            console.log("🔍 DEBUG: About to write file to:", outputPath);
            const { writeFile } = await import('@tauri-apps/plugin-fs');
            await writeFile(outputPath, data_);
            console.log("✅ DEBUG: File written successfully to:", outputPath);
            files.update(f => f.map(file => file.id === fileId ? { ...file, status: 'completed', progress: 100, downloadPath: outputPath } : file));
            showNotification(tr('download.notifications.downloadComplete', { values: { name: fileToDownload.name } }), 'success');
            activeSimulations.delete(fileId);
            console.log("Done with downloading file")
            return;
          } catch (e) {
            console.error('❌ DEBUG: Local copy fallback failed:', e);
            console.error('❌ DEBUG: Error details:', e);
            showNotification(`Download failed: ${e}`, 'error');
            activeSimulations.delete(fileId);
            files.update(f => f.map(file =>
              file.id === fileId
                ? { ...file, status: 'failed' }
                : file
            ));
            return; // Don't continue to P2P download
          }
        }

      // Show "automatically started" message now that download is proceeding
      showNotification(tr('download.notifications.autostart'), 'info');

       if (fileToDownload.isEncrypted && fileToDownload.manifest) {
        // 1. Download all the required encrypted chunks using the P2P service.
        //    This new function will handle fetching multiple chunks in parallel.
        showNotification(`Downloading encrypted chunks for "${fileToDownload.name}"...`, 'info');

        const { p2pFileTransferService } = await import('$lib/services/p2pFileTransfer');


        await p2pFileTransferService.downloadEncryptedChunks(
          fileToDownload.manifest,
          seeders, // Pass the list of seeders
          (progress) => { // This is the progress callback
            files.update(f => f.map(file =>
              file.id === fileId ? { ...file, progress: progress.percentage, status: 'downloading', speed: progress.speed, eta: progress.eta } : file
            ));
          }
        );

        // 2. Once all chunks are downloaded, call the backend to decrypt.
        showNotification(`All chunks received. Decrypting file...`, 'info');
        const { encryptionService } = await import('$lib/services/encryption');
        await encryptionService.decryptFile(fileToDownload.manifest, outputPath);

        // 3. Mark the download as complete.
        files.update(f => f.map(file =>
          file.id === fileId ? { ...file, status: 'completed', progress: 100, downloadPath: outputPath } : file
        ));
        showNotification(`Successfully decrypted and saved "${fileToDownload.name}"!`, 'success');
        activeSimulations.delete(fileId);

      } else {
        // Check if we should use multi-source download
        const seeders = fileToDownload.seederAddresses || [];

        if (multiSourceEnabled && seeders.length >= 2 && fileToDownload && fileToDownload.size > 1024 * 1024) {
          // Use multi-source download for files > 1MB with multiple seeders
          const downloadStartTime = Date.now();
          try {
            showNotification(`Starting multi-source download from ${seeders.length} peers...`, 'info');

            if (!outputPath) {
              throw new Error('Output path is required for download');
            }

            await MultiSourceDownloadService.startDownload(
              fileToDownload.hash,
              outputPath,
              {
                maxPeers: maxPeersPerDownload,
                selectedPeers: seeders,  // Pass selected peers from peer selection modal
                peerAllocation: (fileToDownload as any).peerAllocation  // Pass manual allocation if available
              }
            );

            // The progress updates will be handled by the event listeners in onMount
            activeSimulations.delete(fileId);

            // Record transfer success metrics for each peer
            const downloadDuration = Date.now() - downloadStartTime;
            for (const peerId of seeders) {
              try {
                await PeerSelectionService.recordTransferSuccess(
                  peerId,
                  fileToDownload.size,
                  downloadDuration
                );
              } catch (error) {
                console.error(`Failed to record success for peer ${peerId}:`, error);
              }
            }

          } catch (error) {
            console.error('Multi-source download failed, falling back to P2P:', error);

            // Record transfer failures for each peer
            for (const peerId of seeders) {
              try {
                await PeerSelectionService.recordTransferFailure(
                  peerId,
                  error instanceof Error ? error.message : 'Multi-source download failed'
                );
              } catch (recordError) {
                console.error(`Failed to record failure for peer ${peerId}:`, recordError);
              }
            }

            // Fall back to single-peer P2P download
            await fallbackToP2PDownload();
          }
        } else {
          // Use traditional P2P download for smaller files or single seeder
          await fallbackToP2PDownload();
        }

        async function fallbackToP2PDownload() {
          const { p2pFileTransferService } = await import('$lib/services/p2pFileTransfer');

          try {
            if (seeders.length === 0) {
              throw new Error('No seeders available for this file');
            }

            // Create file metadata for P2P transfer
            const fileMetadata = fileToDownload ? {
              fileHash: fileToDownload.hash,
              fileName: fileToDownload.name,
              fileSize: fileToDownload.size,
              seeders: seeders,
              createdAt: Date.now(),
              isEncrypted: false
            } : null;

            if (!fileMetadata) {
              throw new Error('File metadata is not available');
            }

            // Track download start time for metrics
            const p2pStartTime = Date.now();

            // Initiate P2P download with file saving
            const transferId = await p2pFileTransferService.initiateDownloadWithSave(
              fileMetadata,
              seeders,
              outputPath || undefined,
              async (transfer) => {
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

                // Show notification and record metrics on completion or failure
                if (transfer.status === 'completed' && fileToDownload) {
                  showNotification(tr('download.notifications.downloadComplete', { values: { name: fileToDownload.name } }), 'success');

                  // Record success metrics for each peer
                  const duration = Date.now() - p2pStartTime;
                  for (const peerId of seeders) {
                    try {
                      await PeerSelectionService.recordTransferSuccess(peerId, fileToDownload.size, duration);
                    } catch (error) {
                      console.error(`Failed to record P2P success for peer ${peerId}:`, error);
                    }
                  }
                } else if (transfer.status === 'failed' && fileToDownload) {
                  showNotification(tr('download.notifications.downloadFailed', { values: { name: fileToDownload.name } }), 'error');

                  // Record failure metrics for each peer
                  for (const peerId of seeders) {
                    try {
                      await PeerSelectionService.recordTransferFailure(peerId, 'P2P download failed');
                    } catch (error) {
                      console.error(`Failed to record P2P failure for peer ${peerId}:`, error);
                    }
                  }
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
            showNotification("BAD","error");
            activeSimulations.delete(fileId);
            files.update(f => f.map(file =>
              file.id === fileId
                ? { ...file, status: 'failed' }
                : file
            ));
          }
        }
      }
    } catch (error) {
      // Download failed
      showNotification("BADHI", 'error');
      activeSimulations.delete(fileId);

      files.update(f => f.map(file =>
        file.id === fileId
          ? { ...file, status: 'failed' }
          : file
      ));

      const errorMsg = error instanceof Error ? error.message : String(error);
      console.error('Download failed:', error, fileToDownload);
      showNotification(
        tr('download.notifications.downloadFailed', { values: { name: fileToDownload?.name || 'Unknown file' } }) + (errorMsg ? `: ${errorMsg}` : ''),
        'error'
      );
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

  <!-- Protocol Selection -->
  {#if !hasSelectedProtocol}
   <Card>
      <div class="p-6">
        <h2 class="text-2xl font-bold mb-6 text-center">{$t('download.selectProtocol')}</h2>
        <div class="grid grid-cols-1 md:grid-cols-2 gap-6 max-w-2xl mx-auto">
          <!-- WebRTC Option -->
          <button
            class="p-6 border-2 rounded-lg hover:border-blue-500 transition-colors duration-200 flex flex-col items-center gap-4 {selectedProtocol === 'WebRTC' ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/20' : 'border-gray-200 dark:border-gray-700'}"
            on:click={() => handleProtocolSelect('WebRTC')}
          >
            <div class="w-16 h-16 flex items-center justify-center bg-blue-100 rounded-full">
              <Globe class="w-8 h-8 text-blue-600" />
            </div>
            <div class="text-center">
              <h3 class="text-lg font-semibold mb-2">WebRTC</h3>
              <p class="text-sm text-gray-600 dark:text-gray-400">
                {$t('upload.webrtcDescription')}
              </p>
            </div>
          </button>

          <!-- Bitswap Option -->
          <button
            class="p-6 border-2 rounded-lg hover:border-blue-500 transition-colors duration-200 flex flex-col items-center gap-4 {selectedProtocol === 'Bitswap' ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/20' : 'border-gray-200 dark:border-gray-700'}"
            on:click={() => handleProtocolSelect('Bitswap')}
          >
            <div class="w-16 h-16 flex items-center justify-center bg-blue-100 rounded-full">
              <Blocks class="w-8 h-8 text-blue-600" />
            </div>
            <div class="text-center">
              <h3 class="text-lg font-semibold mb-2">Bitswap</h3>
              <p class="text-sm text-gray-600 dark:text-gray-400">
                {$t('upload.bitswapDescription')}
              </p>
            </div>
          </button>
        </div>
      </div>
    </Card>

  {:else}
    <DownloadSearchSection
      on:download={(event) => handleSearchDownload(event.detail)}
      on:message={handleSearchMessage}
      isBitswap={selectedProtocol === 'Bitswap'}
    />
  {/if}

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
        <!-- Filter Buttons and Clear Finished -->
        <div class="flex flex-wrap items-center gap-2">
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

          {#if completedCount > 0 || failedCount > 0 || allFilteredDownloads.filter(f => f.status === 'canceled').length > 0}
            <Button
              size="sm"
              variant="outline"
              on:click={clearAllFinished}
              class="text-xs text-destructive border-destructive hover:bg-destructive/10 hover:text-destructive"
            >
              <X class="h-3 w-3 mr-1" />
              {$t('download.clearFinished')}
            </Button>
          {/if}
        </div>

        <!-- Settings Toggle Button -->
        <Button
          size="sm"
          variant="outline"
          on:click={() => showSettings = !showSettings}
          class="text-xs"
        >
          <Settings class="h-3 w-3 mr-1" />
          {$t('download.settings.title')}
          {#if showSettings}
            <ChevronUp class="h-3 w-3 ml-1" />
          {:else}
            <ChevronDown class="h-3 w-3 ml-1" />
          {/if}
        </Button>
      </div>

      <!-- Collapsible Settings Panel -->
      {#if showSettings}
        <Card class="p-4 bg-muted/50 border-dashed">
          <div class="space-y-4">
            <h3 class="text-sm font-semibold text-muted-foreground uppercase tracking-wide">
              {$t('download.settings.title')}
            </h3>

            <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
              <!-- Concurrency Settings -->
              <div class="space-y-3">
                <h4 class="text-xs font-medium text-muted-foreground uppercase tracking-wide">
                  {$t('download.settings.concurrency')}
                </h4>
                <div class="space-y-2">
                  <div class="flex items-center justify-between">
                    <Label class="text-sm">{$t('download.settings.maxConcurrent')}:</Label>
                    <input
                      type="number"
                      bind:value={maxConcurrentDownloads}
                      on:input={handleMaxConcurrentInput}
                      on:blur={validateMaxConcurrent}
                      min="1"
                      step="1"
                      class="w-16 h-8 text-center text-sm border border-input bg-background px-2 py-1 rounded-md focus:ring-2 focus:ring-ring focus:ring-offset-2"
                    />
                  </div>

                  {#if multiSourceEnabled}
                    <div class="flex items-center justify-between">
                      <Label class="text-sm">{$t('download.maxPeers')}:</Label>
                      <input
                        type="number"
                        bind:value={maxPeersPerDownload}
                        min="2"
                        max="10"
                        step="1"
                        class="w-16 h-8 text-center text-sm border border-input bg-background px-2 py-1 rounded-md focus:ring-2 focus:ring-ring focus:ring-offset-2"
                      />
                    </div>
                  {/if}
                </div>
              </div>

              <!-- Automation Settings -->
              <div class="space-y-3">
                <h4 class="text-xs font-medium text-muted-foreground uppercase tracking-wide">
                  {$t('download.settings.automation')}
                </h4>
                <div class="space-y-3">
                  <div class="flex items-center justify-between">
                    <Label class="text-sm">{$t('download.settings.autoStart')}:</Label>
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

                  <div class="flex items-center justify-between">
                    <Label class="text-sm">{$t('download.autoClear')}:</Label>
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

              <!-- Feature Settings -->
              <div class="space-y-3">
                <h4 class="text-xs font-medium text-muted-foreground uppercase tracking-wide">
                  {$t('download.settings.features')}
                </h4>
                <div class="space-y-3">
                  <div class="flex items-center justify-between">
                    <Label class="text-sm">{$t('download.multiSource')}:</Label>
                    <button
                      type="button"
                      aria-label="Toggle multi-source downloads"
                      on:click={() => multiSourceEnabled = !multiSourceEnabled}
                      class="relative inline-flex h-4 w-8 items-center rounded-full transition-colors focus:outline-none"
                      class:bg-green-500={multiSourceEnabled}
                      class:bg-muted-foreground={!multiSourceEnabled}
                    >
                      <span
                        class="inline-block h-3 w-3 rounded-full bg-white transition-transform shadow-sm"
                        style="transform: translateX({multiSourceEnabled ? '18px' : '2px'})"
                      ></span>
                    </button>
                  </div>
                </div>
              </div>
            </div>
          </div>
        </Card>
      {/if}
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
          <div class="p-3 bg-muted/60 rounded-lg hover:bg-muted/80 transition-colors">
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
                        {#if multiSourceProgress.has(file.hash)}
                          <Badge class="bg-purple-100 text-purple-800 text-xs px-2 py-0.5">
                            Multi-source
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
                    <span>Speed: {file.status === 'paused' ? '0 B/s' : (file.speed || '0 B/s')}</span>
                    <span>ETA: {file.status === 'paused' ? 'N/A' : (file.eta || 'N/A')}</span>
                    {#if multiSourceProgress.has(file.hash) && file.status === 'downloading'}
                      {@const msProgress = multiSourceProgress.get(file.hash)}
                      {#if msProgress}
                        <span class="text-purple-600">Peers: {msProgress.activePeers}</span>
                        <span class="text-purple-600">Chunks: {msProgress.completedChunks}/{msProgress.totalChunks}</span>
                      {/if}
                    {/if}
                  </div>
                  <span class="text-foreground">{(file.progress || 0).toFixed(2)}%</span>
                </div>
                <Progress
                  value={file.progress || 0}
                  max={100}
                  class="h-2 bg-border [&>div]:bg-green-500 w-full"
                />
                {#if multiSourceProgress.has(file.hash)}
                  {@const msProgress = multiSourceProgress.get(file.hash)}
                  {#if msProgress && msProgress.peerAssignments.length > 0}
                    <div class="mt-2 space-y-1">
                      <div class="text-xs text-muted-foreground">Peer progress:</div>
                      {#each msProgress.peerAssignments as peerAssignment}
                        <div class="flex items-center gap-2 text-xs">
                          <span class="w-20 truncate">{peerAssignment.peerId.slice(0, 8)}...</span>
                          <div class="flex-1 bg-muted rounded-full h-1">
                            <div
                              class="bg-purple-500 h-1 rounded-full transition-all duration-300"
                              style="width: {peerAssignment.status === 'Completed' ? 100 : peerAssignment.status === 'Downloading' ? 50 : 0}%"
                            ></div>
                          </div>
                          <span class="text-muted-foreground">{peerAssignment.status}</span>
                        </div>
                      {/each}
                    </div>
                  {/if}
                {/if}
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
