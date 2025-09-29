<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import { File as FileIcon, X, Plus, FolderOpen, FileText, Image, Music, Video, Archive, Code, FileSpreadsheet, Upload, Download, RefreshCw } from 'lucide-svelte'
  import { files } from '$lib/stores'
  import { t } from 'svelte-i18n';
  import { get } from 'svelte/store'
  import { onMount, onDestroy, tick } from 'svelte';
  import { showToast } from '$lib/toast'
  import { getStorageStatus, isDuplicateHash } from '$lib/uploadHelpers'
  import { fileService } from '$lib/services/fileService'
  import { open } from "@tauri-apps/plugin-dialog";
  import { invoke } from "@tauri-apps/api/core";
  import { dhtService } from '$lib/dht';
  import type { FileMetadata } from '$lib/dht';
  import Button from '$lib/components/ui/button.svelte';


  const tr = (k: string, params?: Record<string, any>) => get(t)(k, params)

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

  // Hash copied popup state
  let copiedHash: string | null = null;
  let showCopied = false;

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
    try {
      const result = await fileService.getAvailableStorage()
      storageStatus = getStorageStatus(result, LOW_STORAGE_THRESHOLD)

      if (result === null) {
        storageError = tr('upload.storage.error')
        availableStorage = null
        lastChecked = null
      } else {
        availableStorage = Math.max(0, Math.floor(result))
        storageError = null
        lastChecked = new Date()
      }
    } finally {
      isRefreshingStorage = false
    }
  }

  onMount(async () => {
    refreshAvailableStorage()

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

      const handleDrop = async (e: DragEvent) => {
        e.preventDefault()
        e.stopPropagation()
        isDragging = false

        const droppedFiles = Array.from(e.dataTransfer?.files || [])

        if (droppedFiles.length > 0) {
          // Check if we're in Tauri environment
          if (!isTauri) {
            showToast('File upload is only available in the desktop app', 'error')
            return
          }

          try {
            let duplicateCount = 0
            let addedCount = 0

            // Process each dropped file directly using the File object
            for (const file of droppedFiles) {
              try {
                // Use the fileService.uploadFile method which handles File objects
                const hash = await fileService.uploadFile(file)

                // Check if this hash is already in our files
                if (isDuplicateHash(get(files), hash)) {
                  duplicateCount++
                  continue;
                }

                const newFile = {
                  id: `file-${Date.now()}-${Math.random()}`,
                  name: file.name,
                  path: file.name, // Use file name as path for display
                  hash: hash,
                  size: file.size,
                  status: 'seeding' as const,
                  seeders: 1,
                  leechers: 0,
                  uploadDate: new Date()
                };

                files.update((currentFiles) => [...currentFiles, newFile]);
                addedCount++;

                // Publish file metadata to DHT network for discovery
                try {
                  await dhtService.publishFile({
                    fileHash: hash,
                    fileName: file.name,
                    fileSize: file.size,
                    seeders: [],
                    createdAt: Date.now(),
                    isEncrypted: false
                  });
                  console.log('Dropped file published to DHT:', hash);
                } catch (publishError) {
                  console.warn('Failed to publish dropped file to DHT:', publishError);
                }
              } catch (error) {
                console.error('Error uploading dropped file:', file.name, error);
                showToast(tr('upload.fileFailed', { values: { name: file.name, error: String(error) } }), 'error');
              }
            }

            if (duplicateCount > 0) {
              showToast(tr('upload.duplicateSkipped', { values: { count: duplicateCount } }), 'warning')
            }

            if (addedCount > 0) {
              showToast(tr('upload.filesAdded', { values: { count: addedCount } }), 'success')
              showToast('Files published to DHT network for sharing!', 'success')
              refreshAvailableStorage()
            }
          } catch (error) {
            console.error('Error handling dropped files:', error)
            showToast('Error processing dropped files. Please try again or use the "Add Files" button instead.', 'error')
          }
        }
      }

      dropZone.addEventListener('dragenter', handleDragEnter)
      dropZone.addEventListener('dragover', handleDragOver)
      dropZone.addEventListener('dragleave', handleDragLeave)
      dropZone.addEventListener('drop', handleDrop)

      // Store cleanup function
      ;(window as any).dragDropCleanup = () => {
        dropZone.removeEventListener('dragenter', handleDragEnter)
        dropZone.removeEventListener('dragover', handleDragOver)
        dropZone.removeEventListener('dragleave', handleDragLeave)
        dropZone.removeEventListener('drop', handleDrop)
      }
    }
  })

  onDestroy(() => {
    // Cleanup drag and drop listeners
    if ((window as any).dragDropCleanup) {
      (window as any).dragDropCleanup()
    }
  })

  async function openFileDialog() {
    try {
      const selectedPaths = await open({
        multiple: true,
      });

      if (Array.isArray(selectedPaths)) {
        addFilesFromPaths(selectedPaths);
      } else if (selectedPaths) {
        addFilesFromPaths([selectedPaths]);
      }
    } catch (e) {
      showToast(tr('upload.fileDialogError'), 'error');
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
    let duplicateCount = 0
    let addedCount = 0

    for (const filePath of paths) {
      try {
        const metadata = await invoke('upload_file_to_network',{filePath}) as FileMetadata;

        if (isDuplicateHash(get(files), metadata.fileHash)) {
          duplicateCount++
          continue;
        }

        const newFile = {
          id: `file-${Date.now()}-${Math.random()}`,
          name: metadata.fileName,
          path: filePath,
          hash: metadata.fileHash,
          size: metadata.fileSize,
          status: 'seeding' as const,
          seeders: 1,
          leechers: 0,
          uploadDate: new Date(metadata.createdAt)
        };

        files.update(f => [...f, newFile]);
        addedCount++;

        // Publish file metadata to DHT network for discovery
        try {
          await dhtService.publishFile(metadata);
          console.log('File published to DHT:', metadata.fileHash);
        } catch (publishError) {
          console.warn('Failed to publish file to DHT:', publishError);
          // Don't show error to user as upload succeeded, just DHT publishing failed
        }
      } catch (error) {
        console.error(error);
        showToast(tr('upload.fileFailed', { values: { name: filePath.split(/[\/]/).pop(), error: String(error) } }), 'error');
      }
    }

    if (duplicateCount > 0) {
      showToast(tr('upload.duplicateSkipped', { values: { count: duplicateCount } }), 'warning')
    }

    if (addedCount > 0) {
      showToast(tr('upload.filesAdded', { values: { count: addedCount } }), 'success')
      showToast('Files published to DHT network for sharing!', 'success')
      refreshAvailableStorage()
    }
  }

  function formatFileSize(bytes: number): string {
    if (bytes < 1024) return bytes + ' B'
    if (bytes < 1048576) return (bytes / 1024).toFixed(2) + ' KB'
    return (bytes / 1048576).toFixed(2) + ' MB'
  }

  function toHumanReadableSize(bytes: number): string {
    return formatFileSize(bytes);
  }

  async function handleCopy(hash: string) {
    await navigator.clipboard.writeText(hash);
    copiedHash = hash;
    showCopied = true;
    await tick();
    setTimeout(() => {
      showCopied = false;
    }, 1200);
  }

  let selectedFile: File | null = null;
  let existingVersions: any[] = [];
  let uploadMsg = '';
  let errorMsg = '';

  async function handleFileSelect(e: Event) {
    errorMsg = '';
    uploadMsg = '';
    selectedFile = (e.target as HTMLInputElement).files?.[0] ?? null;
    existingVersions = [];
    if (selectedFile) {
      try {
        existingVersions = await invoke('get_file_versions_by_name', {
          file_name: selectedFile.name
        }) as any[];
      } catch (err) {
        errorMsg = 'Could not query versions: ' + String(err);
      }
    }
  }

  async function handleUpload() {
    if (!selectedFile) return;
    uploadMsg = '';
    errorMsg = '';
    try {
      // Use .path if in Tauri; fallback to file.name (may need adjustment for your environment)
      const filePath = (selectedFile as any).path ?? selectedFile.name;
      const metadata = await invoke('upload_versioned_file', {
        file_name: selectedFile.name,
        file_path: filePath,
        file_size: selectedFile.size,
        mime_type: selectedFile.type ?? null,
        is_encrypted: false,
        encryption_method: null,
        key_fingerprint: null,
      }) as any;

      uploadMsg = `Uploaded as v${metadata.version} (${metadata.file_hash.slice(0,8)}...)`;
      selectedFile = null;
      existingVersions = [];
    } catch (err) {
      errorMsg = 'Upload failed: ' + String(err);
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

  <Card class="drop-zone relative p-6 transition-all duration-200 border-dashed {isDragging ? 'border-primary bg-primary/5 scale-[1.01]' : 'border-muted-foreground/25 hover:border-muted-foreground/50'}"
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
            <h3 class="text-2xl font-bold mb-3 transition-all duration-300 {isDragging ? 'text-primary scale-110' : 'text-foreground'}">{isDragging ? '✨ Drop files here!' : $t('upload.dropFiles')}</h3>
            <p class="text-muted-foreground mb-8 text-lg transition-colors duration-300">
              {isDragging
                ? (isTauri ? 'Release to upload your files instantly' : 'Drag and drop not available in web version')
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
                  <button class="group inline-flex items-center justify-center h-12 rounded-xl px-6 text-sm font-medium bg-gradient-to-r from-primary to-primary/90 text-primary-foreground hover:from-primary/90 hover:to-primary shadow-lg hover:shadow-xl transition-all duration-300 hover:scale-105" on:click={openFileDialog}>
                    <Plus class="h-5 w-5 mr-2 group-hover:rotate-90 transition-transform duration-300" />
                    {$t('upload.addFiles')}
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
                  Supports images, videos, audio, documents, code files and more
                {:else}
                  Desktop app supports images, videos, audio, documents, code files and more
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
            </p>
            <p class="text-xs text-muted-foreground mt-1">{$t('upload.tip')}</p>
          </div>
          <div class="flex gap-2">
            {#if isTauri}
              <button class="inline-flex items-center justify-center h-9 rounded-md px-3 text-sm font-medium bg-primary text-primary-foreground hover:bg-primary/90" on:click={openFileDialog}>
                <Plus class="h-4 w-4 mr-2" />
                {$t('upload.addMoreFiles')}
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
                  <Badge variant="secondary" class="bg-green-500/10 text-green-600 border-green-500/20 font-medium">
                    <div class="w-1.5 h-1.5 bg-green-500 rounded-full mr-1.5 animate-pulse"></div>
                    {$t('upload.seeding')}
                  </Badge>

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

<Card class="max-w-xl mx-auto mt-10 p-8 shadow border border-muted bg-background rounded-xl">
  <h2 class="text-xl font-semibold text-primary mb-6 flex items-center gap-2">
    <Upload class="h-6 w-6 text-primary mr-2" /> Upload File <span class="ml-1 font-normal opacity-70">(with Versioning)</span>
  </h2>
  <div class="mb-6">
    <label class="block mb-2 font-medium text-muted-foreground">Choose a file</label>
    <input
      type="file"
      class="block w-full text-sm text-muted-foreground file:mr-4 file:py-2 file:px-4
            file:rounded-full file:border-0
            file:text-sm file:font-semibold
            file:bg-blue-50 file:text-blue-700
            hover:file:bg-blue-100 focus:outline-none focus:ring-2 focus:ring-primary/50"
      on:change={handleFileSelect}
    />
  </div>
  {#if selectedFile}
    <div class="mb-4 p-4 bg-muted border border-border rounded-lg">
      <div class="mb-2 flex flex-col sm:flex-row sm:items-center gap-2">
        <div class="flex items-center gap-2 flex-1">
          <FileText class="h-5 w-5 text-blue-500" />
          <span class="font-semibold text-foreground">{selectedFile.name}</span>
          <span class="text-xs text-muted-foreground">({toHumanReadableSize(selectedFile.size)})</span>
        </div>
        <Button on:click={handleUpload} class="ml-auto" size="sm">Upload New Version</Button>
      </div>
      {#if existingVersions.length}
        <div class="mt-3">
          <span class="block text-sm font-medium text-muted-foreground mb-1">Previous versions:</span>
          <ul class="ml-2 mt-1 list-disc text-sm text-muted-foreground space-y-1">
            {#each existingVersions as v}
              <li>
                <Badge class="bg-blue-100 text-blue-700 mr-2">v{v.version}</Badge>
                <span class="font-mono text-xs bg-muted px-2 py-0.5 rounded">{v.file_hash.slice(0,8)}...</span>
                <span class="ml-2 text-gray-400">{new Date(v.created_at * 1000).toLocaleString()}</span>
              </li>
            {/each}
          </ul>
          <div class="mt-2 text-blue-800 font-semibold">
            Latest: v{existingVersions[0].version}. This will be v{(existingVersions[0].version ?? 1)+1}.
          </div>
        </div>
      {:else}
        <div class="mt-3 text-muted-foreground">No previous version. This will be <span class="font-semibold">v1</span>.</div>
      {/if}
    </div>
  {/if}
  {#if uploadMsg}
    <div class="mt-2 px-4 py-2 bg-green-100 text-green-800 border border-green-200 rounded">{uploadMsg}</div>
  {/if}
  {#if errorMsg}
    <div class="mt-2 px-4 py-2 bg-red-100 text-red-700 border border-red-200 rounded">{errorMsg}</div>
  {/if}
</Card>
