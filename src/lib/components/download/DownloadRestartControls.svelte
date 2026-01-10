<script lang="ts">
  import { invoke } from '@tauri-apps/api/core'
  import { listen } from '@tauri-apps/api/event'
  import { onMount, onDestroy } from 'svelte'
  import Button from '$lib/components/ui/button.svelte'
  import Card from '$lib/components/ui/card.svelte'
  import Progress from '$lib/components/ui/progress.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import { Play, Pause, AlertCircle, CheckCircle, Download as DownloadIcon } from 'lucide-svelte'
  import { toHumanReadableSize } from '$lib/utils'

  export let downloadId: string = ''
  export let sourceUrl: string = ''
  export let destinationPath: string = ''
  export let expectedSha256: string | null = null

  // Download status from backend
  interface DownloadStatus {
    download_id: string
    state: string
    bytes_downloaded: number
    expected_size: number | null
    etag: string | null
    lease_exp: number | null
    last_error: string | null
  }

  let status: DownloadStatus | null = null
  let unlisten: (() => void) | null = null
  let isStarting = false
  let isPausing = false
  let isResuming = false

  // Banner messages for different restart scenarios
  $: restartBanner = getRestartBanner(status?.state, status?.last_error ?? null)

  function getRestartBanner(state: string | undefined, error: string | null): { type: 'info' | 'warning' | 'error' | null, message: string } | null {
    if (!state) return null

    // Check for restart scenarios based on error messages
    if (error) {
      if (error.includes('weak ETag') || error.includes('ETag changed')) {
        return {
          type: 'warning',
          message: 'Download restarting from beginning: Server returned weak ETag or ETag changed. Your progress has been saved but the file needs to be re-downloaded.'
        }
      }
      if (error.includes('range unsupported') || error.includes('Accept-Ranges')) {
        return {
          type: 'warning',
          message: 'Download restarting from beginning: Server does not support resumable downloads (Range requests unavailable).'
        }
      }
      if (error.includes('416') || error.includes('Range Not Satisfiable')) {
        return {
          type: 'warning',
          message: 'Download restarting from beginning: Saved offset exceeds file size (HTTP 416). The file may have been modified on the server.'
        }
      }
      if (error.includes('disk space') || error.includes('STORAGE_EXHAUSTED')) {
        return {
          type: 'error',
          message: 'Insufficient disk space. Please free up space and resume the download.'
        }
      }
    }

    // State-specific messages
    if (state === 'Restarting') {
      return {
        type: 'info',
        message: 'Download is restarting from the beginning due to server changes.'
      }
    }
    if (state === 'LeaseExpired') {
      return {
        type: 'info',
        message: 'Download lease expired. Requesting a new lease from the seeder...'
      }
    }

    return null
  }

  // Start a new download
  async function startDownload() {
    if (!sourceUrl || !destinationPath) {
      alert('Please provide source URL and destination path')
      return
    }

    isStarting = true
    try {
      const response = await invoke<string>('start_download_restart', {
        request: {
          download_id: downloadId || null,
          source_url: sourceUrl,
          destination_path: destinationPath,
          expected_sha256: expectedSha256 || null
        }
      })

      downloadId = response
      await fetchStatus()
    } catch (error) {
      console.error('Failed to start download:', error)
      alert(`Failed to start download: ${error}`)
    } finally {
      isStarting = false
    }
  }

  // Pause download
  async function pauseDownload() {
    if (!downloadId) return

    isPausing = true
    try {
      await invoke('pause_download_restart', { downloadId })
      await fetchStatus()
    } catch (error) {
      console.error('Failed to pause download:', error)
      alert(`Failed to pause download: ${error}`)
    } finally {
      isPausing = false
    }
  }

  // Resume download
  async function resumeDownload() {
    if (!downloadId) return

    isResuming = true
    try {
      await invoke('resume_download_restart', { downloadId })
      await fetchStatus()
    } catch (error) {
      console.error('Failed to resume download:', error)
      alert(`Failed to resume download: ${error}`)
    } finally {
      isResuming = false
    }
  }

  // Fetch current status
  async function fetchStatus() {
    if (!downloadId) return

    try {
      status = await invoke<DownloadStatus>('get_download_status_restart', { downloadId })
    } catch (error) {
      console.error('Failed to fetch status:', error)
    }
  }

  // Listen for download_status events
  onMount(async () => {
    const unlistenFn = await listen<DownloadStatus>('download_status', (event) => {
      if (event.payload.download_id === downloadId) {
        status = event.payload
      }
    })
    unlisten = unlistenFn

    // Fetch initial status if downloadId is provided
    if (downloadId) {
      await fetchStatus()
    }
  })

  onDestroy(() => {
    if (unlisten) {
      unlisten()
    }
  })

  // Calculate progress percentage
  $: progressPercentage = status?.expected_size
    ? Math.round((status.bytes_downloaded / status.expected_size) * 100)
    : 0


  // Human-readable state names
  const stateNames: Record<string, string> = {
    'Idle': 'Idle',
    'Handshake': 'Requesting lease',
    'HandshakeRetry': 'Retrying lease request',
    'LeaseRenewDue': 'Renewing lease',
    'PreparingHead': 'Fetching metadata',
    'HeadBackoff': 'Retrying metadata',
    'Restarting': 'Restarting download',
    'PreflightStorage': 'Checking disk space',
    'ValidatingMetadata': 'Validating resume data',
    'Downloading': 'Downloading',
    'PersistingProgress': 'Saving progress',
    'Paused': 'Paused',
    'AwaitingResume': 'Ready to resume',
    'LeaseExpired': 'Lease expired',
    'VerifyingSha': 'Verifying integrity',
    'FinalizingIo': 'Finalizing file',
    'Completed': 'Completed',
    'Failed': 'Failed',
  }

  // Determine which buttons to show
  $: canStart = !status || status.state === 'Idle' || status.state === 'Failed'
  $: canPause = status && (status.state === 'Downloading' || status.state === 'PersistingProgress')
  $: canResume = status && (status.state === 'Paused' || status.state === 'AwaitingResume')
