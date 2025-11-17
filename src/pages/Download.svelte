<script lang="ts">
  import Button from '$lib/components/ui/button.svelte'
  import Card from '$lib/components/ui/card.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import Progress from '$lib/components/ui/progress.svelte'
  import { Search, Pause, Play, X, ChevronUp, ChevronDown, Settings, FolderOpen, File as FileIcon, FileText, FileImage, FileVideo, FileAudio, Archive, Code, FileSpreadsheet, Presentation, History, Download as DownloadIcon, Upload as UploadIcon, Trash2, RefreshCw } from 'lucide-svelte'
  import { files, downloadQueue, activeTransfers, wallet } from '$lib/stores'
  import { dhtService } from '$lib/dht'
  import { paymentService } from '$lib/services/paymentService'
  import DownloadSearchSection from '$lib/components/download/DownloadSearchSection.svelte'
  import type { FileMetadata } from '$lib/dht'
  import { onDestroy, onMount } from 'svelte'
  import { t } from 'svelte-i18n'
  import { get } from 'svelte/store'
  import { toHumanReadableSize } from '$lib/utils'
  import { buildSaveDialogOptions } from '$lib/utils/saveDialog'
  import { initDownloadTelemetry, disposeDownloadTelemetry } from '$lib/downloadTelemetry'
  import { MultiSourceDownloadService, type MultiSourceProgress } from '$lib/services/multiSourceDownloadService'
  import { listen } from '@tauri-apps/api/event'
  import PeerSelectionService from '$lib/services/peerSelectionService'
  import { downloadHistoryService, type DownloadHistoryEntry } from '$lib/services/downloadHistoryService'
  import { showToast } from '$lib/toast'

  import { invoke } from '@tauri-apps/api/core'
  import { homeDir } from '@tauri-apps/api/path'

  const tr = (k: string, params?: Record<string, any>) => $t(k, params)

 // Auto-detect protocol based on file metadata
  let detectedProtocol: 'WebRTC' | 'Bitswap' | null = null
  let torrentDownloads = new Map<string, any>();
  onMount(() => {
    // Initialize payment service to load persisted wallet and transactions
    paymentService.initialize();

    initDownloadTelemetry()

    // Listen for multi-source download events
    const setupEventListeners = async () => {
      // Listen for BitTorrent events
      const unlistenTorrentEvent = await listen('torrent_event', (event) => {
        const payload = event.payload as any;
        console.log('Received torrent event:', payload);

        if (payload.Progress) {
          const { info_hash, downloaded, total, speed, peers, eta_seconds } = payload.Progress;
          torrentDownloads.set(info_hash, {
            info_hash,
            name: torrentDownloads.get(info_hash)?.name || 'Fetching name...',
            status: 'downloading',
            progress: total > 0 ? (downloaded / total) * 100 : 0,
            speed: toHumanReadableSize(speed) + '/s',
            eta: eta_seconds ? `${eta_seconds}s` : 'N/A',
            peers,
            size: total,
          });
          torrentDownloads = new Map(torrentDownloads); // Trigger reactivity
        } else if (payload.Complete) {
          const { info_hash, name } = payload.Complete;
          const existing = torrentDownloads.get(info_hash);
          if (existing) {
            torrentDownloads.set(info_hash, { ...existing, status: 'completed', progress: 100 });
            torrentDownloads = new Map(torrentDownloads);
            showNotification(`Torrent download complete: ${name}`, 'success');
          }
        } else if (payload.Added) {
            const { info_hash, name } = payload.Added;
            torrentDownloads.set(info_hash, {
                info_hash,
                name,
                status: 'downloading',
                progress: 0,
                speed: '0 B/s',
                eta: 'N/A',
                peers: 0,
                size: 0,
            });
            torrentDownloads = new Map(torrentDownloads);
            showNotification(`Torrent added: ${name}`, 'info');
        } else if (payload.Removed) {
            const { info_hash } = payload.Removed;
            if (torrentDownloads.has(info_hash)) {
                const name = torrentDownloads.get(info_hash)?.name || 'Unknown';
                torrentDownloads.delete(info_hash);
                torrentDownloads = new Map(torrentDownloads);
                showNotification(`Torrent removed: ${name}`, 'warning');
            }
        }
      });

      // Cleanup torrent listener
      onDestroy(() => {
        unlistenTorrentEvent();
      });
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

        const unlistenBitswapProgress = await listen('bitswap_chunk_downloaded', (event) => {
          const progress = event.payload as {
                fileHash: string;
                chunkIndex: number;
                totalChunks: number;
                chunkSize: number;
            };

            files.update(f => f.map(file => {
                if (file.hash === progress.fileHash) {
                    const downloadedChunks = new Set(file.downloadedChunks || []);
                    
                    if (downloadedChunks.has(progress.chunkIndex)) {
                        return file; // Already have this chunk, do nothing.
                    }
                    downloadedChunks.add(progress.chunkIndex);
                    const newSize = downloadedChunks.size;

                    let bitswapStartTime = file.downloadStartTime;
                    if (newSize === 1) {
                        // This is the first chunk, start the timer
                        bitswapStartTime = Date.now();
                    }

                    let speed = file.speed || '0 B/s';
                    let eta = file.eta || 'N/A';

                    if (bitswapStartTime) {
                        const elapsedTimeMs = Date.now() - bitswapStartTime;
                        
                        // We have downloaded `newSize - 1` chunks since the timer started.
                        const downloadedBytesSinceStart = (newSize - 1) * progress.chunkSize;
                        
                        if (elapsedTimeMs > 500) { // Get a better average over a short time.
                            const speedBytesPerSecond = downloadedBytesSinceStart > 0 ? (downloadedBytesSinceStart / elapsedTimeMs) * 1000 : 0;
                            
                            if (speedBytesPerSecond < 1000) {
                                speed = `${speedBytesPerSecond.toFixed(0)} B/s`;
                            } else if (speedBytesPerSecond < 1000 * 1000) {
                                speed = `${(speedBytesPerSecond / 1000).toFixed(2)} KB/s`;
                            } else {
                                speed = `${(speedBytesPerSecond / (1000 * 1000)).toFixed(2)} MB/s`;
                            }

                            const remainingChunks = progress.totalChunks - newSize;
                            if (speedBytesPerSecond > 0) {
                                const remainingBytes = remainingChunks * progress.chunkSize;
                                const etaSeconds = remainingBytes / speedBytesPerSecond;
                                eta = `${Math.round(etaSeconds)}s`;
                            } else {
                                eta = 'N/A';
                            }
                        }
                    }
                    
                    const percentage = (newSize / progress.totalChunks) * 100;
                    
                    return {
                        ...file,
                        progress: percentage,
                        status: 'downloading' as const,
                        downloadedChunks: Array.from(downloadedChunks),
                        totalChunks: progress.totalChunks,
                        downloadStartTime: bitswapStartTime,
                        speed: speed,
                        eta: eta,
                    };
                }
                return file;
            }));
        });

        const unlistenDownloadCompleted = await listen('file_content', async (event) => {
            const metadata = event.payload as any;

            // Find the file that just completed
            const completedFile = $files.find(f => f.hash === metadata.merkleRoot);

            if (completedFile && !paidFiles.has(completedFile.hash)) {
                // Process payment for Bitswap download (only once per file)
                console.log('💰 Bitswap download completed, processing payment...');
                const paymentAmount = await paymentService.calculateDownloadCost(completedFile.size);
                
                // Skip payment check for free files (price = 0)
                if (paymentAmount === 0) {
                    console.log('Free file, skipping payment');
                    paidFiles.add(completedFile.hash);
                    showNotification(`Download complete! "${completedFile.name}" (Free)`, 'success');
                    return;
                }


                const seederPeerId = completedFile.seederAddresses?.[0];
                const seederWalletAddress = paymentService.isValidWalletAddress(completedFile.seederAddresses?.[0])
                  ? completedFile.seederAddresses?.[0]!
                  : null;                if (!seederWalletAddress) {
                  console.warn('Skipping Bitswap payment due to missing or invalid uploader wallet address', {
                      file: completedFile.name,
                      seederAddresses: completedFile.seederAddresses
                  });
                  showNotification('Payment skipped: missing uploader wallet address', 'warning');
              } else {
                    try {
                        const paymentResult = await paymentService.processDownloadPayment(
                            completedFile.hash,
                            completedFile.name,
                            completedFile.size,
                            seederWalletAddress,
                            seederPeerId
                        );

                        if (paymentResult.success) {
                            paidFiles.add(completedFile.hash); // Mark as paid
                            console.log(`✅ Bitswap payment processed: ${paymentAmount.toFixed(6)} Chiral to ${seederWalletAddress} (peer: ${seederPeerId})`);
                            showNotification(
                                `Download complete! Paid ${paymentAmount.toFixed(4)} Chiral`,
                                'success'
                            );
                        } else {
                            console.error('Bitswap payment failed:', paymentResult.error);
                            showNotification(`Payment failed: ${paymentResult.error}`, 'warning');
                        }
                    } catch (error) {
                        console.error('Error processing Bitswap payment:', error);
                        showNotification(`Payment failed: ${error instanceof Error ? error.message : 'Unknown error'}`, 'warning');
                    }
                }
            }

            // Update file status
            files.update(f => f.map(file => {
                if (file.hash === metadata.merkleRoot) {
                    return {
                        ...file,
                        status: 'completed' as const,
                        progress: 100,
                        downloadPath: metadata.downloadPath
                    };
                }
                return file;
            }));
        });

        // Listen for DHT errors (like missing CIDs)
        const unlistenDhtError = await listen('dht_event', (event) => {
          const eventStr = event.payload as string;
          if (eventStr.startsWith('error:')) {
            const errorMsg = eventStr.substring(6); // Remove 'error:' prefix
            console.error('DHT Error:', errorMsg);

            // Try to match error to a download in progress
            if (errorMsg.includes('No root CID found')) {
              // Find downloading files and mark them as failed
              files.update(f => f.map(file => {
                if (file.status === 'downloading' && (!file.cids || file.cids.length === 0)) {
                  showNotification(
                    `Download failed for "${file.name}": ${errorMsg}`,
                    'error',
                    6000
                  )
                  return { ...file, status: 'failed' as const }
                }
                return file
              }))
            }
          }
        });


        // Listen for WebRTC download completion
const unlistenWebRTCComplete = await listen('webrtc_download_complete', async (event) => {
  const data = event.payload as {
    fileHash: string;
    fileName: string;
    fileSize: number;
    data: number[]; // Array of bytes
  };

  try {
    // ✅ GET SETTINGS PATH
    const stored = localStorage.getItem("chiralSettings");
    if (!stored) {
      showNotification(
        'Please configure a download path in Settings before downloading files.',
        'error',
        8000
      );
      return;
    }
    
    const settings = JSON.parse(stored);
    let storagePath = settings.storagePath;
    
    if (!storagePath || storagePath === '.') {
      showNotification(
        'Please set a valid download path in Settings.',
        'error',
        8000
      );
      return;
    }
    
    // Expand ~ to home directory if needed
    if (storagePath.startsWith("~")) {
      const home = await homeDir();
      storagePath = storagePath.replace("~", home);
    }
    
    // Validate directory exists
    const dirExists = await invoke('check_directory_exists', { path: storagePath });
    if (!dirExists) {
      showNotification(
        `Download path "${settings.storagePath}" does not exist. Please update it in Settings.`,
        'error',
        8000
      );
      return;
    }

    // Construct full file path
    const { join } = await import('@tauri-apps/api/path');
    const outputPath = await join(storagePath, data.fileName);
    
    console.log(`✅ Saving WebRTC file to: ${outputPath}`);

    // Write the file to disk
    const { writeFile } = await import('@tauri-apps/plugin-fs');
    const fileData = new Uint8Array(data.data);
    await writeFile(outputPath, fileData);

    console.log(`✅ File saved successfully: ${outputPath}`);

    // Update status to completed
    files.update(f => f.map(file => 
      file.hash === data.fileHash
        ? { ...file, status: 'completed', progress: 100, downloadPath: outputPath }
        : file
    ));

    showNotification(`Successfully saved "${data.fileName}"`, 'success');
    
  } catch (error) {
    console.error('Failed to save WebRTC file:', error);
    const errorMessage = error instanceof Error ? error.message : String(error);
    showNotification(`Failed to save file: ${errorMessage}`, 'error');

    files.update(f => f.map(file =>
      file.hash === data.fileHash
        ? { ...file, status: 'failed' }
        : file
    ));
  }
});

        // Cleanup listeners on destroy
        return () => {
          unlistenProgress()
          unlistenCompleted()
          unlistenStarted()
          unlistenFailed()
          unlistenBitswapProgress()
          unlistenDownloadCompleted()
          unlistenDhtError()
          unlistenWebRTCComplete()
          unlistenTorrentEvent()
        }
      } catch (error) {
        console.error('Failed to setup event listeners:', error)
        return () => {} // Return empty cleanup function
      }
    }

    setupEventListeners()

    // Smart Resume: Load and auto-resume interrupted downloads
    loadAndResumeDownloads()
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

  // Smart Resume: Track resumed downloads
  let resumedDownloads = new Set<string>() // Track which downloads were auto-resumed

  // Track which files have already had payment processed
  let paidFiles = new Set<string>()

  // Download History state
  let showHistory = false
  let downloadHistory: DownloadHistoryEntry[] = []
  let historySearchQuery = ''
  let historyFilter: 'all' | 'completed' | 'failed' | 'canceled' = 'all'

  // Load history on mount
  $: downloadHistory = downloadHistoryService.getFilteredHistory(
    historyFilter === 'all' ? undefined : historyFilter,
    historySearchQuery
  )

  // Track files to add to history when they complete/fail
  $: {
    for (const file of $files) {
      if (['completed', 'failed', 'canceled'].includes(file.status)) {
        downloadHistoryService.addToHistory(file)
      }
    }
  }

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

  // Smart Resume: Save in-progress downloads to localStorage
  function saveDownloadState() {
    try {
      const activeDownloads = $files.filter(f => 
        f.status === 'downloading' || f.status === 'paused'
      ).map(file => ({
        id: file.id,
        name: file.name,
        hash: file.hash,
        size: file.size,
        progress: file.progress || 0,
        status: file.status,
        cids: file.cids,
        seederAddresses: file.seederAddresses,
        isEncrypted: file.isEncrypted,
        manifest: file.manifest,
        downloadPath: file.downloadPath,
        downloadStartTime: file.downloadStartTime,
        downloadedChunks: file.downloadedChunks,
        totalChunks: file.totalChunks
      }))

      const queuedDownloads = $downloadQueue.map(file => ({
        id: file.id,
        name: file.name,
        hash: file.hash,
        size: file.size,
        cids: file.cids,
        seederAddresses: file.seederAddresses,
        isEncrypted: file.isEncrypted,
        manifest: file.manifest
      }))

      localStorage.setItem('pendingDownloads', JSON.stringify({
        active: activeDownloads,
        queued: queuedDownloads,
        timestamp: Date.now()
      }))
    } catch (error) {
      console.error('Failed to save download state:', error)
    }
  }

  // Smart Resume: Load and resume interrupted downloads
  async function loadAndResumeDownloads() {
    try {
      const saved = localStorage.getItem('pendingDownloads')
      if (!saved) return

      const { active, queued, timestamp } = JSON.parse(saved)
      
      // Only auto-resume if less than 24 hours old
      const hoursSinceLastSave = (Date.now() - timestamp) / (1000 * 60 * 60)
      if (hoursSinceLastSave > 24) {
        console.log('Saved downloads are too old (>24h), skipping auto-resume')
        localStorage.removeItem('pendingDownloads')
        return
      }

      let resumeCount = 0

      // Restore queued downloads
      if (queued && queued.length > 0) {
        downloadQueue.set(queued)
        resumeCount += queued.length
      }

      // Restore active downloads (mark as paused, user can resume manually)
      if (active && active.length > 0) {
        const restoredFiles = active.map((file: any) => ({
          ...file,
          status: 'paused' as const, // Don't auto-start, let user resume
          speed: '0 B/s',
          eta: 'N/A'
        }))
        
        files.update(f => [...f, ...restoredFiles])
        
        // Track which downloads were resumed
        active.forEach((file: any) => resumedDownloads.add(file.id))
        resumeCount += active.length
      }

      if (resumeCount > 0) {
        const message = resumeCount === 1 
          ? `Restored 1 interrupted download. Resume it from the Downloads page.`
          : `Restored ${resumeCount} interrupted downloads. Resume them from the Downloads page.`
        showNotification(message, 'info', 6000)
      }

      // Clear saved state after successful restore
      localStorage.removeItem('pendingDownloads')
    } catch (error) {
      console.error('Failed to load download state:', error)
      localStorage.removeItem('pendingDownloads')
    }
  }

  function handleSearchMessage(event: CustomEvent<{ message: string; type?: 'success' | 'error' | 'info' | 'warning'; duration?: number }>) {
    const { message, type = 'info', duration = 4000 } = event.detail
    showNotification(message, type, duration)
  }

  async function handleSearchDownload(metadata: FileMetadata) {
    console.log('🔍 handleSearchDownload called with metadata:', metadata)

    // Auto-detect protocol based on file metadata
    const hasCids = metadata.cids && metadata.cids.length > 0
    detectedProtocol = hasCids ? 'Bitswap' : 'WebRTC'
    
    console.log(`🔍 Auto-detected protocol: ${detectedProtocol} (hasCids: ${hasCids})`)

    const allFiles = [...$downloadQueue]
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
      price: metadata.price ?? 0,
      status: 'queued' as const,
      priority: 'normal' as const,
      seeders: metadata.seeders.length, // Convert array length to number
      seederAddresses: metadata.seeders, // Array that only contains selected seeder rather than all seeders
      // Pass encryption info to the download item
      isEncrypted: metadata.isEncrypted,
      manifest: metadata.manifest ? JSON.parse(metadata.manifest) : null,
      cids: metadata.cids // IMPORTANT: Pass CIDs for Bitswap downloads
    }

    console.log('📦 Created new file for queue:', newFile)
    downloadQueue.update((queue) => [...queue, newFile])
    showNotification(tr('download.search.status.addedToQueue', { values: { name: metadata.fileName } }), 'success')

    console.log('⏭️ autoStartQueue:', autoStartQueue)
    if (autoStartQueue) {
      console.log('▶️ Calling processQueue...')
      await processQueue()
    }
  }

  async function addToDownloadQueue(metadata: FileMetadata) {
    await handleSearchDownload(metadata)
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

  })()  // Calculate counts from the filtered set (excluding uploaded/seeding)
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
        if (detectedProtocol!=='Bitswap')
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

  // Auto-clear completed downloads when setting is enabled
  $: if (autoClearCompleted) {
    files.update(f => f.filter(file => file.status !== 'completed'))
  }

  // Smart Resume: Auto-save download state when files or queue changes
  $: if ($files || $downloadQueue) {
    saveDownloadState()
  }

  // New function to download from search results
  async function processQueue() {
    console.log('📋 processQueue called')
    // Only prevent starting new downloads if we've reached the max concurrent limit
    const activeDownloads = $files.filter(f => f.status === 'downloading').length
    // Handle case where maxConcurrentDownloads might be empty during typing
    const maxConcurrent = Math.max(1, Number(maxConcurrentDownloads) || 3)
    console.log(`  Active downloads: ${activeDownloads}, Max: ${maxConcurrent}`)
    if (activeDownloads >= maxConcurrent) {
      console.log('  ⏸️ Max concurrent downloads reached, waiting...')
      return
    }

    const nextFile = $downloadQueue[0]
    if (!nextFile) {
      console.log('  ℹ️ Queue is empty')
      return
    }
    console.log('  📄 Next file from queue:', nextFile)
    downloadQueue.update(q => q.filter(f => f.id !== nextFile.id))
    const downloadingFile = {
      ...nextFile,
      status: 'downloading' as const,
      progress: 0,
      speed: '0 B/s', // Ensure speed property exists
      eta: 'N/A',     // Ensure eta property exists
      downloadStartTime: Date.now(), // Track start time for speed calculation
      downloadedChunks: [], // Track downloaded chunks for Bitswap
      totalChunks: 0 // Will be set when first chunk arrives
    }
    console.log('  ✏️ Created downloadingFile object:', downloadingFile)
    files.update(f => [...f, downloadingFile])
    console.log('  ✅ Added file to files store, detected protocol:', detectedProtocol)

    if (detectedProtocol === "Bitswap"){
  console.log('  🔍 Starting Bitswap download for:', downloadingFile.name)

  // CRITICAL: Bitswap requires CIDs to download
  if (!downloadingFile.cids || downloadingFile.cids.length === 0) {
    console.error('  ❌ No CIDs found for Bitswap download')
    files.update(f => f.map(file =>
      file.id === downloadingFile.id
        ? { ...file, status: 'failed' }
        : file
    ))
    showNotification(
      `Cannot download "${downloadingFile.name}": This file was not uploaded via Bitswap and has no CIDs. Please use WebRTC protocol instead.`,
      'error',
      8000
    )
    return
  }

  // Verify seeders are available
  if (!downloadingFile.seederAddresses || downloadingFile.seederAddresses.length === 0) {
    console.error('  ❌ No seeders found for download')
    files.update(f => f.map(file =>
      file.id === downloadingFile.id
        ? { ...file, status: 'failed' }
        : file
    ))
    showNotification(
      `Cannot download "${downloadingFile.name}": No seeders are currently online for this file.`,
      'error',
      6000
    )
    return
  }

  // ✅ VALIDATE SETTINGS PATH BEFORE DOWNLOADING
  try {
    const stored = localStorage.getItem("chiralSettings");
    if (!stored) {
      showNotification(
        'Please configure a download path in Settings before downloading files.',
        'error',
        8000
      );
      files.update(f => f.map(file =>
        file.id === downloadingFile.id
          ? { ...file, status: 'failed' }
          : file
      ));
      return;
    }
    
    const settings = JSON.parse(stored);
    let storagePath = settings.storagePath;
    
    if (!storagePath || storagePath === '.') {
      showNotification(
        'Please set a valid download path in Settings before downloading files.',
        'error',
        8000
      );
      files.update(f => f.map(file =>
        file.id === downloadingFile.id
          ? { ...file, status: 'failed' }
          : file
      ));
      return;
    }
    
    // Expand ~ to home directory if needed
    if (storagePath.startsWith("~")) {
      const home = await homeDir();
      storagePath = storagePath.replace("~", home);
    }
    
    // Validate directory exists using Tauri command
    const dirExists = await invoke('check_directory_exists', { path: storagePath });
    if (!dirExists) {
      showNotification(
        `Download path "${settings.storagePath}" does not exist. Please update it in Settings.`,
        'error',
        8000
      );
      files.update(f => f.map(file =>
        file.id === downloadingFile.id
          ? { ...file, status: 'failed' }
          : file
      ));
      return;
    }

    // Construct full file path: directory + filename
    const fullPath = `${storagePath}/${downloadingFile.name}`;
    
    console.log('✅ Using settings download path:', fullPath);

    // Now start the actual Bitswap download
    const metadata = {
      fileHash: downloadingFile.hash,
      fileName: downloadingFile.name,
      fileSize: downloadingFile.size,
      seeders: downloadingFile.seederAddresses,
      createdAt: Date.now(),
      isEncrypted: downloadingFile.isEncrypted || false,
      manifest: downloadingFile.manifest ? JSON.stringify(downloadingFile.manifest) : undefined,
      cids: downloadingFile.cids,
      downloadPath: fullPath  // Pass the full path
    }
    
    console.log('  📤 Calling dhtService.downloadFile with metadata:', metadata)
    console.log('  📦 CIDs:', downloadingFile.cids)
    console.log('  👥 Seeders:', downloadingFile.seederAddresses)
    console.log('  💾 Download path:', fullPath)

    // Start the download asynchronously
    dhtService.downloadFile(metadata)
      .then((result) => {
        console.log('  ✅ Bitswap download completed for:', downloadingFile.name, result)
        showNotification(`Successfully downloaded "${downloadingFile.name}"`, 'success')
      })
      .catch((error) => {
        console.error('  ❌ Bitswap download failed:', error)
        const errorMessage = error instanceof Error ? error.message : String(error)

        files.update(f => f.map(file =>
          file.id === downloadingFile.id
            ? { ...file, status: 'failed' }
            : file
        ))

        showNotification(
          `Download failed for "${downloadingFile.name}": ${errorMessage}`,
          'error',
          6000
        )
      })
  } catch (error) {
    console.error('Path validation error:', error);
    files.update(f => f.map(file =>
      file.id === downloadingFile.id
        ? { ...file, status: 'failed' }
        : file
    ))
    showNotification('Failed to validate download path', 'error', 6000);
    return;
  }
} 
    else {
      console.log('  🎬 Simulating download')
      simulateDownloadProgress(downloadingFile.id)
    }
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
        const outputPath = await save(buildSaveDialogOptions(fileToDownload.name));
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

        // PAYMENT PROCESSING: Calculate and deduct payment before download
        const paymentAmount = await paymentService.calculateDownloadCost(fileToDownload.size);
        console.log(`💰 Payment required: ${paymentAmount.toFixed(6)} Chiral for ${fileToDownload.name}`);

        // Check if user has sufficient balance
        if (paymentAmount > 0 && !paymentService.hasSufficientBalance(paymentAmount)) {
          showNotification(
            `Insufficient balance. Need ${paymentAmount.toFixed(4)} Chiral, have ${$wallet.balance.toFixed(4)} Chiral`,
            'error',
            6000
          );
          activeSimulations.delete(fileId);
          files.update(f => f.map(file =>
            file.id === fileId
              ? { ...file, status: 'failed' }
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

            // Ensure the directory exists before writing
            await invoke('ensure_directory_exists', { path: outputPath });
            
            // Write the file data to the output path
            console.log("🔍 DEBUG: About to write file to:", outputPath);
            const { writeFile } = await import('@tauri-apps/plugin-fs');
            await writeFile(outputPath, data_);
            console.log("✅ DEBUG: File written successfully to:", outputPath);

            // Process payment for local download (only if not already paid)
            if (!paidFiles.has(fileToDownload.hash)) {
              const seederPeerId = localPeerIdNow || seeders[0];
              const seederWalletAddress = paymentService.isValidWalletAddress(fileToDownload.seederAddresses?.[0])
                ? fileToDownload.seederAddresses?.[0]!
                : null;

              if (!seederWalletAddress) {
                console.warn('Skipping local copy payment due to missing or invalid uploader wallet address', {
                  file: fileToDownload.name,
                  seederAddresses: fileToDownload.seederAddresses
                });
                showNotification('Payment skipped: missing uploader wallet address', 'warning');
              } else {
                const paymentResult = await paymentService.processDownloadPayment(
                  fileToDownload.hash,
                  fileToDownload.name,
                  fileToDownload.size,
                  seederWalletAddress,
                  seederPeerId
                );

                if (paymentResult.success) {
                  paidFiles.add(fileToDownload.hash); // Mark as paid
                  console.log(`✅ Payment processed: ${paymentAmount.toFixed(6)} Chiral to ${seederWalletAddress} (peer: ${seederPeerId})`);
                  showNotification(
                    `${tr('download.notifications.downloadComplete', { values: { name: fileToDownload.name } })} - Paid ${paymentAmount.toFixed(4)} Chiral`,
                    'success'
                  );
                } else {
                  console.error('Payment failed:', paymentResult.error);
                  showNotification(`Payment failed: ${paymentResult.error}`, 'warning');
                }
              }
            }

            files.update(f => f.map(file => file.id === fileId ? { ...file, status: 'completed', progress: 100, downloadPath: outputPath } : file));
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

        // 3. Process payment for encrypted download (only if not already paid)
        if (!paidFiles.has(fileToDownload.hash)) {
          const seederPeerId = seeders[0];
          const seederWalletAddress = paymentService.isValidWalletAddress(fileToDownload.seederAddresses?.[0])
            ? fileToDownload.seederAddresses?.[0]!
            : null;

          if (!seederWalletAddress) {
            console.warn('Skipping encrypted download payment due to missing or invalid uploader wallet address', {
              file: fileToDownload.name,
              seederAddresses: fileToDownload.seederAddresses
            });
            showNotification('Payment skipped: missing uploader wallet address', 'warning');
          } else {
            const paymentResult = await paymentService.processDownloadPayment(
              fileToDownload.hash,
              fileToDownload.name,
              fileToDownload.size,
              seederWalletAddress,
              seederPeerId
            );

            if (paymentResult.success) {
              paidFiles.add(fileToDownload.hash); // Mark as paid
              console.log(`✅ Payment processed: ${paymentAmount.toFixed(6)} Chiral to ${seederWalletAddress} (peer: ${seederPeerId})`);
            } else {
              console.error('Payment failed:', paymentResult.error);
              showNotification(`Payment failed: ${paymentResult.error}`, 'warning');
            }
          }
        }

        // 4. Mark the download as complete.
        files.update(f => f.map(file =>
          file.id === fileId ? { ...file, status: 'completed', progress: 100, downloadPath: outputPath } : file
        ));
        showNotification(`Successfully decrypted and saved "${fileToDownload.name}"! Paid ${paymentAmount.toFixed(4)} Chiral`, 'success');
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

            // Process payment for multi-source download (only if not already paid)
            if (!paidFiles.has(fileToDownload.hash)) {
              const seederPeerId = seeders[0];
              const seederWalletAddress = paymentService.isValidWalletAddress(fileToDownload.seederAddresses?.[0])
                ? fileToDownload.seederAddresses?.[0]!
                : null;

              if (!seederWalletAddress) {
                console.warn('Skipping multi-source payment due to missing or invalid uploader wallet address', {
                  file: fileToDownload.name,
                  seederAddresses: fileToDownload.seederAddresses
                });
                showNotification('Payment skipped: missing uploader wallet address', 'warning');
              } else {
                const paymentResult = await paymentService.processDownloadPayment(
                  fileToDownload.hash,
                  fileToDownload.name,
                  fileToDownload.size,
                  seederWalletAddress,
                  seederPeerId
                );

                if (paymentResult.success) {
                  paidFiles.add(fileToDownload.hash); // Mark as paid
                  console.log(`✅ Multi-source payment processed: ${paymentAmount.toFixed(6)} Chiral to ${seederWalletAddress} (peer: ${seederPeerId})`);
                  showNotification(`Multi-source download completed! Paid ${paymentAmount.toFixed(4)} Chiral`, 'success');
                } else {
                  console.error('Multi-source payment failed:', paymentResult.error);
                  showNotification(`Payment failed: ${paymentResult.error}`, 'warning');
                }
              }
            }

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
                  // Process payment for P2P download (only if not already paid)
                  if (!paidFiles.has(fileToDownload.hash)) {
                    const seederPeerId = seeders[0];
                    const seederWalletAddress = paymentService.isValidWalletAddress(fileToDownload.seederAddresses?.[0])
                      ? fileToDownload.seederAddresses?.[0]!
                      : null;

                    if (!seederWalletAddress) {
                      console.warn('Skipping P2P payment due to missing or invalid uploader wallet address', {
                        file: fileToDownload.name,
                        seederAddresses: fileToDownload.seederAddresses
                      });
                      showNotification('Payment skipped: missing uploader wallet address', 'warning');
                    } else {
                      const paymentResult = await paymentService.processDownloadPayment(
                        fileToDownload.hash,
                        fileToDownload.name,
                        fileToDownload.size,
                        seederWalletAddress,
                        seederPeerId
                      );

                      if (paymentResult.success) {
                        paidFiles.add(fileToDownload.hash); // Mark as paid
                        console.log(`✅ Payment processed: ${paymentAmount.toFixed(6)} Chiral to ${seederWalletAddress} (peer: ${seederPeerId})`);
                        showNotification(
                          `${tr('download.notifications.downloadComplete', { values: { name: fileToDownload.name } })} - Paid ${paymentAmount.toFixed(4)} Chiral`,
                          'success'
                        );
                      } else {
                        console.error('Payment failed:', paymentResult.error);
                        showNotification(tr('download.notifications.downloadComplete', { values: { name: fileToDownload.name } }), 'success');
                        showNotification(`Payment failed: ${paymentResult.error}`, 'warning');
                      }
                    }
                  }

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
            const errorMessage = error instanceof Error ? error.message : String(error);
            showNotification(`P2P download failed: ${errorMessage}`, 'error');
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
      const errorMessage = error instanceof Error ? error.message : String(error);
      showNotification(`Download failed: ${errorMessage}`, 'error');
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

  // Download History functions
  function exportHistory() {
    const data = downloadHistoryService.exportHistory()
    const blob = new Blob([data], { type: 'application/json' })
    const url = URL.createObjectURL(blob)
    const a = document.createElement('a')
    a.href = url
    a.download = `chiral-download-history-${new Date().toISOString().split('T')[0]}.json`
    document.body.appendChild(a)
    a.click()
    document.body.removeChild(a)
    URL.revokeObjectURL(url)
    showToast(tr('downloadHistory.messages.exportSuccess'), 'success')
  }

  function importHistory() {
    const input = document.createElement('input')
    input.type = 'file'
    input.accept = '.json'
    input.onchange = async (e) => {
      const file = (e.target as HTMLInputElement).files?.[0]
      if (!file) return

      try {
        const text = await file.text()
        const result = downloadHistoryService.importHistory(text)
        
        if (result.success) {
          showToast(tr('downloadHistory.messages.importSuccess', { count: result.imported }), 'success')
          downloadHistory = downloadHistoryService.getFilteredHistory()
        } else {
          showToast(tr('downloadHistory.messages.importError', { error: result.error }), 'error')
        }
      } catch (error) {
        showToast(tr('downloadHistory.messages.importError', { error: error instanceof Error ? error.message : 'Unknown error' }), 'error')
      }
    }
    input.click()
  }

  function clearAllHistory() {
    if (confirm(tr('downloadHistory.confirmClear'))) {
      downloadHistoryService.clearHistory()
      downloadHistory = []
      showToast(tr('downloadHistory.messages.historyCleared'), 'success')
    }
  }

  function clearFailedHistory() {
    if (confirm(tr('downloadHistory.confirmClearFailed'))) {
      downloadHistoryService.clearFailedDownloads()
      downloadHistory = downloadHistoryService.getFilteredHistory()
      showToast(tr('downloadHistory.messages.failedCleared'), 'success')
    }
  }

  function removeHistoryEntry(hash: string) {
    downloadHistoryService.removeFromHistory(hash)
    downloadHistory = downloadHistoryService.getFilteredHistory()
    showToast(tr('downloadHistory.messages.entryRemoved'), 'success')
  }

  async function redownloadFile(entry: DownloadHistoryEntry) {
    showToast(tr('downloadHistory.messages.redownloadStarted', { name: entry.name }), 'info')
    
    // Create metadata object from history entry
    const metadata: FileMetadata = {
      fileHash: entry.hash,
      fileName: entry.name,
      fileSize: entry.size,
      seeders: entry.seederAddresses || [],
      createdAt: Date.now(),
      price: entry.price || 0,
      isEncrypted: entry.encrypted || false,
      manifest: entry.manifest ? JSON.stringify(entry.manifest) : undefined,
      cids: entry.cids || []
    }

    // Add to queue
    await addToDownloadQueue(metadata)
  }

  const formatFileSize = toHumanReadableSize

</script>

<div class="space-y-6">
  <div>
    <h1 class="text-3xl font-bold">{$t('download.title')}</h1>
    <p class="text-muted-foreground mt-2">{$t('download.subtitle')}</p>
  </div>

  <!-- Combined Download Section (Chiral DHT + BitTorrent) -->
  <Card class="overflow-hidden">
    <!-- Chiral DHT Search Section with integrated BitTorrent -->
    <div class="border-b">
      <DownloadSearchSection
        on:download={(event) => handleSearchDownload(event.detail)}
        on:message={handleSearchMessage}
      />
    </div>
  </Card>

  <!-- BitTorrent Downloads List -->
  {#if torrentDownloads.size > 0}
    <Card class="p-6">
      <h2 class="text-xl font-semibold mb-4">BitTorrent Downloads</h2>
      <div class="space-y-3">
        {#each [...torrentDownloads.values()] as torrent (torrent.info_hash)}
          <div class="p-3 bg-muted/60 rounded-lg">
            <div class="flex items-center justify-between">
              <div>
                <h3 class="font-semibold text-sm">{torrent.name}</h3>
                <p class="text-xs text-muted-foreground truncate">Info Hash: {torrent.info_hash}</p>
              </div>
              <Badge>{torrent.status}</Badge>
            </div>
            {#if torrent.status === 'downloading'}
              <div class="mt-2">
                <Progress value={torrent.progress || 0} class="h-2" />
                <div class="flex justify-between text-xs text-muted-foreground mt-1">
                  <span>{torrent.progress.toFixed(2)}%</span>
                  <span>{torrent.speed}</span>
                  <span>ETA: {torrent.eta}</span>
                  <span>Peers: {torrent.peers}</span>
                </div>
              </div>
            {/if}
            <div class="flex gap-2 mt-2">
                <Button size="sm" variant="outline" on:click={() => invoke('pause_torrent', { infoHash: torrent.info_hash })}>
                    <Pause class="h-3 w-3 mr-1" /> Pause
                </Button>
                <Button size="sm" variant="outline" on:click={() => invoke('resume_torrent', { infoHash: torrent.info_hash })}>
                    <Play class="h-3 w-3 mr-1" /> Resume
                </Button>
                <Button size="sm" variant="destructive" on:click={() => invoke('remove_torrent', { infoHash: torrent.info_hash, deleteFiles: false })}>
                    <X class="h-3 w-3 mr-1" /> Remove
                </Button>
            </div>
          </div>
        {/each}
      </div>
    </Card>
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
                        {#if resumedDownloads.has(file.id)}
                          <Badge class="bg-blue-100 text-blue-800 text-xs px-2 py-0.5">
                            Resumed
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
                        <span class="text-purple-600">Peers: {msProgress.activeSources}</span>
                        <span class="text-purple-600">Chunks: {msProgress.completedChunks}/{msProgress.totalChunks}</span>
                      {/if}
                    {/if}
                  </div>
                  <span class="text-foreground">{(file.progress || 0).toFixed(2)}%</span>
                </div>
                {#if detectedProtocol === 'Bitswap' && file.totalChunks}
                  <div class="w-full bg-border rounded-full h-2 flex overflow-hidden" title={`Chunks: ${file.downloadedChunks?.length || 0} / ${file.totalChunks || '?'}`}>
                    {#if file.totalChunks && file.totalChunks > 0}
                      {@const chunkWidth = 100 / file.totalChunks}
                      {#each Array.from({ length: file.totalChunks }) as _, i}
                        <div
                          class="h-2 {file.downloadedChunks?.includes(i) ? 'bg-green-500' : 'bg-transparent'}"
                          style="width: {chunkWidth}%"
                        ></div>
                      {/each}
                    {/if}
                  </div>
                {:else}
                  <Progress
                    value={file.progress || 0}
                    max={100}
                    class="h-2 bg-border [&>div]:bg-green-500 w-full"
                  />
                {/if}
                {#if multiSourceProgress.has(file.hash)}
                  {@const msProgress = multiSourceProgress.get(file.hash)}
                  {#if msProgress && msProgress.sourceAssignments.length > 0}
                    <div class="mt-2 space-y-1">
                      <div class="text-xs text-muted-foreground">Peer progress:</div>
                      {#each msProgress.sourceAssignments as peerAssignment}
                        <div class="flex items-center gap-2 text-xs">
                          <span class="w-20 truncate">{peerAssignment.source.type === 'p2p' ? peerAssignment.source.p2p.peerId.slice(0, 8) : 'N/A'}...</span>
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

  <!-- Download History Section -->
  <Card class="p-6">
    <div class="flex items-center justify-between mb-4">
      <div class="flex items-center gap-3">
        <History class="h-5 w-5" />
        <h2 class="text-lg font-semibold">{$t('downloadHistory.title')}</h2>
        <Badge variant="secondary">{downloadHistoryService.getStatistics().total}</Badge>
      </div>
      <Button
        size="sm"
        variant="outline"
        on:click={() => showHistory = !showHistory}
      >
        {showHistory ? $t('downloadHistory.hideHistory') : $t('downloadHistory.showHistory')}
        {#if showHistory}
          <ChevronUp class="h-4 w-4 ml-1" />
        {:else}
          <ChevronDown class="h-4 w-4 ml-1" />
        {/if}
      </Button>
    </div>

    {#if showHistory}
      <!-- History Controls -->
      <div class="mb-4 space-y-3">
        <!-- Search and Filter -->
        <div class="flex flex-wrap gap-2">
          <div class="relative flex-1 min-w-[200px]">
            <Search class="absolute left-3 top-1/2 -translate-y-1/2 h-4 w-4 text-muted-foreground" />
            <Input
              type="text"
              bind:value={historySearchQuery}
              placeholder={$t('downloadHistory.search')}
              class="pl-10"
            />
          </div>
          <div class="flex gap-2">
            <Button
              size="sm"
              variant={historyFilter === 'all' ? 'default' : 'outline'}
              on:click={() => historyFilter = 'all'}
            >
              {$t('downloadHistory.filterAll')} ({downloadHistoryService.getStatistics().total})
            </Button>
            <Button
              size="sm"
              variant={historyFilter === 'completed' ? 'default' : 'outline'}
              on:click={() => historyFilter = 'completed'}
            >
              {$t('downloadHistory.filterCompleted')} ({downloadHistoryService.getStatistics().completed})
            </Button>
            <Button
              size="sm"
              variant={historyFilter === 'failed' ? 'default' : 'outline'}
              on:click={() => historyFilter = 'failed'}
            >
              {$t('downloadHistory.filterFailed')} ({downloadHistoryService.getStatistics().failed})
            </Button>
          </div>
        </div>

        <!-- History Actions -->
        <div class="flex flex-wrap gap-2">
          <Button
            size="sm"
            variant="outline"
            on:click={exportHistory}
          >
            <UploadIcon class="h-3 w-3 mr-1" />
            {$t('downloadHistory.exportHistory')}
          </Button>
          <Button
            size="sm"
            variant="outline"
            on:click={importHistory}
          >
            <DownloadIcon class="h-3 w-3 mr-1" />
            {$t('downloadHistory.importHistory')}
          </Button>
          {#if downloadHistoryService.getStatistics().failed > 0}
            <Button
              size="sm"
              variant="outline"
              on:click={clearFailedHistory}
              class="text-orange-600 border-orange-600 hover:bg-orange-50"
            >
              <Trash2 class="h-3 w-3 mr-1" />
              {$t('downloadHistory.clearFailed')}
            </Button>
          {/if}
          {#if downloadHistory.length > 0}
            <Button
              size="sm"
              variant="outline"
              on:click={clearAllHistory}
              class="text-destructive border-destructive hover:bg-destructive/10"
            >
              <Trash2 class="h-3 w-3 mr-1" />
              {$t('downloadHistory.clearHistory')}
            </Button>
          {/if}
        </div>
      </div>

      <!-- History List -->
      {#if downloadHistory.length === 0}
        <div class="text-center py-12 text-muted-foreground">
          <History class="h-12 w-12 mx-auto mb-3 opacity-50" />
          <p class="font-medium">{$t('downloadHistory.empty')}</p>
          <p class="text-sm">{$t('downloadHistory.emptyDescription')}</p>
        </div>
      {:else}
        <div class="space-y-2">
          {#each downloadHistory as entry (entry.id + entry.downloadDate)}
            <div class="flex items-center gap-3 p-3 rounded-lg border bg-card hover:bg-muted/50 transition-colors">
              <!-- File Icon -->
              <div class="flex-shrink-0">
                <svelte:component this={getFileIcon(entry.name)} class="h-5 w-5 text-muted-foreground" />
              </div>

              <!-- File Info -->
              <div class="flex-1 min-w-0">
                <p class="font-medium truncate">{entry.name}</p>
                <p class="text-xs text-muted-foreground">
                  {toHumanReadableSize(entry.size)}
                  {#if entry.price}
                    · {entry.price.toFixed(4)} Chiral
                  {/if}
                  · {new Date(entry.downloadDate).toLocaleString()}
                </p>
              </div>

              <!-- Status Badge -->
              <Badge
                variant={entry.status === 'completed' ? 'default' : entry.status === 'failed' ? 'destructive' : 'secondary'}
              >
                {entry.status}
              </Badge>

              <!-- Actions -->
              <div class="flex gap-1">
                <Button
                  size="sm"
                  variant="ghost"
                  on:click={() => redownloadFile(entry)}
                  title={$t('downloadHistory.redownload')}
                >
                  <RefreshCw class="h-4 w-4" />
                </Button>
                <Button
                  size="sm"
                  variant="ghost"
                  on:click={() => removeHistoryEntry(entry.hash)}
                  title={$t('downloadHistory.remove')}
                  class="text-muted-foreground hover:text-destructive"
                >
                  <X class="h-4 w-4" />
                </Button>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    {/if}
  </Card>
</div>
