<script lang="ts">
  import Card from '$lib/components/ui/card.svelte';
  import Button from '$lib/components/ui/button.svelte';
  import Badge from '$lib/components/ui/badge.svelte';
  import { History, RotateCcw, Search, AlertCircle, CheckCircle2 } from 'lucide-svelte';
  import { createEventDispatcher } from 'svelte';
  import { get } from 'svelte/store';
  import { t } from 'svelte-i18n';
  import type { SearchHistoryEntry } from '$lib/stores/searchHistory';

  export let entries: SearchHistoryEntry[] = [];
  export let activeId: string | null = null;
  export let isSearching = false;

  const dispatch = createEventDispatcher<{ select: string; clear: void }>();

  const tr = (key: string, params?: Record<string, unknown>) => get(t)(key, params);

  function statusLabel(status: string) {
    switch (status) {
      case 'found':
        return tr('download.search.status.found');
      case 'not_found':
        return tr('download.search.status.notFound');
      case 'error':
        return tr('download.search.status.error');
      default:
        return tr('download.search.status.pending');
    }
  }

  function statusIcon(status: string) {
    switch (status) {
      case 'found':
        return CheckCircle2;
      case 'error':
        return AlertCircle;
      default:
        return Search;
    }
  }

  function statusClass(status: string) {
    switch (status) {
      case 'found':
        return 'text-emerald-600';
      case 'error':
        return 'text-red-600';
      case 'not_found':
        return 'text-amber-600';
      default:
        return 'text-muted-foreground';
    }
  }
</script>

<Card class="p-4 space-y-3 md:p-5">
  <div class="flex items-center justify-between gap-2">
    <div class="flex items-center gap-2">
      <History class="h-4 w-4 text-muted-foreground" />
      <h3 class="text-sm font-medium">{tr('download.search.history.title')}</h3>
    </div>
    <Button
      variant="ghost"
      size="sm"
      class="h-8 px-2 text-xs"
      on:click={() => dispatch('clear')}
      disabled={entries.length === 0}
    >
      <RotateCcw class="h-3.5 w-3.5 mr-1" />
      {tr('download.search.history.clear')}
    </Button>
  </div>

  {#if entries.length === 0}
    <div class="rounded-md border border-dashed border-muted p-4 text-center text-xs text-muted-foreground">
      {tr('download.search.history.empty')}
    </div>
  {:else}
    <div class="space-y-2 max-h-72 overflow-auto pr-1">
      {#each entries as entry}
        <button
          type="button"
          class={`w-full rounded-md border p-3 text-left transition-colors ${
            entry.id === activeId ? 'border-primary bg-primary/5' : 'border-border hover:bg-muted/60'
          }`}
          on:click={() => dispatch('select', entry.id)}
        >
          <div class="flex items-center justify-between">
            <div class="flex items-center gap-2 truncate">
              <span class="text-xs font-medium text-foreground truncate">{entry.hash}</span>
              <Badge variant="outline" class="text-[10px] uppercase tracking-wide">
                {statusLabel(entry.status)}
              </Badge>
            </div>
            <span class="text-[10px] text-muted-foreground">
              {entry.elapsedMs ? `${(entry.elapsedMs / 1000).toFixed(1)}s` : ''}
            </span>
          </div>

          {#if entry.metadata?.fileName}
            <p class="mt-1 text-xs text-muted-foreground truncate">{entry.metadata.fileName}</p>
          {/if}

          <div class="mt-2 flex items-center gap-2 text-xs text-muted-foreground">
            <svelte:component this={statusIcon(entry.status)} class={`h-3.5 w-3.5 ${statusClass(entry.status)}`} />
            {#if entry.errorMessage}
              <span class="truncate">{entry.errorMessage}</span>
            {:else if entry.status === 'found'}
              <span>{tr('download.search.history.foundSeeders', { values: { count: entry.metadata?.seeders?.length ?? 0 } })}</span>
            {:else if entry.status === 'not_found'}
              <span>{tr('download.search.history.notFound')}</span>
            {:else}
              <span>{tr('download.search.history.pending')}</span>
            {/if}
          </div>
        </button>
      {/each}
    </div>
  {/if}

  {#if isSearching}
    <p class="text-[10px] text-muted-foreground italic">{tr('download.search.status.searching')}</p>
  {/if}
</Card>