</script>

<Card class="p-6 space-y-4">
  <div class="flex items-center justify-between">
    <h3 class="text-lg font-semibold flex items-center gap-2">
      <DownloadIcon class="h-5 w-5" />
      Download with Pause/Resume
    </h3>
    {#if status}
      <Badge variant={status.state === 'Completed' ? 'default' : status.state === 'Failed' ? 'destructive' : 'secondary'}>
        {stateNames[status.state] || status.state}
      </Badge>
    {/if}
  </div>

  <!-- Restart banner -->
  {#if restartBanner}
    <div class="p-4 rounded-lg border {restartBanner.type === 'error' ? 'bg-red-50 border-red-200 dark:bg-red-950 dark:border-red-800' : restartBanner.type === 'warning' ? 'bg-yellow-50 border-yellow-200 dark:bg-yellow-950 dark:border-yellow-800' : 'bg-blue-50 border-blue-200 dark:bg-blue-950 dark:border-blue-800'}">
      <div class="flex items-start gap-3">
        <AlertCircle class="h-5 w-5 {restartBanner.type === 'error' ? 'text-red-600 dark:text-red-400' : restartBanner.type === 'warning' ? 'text-yellow-600 dark:text-yellow-400' : 'text-blue-600 dark:text-blue-400'} flex-shrink-0 mt-0.5" />
        <p class="text-sm {restartBanner.type === 'error' ? 'text-red-800 dark:text-red-200' : restartBanner.type === 'warning' ? 'text-yellow-800 dark:text-yellow-200' : 'text-blue-800 dark:text-blue-200'}">
          {restartBanner.message}
        </p>
      </div>
    </div>
  {/if}

  <!-- Progress -->
  {#if status && status.expected_size}
    <div class="space-y-2">
      <div class="flex justify-between text-sm">
        <span class="text-muted-foreground">Progress</span>
        <span class="font-medium">{toHumanReadableSize(status.bytes_downloaded)} / {toHumanReadableSize(status.expected_size ?? 0)}</span>
      </div>
      <Progress value={progressPercentage} max={100} />
      <div class="flex justify-between text-xs text-muted-foreground">
        <span>{progressPercentage}%</span>
        {#if status.etag}
          <span>ETag: {status.etag.slice(0, 12)}...</span>
        {/if}
      </div>
    </div>
  {/if}

  <!-- Error message -->
  {#if status?.last_error && !restartBanner}
    <div class="p-3 rounded bg-red-50 dark:bg-red-950 border border-red-200 dark:border-red-800">
      <p class="text-sm text-red-800 dark:text-red-200">{status.last_error}</p>
    </div>
  {/if}

  <!-- Controls -->
  <div class="flex gap-2">
    {#if canStart}
      <Button onclick={startDownload} disabled={isStarting} class="flex items-center gap-2">
        <Play class="h-4 w-4" />
        {isStarting ? 'Starting...' : 'Start Download'}
      </Button>
    {/if}

    {#if canPause}
      <Button onclick={pauseDownload} disabled={isPausing} variant="secondary" class="flex items-center gap-2">
        <Pause class="h-4 w-4" />
        {isPausing ? 'Pausing...' : 'Pause'}
      </Button>
    {/if}

    {#if canResume}
      <Button onclick={resumeDownload} disabled={isResuming} class="flex items-center gap-2">
        <Play class="h-4 w-4" />
        {isResuming ? 'Resuming...' : 'Resume'}
      </Button>
    {/if}

    {#if status?.state === 'Completed'}
      <div class="flex items-center gap-2 text-green-600 dark:text-green-400">
        <CheckCircle class="h-5 w-5" />
        <span class="font-medium">Download Complete</span>
      </div>
    {/if}
  </div>

  <!-- Download info (for debugging) -->
  {#if status}
    <details class="text-xs text-muted-foreground">
      <summary class="cursor-pointer hover:text-foreground">Debug Info</summary>
      <pre class="mt-2 p-2 bg-muted rounded overflow-x-auto">{JSON.stringify(status, null, 2)}</pre>
    </details>
  {/if}
</Card>
