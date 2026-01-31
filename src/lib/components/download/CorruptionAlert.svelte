<script lang="ts">
  import { createEventDispatcher } from 'svelte'
  import Button from '$lib/components/ui/button.svelte'
  import { AlertTriangle, X, RefreshCw, Trash2 } from 'lucide-svelte'
  import type { CorruptionReport } from '$lib/services/p2pChunkService'

  export let report: CorruptionReport

  const dispatch = createEventDispatcher()

  $: totalBad = report.corrupted_chunks.length + report.missing_chunks.length
  $: pctBad = report.total_chunks > 0 ? (totalBad / report.total_chunks) * 100 : 0

  function dismiss() {
    dispatch('dismiss', { merkleRoot: report.merkle_root })
  }

  function fix() {
    dispatch('fix', { report })
  }

  function remove() {
    dispatch('remove', { report })
  }
</script>

<div class="corruption-alert">
  <div class="icon">
    <AlertTriangle size={20} />
  </div>

  <div class="content">
    <div class="title">Corruption Detected</div>
    <div class="details">
      {#if report.corrupted_chunks.length > 0}
        <span class="corrupted">{report.corrupted_chunks.length} corrupted</span>
      {/if}
      {#if report.missing_chunks.length > 0}
        <span class="missing">{report.missing_chunks.length} missing</span>
      {/if}
      <span class="total">of {report.total_chunks} chunks ({pctBad.toFixed(1)}%)</span>
    </div>
    <div class="hash" title={report.merkle_root}>
      {report.merkle_root.slice(0, 16)}...
    </div>
  </div>

  <div class="actions">
    <Button size="sm" variant="destructive" on:click={fix}>
      <RefreshCw size={14} />
      Re-download
    </Button>
    <Button size="sm" variant="ghost" on:click={remove}>
      <Trash2 size={14} />
    </Button>
    <button class="dismiss" on:click={dismiss}>
      <X size={16} />
    </button>
  </div>
</div>

<style>
  .corruption-alert {
    display: flex;
    align-items: flex-start;
    gap: 12px;
    padding: 12px 16px;
    background: rgba(239, 68, 68, 0.1);
    border: 1px solid var(--destructive);
    border-radius: 8px;
    margin-bottom: 12px;
  }

  .icon {
    color: var(--destructive);
    flex-shrink: 0;
    padding-top: 2px;
  }

  .content {
    flex: 1;
    min-width: 0;
  }

  .title {
    font-weight: 600;
    font-size: 14px;
    color: var(--destructive);
    margin-bottom: 4px;
  }

  .details {
    display: flex;
    gap: 8px;
    font-size: 12px;
    margin-bottom: 4px;
  }

  .corrupted {
    color: var(--destructive);
    font-weight: 500;
  }

  .missing {
    color: var(--warning, #f59e0b);
    font-weight: 500;
  }

  .total {
    color: var(--muted-foreground);
  }

  .hash {
    font-family: monospace;
    font-size: 11px;
    color: var(--muted-foreground);
  }

  .actions {
    display: flex;
    gap: 6px;
    align-items: flex-start;
    flex-shrink: 0;
  }

  .dismiss {
    background: none;
    border: none;
    padding: 4px;
    cursor: pointer;
    color: var(--muted-foreground);
    border-radius: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .dismiss:hover {
    background: var(--muted);
    color: var(--foreground);
  }
</style>
