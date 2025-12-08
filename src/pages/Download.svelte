<script lang="ts">
  import Button from '$lib/components/ui/button.svelte'
  import Card from '$lib/components/ui/card.svelte'
  import Input from '$lib/components/ui/input.svelte'
  import Label from '$lib/components/ui/label.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import Progress from '$lib/components/ui/progress.svelte'
  import { Search, Pause, Play, X, ChevronUp, ChevronDown, Settings, FolderOpen, File as FileIcon, FileText, FileImage, FileVideo, FileAudio, Archive, Code, FileSpreadsheet, Presentation, History, Download as DownloadIcon, Upload as UploadIcon, Trash2, RefreshCw } from 'lucide-svelte'
  import { files, downloadQueue, activeTransfers, wallet, type FileItem } from '$lib/stores'
  import { dhtService } from '$lib/dht'
  import { paymentService } from '$lib/services/paymentService'
  import DownloadSearchSection from '$lib/components/download/DownloadSearchSection.svelte'
  import ProtocolTestPanel from '$lib/components/ProtocolTestPanel.svelte'
  import type { FileMetadata } from '$lib/dht'
  import { onDestroy, onMount } from 'svelte'
  import { t } from 'svelte-i18n'
  import { get } from 'svelte/store'
  import { toHumanReadableSize } from '$lib/utils'
  import { initDownloadTelemetry, disposeDownloadTelemetry } from '$lib/downloadTelemetry'
  import { MultiSourceDownloadService, type MultiSourceProgress } from '$lib/services/multiSourceDownloadService'
  import { listen } from '@tauri-apps/api/event'
  import PeerSelectionService from '$lib/services/peerSelectionService'
  import { downloadHistoryService, type DownloadHistoryEntry } from '$lib/services/downloadHistoryService'
  import { showToast } from '$lib/toast'
  import { diagnosticLogger, fileLogger, errorLogger } from '$lib/diagnostics/logger'
  import DownloadRestartControls from '$lib/components/download/DownloadRestartControls.svelte'
  // Import transfer events store for centralized transfer state management
  import {
    transferStore,
    activeTransfers as storeActiveTransfers,
    subscribeToTransferEvents,
    unsubscribeFromTransferEvents,
    type Transfer
  } from '$lib/stores/transferEventsStore'
  import { invoke } from '@tauri-apps/api/core'
  import { homeDir, join } from '@tauri-apps/api/path'

  const tr = (k: string, params?: Record<string, any>) => $t(k, params)

 // Auto-detect protocol based on file metadata
  let detectedProtocol: 'WebRTC' | 'Bitswap' | undefined = undefined
  let torrentDownloads = new Map<string, any>();
  onMount(() => {
    // Initialize payment service to load persisted wallet and transactions
    paymentService.initialize();

    initDownloadTelemetry()
    
    console.log('📡 Download page mounted, setting up event listeners...');

    // Subscribe to transfer events from backend (FTP, HTTP, etc.)
    subscribeToTransferEvents().catch(err => {
      console.error('Failed to subscribe to transfer events:', err);
    });

    // Listen for multi-source download events
    const setupEventListeners = async () => {
      console.log('📡 Setting up file_content event listener...');
      // Listen for BitTorrent events
      const unlistenTorrentEvent = await listen('torrent_event', (event) => {
        const payload = event.payload as any;

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
            showToast(`Torrent download complete: ${name}`, 'success');
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
            showToast(`Torrent added: ${name}`, 'info');
        } else if (payload.Removed) {
            const { info_hash } = payload.Removed;
            if (torrentDownloads.has(info_hash)) {
                const name = torrentDownloads.get(info_hash)?.name || 'Unknown';
                torrentDownloads.delete(info_hash);
                torrentDownloads = new Map(torrentDownloads);
                showToast(`Torrent removed: ${name}`, 'warning');
            }
        }
      });

      try {
        const unlistenProgress = await listen('multi_source_progress_update', (event) => {
          const progress = event.payload as MultiSourceProgress

          // Find the corresponding file and update its progress
          // Only update files that are actively downloading, not seeding files with the same hash
          files.update(f => f.map(file => {
            if (file.hash === progress.fileHash && file.status === 'downloading') {
              const percentage = MultiSourceDownloadService.getCompletionPercentage(progress);
              // If download is complete (100%), set status to completed
              const isComplete = percentage >= 100;
              return {
                ...file,
                progress: percentage,
                status: isComplete ? 'completed' as const : 'downloading' as const,
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

          // Update file status to completed - only update files that are actively downloading
          // to avoid overwriting seeding files with the same hash
          files.update(f => f.map(file => {
            if (file.hash === data.file_hash && file.status === 'downloading') {
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
          showToast(`Multi-source download completed: ${data.file_name}`, 'success')
        })

        const unlistenStarted = await listen('multi_source_download_started', (event) => {
          const data = event.payload as any
          showToast(`Multi-source download started with ${data.total_peers} peers`, 'info')
        })

        const unlistenFailed = await listen('multi_source_download_failed', (event) => {
          const data = event.payload as any

          // Update file status to failed - only update files that are actively downloading
          // to avoid overwriting seeding files with the same hash
          files.update(f => f.map(file => {
            if (file.hash === data.file_hash && file.status === 'downloading') {
              return {
                ...file,
                status: 'failed' as const
              };
            }
            return file;
          }));

          multiSourceProgress.delete(data.file_hash)
          multiSourceProgress = multiSourceProgress
          showToast(`Multi-source download failed: ${data.error}`, 'error')
        })

        const unlistenBitswapProgress = await listen('bitswap_chunk_downloaded', (event) => {
          const progress = event.payload as {
                fileHash: string;
                chunkIndex: number;
                totalChunks: number;
                chunkSize: number;
            };

            console.log('📦 Bitswap chunk received:', {
                fileHash: progress.fileHash,
                chunkIndex: progress.chunkIndex,
                totalChunks: progress.totalChunks,
                chunkSize: progress.chunkSize
            });

            // Only update files that are actively downloading, not seeding files with the same hash
            files.update(f => {
                // Log all downloading files to help debug hash matching
                const downloadingFiles = f.filter(file => file.status === 'downloading');
                console.log('📦 Currently downloading files:', downloadingFiles.map(file => ({
                    name: file.name,
                    hash: file.hash,
                    hashMatch: file.hash === progress.fileHash
                })));
                
                return f.map(file => {
                if (file.hash === progress.fileHash && file.status === 'downloading') {
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
                    // If download is complete (100% progress), set status to completed
                    const isComplete = percentage >= 100;

                    return {
                        ...file,
                        progress: percentage,
                        status: isComplete ? 'completed' as const : 'downloading' as const,
                        downloadedChunks: Array.from(downloadedChunks),
                        totalChunks: progress.totalChunks,
                        downloadStartTime: bitswapStartTime,
                        speed: speed,
                        eta: eta,
                    };
                }
                return file;
            });
            });
        });

        const unlistenDownloadCompleted = await listen('file_content', async (event) => {
            const metadata = event.payload as any;
            console.log('🎉 file_content event received:', {
              merkleRoot: metadata.merkleRoot,
              fileName: metadata.file_name
            });
            
            diagnosticLogger.info('Download', 'Received file_content event', {
                merkleRoot: metadata.merkleRoot,
                downloadPath: metadata.downloadPath,
                fileName: metadata.file_name
            });

            // Find the file that just completed
            const completedFile = $files.find(f => f.hash === metadata.merkleRoot);
            
            console.log('🔍 File download completed:', {
              merkleRoot: metadata.merkleRoot,
              completedFile: completedFile ? {
                name: completedFile.name,
                hash: completedFile.hash,
                uploaderAddress: completedFile.uploaderAddress,
                seederAddresses: completedFile.seederAddresses,
                size: completedFile.size
              } : null,
              alreadyPaid: paidFiles.has(metadata.merkleRoot),
              filesInStore: $files.length,
              allFileHashes: $files.map(f => ({ name: f.name, hash: f.hash?.slice(0, 16) }))
            });

            if (completedFile && !paidFiles.has(completedFile.hash)) {
                // Process payment for Bitswap download (only once per file)
                diagnosticLogger.info('Download', 'Bitswap download completed, processing payment', { 
                  fileName: completedFile.name,
                  uploaderAddress: completedFile.uploaderAddress,
                  seederAddresses: completedFile.seederAddresses,
                  fileHash: completedFile.hash
                });
                const paymentAmount = await paymentService.calculateDownloadCost(completedFile.size);
                
                // Payment is always required (minimum 0.0001 Chiral enforced by paymentService)

                // Get seeder information from file metadata
                const seederPeerId = completedFile.seederAddresses?.[0];
                const seederWalletAddress = completedFile.uploaderAddress || 
                                             (paymentService.isValidWalletAddress(completedFile.seederAddresses?.[0])
                                               ? completedFile.seederAddresses?.[0]!
                                               : null);
                
                console.log('💰 Payment Check:', {
                  uploaderAddress: completedFile.uploaderAddress,
                  seederAddresses: completedFile.seederAddresses,
                  seederWalletAddress,
                  isValid: !!seederWalletAddress
                });
                
                if (!seederWalletAddress) {
                  diagnosticLogger.warn('Download', 'Skipping Bitswap payment due to missing or invalid uploader wallet address', {
                      file: completedFile.name,
                      seederAddresses: completedFile.seederAddresses,
                      uploaderAddress: completedFile.uploaderAddress
                  });
                  showToast('Payment skipped: missing uploader wallet address', 'warning');
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
                            
                            // Update reputation for the seeder peer after successful payment
                            if (seederPeerId) {
                              try {
                                await invoke('record_transfer_success', {
                                  peerId: seederPeerId,
                                  bytes: completedFile.size,
                                  durationMs: 0, // Bitswap doesn't track duration here
                                });
                                // Also update frontend reputation store for immediate UI feedback
                                PeerSelectionService.notePeerSuccess(seederPeerId);
                                console.log(`✅ Updated reputation for seeder peer ${seederPeerId.substring(0, 20)}... after Bitswap download (+${completedFile.size} bytes)`);
                              } catch (repError) {
                                console.error('Failed to update seeder reputation:', repError);
                              }
                            }
                            
                            diagnosticLogger.info('Download', 'Bitswap payment processed', { 
                              amount: paymentAmount.toFixed(6), 
                              seederWalletAddress, 
                              seederPeerId 
                            });
                            showToast(
                                `Download complete! Paid ${paymentAmount.toFixed(4)} Chiral`,
                                'success'
                            );
                        } else {
                            errorLogger.fileOperationError('Bitswap payment', paymentResult.error || 'Unknown error');
                            showToast(`Payment failed: ${paymentResult.error}`, 'warning');
                        }
                    } catch (error) {
                        errorLogger.fileOperationError('Bitswap payment processing', error instanceof Error ? error.message : String(error));
                        showToast(`Payment failed: ${error instanceof Error ? error.message : 'Unknown error'}`, 'warning');
                    }
                }
            }

            // Update file status - update files that are actively downloading OR might be stuck
            // Allow completion for downloading, paused, or queued files to handle edge cases
            // Also update completed files that don't have downloadPath set yet
            files.update(f => f.map(file => {
                const hashMatches = file.hash === metadata.merkleRoot;
                const isActiveDownload = ['downloading', 'paused', 'queued'].includes(file.status);
                const isStuck = file.status === 'downloading' && file.progress === 0;
                const isCompletedWithoutPath = file.status === 'completed' && !file.downloadPath;

                diagnosticLogger.info('Download', 'Checking file for completion update', {
                    fileName: file.name,
                    fileHash: file.hash,
                    fileStatus: file.status,
                    fileProgress: file.progress,
                    hashMatches,
                    isActiveDownload,
                    isStuck,
                    isCompletedWithoutPath,
                    willUpdate: hashMatches && (isActiveDownload || isStuck || isCompletedWithoutPath)
                });

                if (file.hash === metadata.merkleRoot && (isActiveDownload || isStuck || isCompletedWithoutPath)) {
                    const reason = isStuck ? 'stuck download recovery' :
                                   isCompletedWithoutPath ? 'setting download path for completed file' :
                                   'normal completion';
                    diagnosticLogger.info('Download', 'Updating file', {
                        fileName: file.name,
                        downloadPath: metadata.downloadPath,
                        reason
                    });
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
            errorLogger.dhtInitError(errorMsg);

            // Try to match error to a download in progress
            if (errorMsg.includes('No root CID found')) {
              // Find downloading files and mark them as failed
              files.update(f => f.map(file => {
                if (file.status === 'downloading' && (!file.cids || file.cids.length === 0)) {
                  showToast(
                    `Download failed for "${file.name}": ${errorMsg}`,
                    'error'
                  )
                  return { ...file, status: 'failed' as const }
                }
                return file
              }))
            }
          }
        });


        // Listen for WebRTC download progress
        const unlistenWebRTCProgress = await listen('webrtc_download_progress', (event) => {
          const data = event.payload as {
            fileHash: string;
            progress: number;
            chunksReceived: number;
            totalChunks: number;
            bytesReceived: number;
            totalBytes: number;
          };

          // Update file progress (FileItem uses 'hash' property)
          // Only update files that are actively downloading, not seeding files with the same hash
          files.update(f => f.map(file =>
            file.hash === data.fileHash && file.status === 'downloading'
              ? { ...file, status: data.progress >= 100 ? 'completed' : 'downloading', progress: data.progress }
              : file
          ));
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
      showToast(
        'Please configure a download path in Settings before downloading files.',
        'error'
      );
      return;
    }

    // Get canonical download directory from backend (single source of truth)
    let storagePath: string;
    try {
      storagePath = await invoke('get_download_directory');
    } catch (error) {
      showToast(
        'Failed to resolve download directory. Please check your settings.',
        'error'
      );
      // Only update files that are actively downloading, not seeding files with the same hash
      files.update(f => f.map(file =>
        file.hash === data.fileHash && file.status === 'downloading'
          ? { ...file, status: 'failed' }
          : file
      ));
      return;
    }
    
    // Ensure directory exists (create it if it doesn't)
    try {
      await invoke('ensure_directory_exists', { path: storagePath });
    } catch (error) {
      showToast(
        `Failed to create download directory: ${error instanceof Error ? error.message : String(error)}`,
        'error'
      );
      return;
    }

    // Construct full file path
    const { join } = await import('@tauri-apps/api/path');
    const outputPath = await join(storagePath, data.fileName);
    
    fileLogger.downloadStarted(data.fileName);

    // Write the file to disk
    const { writeFile } = await import('@tauri-apps/plugin-fs');
    const fileData = new Uint8Array(data.data);
    await writeFile(outputPath, fileData);

    fileLogger.downloadCompleted(data.fileName);

    // Update status to completed
    // Only update files that are actively downloading, not seeding files with the same hash
    files.update(f => f.map(file => 
      file.hash === data.fileHash && file.status === 'downloading'
        ? { ...file, status: 'completed', progress: 100, downloadPath: outputPath }
        : file
    ));

    // Process payment for WebRTC download (only once per file)
    const completedFile = $files.find(f => f.hash === data.fileHash);
    
    if (completedFile && !paidFiles.has(completedFile.hash)) {
      diagnosticLogger.info('Download', 'WebRTC download completed, processing payment', { fileName: completedFile.name });
      const paymentAmount = await paymentService.calculateDownloadCost(completedFile.size);
      
      // Get seeder information from file metadata
      const seederPeerId = completedFile.seederAddresses?.[0];
      const seederWalletAddress = completedFile.uploaderAddress || 
                                   (paymentService.isValidWalletAddress(completedFile.seederAddresses?.[0])
                                     ? completedFile.seederAddresses?.[0]!
                                     : null);
      
      if (!seederWalletAddress) {
        diagnosticLogger.warn('Download', 'Skipping WebRTC payment due to missing or invalid uploader wallet address', {
          file: completedFile.name,
          seederAddresses: completedFile.seederAddresses,
          uploaderAddress: completedFile.uploaderAddress
        });
        showToast('Payment skipped: missing uploader wallet address', 'warning');
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
            
            // Update reputation for the seeder peer after successful payment
            if (seederPeerId) {
              try {
                await invoke('record_transfer_success', {
                  peerId: seederPeerId,
                  bytes: completedFile.size,
                  durationMs: 0, // WebRTC doesn't track duration here
                });
                // Also update frontend reputation store for immediate UI feedback
                PeerSelectionService.notePeerSuccess(seederPeerId);
                console.log(`✅ Updated reputation for seeder peer ${seederPeerId.substring(0, 20)}... after WebRTC download (+${completedFile.size} bytes)`);
              } catch (repError) {
                console.error('Failed to update seeder reputation:', repError);
              }
            }
            
            diagnosticLogger.info('Download', 'WebRTC payment processed', { 
              amount: paymentAmount.toFixed(6), 
              seederWalletAddress, 
              seederPeerId 
            });
            showToast(
              `Download complete! Paid ${paymentAmount.toFixed(4)} Chiral`,
              'success'
            );
          } else {
            errorLogger.fileOperationError('WebRTC payment', paymentResult.error || 'Unknown error');
            showToast(`Payment failed: ${paymentResult.error}`, 'warning');
          }
        } catch (error) {
          errorLogger.fileOperationError('WebRTC payment processing', error instanceof Error ? error.message : String(error));
          showToast(`Payment failed: ${error instanceof Error ? error.message : 'Unknown error'}`, 'warning');
        }
      }
    } else if (completedFile) {
      // File already paid for or free - still update reputation for successful transfer
      const seederPeerId = completedFile.seederAddresses?.[0];
      if (seederPeerId) {
        try {
          await invoke('record_transfer_success', {
            peerId: seederPeerId,
            bytes: completedFile.size,
            durationMs: 0,
          });
          // Also update frontend reputation store for immediate UI feedback
          PeerSelectionService.notePeerSuccess(seederPeerId);
          console.log(`✅ Updated reputation for seeder peer ${seederPeerId.substring(0, 20)}... after WebRTC download (already paid)`);
        } catch (repError) {
          console.error('Failed to update seeder reputation:', repError);
        }
      }
      showToast(`Successfully saved "${data.fileName}"`, 'success');
    } else {
      showToast(`Successfully saved "${data.fileName}"`, 'success');
    }
    
  } catch (error) {
    errorLogger.fileOperationError('Save WebRTC file', error instanceof Error ? error.message : String(error));
    const errorMessage = error instanceof Error ? error.message : String(error);
    showToast(`Failed to save file: ${errorMessage}`, 'error');

    // Only update files that are actively downloading, not seeding files with the same hash
    files.update(f => f.map(file =>
      file.hash === data.fileHash && file.status === 'downloading'
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
          unlistenWebRTCProgress()
          unlistenWebRTCComplete()
          unlistenTorrentEvent()
        }
      } catch (error) {
        errorLogger.fileOperationError('Setup event listeners', error instanceof Error ? error.message : String(error));
        return () => {} // Return empty cleanup function
      }
    }

    setupEventListeners()

    // Smart Resume: Load and auto-resume interrupted downloads
    loadAndResumeDownloads()
  })

  onDestroy(() => {
    disposeDownloadTelemetry()
    // Unsubscribe from transfer events
    unsubscribeFromTransferEvents()
  })

  // Load saved download page settings
  const loadDownloadSettings = () => {
    try {
      const saved = localStorage.getItem('downloadPageSettings')
      if (saved) {
        return JSON.parse(saved)
      }
    } catch (error) {
      console.error('Failed to load download settings:', error)
    }
    return {
      autoStartQueue: true,
      maxConcurrentDownloads: 3,
      autoClearCompleted: false,
      filterStatus: 'all',
      multiSourceEnabled: true,
      maxPeersPerDownload: 3
    }
  }

  const savedSettings = loadDownloadSettings()

  let searchFilter = ''  // For searching existing downloads
  let maxConcurrentDownloads: string | number = savedSettings.maxConcurrentDownloads
  let lastValidMaxConcurrent = 3 // Store the last valid value
  let autoStartQueue = savedSettings.autoStartQueue
  let autoClearCompleted = savedSettings.autoClearCompleted
  let filterStatus = savedSettings.filterStatus
  let activeSimulations = new Set<string>() // Track files with active progress simulations

  // Multi-source download state
  let multiSourceProgress = new Map<string, MultiSourceProgress>()
  let multiSourceEnabled = savedSettings.multiSourceEnabled
  let maxPeersPerDownload = savedSettings.maxPeersPerDownload

  // Sync transfer events store with local state
  // This reactive block updates the multiSourceProgress map when transfer events come in
  $: {
    // Update multiSourceProgress from transferEventsStore for active transfers
    for (const transfer of $storeActiveTransfers) {
      if (!multiSourceProgress.has(transfer.fileHash)) {
        // Create a compatible MultiSourceProgress object from the transfer event
        const progress: MultiSourceProgress = {
          fileHash: transfer.fileHash,
          fileName: transfer.fileName,
          totalSize: transfer.fileSize,
          downloadedSize: transfer.downloadedBytes,
          totalChunks: transfer.totalChunks,
          completedChunks: transfer.completedChunks,
          activeSources: transfer.activeSources,
          downloadSpeedBps: transfer.downloadSpeedBps,
          etaSeconds: transfer.etaSeconds,
          sourceAssignments: []
        };
        multiSourceProgress.set(transfer.fileHash, progress);
        multiSourceProgress = multiSourceProgress; // Trigger reactivity
      } else {
        // Update existing progress
        const existing = multiSourceProgress.get(transfer.fileHash);
        if (existing) {
          existing.downloadedSize = transfer.downloadedBytes;
          existing.completedChunks = transfer.completedChunks;
          existing.downloadSpeedBps = transfer.downloadSpeedBps;
          existing.etaSeconds = transfer.etaSeconds;
          existing.activeSources = transfer.activeSources;
          multiSourceProgress = multiSourceProgress; // Trigger reactivity
        }
      }
    }

    // Log transfer events store activity for debugging
    if ($transferStore.lastEventTimestamp > 0 && import.meta.env.DEV) {
      console.log('📦 Transfer store update:', {
        active: $transferStore.activeCount,
        queued: $transferStore.queuedCount,
        completed: $transferStore.completedCount,
        failed: $transferStore.failedCount,
        totalDownloadSpeed: MultiSourceDownloadService.formatSpeed($transferStore.totalDownloadSpeed)
      });
    }
  }

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
  let statistics = downloadHistoryService.getStatistics()

  // Load history on mount
  $: downloadHistory = downloadHistoryService.getFilteredHistory(
    historyFilter === 'all' ? undefined : historyFilter,
    historySearchQuery
  )

  $: if (downloadHistory) {
    statistics = downloadHistoryService.getStatistics()
  }


  // Track files to add to history when they complete/fail
  $: {
    for (const file of $files) {
      if (['completed', 'failed', 'canceled'].includes(file.status)) {
        downloadHistoryService.addToHistory(file)
        //Refeshing history to keep it most updated
        downloadHistory = downloadHistoryService.getFilteredHistory(
          historyFilter === 'all' ? undefined : historyFilter,
          historySearchQuery
      )
      }
    }
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
  //       showToast(`Successfully saved "${fileName}"`, 'success');
  //     }
  //   } catch (error) {
  //     showToast(`Error saving "${fileName}"`, 'error');
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
      errorLogger.fileOperationError('Save download state', error instanceof Error ? error.message : String(error));
    }
  }

  // Smart Resume: Load and resume interrupted downloads
async function loadAndResumeDownloads() {
  try {
    // Check if we've already restored in this session
    if (sessionStorage.getItem('downloadsRestored') === 'true') {
      return
    }

    const saved = localStorage.getItem('pendingDownloads')
    if (!saved) {
      sessionStorage.setItem('downloadsRestored', 'true')
      return
    }

    const { active, queued, timestamp } = JSON.parse(saved)

    // Only auto-resume if less than 24 hours old
    const hoursSinceLastSave = (Date.now() - timestamp) / (1000 * 60 * 60)
    if (hoursSinceLastSave > 24) {
      localStorage.removeItem('pendingDownloads')
      sessionStorage.setItem('downloadsRestored', 'true')
      return
    }

    let resumeCount = 0

    // Restore queued downloads
    if (queued && queued.length > 0) {
      downloadQueue.set(queued)
      resumeCount += queued.length
    }

    // Restore active downloads - dedupe by id/hash/name+size before adding
    if (active && active.length > 0) {
      const restoredFiles = active.map((file: any) => ({
        ...file,
        status: 'paused' as const,
        speed: '0 B/s',
        eta: 'N/A'
      }))

      let addedRestored: typeof restoredFiles = []

      files.update(existing => {
        const existingKeys = new Set(
          existing.map(file => file.id ?? file.hash ?? `${file.name}-${file.size}`)
        )

        const deduped = restoredFiles.filter((file: any) => {
          const key = file.id ?? file.hash ?? `${file.name}-${file.size}`
          if (existingKeys.has(key)) {
            return false
          }
          existingKeys.add(key)
          return true
        })

        addedRestored = deduped
        return deduped.length > 0 ? [...existing, ...deduped] : existing
      })

      if (addedRestored.length > 0) {
        addedRestored.forEach((file: any) => {
          if (file.id) {
            resumedDownloads.add(file.id)
          } else if (file.hash) {
            resumedDownloads.add(file.hash)
          }
        })
        resumeCount += addedRestored.length
      }
    }

    if (resumeCount > 0) {
      const message = resumeCount === 1
        ? `Restored 1 interrupted download. Resume it from the Downloads page.`
        : `Restored ${resumeCount} interrupted downloads. Resume them from the Downloads page.`
      showToast(message, 'info')
    }

    localStorage.removeItem('pendingDownloads')
    sessionStorage.setItem('downloadsRestored', 'true')
  } catch (error) {
    errorLogger.fileOperationError('Load download state', error instanceof Error ? error.message : String(error));
    localStorage.removeItem('pendingDownloads')
    sessionStorage.setItem('downloadsRestored', 'true')
  }
}


  function handleSearchMessage(event: CustomEvent<{ message: string; type?: 'success' | 'error' | 'info' | 'warning'; duration?: number }>) {
    const { message, type = 'info' } = event.detail
    showToast(message, type)
  }

  async function handleSearchDownload(metadata: FileMetadata & { selectedProtocol?: string }) {
    console.log('📥 handleSearchDownload called:', {
      fileName: metadata.fileName,
      selectedProtocol: metadata.selectedProtocol,
      seeders: metadata.seeders?.length,
      cids: metadata.cids?.length
    });

    // Use user's protocol selection if provided, otherwise auto-detect
    if (metadata.selectedProtocol) {
      detectedProtocol = metadata.selectedProtocol === 'webrtc' ? 'WebRTC' : 'Bitswap';
      console.log('📥 Protocol explicitly set:', detectedProtocol);
    } else {
      // Auto-detect protocol based on file metadata
      // BitSwap files have CIDs, WebRTC files have seeders but NO CIDs
      const hasCids = metadata.cids && metadata.cids.length > 0
      const hasSeeders = metadata.seeders && metadata.seeders.length > 0
      // WebRTC is only valid if uploaded via WebRTC (seeders exist but no CIDs)
      const isWebRTCUpload = hasSeeders && !hasCids && !metadata.infoHash && 
                            !metadata.httpSources?.length && !metadata.ftpSources?.length && !metadata.ed2kSources?.length
      detectedProtocol = isWebRTCUpload ? 'WebRTC' : (hasCids ? 'Bitswap' : undefined)
    }

    // Check both download queue and files store for duplicates
    // This ensures we detect if user tries to download a file they're already seeding
    const allFiles = [...$downloadQueue, ...$files]
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
        case 'canceled':
        case 'seeding':
        case 'uploaded':
          // Don't show warning for these statuses - user can re-download failed/canceled files
          // or intentionally download their own seeding files
          break
        default:
          statusMessage = tr('download.search.queue.status.other', { values: { status: existingFile.status } })
      }

      // Show warning toast only for active statuses (completed, downloading, paused, queued)
      if (statusMessage) {
        showToast(statusMessage, 'warning')
      }

      // Allow downloading if status is failed, canceled, or seeding (user may want to re-download/test)
      if (existingFile.status !== 'failed' && existingFile.status !== 'canceled' && existingFile.status !== 'seeding') {
        return
      }
    }

    // If no valid P2P protocol detected, check for other protocols (HTTP, FTP, etc.)
    // handled by DownloadSearchSection, but for direct queue additions we need to check
    if (!detectedProtocol && !metadata.httpSources?.length && !metadata.ftpSources?.length && 
        !metadata.ed2kSources?.length && !metadata.infoHash) {
      showToast(`Cannot download "${metadata.fileName}": No valid download protocol available for this file.`, 'error');
      return;
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
      cids: metadata.cids, // IMPORTANT: Pass CIDs for Bitswap downloads
      protocol: detectedProtocol, // Store the selected protocol with the file
      uploaderAddress: metadata.uploaderAddress // Wallet address for payment
    }

    downloadQueue.update((queue) => [...queue, newFile])
    showToast(tr('download.search.status.addedToQueue', { values: { name: metadata.fileName } }), 'success')

    if (autoStartQueue) {
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

  // Helper function to convert Transfer (from transferStore) to FileItem format
  function transferToFileItem(transfer: Transfer): FileItem {
    // Map transfer status to FileItem status
    const statusMap: Record<string, FileItem['status']> = {
      'queued': 'queued',
      'starting': 'downloading',
      'downloading': 'downloading',
      'paused': 'paused',
      'completed': 'completed',
      'failed': 'failed',
      'canceled': 'canceled'
    }

    return {
      id: transfer.transferId,
      name: transfer.fileName,
      hash: transfer.fileHash,
      size: transfer.fileSize,
      status: statusMap[transfer.status] || 'downloading',
      progress: transfer.progressPercentage,
      downloadPath: transfer.outputPath,
      speed: transfer.downloadSpeedBps > 0 
        ? `${(transfer.downloadSpeedBps / 1024).toFixed(1)} KB/s` 
        : undefined,
      eta: transfer.etaSeconds 
        ? `${Math.round(transfer.etaSeconds)}s` 
        : undefined,
      downloadedChunks: Array.from({ length: transfer.completedChunks }, (_, i) => i),
      totalChunks: transfer.totalChunks,
      price: 0, // FTP downloads are free
      protocol: 'FTP' as const
    }
  }

  // Combine all files and queue into single list with stable sorting
  $: allDownloads = (() => {
    // Get transfers from the transferStore and convert to FileItem format
    const transferFileItems: FileItem[] = Array.from($transferStore.transfers.values())
      .map(transferToFileItem)

    // Filter out any transfers that already exist in $files or $downloadQueue (by id)
    const existingIds = new Set([...$files.map(f => f.id), ...$downloadQueue.map(f => f.id)])
    const uniqueTransfers = transferFileItems.filter(t => !existingIds.has(t.id))

    const combined = [...$files, ...$downloadQueue, ...uniqueTransfers]

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
  // NOTE: Bitswap and WebRTC downloads are handled by the backend via events,
  // so we don't need to call simulateDownloadProgress for those protocols.
  // This reactive block is only for legacy/fallback P2P downloads.
  $: if ($files.length > 0) {
    $files.forEach(file => {
      if (file.status === 'downloading' && !activeSimulations.has(file.id)) {
        // Skip if file is using Bitswap or WebRTC - those are handled by backend events
        const fileProtocol = (file as any).protocol;
        if (fileProtocol === 'Bitswap' || fileProtocol === 'WebRTC') {
          return; // Backend handles these via events
        }
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

  // Persist download page settings
  $: {
    const settings = {
      autoStartQueue,
      maxConcurrentDownloads: typeof maxConcurrentDownloads === 'number' ? maxConcurrentDownloads : parseInt(maxConcurrentDownloads as string) || 3,
      autoClearCompleted,
      filterStatus,
      multiSourceEnabled,
      maxPeersPerDownload
    }
    localStorage.setItem('downloadPageSettings', JSON.stringify(settings))
  }

  // Smart Resume: Auto-save download state when files or queue changes
  $: if ($files || $downloadQueue) {
    saveDownloadState()
  }

  // New function to download from search results
  async function processQueue() {
    // Only prevent starting new downloads if we've reached the max concurrent limit
    const activeDownloads = $files.filter(f => f.status === 'downloading').length
    // Handle case where maxConcurrentDownloads might be empty during typing
    const maxConcurrent = Math.max(1, Number(maxConcurrentDownloads) || 3)
    if (activeDownloads >= maxConcurrent) {
      return
    }

    const nextFile = $downloadQueue[0]
    if (!nextFile) {
      return
    }
    
    console.log('📦 processQueue: Processing file:', {
      name: nextFile.name,
      protocol: nextFile.protocol,
      hash: nextFile.hash?.slice(0, 12)
    });
    
    downloadQueue.update(q => q.filter(f => f.id !== nextFile.id))
    const downloadingFile = {
      ...nextFile,
      hash: nextFile.hash, // Use hash property from FileItem
      status: 'downloading' as const,
      progress: 0,
      speed: '0 B/s', // Ensure speed property exists
      eta: 'N/A',     // Ensure eta property exists
      downloadStartTime: Date.now(), // Track start time for speed calculation
      downloadedChunks: [], // Track downloaded chunks for Bitswap
      totalChunks: 0 // Will be set when first chunk arrives
    }
    files.update(f => [...f, downloadingFile])

    // Use the protocol stored with the file, or fall back to global detectedProtocol
    const fileProtocol = downloadingFile.protocol || detectedProtocol;
    console.log('📦 processQueue: Resolved protocol:', fileProtocol, 'detectedProtocol:', detectedProtocol);

    // Validate protocol before attempting P2P download
    if (!fileProtocol) {
      errorLogger.fileOperationError('Download', 'No valid P2P protocol for this file');
      files.update(f => f.map(file =>
        file.id === downloadingFile.id
          ? { ...file, status: 'failed' }
          : file
      ));
      showToast(
        `Cannot download "${downloadingFile.name}": No valid download protocol. File may have been uploaded via a different protocol.`,
        'error'
      );
      return;
    }

    if (fileProtocol === "Bitswap") {

  // CRITICAL: Bitswap requires CIDs to download
  if (!downloadingFile.cids || downloadingFile.cids.length === 0) {
    errorLogger.fileOperationError('Bitswap download', 'No CIDs found for Bitswap download');
    files.update(f => f.map(file =>
      file.id === downloadingFile.id
        ? { ...file, status: 'failed' }
        : file
    ))
    showToast(
      `Cannot download "${downloadingFile.name}": File metadata is missing CIDs required for Bitswap download.`,
      'error'
    )
    return
  }

  // Verify seeders are available
  if (!downloadingFile.seederAddresses || downloadingFile.seederAddresses.length === 0) {
    errorLogger.fileOperationError('Download', 'No seeders found for download');
    files.update(f => f.map(file =>
      file.id === downloadingFile.id
        ? { ...file, status: 'failed' }
        : file
    ))
    showToast(
      `Cannot download "${downloadingFile.name}": No seeders are currently online for this file.`,
      'error'
    )
    return
  }

  try {
    const stored = localStorage.getItem("chiralSettings");
    if (!stored) {
      showToast(
        'Please configure a download path in Settings before downloading files.',
        'error'
      );
      files.update(f => f.map(file =>
        file.id === downloadingFile.id
          ? { ...file, status: 'failed' }
          : file
      ));
      return;
    }
    
    // Get canonical download directory from backend (single source of truth)
    let storagePath: string;
    try {
      storagePath = await invoke('get_download_directory');
    } catch (error) {
      showToast(
        'Failed to resolve download directory. Please check your settings.',
        'error'
      );
      files.update(f => f.map(file =>
        file.id === downloadingFile.id
          ? { ...file, status: 'failed' }
          : file
      ));
      return;
    }
    
    // Ensure directory exists (create it if it doesn't)
    try {
      await invoke('ensure_directory_exists', { path: storagePath });
    } catch (error) {
      showToast(
        `Failed to create download directory: ${error instanceof Error ? error.message : String(error)}`,
        'error'
      );
      files.update(f => f.map(file =>
        file.id === downloadingFile.id
          ? { ...file, status: 'failed' }
          : file
      ));
      return;
    }

    // Construct full file path: directory + filename
    const fullPath = await join(storagePath, downloadingFile.name);

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
      downloadPath: fullPath,  // Pass the full path
      price: downloadingFile.price ?? 0  // Add price field
    }

    console.log('📦 Bitswap download starting:', {
      fileHash: metadata.fileHash,
      fileName: metadata.fileName,
      fileSize: metadata.fileSize,
      cidsCount: metadata.cids?.length,
      seedersCount: metadata.seeders?.length
    });

    // Start the download asynchronously
    dhtService.downloadFile(metadata)
      .then((_result) => {
        showToast(`Successfully downloaded "${downloadingFile.name}"`, 'success')
      })
      .catch((error) => {
        errorLogger.fileOperationError('Bitswap download', error instanceof Error ? error.message : String(error));
        const errorMessage = error instanceof Error ? error.message : String(error)

        files.update(f => f.map(file =>
          file.id === downloadingFile.id
            ? { ...file, status: 'failed' }
            : file
        ))

        showToast(
          `Download failed for "${downloadingFile.name}": ${errorMessage}`,
          'error'
        )
      })
  } catch (error) {
    errorLogger.fileOperationError('Download settings validation', error instanceof Error ? error.message : String(error));
    files.update(f => f.map(file =>
      file.id === downloadingFile.id
        ? { ...file, status: 'failed' }
        : file
    ));
    showToast(
      `Download failed: ${error instanceof Error ? error.message : String(error)}`,
      'error'
    );
    return;
  }
} else if (fileProtocol === "WebRTC") {
    // WebRTC download path - Use backend Rust WebRTC (works in Tauri)
    console.log('🌐 WebRTC download path triggered for:', downloadingFile.name);
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        const { join } = await import('@tauri-apps/api/path');

        // Get canonical download directory from backend (single source of truth)
        let storagePath: string;
        try {
          storagePath = await invoke('get_download_directory');
        } catch (error) {
          showToast(
            'Failed to resolve download directory. Please check your settings.',
            'error'
          );
          files.update(f => f.map(file =>
            file.id === downloadingFile.id
              ? { ...file, status: 'failed' }
              : file
          ));
          return;
        }

        // Ensure directory exists (create it if it doesn't)
        try {
          await invoke('ensure_directory_exists', { path: storagePath });
        } catch (error) {
          showToast(
            `Failed to create download directory: ${error instanceof Error ? error.message : String(error)}`,
            'error'
          );
          files.update(f => f.map(file =>
            file.id === downloadingFile.id
              ? { ...file, status: 'failed' }
              : file
          ));
          return;
        }

        // Construct full file path: directory + filename
        const outputPath = await join(storagePath, downloadingFile.name);

        console.log('🌐 Calling download_file_from_network:', {
          fileHash: downloadingFile.hash,
          outputPath: outputPath
        });

        // Call backend Rust WebRTC via Tauri command
        // This uses the WebRTCService with webrtc-rs crate (works in Tauri)
        const result = await invoke('download_file_from_network', {
          fileHash: downloadingFile.hash,
          outputPath: outputPath
        });

        console.log('🌐 download_file_from_network result:', result);

        // Check if this was a local copy (completed immediately)
        if (typeof result === 'string' && result.includes('local copy')) {
          // Local copy completed - mark as done
          files.update(f => f.map(file =>
            file.id === downloadingFile.id
              ? {
                  ...file,
                  status: 'completed',
                  progress: 100,
                  downloadPath: outputPath
                }
              : file
          ));
          showToast(`Download completed (local copy): "${downloadingFile.name}"`, 'success');
        } else {
          // WebRTC download initiated - update status to downloading
          files.update(f => f.map(file =>
            file.id === downloadingFile.id
              ? {
                  ...file,
                  status: 'downloading',
                  progress: 0,
                  downloadPath: outputPath
                }
              : file
          ));
          showToast(`WebRTC download started for "${downloadingFile.name}"`, 'success');
        }

      } catch (error) {
        errorLogger.fileOperationError('WebRTC download', error instanceof Error ? error.message : String(error));
        files.update(f => f.map(file =>
          file.id === downloadingFile.id
            ? { ...file, status: 'failed' }
            : file
        ));
        showToast(
          `WebRTC download failed: ${error instanceof Error ? error.message : String(error)}`,
          'error'
        );
      }
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

      // Get download path from settings
      try {
        const { join } = await import('@tauri-apps/api/path');

        // Get canonical download directory from backend (single source of truth)
        let storagePath: string;
        try {
          storagePath = await invoke('get_download_directory');
        } catch (error) {
          showToast(
            'Failed to resolve download directory. Please check your settings.',
            'error'
          );
          activeSimulations.delete(fileId);
          files.update(f => f.map(file =>
            file.id === fileId
              ? { ...file, status: 'failed' }
              : file
          ));
          return;
        }

        // Ensure directory exists (create it if it doesn't)
        try {
          await invoke('ensure_directory_exists', { path: storagePath });
        } catch (error) {
          showToast(
            `Failed to create download directory: ${error instanceof Error ? error.message : String(error)}`,
            'error'
          );
          activeSimulations.delete(fileId);
          files.update(f => f.map(file =>
            file.id === fileId
              ? { ...file, status: 'failed' }
              : file
          ));
          return;
        }

        // Construct full file path: directory + filename
        const outputPath = await join(storagePath, fileToDownload.name);

        // PAYMENT PROCESSING: Calculate and deduct payment before download
        const paymentAmount = await paymentService.calculateDownloadCost(fileToDownload.size);
        diagnosticLogger.info('Download', 'Payment required', { 
          fileName: fileToDownload.name, 
          amount: paymentAmount.toFixed(6) 
        });

        // Check if user has sufficient balance
        if (paymentAmount > 0 && !paymentService.hasSufficientBalance(paymentAmount)) {
          showToast(
            `Insufficient balance. Need ${paymentAmount.toFixed(4)} Chiral, have ${$wallet.balance.toFixed(4)} Chiral`,
            'error'
          );
          activeSimulations.delete(fileId);
          files.update(f => f.map(file =>
            file.id === fileId
              ? { ...file, status: 'failed' }
              : file
          ));
          return;
        }

      // Determine seeders for the download
        let seeders = (fileToDownload.seederAddresses || []).slice();

      // Show "automatically started" message now that download is proceeding
      showToast(tr('download.notifications.autostart'), 'info');

       if (fileToDownload.isEncrypted && fileToDownload.manifest) {
        // 1. Download all the required encrypted chunks using the P2P service.
        //    This new function will handle fetching multiple chunks in parallel.
        showToast(`Downloading encrypted chunks for "${fileToDownload.name}"...`, 'info');

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
        showToast(`All chunks received. Decrypting file...`, 'info');
        const { encryptionService } = await import('$lib/services/encryption');
        await encryptionService.decryptFile(fileToDownload.manifest, outputPath);

        // 3. Process payment for encrypted download (only if not already paid)
        if (!paidFiles.has(fileToDownload.hash)) {
          const seederPeerId = seeders[0];
          const seederWalletAddress = paymentService.isValidWalletAddress(fileToDownload.seederAddresses?.[0])
            ? fileToDownload.seederAddresses?.[0]!
            : null;

          if (!seederWalletAddress) {
            diagnosticLogger.warn('Download', 'Skipping encrypted download payment due to missing or invalid uploader wallet address', {
              file: fileToDownload.name,
              seederAddresses: fileToDownload.seederAddresses
            });
            showToast('Payment skipped: missing uploader wallet address', 'warning');
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
              diagnosticLogger.info('Download', 'Payment processed', { 
                amount: paymentAmount.toFixed(6), 
                seederWalletAddress, 
                seederPeerId 
              });
            } else {
              errorLogger.fileOperationError('Payment', paymentResult.error || 'Unknown error');
              showToast(`Payment failed: ${paymentResult.error}`, 'warning');
            }
          }
        }

        // 4. Mark the download as complete.
        files.update(f => f.map(file =>
          file.id === fileId ? { ...file, status: 'completed', progress: 100, downloadPath: outputPath } : file
        ));
        showToast(`Successfully decrypted and saved "${fileToDownload.name}"! Paid ${paymentAmount.toFixed(4)} Chiral`, 'success');
        activeSimulations.delete(fileId);

      } else {
        // Check if we should use multi-source download
        const seeders = fileToDownload.seederAddresses || [];

        if (multiSourceEnabled && seeders.length >= 2 && fileToDownload && fileToDownload.size > 1024 * 1024) {
          // Use multi-source download for files > 1MB with multiple seeders
          const downloadStartTime = Date.now();
          try {
            showToast(`Starting multi-source download from ${seeders.length} peers...`, 'info');

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
                diagnosticLogger.warn('Download', 'Skipping multi-source payment due to missing or invalid uploader wallet address', {
                  file: fileToDownload.name,
                  seederAddresses: fileToDownload.seederAddresses
                });
                showToast('Payment skipped: missing uploader wallet address', 'warning');
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
                  diagnosticLogger.info('Download', 'Multi-source payment processed', { 
                    amount: paymentAmount.toFixed(6), 
                    seederWalletAddress, 
                    seederPeerId 
                  });
                  showToast(`Multi-source download completed! Paid ${paymentAmount.toFixed(4)} Chiral`, 'success');
                } else {
                  errorLogger.fileOperationError('Multi-source payment', paymentResult.error || 'Unknown error');
                  showToast(`Payment failed: ${paymentResult.error}`, 'warning');
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
                errorLogger.networkError(`Failed to record success for peer ${peerId}: ${error instanceof Error ? error.message : String(error)}`);
              }
            }

          } catch (error) {
            errorLogger.fileOperationError('Multi-source download', error instanceof Error ? error.message : String(error));

            // Record transfer failures for each peer
            for (const peerId of seeders) {
              try {
                await PeerSelectionService.recordTransferFailure(
                  peerId,
                  error instanceof Error ? error.message : 'Multi-source download failed'
                );
              } catch (recordError) {
                errorLogger.networkError(`Failed to record failure for peer ${peerId}: ${recordError instanceof Error ? recordError.message : String(recordError)}`);
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
              isEncrypted: false,
              price: fileToDownload.price ?? 0  // Add price field
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
                diagnosticLogger.info('P2P Download', 'Transfer status update', {
                  fileId,
                  transferStatus: transfer.status,
                  progress: transfer.progress,
                  speed: transfer.speed,
                  outputPath: transfer.outputPath
                });

                files.update(f => f.map(file => {
                  if (file.id === fileId) {
                    const newStatus = transfer.status === 'completed' ? 'completed' :
                                    transfer.status === 'failed' ? 'failed' :
                                    transfer.status === 'transferring' ? 'downloading' : file.status;

                    if (newStatus !== file.status) {
                      diagnosticLogger.info('P2P Download', 'Status changed', {
                        fileId,
                        oldStatus: file.status,
                        newStatus,
                        progress: transfer.progress
                      });
                    }

                    return {
                      ...file,
                      progress: transfer.progress,
                      status: newStatus,
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
                      diagnosticLogger.warn('Download', 'Skipping P2P payment due to missing or invalid uploader wallet address', {
                        file: fileToDownload.name,
                        seederAddresses: fileToDownload.seederAddresses
                      });
                      showToast('Payment skipped: missing uploader wallet address', 'warning');
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
                        diagnosticLogger.info('Download', 'Payment processed', { 
                          amount: paymentAmount.toFixed(6), 
                          seederWalletAddress, 
                          seederPeerId 
                        });
                        showToast(
                          `${tr('download.notifications.downloadComplete', { values: { name: fileToDownload.name } })} - Paid ${paymentAmount.toFixed(4)} Chiral`,
                          'success'
                        );
                      } else {
                        errorLogger.fileOperationError('Payment', paymentResult.error || 'Unknown error');
                        showToast(tr('download.notifications.downloadComplete', { values: { name: fileToDownload.name } }), 'success');
                        showToast(`Payment failed: ${paymentResult.error}`, 'warning');
                      }
                    }
                  }

                  // Record success metrics for each peer
                  const duration = Date.now() - p2pStartTime;
                  for (const peerId of seeders) {
                    try {
                      await PeerSelectionService.recordTransferSuccess(peerId, fileToDownload.size, duration);
                    } catch (error) {
                      errorLogger.networkError(`Failed to record P2P success for peer ${peerId}: ${error instanceof Error ? error.message : String(error)}`);
                    }
                  }
                } else if (transfer.status === 'failed' && fileToDownload) {
                  showToast(tr('download.notifications.downloadFailed', { values: { name: fileToDownload.name } }), 'error');

                  // Record failure metrics for each peer
                  for (const peerId of seeders) {
                    try {
                      await PeerSelectionService.recordTransferFailure(peerId, 'P2P download failed');
                    } catch (error) {
                      errorLogger.networkError(`Failed to record P2P failure for peer ${peerId}: ${error instanceof Error ? error.message : String(error)}`);
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
            errorLogger.fileOperationError('P2P download', error instanceof Error ? error.message : String(error));
            const errorMessage = error instanceof Error ? error.message : String(error);
            showToast(`P2P download failed: ${errorMessage}`, 'error');
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
      showToast(`Download failed: ${errorMessage}`, 'error');
      activeSimulations.delete(fileId);

      files.update(f => f.map(file =>
        file.id === fileId
          ? { ...file, status: 'failed' }
          : file
      ));

      const errorMsg = error instanceof Error ? error.message : String(error);
      errorLogger.fileOperationError('Download', error instanceof Error ? error.message : String(error));
      showToast(
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

  async function showInFolder(_fileId: string) {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      // Always open the storage directory set by user in Settings
      const storagePath = await invoke('get_download_directory');
      await invoke('show_in_folder', { path: storagePath });
    } catch (error) {
      errorLogger.fileOperationError('Show storage folder', error instanceof Error ? error.message : String(error));
      showToast('Failed to open storage folder', 'error');
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
    showToast(`Retrying download for "${newFile.name}"`, 'info');
  }

  async function moveInQueue(fileId: string, direction: 'up' | 'down' | 'drop', targetId?: string) {
    downloadQueue.update(queue => {
      const fromIndex = queue.findIndex(f => f.id === fileId);
      if (fromIndex === -1) return queue;

      const newQueue = [...queue];
      const [removed] = newQueue.splice(fromIndex, 1);

      if (direction === 'drop' && targetId) {
        const toIndex = queue.findIndex(f => f.id === targetId);
        if (toIndex !== -1) {
          newQueue.splice(toIndex, 0, removed);
        } else {
          return queue; // Target not found, abort
        }
      } else {
        const newIndex = direction === 'up' ? Math.max(0, fromIndex - 1) : Math.min(queue.length - 1, fromIndex + 1);
        newQueue.splice(newIndex, 0, removed);
      }

      return newQueue;
    })

    // After any reordering, persist the new priority to the backend.
    const newQueue = get(downloadQueue);
    const orderedInfoHashes = newQueue.map(f => f.hash);
    await invoke('update_download_priorities', { orderedInfoHashes });
    showToast('Download queue order updated', 'success');
  }

  // Drag and Drop state
  let draggedItemId: string | null = null;
  let dropTargetId: string | null = null;

  function handleDragStart(event: DragEvent, fileId: string) {
    draggedItemId = fileId;
    event.dataTransfer!.effectAllowed = 'move';
  }

  function handleDragOver(event: DragEvent, fileId:string) {
    event.preventDefault();
    if (fileId !== draggedItemId) {
      dropTargetId = fileId;
    }
  }

  function handleDragLeave() {
    dropTargetId = null;
  }

  async function handleDrop(event: DragEvent, targetFileId: string) {
    event.preventDefault();
    if (!draggedItemId || draggedItemId === targetFileId) return;

    // Reorder the queue and persist the changes
    await moveInQueue(draggedItemId, 'drop', targetFileId);
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

  async function clearAllHistory() {
    if (await confirm(tr('downloadHistory.confirmClear'))) {
      await downloadHistoryService.clearHistory()
      downloadHistory = []
      showToast(tr('downloadHistory.messages.historyCleared'), 'success')
    }
  }

  async function clearFailedHistory() {
    if (await confirm(tr('downloadHistory.confirmClearFailed'))) {
      await downloadHistoryService.clearFailedDownloads()
      downloadHistory = downloadHistoryService.getFilteredHistory()
      showToast(tr('downloadHistory.messages.failedCleared'), 'success')
    }
  }

  async function clearCanceledHistory() {
    if (await confirm(tr('downloadHistory.confirmClearCanceled'))) {
      await downloadHistoryService.clearCanceledDownloads()
      downloadHistory = downloadHistoryService.getFilteredHistory()
      showToast(tr('downloadHistory.messages.canceledCleared'), 'success')
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

  // Restartable HTTP download controls
  let showRestartSection = false
  let restartDownloadId = ''
  let restartSourceUrl = ''
  let restartDestinationPath = ''
  let restartSha256 = ''

  async function chooseRestartDestination() {
    try {
      const defaultDir = await homeDir()
      const suggestedPath =
        restartDestinationPath || `${defaultDir.replace(/\/$/, '')}/Downloads/restart-download.bin`
      const { save } = await import('@tauri-apps/plugin-dialog')
      const selection = await save({
        defaultPath: suggestedPath,
        filters: [
          {
            name: 'All Files',
            extensions: ['*']
          }
        ]
      })
      if (selection) {
        restartDestinationPath = selection
      }
    } catch (error) {
      console.error('Failed to choose destination path', error)
      showToast('Failed to choose destination path', 'error')
    }
  }
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-3xl font-bold">{$t('download.title')}</h1>
    <p class="text-muted-foreground mt-2">{$t('download.subtitle')}</p>
  </div>

  <!-- DEV ONLY: Protocol Test Panel - Remove before production -->
  <!-- File: src/lib/components/ProtocolTestPanel.svelte -->
  <ProtocolTestPanel />

  <!-- Combined Download Section (Chiral DHT + BitTorrent) -->
  <Card class="">
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
      <div class="space-y-3" role="list">
        {#each filteredDownloads as file, index}
          <div
            role="listitem"
            class="p-3 bg-muted/60 rounded-lg hover:bg-muted/80 transition-colors"
            draggable={file.status === 'queued'}
            on:dragstart={(e) => handleDragStart(e, file.id)}
            on:dragover={(e) => handleDragOver(e, file.id)}
            on:dragleave={handleDragLeave}
            on:drop={(e) => handleDrop(e, file.id)}
            class:cursor-move={file.status === 'queued'}
            class:border-primary={dropTargetId === file.id}
            class:border-2={dropTargetId === file.id}
          >
            <!-- File Header -->
            <div class="pb-2">
              <div class="flex items-start justify-between gap-4">
                <div class="flex items-start gap-3 flex-1 min-w-0">
                  {#if file.status === 'queued'}
                    <div class="flex flex-col gap-1 mt-1">
                      <Button
                        size="sm"
                        variant="ghost"
                        on:click={async () => await moveInQueue(file.id, 'up')}
                        disabled={index === 0}
                        class="h-6 w-6 p-0 hover:bg-muted"
                      >
                        <ChevronUp class="h-4 w-4" />
                      </Button>
                      <Button
                        size="sm"
                        variant="ghost"
                        on:click={async () => await moveInQueue(file.id, 'down')}
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

  <!-- Restartable HTTP Download Section -->
  <Card class="p-6">
    <div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
      <div>
        <div class="flex items-center gap-2">
          <DownloadIcon class="h-5 w-5" />
          <h2 class="text-lg font-semibold">Restartable HTTP Download (Beta)</h2>
        </div>
        <p class="text-sm text-muted-foreground mt-1">
          Download any HTTP resource with pause/resume support powered by the restartable engine.
        </p>
      </div>
      <Button size="sm" variant="outline" on:click={() => (showRestartSection = !showRestartSection)}>
        {showRestartSection ? 'Hide Controls' : 'Show Controls'}
      </Button>
    </div>

    {#if showRestartSection}
      <div class="mt-6 space-y-5">
        <div class="grid gap-4 md:grid-cols-2">
          <div class="space-y-2">
            <Label for="restart-url">HTTP Source URL</Label>
            <Input
              id="restart-url"
              type="url"
              placeholder="https://example.com/file.bin"
              bind:value={restartSourceUrl}
            />
          </div>
          <div class="space-y-2">
            <Label for="restart-hash">Expected SHA-256 (optional)</Label>
            <Input
              id="restart-hash"
              placeholder="64-character hex"
              bind:value={restartSha256}
            />
          </div>
          <div class="space-y-2">
            <Label for="restart-id">Download ID (optional)</Label>
            <Input
              id="restart-id"
              placeholder="Leave blank to auto-generate"
              bind:value={restartDownloadId}
            />
          </div>
        </div>
        <div class="space-y-2">
          <Label for="restart-dest">Destination Path</Label>
          <div class="flex flex-col gap-2 md:flex-row">
            <Input
              id="restart-dest"
              placeholder="/home/user/Downloads/file.bin"
              bind:value={restartDestinationPath}
              class="flex-1"
            />
            <Button type="button" variant="outline" on:click={chooseRestartDestination}>
              Choose Path
            </Button>
          </div>
        </div>

        <div class="rounded-md border border-dashed p-4 text-sm text-muted-foreground">
          <p>
            Enter a direct HTTP URL and destination path, then use the controls below to start,
            pause, or resume the transfer. Metadata is stored next to the destination as
            <code>.filename.chiral.meta.json</code> so progress survives restarts.
          </p>
        </div>

        <DownloadRestartControls
          bind:downloadId={restartDownloadId}
          sourceUrl={restartSourceUrl}
          destinationPath={restartDestinationPath}
          expectedSha256={restartSha256 ? restartSha256 : null}
        />
      </div>
    {/if}
  </Card>

  <!-- Download History Section -->
  <Card class="p-6">
    <div class="flex items-center justify-between mb-4">
      <div class="flex items-center gap-3">
        <History class="h-5 w-5" />
        <h2 class="text-lg font-semibold">{$t('downloadHistory.title')}</h2>
        <Badge variant="secondary">{statistics.total}</Badge>
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
              {$t('downloadHistory.filterAll')} ({statistics.total})
            </Button>
            <Button
              size="sm"
              variant={historyFilter === 'completed' ? 'default' : 'outline'}
              on:click={() => historyFilter = 'completed'}
            >
              {$t('downloadHistory.filterCompleted')} ({statistics.completed})
            </Button>
            <Button
              size="sm"
              variant={historyFilter === 'failed' ? 'default' : 'outline'}
              on:click={() => historyFilter = 'failed'}
            >
              {$t('downloadHistory.filterFailed')} ({statistics.failed})
            </Button>
            <Button
              size="sm"
              variant={historyFilter === 'canceled' ? 'default' : 'outline'}
              on:click={() => historyFilter = 'canceled'}
            >
              {$t('downloadHistory.filterCanceled')} ({statistics.canceled})
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
          {#if statistics.failed > 0}
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
          {#if statistics.canceled > 0}
            <Button
              size="sm"
              variant="outline"
              on:click={clearCanceledHistory}
              class="text-orange-600 border-orange-600 hover:bg-orange-50"
            >
              <Trash2 class="h-3 w-3 mr-1" />
              {$t('downloadHistory.clearCanceled')}
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
