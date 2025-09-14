<script lang="ts">
  import Card from '$lib/components/ui/card.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import { File, X, Plus, FolderOpen } from 'lucide-svelte'
  import { files } from '$lib/stores'
  import { t } from 'svelte-i18n';
  
  let isDragging = false
  let dragCounter = 0
  let fileInput: HTMLInputElement

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
    // Generate hashes and add to store
    const newFiles = filesToAdd.map((file, i) => ({
      id: `file-${Date.now()}-${i}`,
      name: file.name,
      hash: `Qm${Math.random().toString(36).substring(2, 15)}${Math.random().toString(36).substring(2, 15)}`,
      size: file.size,
      status: 'seeding' as const,
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
    <h1 class="text-3xl font-bold">{$t('upload.title')}</h1>
    <p class="text-muted-foreground mt-2">{$t('upload.subtitle')}</p>
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
          <h3 class="text-lg font-semibold text-muted-foreground mb-2">{$t('upload.dropFiles')}</h3>
          <p class="text-sm text-muted-foreground mb-4">{$t('upload.dropFilesHint')}</p>
          <div class="flex justify-center gap-2">
            <button class="inline-flex items-center justify-center h-9 rounded-md px-3 text-sm font-medium bg-primary text-primary-foreground hover:bg-primary/90" on:click={() => { console.log('fileInput:', fileInput); fileInput?.click(); }}>
              <Plus class="h-4 w-4 mr-2" />
              {$t('upload.addFiles')}
            </button>
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
        <div class="space-y-2">
          {#each $files.filter(f => f.status === 'seeding' || f.status === 'uploaded') as file}
            <div class="flex flex-wrap items-center justify-between gap-2 p-3 bg-secondary rounded-lg hover:bg-secondary/80 transition-colors">
              <div class="flex items-center gap-3 min-w-0">
                <File class="h-4 w-4 text-muted-foreground" />
                <div class="flex-1 min-w-0">
                  <p class="text-sm font-medium truncate">{file.name}</p>
                  <div class="flex flex-wrap items-center gap-x-3 gap-y-1 mt-1">
                    <p class="text-xs text-muted-foreground truncate">{$t('upload.hash')}: {file.hash}</p>
                    <span class="text-xs text-muted-foreground">•</span>
                    <p class="text-xs text-muted-foreground truncate">{formatFileSize(file.size)}</p>
                    {#if file.seeders !== undefined}
                      <span class="text-xs text-muted-foreground">•</span>
                      <p class="text-xs text-green-600">{file.seeders || 1} {$t('upload.seeder', { count: file.seeders || 1 })}</p>
                    {/if}
                    {#if file.leechers && file.leechers > 0}
                      <span class="text-xs text-muted-foreground">•</span>
                      <p class="text-xs text-orange-600">{file.leechers} {$t('upload.leecher', { count: file.leechers })}</p>
                    {/if}
                  </div>
                </div>
              </div>
              <div class="flex items-center gap-2">
                <Badge variant="secondary" class="text-green-600">
                  {$t('upload.seeding')}
                </Badge>
                <div class="relative inline-block">
                  <button
                    on:click={() => handleCopy(file.hash)}
                    class="p-1 hover:bg-destructive/10 rounded transition-colors"
                    title={$t('upload.copyHash')}
                    aria-label="Copy file hash"
                  >
                    <svg class="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                      <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d="M8 16H6a2 2 0 01-2-2V6a2 2 0 012-2h8a2 2 0 012 2v2m-6 12h8a2 2 0 002-2v-8a2 2 0 00-2-2h-8a2 2 0 00-2 2v8a2 2 0 002 2z" />
                    </svg>
                  </button>
                  {#if showCopied && copiedHash === file.hash}
                    <div class="absolute left-1/2 -translate-x-1/2 bottom-full mb-2 px-2 py-1 rounded bg-primary text-primary-foreground text-xs shadow z-10 whitespace-nowrap">
                      {$t('upload.hashCopied')}
                    </div>
                  {/if}
                </div>
                <button
                  on:click={() => removeFile(file.id)}
                  class="p-1 hover:bg-destructive/10 rounded transition-colors"
                  title={$t('upload.stopSharing')}
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
          <p class="text-sm text-muted-foreground">{$t('upload.noFilesShared')}</p>
          <p class="text-xs text-muted-foreground mt-1">{$t('upload.addFilesHint2')}</p>
        </div>
      {/if}

    </div>
    </div>
  </Card>
</div>
