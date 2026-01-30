<script lang="ts">
  import { createEventDispatcher } from 'svelte'
  import Progress from '$lib/components/ui/progress.svelte'
  import Button from '$lib/components/ui/button.svelte'
  import Badge from '$lib/components/ui/badge.svelte'
  import { RefreshCw, AlertTriangle, CheckCircle, Trash2, Play, Pause, Users } from 'lucide-svelte'
  import type { RecoveryInfo, CoordinatorProgress } from '$lib/services/p2pChunkService'
  import { toHumanReadableSize } from '$lib/utils'

  export let recovery: RecoveryInfo
  export let progress: CoordinatorProgress | null = null
  export let isActive = false

  const dispatch = createEventDispatcher()

  $: pct = recovery.total_chunks > 0
    ? ((recovery.verified + recovery.downloaded) / recovery.total_chunks) * 100
    : 0

  $: hasCorruption = recovery.failed > 0
  $: isComplete = recovery.verified === recovery.total_chunks && recovery.total_chunks > 0

  function resume() {
    dispatch('resume', { tmpPath: recovery.tmp_path })
  }

  function pause() {
    dispatch('pause', { tmpPath: recovery.tmp_path })
  }

  function verify() {
    dispatch('verify', { tmpPath: recovery.tmp_path })
  }

  function remove() {
    dispatch('remove', { tmpPath: recovery.tmp_path })
  }

  function fixCorruption() {
    dispatch('fix', { tmpPath: recovery.tmp_path })
  }
</script>

<div class="chunk-recovery-card" class:active={isActive} class:corrupted={hasCorruption} class:complete={isComplete}>
  <div class="header">
    <div class="file-info">
      <span class="file-name" title={recovery.file_name}>{recovery.file_name}</span>
      <span class="file-size">{toHumanReadableSize(recovery.file_size)}</span>
    </div>
    <div class="badges">
      {#if isComplete}
        <Badge variant="outline" class="text-green-600 border-green-600"><CheckCircle size={12} /> Complete</Badge>
      {:else if hasCorruption}
        <Badge variant="destructive"><AlertTriangle size={12} /> Corrupted</Badge>
      {:else if isActive}
        <Badge variant="default"><RefreshCw size={12} class="spin" /> Recovering</Badge>
      {:else}
        <Badge variant="secondary">Paused</Badge>
      {/if}
    </div>
  </div>

  <div class="progress-section">
    <div class="progress-bar">
      <Progress value={pct} class="h-2" />
    </div>
    <div class="progress-stats">
      <span class="stat">
        <span class="label">Chunks:</span>
        <span class="value">{recovery.verified + recovery.downloaded}/{recovery.total_chunks}</span>
      </span>
      <span class="stat">
        <span class="label">Verified:</span>
        <span class="value verified">{recovery.verified}</span>
      </span>
      {#if recovery.failed > 0}
        <span class="stat">
          <span class="label">Failed:</span>
          <span class="value failed">{recovery.failed}</span>
        </span>
      {/if}
    </div>
  </div>

  {#if progress}
    <div class="coordinator-info">
      <div class="peer-info">
        <Users size={14} />
        <span>{progress.active_peers} peers</span>
      </div>
      {#if progress.avg_speed > 0}
        <span class="speed">{toHumanReadableSize(progress.avg_speed)}/s</span>
      {/if}
      {#if progress.eta_secs > 0}
        <span class="eta">{Math.floor(progress.eta_secs / 60)}m {progress.eta_secs % 60}s</span>
      {/if}
    </div>
  {/if}

  {#if recovery.providers.length > 0}
    <div class="providers">
      <span class="label">Providers:</span>
      <span class="cnt">{recovery.providers.length}</span>
    </div>
  {/if}

  <div class="actions">
    {#if isComplete}
      <Button size="sm" variant="outline" on:click={remove}>
        <Trash2 size={14} />
        Clear
      </Button>
    {:else if hasCorruption}
      <Button size="sm" variant="destructive" on:click={fixCorruption}>
        <RefreshCw size={14} />
        Fix
      </Button>
      <Button size="sm" variant="outline" on:click={remove}>
        <Trash2 size={14} />
      </Button>
    {:else if isActive}
      <Button size="sm" variant="outline" on:click={pause}>
        <Pause size={14} />
        Pause
      </Button>
      <Button size="sm" variant="outline" on:click={verify}>
        <CheckCircle size={14} />
        Verify
      </Button>
    {:else}
      <Button size="sm" variant="default" on:click={resume}>
        <Play size={14} />
        Resume
      </Button>
      <Button size="sm" variant="outline" on:click={verify}>
        <CheckCircle size={14} />
        Verify
      </Button>
      <Button size="sm" variant="ghost" on:click={remove}>
        <Trash2 size={14} />
      </Button>
    {/if}
  </div>
</div>

<style>
  .chunk-recovery-card {
    background: var(--card);
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: 12px;
    margin-bottom: 8px;
  }

  .chunk-recovery-card.active {
    border-color: var(--primary);
  }

  .chunk-recovery-card.corrupted {
    border-color: var(--destructive);
    background: rgba(239, 68, 68, 0.05);
  }

  .chunk-recovery-card.complete {
    border-color: var(--success, #22c55e);
    background: rgba(34, 197, 94, 0.05);
  }

  .header {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    margin-bottom: 8px;
  }

  .file-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }

  .file-name {
    font-weight: 500;
    font-size: 14px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .file-size {
    font-size: 12px;
    color: var(--muted-foreground);
  }

  .badges {
    display: flex;
    gap: 4px;
    flex-shrink: 0;
  }

  .progress-section {
    margin-bottom: 8px;
  }

  .progress-bar {
    margin-bottom: 4px;
  }

  .progress-stats {
    display: flex;
    gap: 12px;
    font-size: 11px;
  }

  .stat {
    display: flex;
    gap: 4px;
  }

  .stat .label {
    color: var(--muted-foreground);
  }

  .stat .value {
    font-weight: 500;
  }

  .stat .value.verified {
    color: var(--success, #22c55e);
  }

  .stat .value.failed {
    color: var(--destructive);
  }

  .coordinator-info {
    display: flex;
    gap: 12px;
    font-size: 12px;
    color: var(--muted-foreground);
    margin-bottom: 8px;
    padding: 6px 8px;
    background: var(--muted);
    border-radius: 4px;
  }

  .peer-info {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .speed {
    color: var(--primary);
    font-weight: 500;
  }

  .providers {
    display: flex;
    gap: 4px;
    font-size: 11px;
    color: var(--muted-foreground);
    margin-bottom: 8px;
  }

  .providers .cnt {
    font-weight: 500;
  }

  .actions {
    display: flex;
    gap: 6px;
    justify-content: flex-end;
  }

  :global(.spin) {
    animation: spin 1s linear infinite;
  }

  @keyframes spin {
    from { transform: rotate(0deg); }
    to { transform: rotate(360deg); }
  }
</style>
