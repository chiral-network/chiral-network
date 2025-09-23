<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import { File as FileIcon, X, Plus, FolderOpen, FileText, Image, Music, Video, Archive, Code, FileSpreadsheet, Upload, Download, RefreshCw } from 'lucide-svelte'
  import { files } from '$lib/stores'
  import { t } from 'svelte-i18n';
  import { get } from 'svelte/store'
  import { onMount, tick } from 'svelte';
  import { showToast } from '$lib/toast'
  import { getStorageStatus, isDuplicateHash } from '$lib/uploadHelpers.js'
  import { fileService } from '$lib/services/fileService'
  const tr = (k: string, params?: Record<string, any>) => get(t)(k, params)

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
  let dragCounter = 0
  let fileInput: HTMLInputElement

  const LOW_STORAGE_THRESHOLD = 5
  let availableStorage: number | null = null
  let storageStatus: 'unknown' | 'ok' | 'low' = 'unknown'
  let isRefreshingStorage = false
  let storageError: string | null = null
  let lastChecked: Date | null = null

  $: storageLabel = isRefreshingStorage
    ? tr('upload.storage.checking')
    : availableStorage !== null
      ? tr('upload.storage.available', { values: { space: availableStorage.toLocaleString() } })
      : tr('upload.storage.unknown')

  $: storageBadgeClass =
    storageStatus === 'low'
      ? 'bg-destructive text-destructive-foreground'
      : storageStatus === 'ok'
        ? 'bg-emerald-500/10 text-emerald-600 dark:text-emerald-300'
        : 'bg-muted text-muted-foreground'

  $: storageBadgeText =
    storageStatus === 'low'
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

  onMount(() => {
    refreshAvailableStorage()
  })

  function handleFileSelect(event: Event) {
    const input = event.target as HTMLInputElement
    if (input.files) {
      addFiles(Array.from(input.files))
      input.value = '' // Reset input
    }
  }

  function handleDrop(event: DragEvent) {
    event.preventDefault()
    isDragging = false
    dragCounter = 0
    if (event.dataTransfer?.files && event.dataTransfer.files.length > 0) {
      addFiles(Array.from(event.dataTransfer.files))
    }
  }

  function handleDragOver(event: DragEvent) {
    event.preventDefault();
    // Allow drop
  }

  function handleDragEnter(event: DragEvent) {
    event.preventDefault()
    dragCounter++
    if (dragCounter === 1) {
      isDragging = true
    }
  }

  function handleDragLeave(event: DragEvent) {
    event.preventDefault()
    dragCounter--
    if (dragCounter === 0) {
      isDragging = false
    }
  }
  
  function removeFile(fileId: string) {
    files.update(f => f.filter(file => file.id !== fileId))
  }
  
  async function addFiles(filesToAdd: File[]) {
    let duplicateCount = 0
    let addedCount = 0

    for (let i = 0; i < filesToAdd.length; i++) {
      const file = filesToAdd[i];
      try {
        // Mock upload for demo purposes (backend not available)
        const fileHash = `mock-hash-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;

        if (isDuplicateHash(get(files), fileHash)) {
          duplicateCount++
          continue;
        }

        const newFile = {
          id: `file-${Date.now()}-${i}`,
          name: file.name,
          hash: fileHash,
          size: file.size,
          status: 'seeding' as const,
          seeders: Math.floor(Math.random() * 10) + 1,
          leechers: Math.floor(Math.random() * 5),
          uploadDate: new Date()
        };

        files.update(f => [...f, newFile]);
        addedCount++;
      } catch (error) {
        console.error(`Failed to upload file "${file.name}":`, error);
        showToast(tr('upload.fileFailed', { values: { name: file.name, error: String(error) } }), 'error');
      }
    }

    if (duplicateCount > 0) {
      showToast(tr('upload.duplicateSkipped', { values: { count: duplicateCount } }), 'warning')
    }

    if (addedCount > 0) {
      showToast(tr('upload.filesAdded', { values: { count: addedCount } }), 'success')
      refreshAvailableStorage()
    }
  }
  
  function formatFileSize(bytes: number): string {
    if (bytes < 1024) return bytes + ' B'
    if (bytes < 1048576) return (bytes / 1024).toFixed(2) + ' KB'
    return (bytes / 1048576).toFixed(2) + ' MB'
  }

  // Hash copied popup state
  let copiedHash: string | null = null;
  let showCopied = false;
  async function handleCopy(hash: string) {
    await navigator.clipboard.writeText(hash);
    copiedHash = hash;
    showCopied = true;
    await tick();
    setTimeout(() => {
      showCopied = false;
    }, 1200);
  }
</script>

<div class="space-y-6">
  <div>
    <h1 class="text-3xl font-bold">{$t('upload.title')}</h1>
    <p class="text-muted-foreground mt-2">{$t('upload.subtitle')}</p>
  </div>

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
  
  <Card class="relative p-6 transition-all duration-200 border-dashed {isDragging ? 'border-primary bg-primary/5 scale-[1.01]' : 'border-muted-foreground/25 hover:border-muted-foreground/50'}">
    <div
      class="space-y-4"
      role="region"
      on:drop={handleDrop}
      on:dragover={handleDragOver}
      on:dragenter={handleDragEnter}
      on:dragleave={handleDragLeave}
   >
    <div class="space-y-4">
      <!-- Drag & Drop Indicator -->
      <input
        bind:this={fileInput}
        type="file"
        multiple
        on:change={handleFileSelect}
        class="hidden"
      />
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
            <p class="text-muted-foreground mb-8 text-lg transition-colors duration-300">{isDragging ? 'Release to upload your files instantly' : $t('upload.dropFilesHint')}</p>
            
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
                <button class="group inline-flex items-center justify-center h-12 rounded-xl px-6 text-sm font-medium bg-gradient-to-r from-primary to-primary/90 text-primary-foreground hover:from-primary/90 hover:to-primary shadow-lg hover:shadow-xl transition-all duration-300 hover:scale-105" on:click={() => fileInput?.click()}>
                  <Plus class="h-5 w-5 mr-2 group-hover:rotate-90 transition-transform duration-300" />
                  {$t('upload.addFiles')}
                </button>
              </div>
              
              <!-- Supported formats hint -->
              <p class="text-xs text-muted-foreground/75 mt-4">
                Supports images, videos, audio, documents, code files and more
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
            <button class="inline-flex items-center justify-center h-9 rounded-md px-3 text-sm font-medium bg-primary text-primary-foreground hover:bg-primary/90" on:click={() => { console.log('fileInput:', fileInput); fileInput?.click(); }}>
              <Plus class="h-4 w-4 mr-2" />
              {$t('upload.addMoreFiles')}
            </button>
          </div>
        </div>
      {/if}
      
      <!-- File List -->
      {#if $files.filter(f => f.status === 'seeding' || f.status === 'uploaded').length > 0}
        <div class="space-y-3">
          {#each $files.filter(f => f.status === 'seeding' || f.status === 'uploaded') as file}
            <div class="group relative overflow-hidden bg-gradient-to-r from-card to-card/80 border border-border/50 rounded-xl p-4 hover:shadow-lg hover:border-border transition-all duration-300 hover:scale-[1.01]">
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
                  
                  <div class="relative inline-block">
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
                    {#if showCopied && copiedHash === file.hash}
                      <div class="absolute left-1/2 -translate-x-1/2 bottom-full mb-2 px-3 py-1.5 rounded-lg bg-primary text-primary-foreground text-xs shadow-lg z-20 whitespace-nowrap animate-in fade-in slide-in-from-bottom-1 duration-200">
                        ✓ {$t('upload.hashCopied')}
                      </div>
                    {/if}
                  </div>
                  
                  <button
                    on:click={() => removeFile(file.id)}
                    class="group/btn p-2 hover:bg-destructive/10 rounded-lg transition-all duration-200 hover:scale-110"
                    title={$t('upload.stopSharing')}
                    aria-label="Stop sharing file"
                  >
                    <X class="h-4 w-4 text-muted-foreground group-hover/btn:text-destructive transition-colors" />
                  </button>
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
