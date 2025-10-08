<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import { File as FileIcon, X, Plus, FolderOpen, FileText, Image, Music, Video, Archive, Code, FileSpreadsheet, Upload, Download, RefreshCw, Lock, Key } from 'lucide-svelte'
  import { files, type FileItem, etcAccount } from '$lib/stores'
  import { loadSeedList, saveSeedList, type SeedRecord } from '$lib/services/seedPersistence'
  import { t } from 'svelte-i18n';
  import { get } from 'svelte/store'
  import { onMount, onDestroy } from 'svelte';
  import { showToast } from '$lib/toast'
  import { getStorageStatus } from '$lib/uploadHelpers'
  import { fileService } from '$lib/services/fileService'
  import { open } from "@tauri-apps/plugin-dialog";
  import { invoke } from "@tauri-apps/api/core";
  import { dhtService } from '$lib/dht';
  import Label from '$lib/components/ui/label.svelte';
  import Input from '$lib/components/ui/input.svelte'; 

  const tr = (k: string, params?: Record<string, any>): string => (get(t) as (key: string, params?: any) => string)(k, params)
  // Check if running in Tauri environment
  const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

  // Enhanced file type detection with icons
  function getFileIcon(fileName: string) {
    const ext = fileName.split('.').pop()?.toLowerCase() || ''

    // Images
    if (['jpg', 'jpeg', 'png', 'gif', 'webp', 'svg', 'bmp', 'ico'].includes(ext)) return Image
    // Videos
    if (['mp4', 'avi', 'mkv', 'mov', 'wmv', 'webm', 'flv', 'm4v'].includes(ext)) return Video
    // Audio
    if (['mp3', 'wav', 'flac', 'aac', 'ogg', 'm4a', 'wma'].includes(ext)) return Music
    // Archives
    if (['zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz'].includes(ext)) return Archive
    // Code files
    if (['js', 'ts', 'html', 'css', 'py', 'java', 'cpp', 'c', 'php', 'rb', 'go', 'rs'].includes(ext)) return Code
    // Documents
    if (['txt', 'md', 'pdf', 'doc', 'docx', 'rtf'].includes(ext)) return FileText
    // Spreadsheets
    if (['xls', 'xlsx', 'csv', 'ods'].includes(ext)) return FileSpreadsheet

    return FileIcon
  }

  function getFileColor(fileName: string) {
    const ext = fileName.split('.').pop()?.toLowerCase() || ''

    if (['jpg', 'jpeg', 'png', 'gif', 'webp', 'svg', 'bmp', 'ico'].includes(ext)) return 'text-blue-500'
    if (['mp4', 'avi', 'mkv', 'mov', 'wmv', 'webm', 'flv', 'm4v'].includes(ext)) return 'text-purple-500'
    if (['mp3', 'wav', 'flac', 'aac', 'ogg', 'm4a', 'wma'].includes(ext)) return 'text-green-500'
    if (['zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz'].includes(ext)) return 'text-orange-500'
    if (['js', 'ts', 'html', 'css', 'py', 'java', 'cpp', 'c', 'php', 'rb', 'go', 'rs'].includes(ext)) return 'text-red-500'
    if (['txt', 'md', 'pdf', 'doc', 'docx', 'rtf'].includes(ext)) return 'text-gray-600'
    if (['xls', 'xlsx', 'csv', 'ods'].includes(ext)) return 'text-emerald-500'

    return 'text-muted-foreground'
  }

  let isDragging = false
  const LOW_STORAGE_THRESHOLD = 5
  let availableStorage: number | null = null
  let storageStatus: 'unknown' | 'ok' | 'low' = 'unknown'
  let isRefreshingStorage = false
  let storageError: string | null = null
  let lastChecked: Date | null = null
  let isUploading = false
  
  // Encrypted sharing state
  let useEncryptedSharing = false
  let recipientPublicKey = ''
  let showEncryptionOptions = false

  $: storageLabel = isRefreshingStorage
    ? tr('upload.storage.checking')
    : availableStorage !== null
      ? tr('upload.storage.available', { values: { space: availableStorage.toLocaleString() } })
      : tr('upload.storage.unknown')

  $: storageBadgeClass = storageStatus === 'low'
    ? 'bg-destructive text-destructive-foreground'
    : storageStatus === 'ok'
      ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-300'
      : 'bg-muted text-muted-foreground'

  $: storageBadgeText = storageStatus === 'low'
    ? tr('upload.storage.lowBadge')
    : storageStatus === 'ok'
      ? tr('upload.storage.okBadge')
      : tr('upload.storage.unknownBadge')

  $: lastCheckedLabel = lastChecked
    ? tr('upload.storage.lastChecked', {
        values: {
          time: lastChecked.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })
        }
      })
    : null

  $: showLowStorageDescription = storageStatus === 'low' && !isRefreshingStorage

  async function refreshAvailableStorage() {
    if (isRefreshingStorage) return
    isRefreshingStorage = true
    storageError = null // Clear any previous errors

    const startTime = Date.now()

    try {
      // Add timeout to prevent infinite hanging
      const timeoutPromise = new Promise<never>((_, reject) =>
        setTimeout(() => reject(new Error('Storage check timeout')), 3000)
      );

      const storagePromise = fileService.getAvailableStorage().catch(error => {
        console.warn('Storage service error:', error);
        return null; // Return null on error instead of throwing
      });

      const result = await Promise.race([storagePromise, timeoutPromise]) as number | null;

      storageStatus = getStorageStatus(result, LOW_STORAGE_THRESHOLD)

      if (result === null || result === undefined || !Number.isFinite(result)) {
        storageError = 'Unable to check disk space'
        availableStorage = null
        lastChecked = null
        storageStatus = 'unknown'
      } else {
        availableStorage = Math.max(0, Math.floor(result))
        storageError = null
        lastChecked = new Date()
      }
    } catch (error) {
      console.error('Storage check failed:', error);
      storageError = error instanceof Error && error.message.includes('timeout')
        ? 'Storage check timed out'
        : 'Unable to check disk space'
      availableStorage = null
      lastChecked = null
      storageStatus = 'unknown'
    } finally {
      // Ensure minimum loading time for better UX (at least 600ms)
      const elapsed = Date.now() - startTime
      const minDelay = 600
      if (elapsed < minDelay) {
        await new Promise(resolve => setTimeout(resolve, minDelay - elapsed))
      }
      isRefreshingStorage = false
    }
  }

  onMount(async () => {
    // Make storage refresh non-blocking on startup to prevent UI hanging
    setTimeout(() => refreshAvailableStorage(), 100)

    // Restore persisted seeding list (if any)
    try {
      const persisted: SeedRecord[] = await loadSeedList()
      if (persisted && persisted.length > 0) {
        const existing = get(files)
        const toAdd: FileItem[] = []
        for (const s of persisted) {
          if (!existing.some(f => f.hash === s.hash)) {
            toAdd.push({
              id: s.id,
              name: s.name || s.path.split(/[\\/]/).pop() || s.hash,
              path: s.path,
              hash: s.hash,
              size: s.size || 0,
              status: 'seeding',
              seeders: 1,
              leechers: 0,
              uploadDate: s.addedAt ? new Date(s.addedAt) : new Date(),
              version: 1,
              isEncrypted: false,
              manifest: s.manifest ?? null,
            })
          }
        }
        if (toAdd.length > 0) {
          files.update(curr => [...curr, ...toAdd])
        }
      }
    } catch (e) {
      console.warn('Failed to restore persisted seed list', e)
    }

    // HTML5 Drag and Drop functionality
    const dropZone = document.querySelector('.drop-zone') as HTMLElement

    if (dropZone) {
      const handleDragOver = (e: DragEvent) => {
        e.preventDefault()
        e.stopPropagation()
        e.dataTransfer!.dropEffect = 'copy'
        isDragging = true
      }

      const handleDragEnter = (e: DragEvent) => {
        e.preventDefault()
        e.stopPropagation()
        e.dataTransfer!.dropEffect = 'copy'
        isDragging = true
      }

      const handleDragLeave = (e: DragEvent) => {
        e.preventDefault()
        e.stopPropagation()
        // Only set isDragging to false if we're leaving the drop zone entirely
        if (e.currentTarget && !dropZone.contains(e.relatedTarget as Node)) {
          isDragging = false
        }
      }

      // Add global drag end handler to reset dragging state
      const handleDragEnd = (_e: DragEvent) => {
        isDragging = false
      }

      const handleDrop = async (e: DragEvent) => {
        e.preventDefault()
        e.stopPropagation()
        isDragging = false

        if (!$etcAccount) {
          showToast('Please create or import an account to upload files', 'warning')
          return
        }

        if (isUploading) {
          showToast('Upload already in progress. Please wait for the current upload to complete.', 'warning')
          return
        }

        const droppedFiles = Array.from(e.dataTransfer?.files || [])

        if (droppedFiles.length > 0) {
          // Check if we're in Tauri environment
          if (!isTauri) {
            showToast('File upload is only available in the desktop app', 'error')
            return
          }

          try {
            isUploading = true
            let duplicateCount = 0
            let addedCount = 0

            // Process each dropped file using versioned upload
            for (const file of droppedFiles) {
              try {
                // Check for existing versions before upload
                let existingVersions: any[] = [];
                try {
                  existingVersions = await invoke('get_file_versions_by_name', { fileName: file.name }) as any[];
                } catch (versionError) {
                  console.log('No existing versions found for', file.name);
                }

                // Get recipient key if encrypted sharing is enabled
                const recipientKey = useEncryptedSharing && recipientPublicKey.trim() ? recipientPublicKey.trim() : undefined;

                // UNIFIED UPLOAD: Save to temp file, then use single backend command
                const buffer = await file.arrayBuffer();
                const fileData = Array.from(new Uint8Array(buffer));
                const tempFilePath = await invoke<string>("save_temp_file_for_upload", {
                  fileName: file.name,
                  fileData,
                });

                // Single command: chunk, encrypt, publish to DHT
                const result = await invoke<{merkleRoot: string, fileName: string, fileSize: number, isEncrypted: boolean, peerId: string, version: number}>(
                  "upload_and_publish_file",
                  {
                    filePath: tempFilePath,
                    fileName: file.name,  // Pass original filename for DHT
                    recipientPublicKey: recipientKey
                  }
                );

                // Check for duplicates
                if (get(files).some(f => f.hash === result.merkleRoot)) {
                  duplicateCount++
                  continue;
                }

                const isNewVersion = existingVersions.length > 0;
                const isDhtRunning = dhtService.getPeerId() !== null;

                const newFile = {
                  id: `file-${Date.now()}-${Math.random()}`,
                  name: file.name,  // Use original file name from drag-drop
                  path: file.name,  // Use original file name as path for display
                  hash: result.merkleRoot,
                  size: result.fileSize,
                  status: isDhtRunning ? ('seeding' as const) : ('uploaded' as const),
                  seeders: isDhtRunning ? 1 : 0,
                  leechers: 0,
                  uploadDate: new Date(),
                  version: result.version,  // Use version from backend
                  isNewVersion: isNewVersion,
                  isEncrypted: result.isEncrypted,
                };

                files.update((currentFiles) => [...currentFiles, newFile]);
                addedCount++;
              } catch (error) {
                console.error('Error uploading dropped file:', file.name, error);
                const fileName = file.name || 'unknown file';
                showToast(tr('upload.fileFailed', { values: { name: fileName, error: String(error) } }), 'error');
              }
            }

            if (duplicateCount > 0) {
              showToast(tr('upload.duplicateSkipped', { values: { count: duplicateCount } }), 'warning')
            }

            if (addedCount > 0) {
              const isDhtRunning = dhtService.getPeerId() !== null;
              if (isDhtRunning) {
                showToast('Files published to DHT network for sharing!', 'success')
              } else {
                showToast('Files stored locally. Start DHT network to share with others.', 'info')
              }
              // Make storage refresh non-blocking to prevent UI hanging
              setTimeout(() => refreshAvailableStorage(), 100)
            }
          } catch (error) {
            console.error('Error handling dropped files:', error)
            showToast('Error processing dropped files. Please try again or use the "Add Files" button instead.', 'error')
          } finally {
            isUploading = false
          }
        }
      }

      dropZone.addEventListener('dragenter', handleDragEnter)
      dropZone.addEventListener('dragover', handleDragOver)
      dropZone.addEventListener('dragleave', handleDragLeave)
      dropZone.addEventListener('drop', handleDrop)

      // Prevent default browser behavior for drag and drop on the entire window
      const preventDefaults = (e: Event) => {
        e.preventDefault()
        e.stopPropagation()
      }

      window.addEventListener('dragover', preventDefaults)
      window.addEventListener('drop', preventDefaults)

      // Add global drag end handlers to ensure dragging state is reset
      document.addEventListener('dragend', handleDragEnd)
      document.addEventListener('drop', handleDragEnd)

      // Store cleanup function
      ;(window as any).dragDropCleanup = () => {
        dropZone.removeEventListener('dragenter', handleDragEnter)
        dropZone.removeEventListener('dragover', handleDragOver)
        dropZone.removeEventListener('dragleave', handleDragLeave)
        dropZone.removeEventListener('drop', handleDrop)
        window.removeEventListener('dragover', preventDefaults)
        window.removeEventListener('drop', preventDefaults)
        document.removeEventListener('dragend', handleDragEnd)
        document.removeEventListener('drop', handleDragEnd)
      }
    }
  })

  onDestroy(() => {
    // Cleanup drag and drop listeners
    if ((window as any).dragDropCleanup) {
      (window as any).dragDropCleanup()
    }
  })

  // Persist seeding list when files store changes (debounced-ish)
  let persistTimeout: ReturnType<typeof setTimeout> | null = null
  const unsubscribeFiles = files.subscribe(($files) => {
    // Collect seeding entries
    const seeds: SeedRecord[] = $files
      .filter(f => f.status === 'seeding' && f.path) // Only include files with a path
      .map(f => ({ id: f.id, path: f.path!, hash: f.hash, name: f.name, size: f.size, addedAt: (f.uploadDate ? f.uploadDate.toISOString() : new Date().toISOString()), manifest: f.manifest }))

    if (persistTimeout) clearTimeout(persistTimeout)
    persistTimeout = setTimeout(() => {
      saveSeedList(seeds).catch(e => console.warn('Failed to persist seed list', e))
    }, 400)
  })

  // Ensure we unsubscribe when leaving the page
  onDestroy(() => {
    unsubscribeFiles()
    if (persistTimeout) clearTimeout(persistTimeout)
  })

  async function openFileDialog() {
    if (!$etcAccount) {
      showToast('Please create or import an account to upload files', 'warning')
      return
    }

    if (isUploading) return

    try {
      const selectedPaths = await open({
        multiple: true,
      }) as string[] | null;

      if (selectedPaths && selectedPaths.length > 0) {
        isUploading = true
        await addFilesFromPaths(selectedPaths);
      }
    } catch (e) {
      showToast(tr('upload.fileDialogError'), 'error');
    } finally {
      isUploading = false
    }
  }

  async function removeFile(fileHash: string) {
    // Check if we're in Tauri environment
    if (!isTauri) {
      showToast('File management is only available in the desktop app', 'error')
      return
    }

    try {
        // Stop publishing file to DHT network
        try {
          await invoke('stop_publishing_file', { fileHash });
          console.log('File unpublished from DHT:', fileHash);
        } catch (unpublishError) {
          console.warn('Failed to unpublish file from DHT:', unpublishError);
        }

        files.update(f => f.filter(file => file.hash !== fileHash))
      } catch (error) {
        console.error(error);
        showToast(tr('upload.fileFailed', { values: { name: fileHash, error: String(error) } }), 'error');
      }
  }

  async function addFilesFromPaths(paths: string[]) {
    if (!$etcAccount) {
      showToast('Please create or import an account to upload files', 'warning')
      return
    }

    let duplicateCount = 0
    let addedCount = 0


    // Process all files concurrently to avoid blocking the UI
    const filePromises = paths.map(async (filePath) => {
      try {
        const fileName = filePath.split(/[\/\\]/).pop() || '';
        const recipientKey = useEncryptedSharing && recipientPublicKey.trim() ? recipientPublicKey.trim() : undefined;

        // UNIFIED UPLOAD: Single command chunks, encrypts, and publishes to DHT
        const result = await invoke<{merkleRoot: string, fileName: string, fileSize: number, isEncrypted: boolean, peerId: string, version: number}>(
          "upload_and_publish_file",
          {
            filePath,
            fileName: null,  // File path already contains correct name
            recipientPublicKey: recipientKey
          }
        );

        // Check for duplicates
        if (get(files).some((f: FileItem) => f.hash === result.merkleRoot)) {
          return { type: 'duplicate', fileName };
        }

        const isDhtRunning = dhtService.getPeerId() !== null;

        const newFile: FileItem = {
          id: `file-${Date.now()}-${Math.random()}`,
          name: result.fileName,
          path: filePath,
          hash: result.merkleRoot,
          size: result.fileSize,
          status: isDhtRunning ? 'seeding' : 'uploaded',
          seeders: isDhtRunning ? 1 : 0,
          leechers: 0,
          uploadDate: new Date(),
          version: result.version,  // Use version from backend
          isEncrypted: result.isEncrypted,
        };

        files.update(f => [...f, newFile]);

        return { type: 'success', fileName };
      } catch (error) {
        console.error(error);
        const fileName = filePath.split(/[\/]/).pop() || 'unknown file';
        showToast(tr('upload.fileFailed', { values: { name: fileName, error: String(error) } }), 'error');
        return { type: 'error', fileName: fileName, error };
      }
    });

    // Wait for all files to be processed concurrently
    const results = await Promise.all(filePromises);

    // Count results
    results.forEach(result => {
      if (result.type === 'duplicate') {
        duplicateCount++;
      } else if (result.type === 'success') {
        addedCount++;
      }
    });

    // Show summary messages
    if (duplicateCount > 0) {
      showToast(tr('upload.duplicateSkipped', { values: { count: duplicateCount } }), 'warning')
    }

    if (addedCount > 0) {
      showUploadSummaryMessage(addedCount);
    }
  }

  function showUploadSummaryMessage(addedCount: number) {
    if (addedCount > 0) {
      const isDhtRunning = dhtService.getPeerId() !== null;
      if (isDhtRunning) {
        showToast('Files published to DHT network for sharing!', 'success')
      } else {
        showToast('Files stored locally. Start DHT network to share with others.', 'info')
      }
      // Make storage refresh non-blocking to prevent UI hanging
      setTimeout(() => refreshAvailableStorage(), 100)
    }
  }

  function formatFileSize(bytes: number): string {
    if (bytes < 1024) return bytes + ' B'
    if (bytes < 1048576) return (bytes / 1024).toFixed(2) + ' KB'
    return (bytes / 1048576).toFixed(2) + ' MB'
  }

  async function handleCopy(hash: string) {
    await navigator.clipboard.writeText(hash);
    showToast('File hash copied to clipboard!', 'success');
  }

  async function showVersionHistory(fileName: string) {
    try {
      const versions = await invoke('get_file_versions_by_name', { fileName }) as any[];
      if (versions.length === 0) {
        showToast('No version history found for this file', 'info');
        return;
      }

      const versionList = versions.map(v =>
        `v${v.version}: ${v.fileHash.slice(0, 8)}... (${new Date(v.createdAt * 1000).toLocaleDateString()})`
      ).join('\n');

      showToast(`Version history for ${fileName}:\n${versionList}`, 'info');
    } catch (error) {
      showToast('Failed to load version history', 'error');
    }
  }
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-3xl font-bold">{$t('upload.title')}</h1>
    <p class="text-muted-foreground mt-2">{$t('upload.subtitle')}</p>
  </div>

  {#if isTauri}
  <Card class="p-4 flex flex-wrap items-start justify-between gap-4">
    <div class="space-y-1">
      <p class="text-sm font-semibold text-foreground">{$t('upload.storage.title')}</p>
      <p class="text-sm text-muted-foreground">{storageLabel}</p>
      {#if lastCheckedLabel}
        <p class="text-xs text-muted-foreground">{lastCheckedLabel}</p>
      {/if}
      {#if showLowStorageDescription}
        <p class="text-xs text-amber-600 dark:text-amber-400">{$t('upload.storage.lowDescription')}</p>
      {/if}
      {#if storageError}
        <p class="text-xs text-destructive">{storageError}</p>
      {/if}
    </div>
    <div class="flex items-center gap-3">
      <Badge class={`text-xs font-medium ${storageBadgeClass}`}>{storageBadgeText}</Badge>
      <button
        class="inline-flex items-center justify-center h-9 rounded-md px-3 text-sm font-medium border border-input bg-background hover:bg-muted disabled:opacity-60 disabled:cursor-not-allowed"
        on:click={() => refreshAvailableStorage()}
        disabled={isRefreshingStorage}
        aria-label={$t('upload.storage.refresh')}>
        <RefreshCw class={`h-4 w-4 mr-2 ${isRefreshingStorage ? 'animate-spin' : ''}`} />
        {$t('upload.storage.refresh')}
      </button>
    </div>
  </Card>
  {:else}
  <Card class="p-4">
    <div class="text-center">
      <p class="text-sm font-semibold text-foreground mb-2">Desktop App Required</p>
      <p class="text-sm text-muted-foreground">Storage monitoring requires the desktop application</p>
    </div>
  </Card>
  {/if}

  <!-- Encrypted Sharing Options -->
  {#if isTauri}
  <Card class="p-4">
    <button 
      class="w-full flex items-center justify-between cursor-pointer hover:opacity-80 transition-opacity"
      on:click={() => showEncryptionOptions = !showEncryptionOptions}
    >
      <div class="flex items-center gap-3">
        <div class="flex items-center justify-center w-10 h-10 bg-gradient-to-br from-purple-500/10 to-purple-500/5 rounded-lg border border-purple-500/20">
          <Lock class="h-5 w-5 text-purple-600" />
        </div>
        <div class="text-left">
          <h3 class="text-sm font-semibold text-foreground">{$t('upload.encryption.title')}</h3>
          <p class="text-xs text-muted-foreground">{$t('upload.encryption.subtitle')}</p>
        </div>
      </div>
      <svg 
        class="h-5 w-5 text-muted-foreground transition-transform duration-200 {showEncryptionOptions ? 'rotate-180' : ''}" 
        fill="none" 
        stroke="currentColor" 
        viewBox="0 0 24 24"
      >
        <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M19 9l-7 7-7-7" />
      </svg>
    </button>
    
    {#if showEncryptionOptions}
      <div class="mt-4 space-y-4 pt-4 border-t border-border">
        <div class="flex items-center gap-2">
          <input
            type="checkbox"
            id="use-encrypted-sharing"
            bind:checked={useEncryptedSharing}
            class="cursor-pointer"
          />
          <Label for="use-encrypted-sharing" class="cursor-pointer text-sm">
            {$t('upload.encryption.enableForRecipient')}
          </Label>
        </div>
        
        {#if useEncryptedSharing}
          <div class="space-y-2 pl-6">
            <div class="flex items-center gap-2">
              <Key class="h-4 w-4 text-muted-foreground" />
              <Label for="recipient-public-key" class="text-sm font-medium">
                {$t('upload.encryption.recipientPublicKey')}
              </Label>
            </div>
            <Input
              id="recipient-public-key"
              bind:value={recipientPublicKey}
              placeholder={$t('upload.encryption.publicKeyPlaceholder')}
              class="font-mono text-sm"
              disabled={isUploading}
            />
            <p class="text-xs text-muted-foreground">
              {$t('upload.encryption.publicKeyHint')}
            </p>
            {#if recipientPublicKey && !(/^[0-9a-fA-F]{64}$/.test(recipientPublicKey.trim()))}
              <p class="text-xs text-destructive">
                {$t('upload.encryption.invalidPublicKey')}
              </p>
            {/if}
          </div>
        {/if}
      </div>
    {/if}
  </Card>
  {/if}

  <Card class="drop-zone relative p-6 transition-all duration-200 border-dashed {isDragging ? 'border-primary bg-primary/5 scale-[1.01]' : isUploading ? 'border-orange-500 bg-orange-500/5' : 'border-muted-foreground/25 hover:border-muted-foreground/50'}"
        role="button"
        tabindex="0"
        aria-label="Drop zone for file uploads">
    <div
      class="space-y-4"
      role="region"
   >
    <div class="space-y-4">
      <!-- Drag & Drop Indicator -->
      {#if $files.filter(f => f.status === 'seeding' || f.status === 'uploaded').length === 0}
        <div class="text-center py-12 border-2 border-dashed rounded-xl transition-all duration-300 relative overflow-hidden {isDragging ? 'border-primary bg-gradient-to-br from-primary/20 via-primary/10 to-primary/5 scale-105 shadow-2xl' : 'border-muted-foreground/25 bg-gradient-to-br from-muted/5 to-muted/10 hover:border-muted-foreground/40 hover:bg-muted/20'}">

          <!-- Animated background when dragging -->
          {#if isDragging}
            <div class="absolute inset-0 bg-gradient-to-r from-transparent via-primary/10 to-transparent animate-pulse"></div>
            <div class="absolute inset-0 bg-[radial-gradient(circle_at_50%_50%,rgba(59,130,246,0.1)_0%,transparent_70%)] animate-ping"></div>
          {/if}

          <div class="relative z-10">
            <!-- Dynamic icon based on drag state -->
            <div class="relative mb-6">
              {#if isDragging}
                <div class="absolute inset-0 animate-ping">
                  <Upload class="h-16 w-16 mx-auto text-primary/60" />
                </div>
                <Upload class="h-16 w-16 mx-auto text-primary animate-bounce" />
              {:else}
                <FolderOpen class="h-16 w-16 mx-auto text-muted-foreground/70 hover:text-primary transition-colors duration-300" />
              {/if}
            </div>

            <!-- Dynamic text -->
            <h3 class="text-2xl font-bold mb-3 transition-all duration-300 {isDragging ? 'text-primary scale-110' : isUploading ? 'text-orange-500 scale-105' : 'text-foreground'}">{isDragging ? '✨ Drop files here!' : isUploading ? '🔄 Uploading files...' : $t('upload.dropFiles')}</h3>
            <p class="text-muted-foreground mb-8 text-lg transition-colors duration-300">
              {isDragging
                ? (isTauri ? 'Release to upload your files instantly' : 'Drag and drop not available in web version')
                : isUploading
                  ? 'Please wait while your files are being processed...'
                  : (isTauri ? $t('upload.dropFilesHint') : 'Drag and drop requires desktop app')
              }
            </p>

            {#if !isDragging}
              <!-- File type icons preview -->
              <div class="flex justify-center gap-4 mb-8 opacity-60">
                <Image class="h-8 w-8 text-blue-500 animate-pulse" style="animation-delay: 0ms;" />
                <Video class="h-8 w-8 text-purple-500 animate-pulse" style="animation-delay: 200ms;" />
                <Music class="h-8 w-8 text-green-500 animate-pulse" style="animation-delay: 400ms;" />
                <Archive class="h-8 w-8 text-orange-500 animate-pulse" style="animation-delay: 600ms;" />
                <Code class="h-8 w-8 text-red-500 animate-pulse" style="animation-delay: 800ms;" />
              </div>

              <div class="flex justify-center gap-3">
                {#if isTauri}
                  <button class="group inline-flex items-center justify-center h-12 rounded-xl px-6 text-sm font-medium bg-gradient-to-r from-primary to-primary/90 text-primary-foreground hover:from-primary/90 hover:to-primary shadow-lg hover:shadow-xl transition-all duration-300 hover:scale-105 disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:scale-100" disabled={isUploading} on:click={openFileDialog}>
                    <Plus class="h-5 w-5 mr-2 group-hover:rotate-90 transition-transform duration-300" />
                    {isUploading ? 'Uploading...' : $t('upload.addFiles')}
                  </button>
                {:else}
                  <div class="text-center">
                    <p class="text-sm text-muted-foreground mb-3">File upload requires the desktop app</p>
                    <p class="text-xs text-muted-foreground">Download the desktop version to upload and share files</p>
                  </div>
                {/if}
              </div>

              <!-- Supported formats hint -->
              <p class="text-xs text-muted-foreground/75 mt-4">
                {#if isTauri}
                  {$t('upload.supportedFormats')}
                {:else}
                  {$t('upload.supportedFormatsDesktop')}
                {/if}
              </p>
            {/if}
          </div>
        </div>
      {:else}
        <div class="flex flex-wrap items-center justify-between gap-4 mb-4">
          <div>
            <h2 class="text-lg font-semibold">{$t('upload.sharedFiles')}</h2>
            <p class="text-sm text-muted-foreground mt-1">
              {$files.filter(f => f.status === 'seeding' || f.status === 'uploaded').length} {$t('upload.files')} •
              {formatFileSize($files.filter(f => f.status === 'seeding' || f.status === 'uploaded').reduce((sum, f) => sum + f.size, 0))} {$t('upload.total')}
              {#if $files.filter(f => f.status === 'seeding').length > 0}
                <span class="text-green-600 font-medium">({$files.filter(f => f.status === 'seeding').length} seeding)</span>
              {/if}
            </p>
            <p class="text-xs text-muted-foreground mt-1">{$t('upload.tip')}</p>
          </div>
          <div class="flex gap-2">
            {#if isTauri}
              <button class="inline-flex items-center justify-center h-9 rounded-md px-3 text-sm font-medium bg-primary text-primary-foreground hover:bg-primary/90 disabled:opacity-50 disabled:cursor-not-allowed" disabled={isUploading} on:click={openFileDialog}>
                <Plus class="h-4 w-4 mr-2" />
                {isUploading ? 'Uploading...' : $t('upload.addMoreFiles')}
              </button>
            {:else}
              <div class="text-center">
                <p class="text-xs text-muted-foreground">Desktop app required for file management</p>
              </div>
            {/if}
          </div>
        </div>
      {/if}

      <!-- File List -->
      {#if $files.filter(f => f.status === 'seeding' || f.status === 'uploaded').length > 0}
        <div class="space-y-3 relative">
          {#each $files.filter(f => f.status === 'seeding' || f.status === 'uploaded') as file}
            <div class="group relative bg-gradient-to-r from-card to-card/80 border border-border/50 rounded-xl p-4 hover:shadow-lg hover:border-border transition-all duration-300 hover:scale-[1.01] overflow-hidden">
              <!-- Background gradient effect -->
              <div class="absolute inset-0 bg-gradient-to-r from-primary/5 via-transparent to-secondary/5 opacity-0 group-hover:opacity-100 transition-opacity duration-300"></div>

              <div class="relative flex items-center justify-between gap-4">
                <div class="flex items-center gap-4 min-w-0 flex-1">
                  <!-- Enhanced file icon with background -->
                  <div class="relative">
                    <div class="absolute inset-0 bg-primary/20 rounded-lg blur-lg opacity-0 group-hover:opacity-100 transition-opacity duration-300"></div>
                    <div class="relative flex items-center justify-center w-12 h-12 bg-gradient-to-br from-primary/10 to-primary/5 rounded-lg border border-primary/20">
                      <svelte:component this={getFileIcon(file.name)} class="h-6 w-6 {getFileColor(file.name)}" />
                    </div>
                  </div>

                  <div class="flex-1 min-w-0 space-y-2">
                    <div class="flex items-center gap-2">
                      <p class="text-sm font-semibold truncate text-foreground">{file.name}</p>
                       {#if file.version}
                        <Badge
                          class="bg-blue-100 text-blue-800 text-xs px-2 py-0.5 cursor-pointer hover:bg-blue-200 transition-colors"
                          title="v{file.version} - Click to view version history"
                          on:click={() => showVersionHistory(file.name)}
                        >
                          v{file.version}
                        </Badge>
                      {/if}
                      {#if file.isEncrypted}
                        <Badge
                          class="bg-purple-100 text-purple-800 text-xs px-2 py-0.5 flex items-center gap-1"
                          title="This file is encrypted end-to-end"
                        >
                          <Lock class="h-3 w-3" />
                          Encrypted
                        </Badge>
                      {/if}
                      <div class="flex items-center gap-1">
                        <div class="w-1.5 h-1.5 bg-green-500 rounded-full animate-pulse"></div>
                        <span class="text-xs text-green-600 font-medium">Active</span>
                      </div>
                    </div>

                    <div class="flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
                      <div class="flex items-center gap-1">
                        <span class="opacity-60">Hash:</span>
                        <code class="bg-muted/50 px-1.5 py-0.5 rounded text-xs font-mono">{file.hash.slice(0, 8)}...{file.hash.slice(-6)}</code>
                      </div>
                      <span>•</span>
                      <span class="font-medium">{formatFileSize(file.size)}</span>
                      {#if file.seeders !== undefined}
                        <span>•</span>
                        <div class="flex items-center gap-1">
                          <Upload class="h-3 w-3 text-green-500" />
                          <span class="text-green-600 font-medium">{file.seeders || 1}</span>
                        </div>
                      {/if}
                      {#if file.leechers && file.leechers > 0}
                        <span>•</span>
                        <div class="flex items-center gap-1">
                          <Download class="h-3 w-3 text-orange-500" />
                          <span class="text-orange-600 font-medium">{file.leechers}</span>
                        </div>
                      {/if}
                    </div>
                  </div>
                </div>

                <div class="flex items-center gap-2">
                  {#if file.status === 'seeding'}
                    <Badge variant="secondary" class="bg-green-500/10 text-green-600 border-green-500/20 font-medium">
                      <div class="w-1.5 h-1.5 bg-green-500 rounded-full mr-1.5 animate-pulse"></div>
                      {$t('upload.seeding')}
                    </Badge>
                  {:else if file.status === 'uploaded'}
                    <Badge variant="secondary" class="bg-blue-500/10 text-blue-600 border-blue-500/20 font-medium">
                      <div class="w-1.5 h-1.5 bg-blue-500 rounded-full mr-1.5"></div>
                      Stored Locally
                    </Badge>
                  {/if}

                  <button
                    on:click={() => handleCopy(file.hash)}
                    class="group/btn p-2 hover:bg-primary/10 rounded-lg transition-all duration-200 hover:scale-110"
                    title={$t('upload.copyHash')}
                    aria-label="Copy file hash"
                  >
                    <svg class="h-4 w-4 text-muted-foreground group-hover/btn:text-primary transition-colors" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" />
                    </svg>
                  </button>

                  {#if isTauri}
                    <button
                      on:click={() => removeFile(file.hash)}
                      class="group/btn p-2 hover:bg-destructive/10 rounded-lg transition-all duration-200 hover:scale-110"
                      title={$t('upload.stopSharing')}
                      aria-label="Stop sharing file"
                    >
                      <X class="h-4 w-4 text-muted-foreground group-hover/btn:text-destructive transition-colors" />
                    </button>
                  {:else}
                    <div
                      class="p-2 text-muted-foreground/50 cursor-not-allowed"
                      title="File management requires desktop app"
                      aria-label="File management not available in web version"
                    >
                      <X class="h-4 w-4" />
                    </div>
                  {/if}
                </div>
              </div>
            </div>
          {/each}
        </div>
      {:else}
        <div class="text-center py-8">
          <FolderOpen class="h-12 w-12 mx-auto text-muted-foreground mb-3" />
          <p class="text-sm text-muted-foreground">{$t('upload.noFilesShared')}</p>
          <p class="text-xs text-muted-foreground mt-1">{$t('upload.addFilesHint2')}</p>
        </div>
      {/if}

    </div>
    </div>
  </Card>
</div>
