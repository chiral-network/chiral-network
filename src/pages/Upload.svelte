<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import { File, X, Plus, FolderOpen } from 'lucide-svelte'
  import { files } from '$lib/stores'
  
  let isDragging = false
  let dragCounter = 0
  let fileInput: HTMLInputElement

  interface FileItem {
    id: string
    name: string
    hash: string
    size: number
    status: 'queued' | 'scheduled' | 'uploading' | 'seeding' | 'uploaded' | 'failed'
    seeders: number
    leechers: number
    uploadDate: Date
    progress?: number
    scheduledTime?: Date
  }

  let uploadQueue: FileItem[] = []
  const MAX_CONCURRENT_UPLOADS = 2
  let activeUploads = 0
  let schedulerInterval: NodeJS.Timer

  function startScheduler() {
    if (!schedulerInterval) {
      schedulerInterval = setInterval(() => {
        const now = new Date()
        files.update(f => {
          const ready = f.filter(fl => fl.status === 'scheduled' && fl.scheduledTime! <= now)
          ready.forEach(fl => {
            fl.status = 'queued'
            uploadQueue.push(fl)
          })
          if (ready.length > 0) processQueue()
          return f
        })
      }, 1000)
    }
  }

  startScheduler() // start on component init

  function scheduleFileUpload(file: FileItem, time: Date) {
    files.update(f => f.map(fl => fl.id === file.id ? { ...fl, status: 'scheduled', scheduledTime: time } : fl))
  }
  
  function processQueue() {
    while (activeUploads < MAX_CONCURRENT_UPLOADS && uploadQueue.length > 0) {
      const nextFile = uploadQueue.shift()!
      uploadFile(nextFile)
    }
  }

  function uploadFile(file: FileItem) {
    activeUploads++
    files.update(f => f.map(fl => fl.id === file.id ? { ...fl, status: 'uploading', progress: 0 } : fl))

    const interval = setInterval(() => {
      files.update(f => f.map(fl => {
        if (fl.id === file.id) {
          const nextProgress = Math.min((fl.progress ?? 0) + 10, 100)
          return { ...fl, progress: nextProgress }
        }
        return fl
      }))
    }, 300)

    setTimeout(() => {
      clearInterval(interval)
      files.update(f => f.map(fl => fl.id === file.id ? { ...fl, status: 'seeding', progress: 100 } : fl))
      activeUploads--
      processQueue()
    }, 500)
  }

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

  function addFiles(filesToAdd: any[]) {
  const newFiles = filesToAdd.map((file, i) => ({
    id: `file-${Date.now()}-${i}`,
    name: file.name,
    hash: `Qm${Math.random().toString(36).substring(2, 15)}${Math.random().toString(36).substring(2, 15)}`,
    size: file.size,
    status: 'queued' as const, 
    seeders: 1,
    leechers: 0,
    uploadDate: new Date()
  }))

  files.update(f => [...f, ...newFiles])
}
  
  function formatFileSize(bytes: number): string {
    if (bytes < 1024) return bytes + ' B'
    if (bytes < 1048576) return (bytes / 1024).toFixed(2) + ' KB'
    return (bytes / 1048576).toFixed(2) + ' MB'
  }

  // Hash copied popup state
  import { tick } from 'svelte';
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
    <h1 class="text-3xl font-bold">Upload Files</h1>
    <p class="text-muted-foreground mt-2">Share your files on the Chiral Network</p>
  </div>
  
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
        <div class="text-center py-8 border-2 border-dashed border-muted-foreground/25 rounded-lg bg-muted/10">
          <FolderOpen class="h-12 w-12 mx-auto text-muted-foreground mb-3" />
          <h3 class="text-lg font-semibold text-muted-foreground mb-2">Drop files here to share</h3>
          <p class="text-sm text-muted-foreground mb-4">Drag and drop files anywhere on this card, or click "Add Files" below</p>
          <div class="flex justify-center gap-2">
            <button class="inline-flex items-center justify-center h-9 rounded-md px-3 text-sm font-medium bg-primary text-primary-foreground hover:bg-primary/90" on:click={() => { console.log('fileInput:', fileInput); fileInput?.click(); }}>
              <Plus class="h-4 w-4 mr-2" />
              Add Files
            </button>
          </div>
        </div>
      {:else}
        <div class="flex flex-wrap items-center justify-between gap-4 mb-4">
          <div>
            <h2 class="text-lg font-semibold">Shared Files</h2>
            <p class="text-sm text-muted-foreground mt-1">
              {$files.filter(f => f.status === 'seeding' || f.status === 'uploaded').length} files •
              {formatFileSize($files.filter(f => f.status === 'seeding' || f.status === 'uploaded').reduce((sum, f) => sum + f.size, 0))} total
            </p>
            <p class="text-xs text-muted-foreground mt-1">💡 Tip: You can also drag and drop files here to add them</p>
          </div>
          <div class="flex gap-2">
            <button class="inline-flex items-center justify-center h-9 rounded-md px-3 text-sm font-medium bg-primary text-primary-foreground hover:bg-primary/90" on:click={() => { console.log('fileInput:', fileInput); fileInput?.click(); }}>
              <Plus class="h-4 w-4 mr-2" />
              Add More Files
            </button>
          </div>
        </div>
      {/if}

     {#if $files.filter(f => ['queued','scheduled','uploading','seeding','uploaded'].includes(f.status)).length > 0}
      <div class="space-y-2">
        {#each $files.filter(f => ['queued','scheduled','uploading','seeding','uploaded'].includes(f.status)) as file}
          <div class="flex flex-wrap items-center justify-between gap-2 p-3 bg-secondary rounded-lg hover:bg-secondary/80 transition-colors">
            <div class="flex items-center gap-3 min-w-0">
              <File class="h-4 w-4 text-muted-foreground" />
              <div class="flex-1 min-w-0">
                <p class="text-sm font-medium truncate">{file.name}</p>
                <div class="flex flex-wrap items-center gap-x-3 gap-y-1 mt-1">
                  <p class="text-xs text-muted-foreground truncate">Hash: {file.hash}</p>
                  <span class="text-xs text-muted-foreground">•</span>
                  <p class="text-xs text-muted-foreground truncate">{formatFileSize(file.size)}</p>
                  {#if file.seeders !== undefined}
                    <span class="text-xs text-muted-foreground">•</span>
                    <p class="text-xs text-green-600">{file.seeders || 1} seeder{(file.seeders || 1) !== 1 ? 's' : ''}</p>
                  {/if}
                  {#if file.leechers && file.leechers > 0}
                    <span class="text-xs text-muted-foreground">•</span>
                    <p class="text-xs text-orange-600">{file.leechers} leecher{file.leechers !== 1 ? 's' : ''}</p>
                  {/if}
                </div>

                {#if file.status === 'scheduled'}
                  <p class="text-xs text-muted-foreground mt-1">
                    Scheduled: {file.scheduledTime?.toLocaleString()}
                  </p>
                {/if}

                {#if file.status === 'queued'}
                  <input type="datetime-local"
                        class="mt-1 text-xs p-1 border rounded"
                        on:change={(e) => scheduleFileUpload(file, new Date(e.target.value))} />
                {/if}

                {#if file.status === 'uploading'}
                  <div class="w-full bg-muted-foreground/20 rounded h-1 mt-1">
                    <div class="bg-primary h-1 rounded" style="width: {file.progress}%"></div>
                  </div>
                {/if}
              </div>
            </div>
            <div class="flex items-center gap-2">
              {#if file.status === 'seeding' || file.status === 'uploaded'}
                <Badge variant="secondary" class="text-green-600">Seeding</Badge>
              {:else if file.status === 'scheduled'}
                <Badge variant="secondary" class="text-blue-600">Scheduled</Badge>
              {:else if file.status === 'queued'}
                <Badge variant="secondary" class="text-yellow-600">Queued</Badge>
              {:else if file.status === 'uploading'}
                <Badge variant="secondary" class="text-purple-600">Uploading</Badge>
              {/if}

              <div class="relative inline-block">
                <button
                  on:click={() => handleCopy(file.hash)}
                  class="p-1 hover:bg-destructive/10 rounded transition-colors"
                  title="Copy hash"
                  aria-label="Copy file hash"
                >
                  <svg class="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                    <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2"
                          d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" />
                  </svg>
                </button>
                {#if showCopied && copiedHash === file.hash}
                  <div class="absolute left-1/2 -translate-x-1/2 bottom-full mb-2 px-2 py-1 rounded bg-primary text-primary-foreground text-xs shadow z-10 whitespace-nowrap">
                    Hash copied!
                  </div>
                {/if}
              </div>
              <button
                on:click={() => removeFile(file.id)}
                class="p-1 hover:bg-destructive/10 rounded transition-colors"
                title="Stop sharing"
                aria-label="Stop sharing file"
              >
                <X class="h-4 w-4" />
              </button>
            </div>
          </div>
        {/each}
      </div>
      {:else}
        <div class="text-center py-8">
          <FolderOpen class="h-12 w-12 mx-auto text-muted-foreground mb-3" />
          <p class="text-sm text-muted-foreground">No files shared yet</p>
          <p class="text-xs text-muted-foreground mt-1">Add files to start sharing on the network</p>
        </div>
      {/if}

    </div>
    </div>
  </Card>
</div>