<script lang="ts">
  import { onMount } from 'svelte'
  import { invoke } from '@tauri-apps/api/core'
  import { listen } from '@tauri-apps/api/event'
  import Card from '$lib/components/ui/card.svelte'
  import Button from '$lib/components/ui/button.svelte'
  import { Download, AlertCircle } from 'lucide-svelte'
  
  export let onComplete: () => void = () => {}
  
  let isChecking = true
  let isInstalled = false
  let isDownloading = false
  let downloadProgress = {
    downloaded: 0,
    total: 0,
    percentage: 0,
    status: ''
  }
  let error = ''
  
  onMount(() => {
    checkGethBinary()
    
    // Listen for download progress updates
    let unlisten: (() => void) | null = null
    
    listen('geth-download-progress', (event) => {
      downloadProgress = event.payload as typeof downloadProgress
    }).then((unlistenFn) => {
      unlisten = unlistenFn
    })
    
    return () => {
      if (unlisten) {
        unlisten()
      }
    }
  })
  
  async function checkGethBinary() {
    isChecking = true
    error = ''
    try {
      isInstalled = await invoke('check_geth_binary') as boolean
      if (isInstalled) {
        onComplete()
      }
    } catch (e) {
      error = String(e)
    } finally {
      isChecking = false
    }
  }
  
  async function downloadGeth() {
    isDownloading = true
    error = ''
    downloadProgress = {
      downloaded: 0,
      total: 0,
      percentage: 0,
      status: 'Starting download...'
    }
    
    try {
      await invoke('download_geth_binary')
      isInstalled = true
      onComplete()
    } catch (e) {
      error = String(e)
    } finally {
      isDownloading = false
    }
  }
</script>

{#if !isInstalled}
  <div class="fixed inset-0 bg-background/80 backdrop-blur-sm z-50 flex items-center justify-center p-4">
    <Card class="w-full max-w-md p-6">
      <div class="space-y-4">
        <div class="text-center">
          <h2 class="text-2xl font-bold mb-2">Chiral Network Setup</h2>
          <p class="text-muted-foreground">
            {#if isChecking}
              Checking for Chiral Network node...
            {:else if error}
              Setup failed
            {:else if isDownloading}
              Downloading Chiral Network node...
            {:else}
              Chiral Network node not found
            {/if}
          </p>
        </div>
        
        {#if error}
          <div class="bg-red-500/10 border border-red-500/20 rounded-lg p-3">
            <div class="flex items-center gap-2">
              <AlertCircle class="h-5 w-5 text-red-500 flex-shrink-0" />
              <p class="text-sm text-red-500">{error}</p>
            </div>
          </div>
        {/if}
        
        {#if isDownloading}
          <div class="space-y-2">
            <div class="flex justify-between text-sm">
              <span>{downloadProgress.status}</span>
              <span>{downloadProgress.percentage.toFixed(0)}%</span>
            </div>
            <div class="w-full bg-secondary rounded-full h-2 overflow-hidden">
              <div 
                class="bg-primary h-full transition-all duration-300"
                style="width: {downloadProgress.percentage}%"
              ></div>
            </div>
            {#if downloadProgress.total > 0}
              <p class="text-xs text-muted-foreground text-center">
                {(downloadProgress.downloaded / 1024 / 1024).toFixed(1)} MB / 
                {(downloadProgress.total / 1024 / 1024).toFixed(1)} MB
              </p>
            {/if}
          </div>
        {:else if !isChecking && !isInstalled}
          <div class="space-y-3">
            <p class="text-sm text-muted-foreground">
              The Chiral Network node (Core-Geth) needs to be downloaded to run the blockchain locally.
              This is a one-time setup (~50 MB).
            </p>
            <Button 
              class="w-full"
              on:click={downloadGeth}
              disabled={isDownloading}
            >
              <Download class="h-4 w-4 mr-2" />
              Download Chiral Node
            </Button>
          </div>
        {:else if isChecking}
          <div class="flex justify-center py-4">
            <div class="animate-spin rounded-full h-8 w-8 border-b-2 border-primary"></div>
          </div>
        {/if}
        
        {#if error && !isDownloading}
          <Button 
            variant="outline"
            class="w-full"
            on:click={checkGethBinary}
          >
            Retry
          </Button>
        {/if}
      </div>
    </Card>
  </div>
{/if}