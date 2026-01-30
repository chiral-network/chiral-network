<script lang="ts">
  import { onMount, onDestroy } from 'svelte'
  import Card from '$lib/components/ui/card.svelte'
  import Button from '$lib/components/ui/button.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import { RefreshCw, HardDrive, ChevronDown, ChevronUp, CheckCircle } from 'lucide-svelte'
  import ChunkRecoveryCard from './ChunkRecoveryCard.svelte'
  import CorruptionAlert from './CorruptionAlert.svelte'
  import {
    recoveryList,
    activeRecoveries,
    corruptionAlerts,
    scanIncomplete,
    verifyChunks,
    checkCorruption,
    removeState,
    createCoordinator,
    getProgress,
    clearAlert,
    type CorruptionReport
  } from '$lib/services/p2pChunkService'
  import { showToast } from '$lib/toast'

  export let dlDir = ''
  export let collapsed = false

  let loading = false
  let pollInterval: ReturnType<typeof setInterval> | null = null
  let activeSet = new Set<string>()

  $: recoveries = $recoveryList
  $: alerts = $corruptionAlerts
  $: progressMap = $activeRecoveries
  $: hasItems = recoveries.length > 0 || alerts.length > 0

  onMount(async () => {
    if (dlDir) {
      await refresh()
    }
  })

  onDestroy(() => {
    if (pollInterval) clearInterval(pollInterval)
  })

  async function refresh() {
    if (!dlDir) return
    loading = true
    try {
      await scanIncomplete(dlDir)
      for (const r of $recoveryList) {
        await checkCorruption(r.tmp_path)
      }
    } catch (e) {
      console.error('refresh failed:', e)
    }
    loading = false
  }

  async function resumeByPath(tmpPath: string) {
    try {
      const progress = await createCoordinator(tmpPath)
      if (progress) {
        activeSet.add(progress.merkle_root)
        activeSet = new Set(activeSet)
        startPolling(tmpPath, progress.merkle_root)
        showToast('Download resumed', 'success')
      }
    } catch (err) {
      showToast('Resume failed', 'error')
    }
  }

  async function handleResume(e: CustomEvent<{ tmpPath: string }>) {
    await resumeByPath(e.detail.tmpPath)
  }

  function handlePause(e: CustomEvent<{ tmpPath: string }>) {
    const recovery = recoveries.find(r => r.tmp_path === e.detail.tmpPath)
    if (recovery) {
      activeSet.delete(recovery.merkle_root)
      activeSet = new Set(activeSet)
    }
  }

  async function handleVerify(e: CustomEvent<{ tmpPath: string }>) {
    const { tmpPath } = e.detail
    try {
      const result = await verifyChunks(tmpPath)
      if (result.valid) {
        showToast('All chunks verified', 'success')
      } else {
        showToast(`${result.failed_chunks.length} chunks failed verification`, 'warning')
      }
      await refresh()
    } catch (err) {
      showToast('Verification failed', 'error')
    }
  }

  async function handleRemove(e: CustomEvent<{ tmpPath: string }>) {
    const { tmpPath } = e.detail
    try {
      await removeState(tmpPath)
      showToast('Download removed', 'info')
      await refresh()
    } catch (err) {
      showToast('Remove failed', 'error')
    }
  }

  async function handleFix(e: CustomEvent<{ tmpPath: string }>) {
    await resumeByPath(e.detail.tmpPath)
  }

  function handleAlertDismiss(e: CustomEvent<{ merkleRoot: string }>) {
    clearAlert(e.detail.merkleRoot)
  }

  async function handleAlertFix(e: CustomEvent<{ report: CorruptionReport }>) {
    const recovery = recoveries.find(r => r.merkle_root === e.detail.report.merkle_root)
    if (recovery) {
      await resumeByPath(recovery.tmp_path)
    }
    clearAlert(e.detail.report.merkle_root)
  }

  async function handleAlertRemove(e: CustomEvent<{ report: CorruptionReport }>) {
    const recovery = recoveries.find(r => r.merkle_root === e.detail.report.merkle_root)
    if (recovery) {
      await removeState(recovery.tmp_path)
    }
    clearAlert(e.detail.report.merkle_root)
    await refresh()
  }

  function startPolling(tmpPath: string, merkleRoot: string) {
    if (pollInterval) clearInterval(pollInterval)
    pollInterval = setInterval(async () => {
      if (!activeSet.has(merkleRoot)) {
        if (pollInterval) clearInterval(pollInterval)
        return
      }
      await getProgress(tmpPath)
      await refresh()
    }, 2000)
  }

  function toggle() {
    collapsed = !collapsed
  }
</script>

{#if hasItems || loading}
  <Card class="chunk-recovery-panel">
    <button class="panel-header" on:click={toggle}>
      <div class="title">
        <HardDrive size={16} />
        <span>Chunk Recovery</span>
        {#if recoveries.length > 0}
          <Badge variant="secondary">{recoveries.length}</Badge>
        {/if}
        {#if alerts.length > 0}
          <Badge variant="destructive">{alerts.length}</Badge>
        {/if}
      </div>
      <div class="actions">
        <Button size="sm" variant="ghost" on:click={(e) => { e.stopPropagation(); refresh() }} disabled={loading}>
          <RefreshCw size={14} class={loading ? 'spin' : ''} />
        </Button>
        {#if collapsed}
          <ChevronDown size={16} />
        {:else}
          <ChevronUp size={16} />
        {/if}
      </div>
    </button>

    {#if !collapsed}
      <div class="panel-content">
        {#if alerts.length > 0}
          <div class="alerts-section">
            {#each alerts as alert (alert.merkle_root)}
              <CorruptionAlert
                report={alert}
                on:dismiss={handleAlertDismiss}
                on:fix={handleAlertFix}
                on:remove={handleAlertRemove}
              />
            {/each}
          </div>
        {/if}

        {#if recoveries.length > 0}
          <div class="recoveries-section">
            {#each recoveries as recovery (recovery.merkle_root)}
              <ChunkRecoveryCard
                {recovery}
                progress={progressMap.get(recovery.merkle_root) || null}
                isActive={activeSet.has(recovery.merkle_root)}
                on:resume={handleResume}
                on:pause={handlePause}
                on:verify={handleVerify}
                on:remove={handleRemove}
                on:fix={handleFix}
              />
            {/each}
          </div>
        {:else if !loading}
          <div class="empty">
            <CheckCircle size={24} />
            <span>No incomplete downloads</span>
          </div>
        {/if}

        {#if loading}
          <div class="loading">
            <RefreshCw size={20} class="spin" />
            <span>Scanning...</span>
          </div>
        {/if}
      </div>
    {/if}
  </Card>
{/if}

<style>
  :global(.chunk-recovery-panel) {
    margin-bottom: 16px;
  }

  .panel-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    width: 100%;
    padding: 12px 16px;
    background: none;
    border: none;
    cursor: pointer;
    text-align: left;
  }

  .panel-header:hover {
    background: var(--muted);
  }

  .title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-weight: 500;
    font-size: 14px;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .panel-content {
    padding: 0 16px 16px;
  }

  .alerts-section {
    margin-bottom: 12px;
  }

  .recoveries-section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 24px;
    color: var(--muted-foreground);
  }

  .loading {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 24px;
    color: var(--muted-foreground);
  }

  :global(.spin) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }
</style>
