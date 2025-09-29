<script lang="ts">
  import Card from '$lib/components/ui/card.svelte';
  import Input from '$lib/components/ui/input.svelte';
  import Label from '$lib/components/ui/label.svelte';
  import Button from '$lib/components/ui/button.svelte';
  import { Search, X, RefreshCcw } from 'lucide-svelte';
  import { createEventDispatcher, onDestroy } from 'svelte';
  import { get } from 'svelte/store';
  import { t } from 'svelte-i18n';
  import { dhtService } from '$lib/dht';
  import type { FileMetadata } from '$lib/dht';
  import SearchResultCard from './SearchResultCard.svelte';
  import SearchHistoryPanel from './SearchHistoryPanel.svelte';
  import { dhtSearchHistory, type SearchHistoryEntry, type SearchStatus } from '$lib/stores/searchHistory';

  type ToastType = 'success' | 'error' | 'info' | 'warning';
  type ToastPayload = { message: string; type?: ToastType; duration?: number };

  const dispatch = createEventDispatcher<{ download: FileMetadata; message: ToastPayload }>();
  const tr = (key: string, params?: Record<string, unknown>) => get(t)(key, params);

  const SEARCH_TIMEOUT_MS = 12_000;

  let searchHash = '';
  let isSearching = false;
  let hasSearched = false;
  let latestStatus: SearchStatus = 'pending';
  let latestMetadata: FileMetadata | null = null;
  let searchError: string | null = null;
  let lastSearchDuration = 0;
  let historyEntries: SearchHistoryEntry[] = [];
  let activeHistoryId: string | null = null;

  const unsubscribe = dhtSearchHistory.subscribe((entries) => {
    historyEntries = entries;
    if (!activeHistoryId && entries.length > 0) {
      activeHistoryId = entries[0].id;
      latestStatus = entries[0].status;
      latestMetadata = entries[0].metadata ?? null;
      searchError = entries[0].errorMessage ?? null;
      hasSearched = entries.length > 0;
    }
  });

  onDestroy(() => {
    unsubscribe();
  });

  function pushMessage(message: string, type: ToastType = 'info', duration = 4000) {
    dispatch('message', { message, type, duration });
  }

  function clearSearch() {
    searchHash = '';
  }

  function hydrateFromEntry(entry: SearchHistoryEntry | undefined) {
    if (!entry) {
      latestStatus = 'pending';
      latestMetadata = null;
      searchError = null;
      return;
    }

    latestStatus = entry.status;
    latestMetadata = entry.metadata ?? null;
    searchError = entry.errorMessage ?? null;
    hasSearched = true;
    searchHash = entry.hash;
    lastSearchDuration = entry.elapsedMs ?? 0;
  }

  async function searchForFile() {
    const trimmed = searchHash.trim();
    if (!trimmed) {
      pushMessage(tr('download.notifications.enterHash'), 'warning');
      return;
    }

    isSearching = true;
    hasSearched = true;
    latestMetadata = null;
    latestStatus = 'pending';
    searchError = null;

    const entry = dhtSearchHistory.addPending(trimmed);
    activeHistoryId = entry.id;
    const startedAt = performance.now();

    try {
      pushMessage(tr('download.search.status.started'), 'info', 2000);
      const metadata = await dhtService.searchFileMetadata(trimmed, SEARCH_TIMEOUT_MS);
      const elapsed = Math.round(performance.now() - startedAt);
      lastSearchDuration = elapsed;

      if (metadata) {
        latestMetadata = metadata;
        latestStatus = 'found';
        dhtSearchHistory.updateEntry(entry.id, {
          status: 'found',
          metadata,
          elapsedMs: elapsed,
        });
        pushMessage(
          tr('download.search.status.foundNotification', { values: { name: metadata.fileName } }),
          'success',
        );
      } else {
        latestStatus = 'not_found';
        dhtSearchHistory.updateEntry(entry.id, {
          status: 'not_found',
          metadata: undefined,
          errorMessage: undefined,
          elapsedMs: elapsed,
        });
        pushMessage(tr('download.search.status.notFoundNotification'), 'warning', 6000);
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : tr('download.search.status.unknownError');
      const elapsed = Math.round(performance.now() - startedAt);
      lastSearchDuration = elapsed;
      latestStatus = 'error';
      searchError = message;
      dhtSearchHistory.updateEntry(entry.id, {
        status: 'error',
        errorMessage: message,
        elapsedMs: elapsed,
      });
      console.error('Search failed:', error);
      pushMessage(`${tr('download.search.status.errorNotification')}: ${message}`, 'error', 6000);
    } finally {
      isSearching = false;
    }
  }

  function handleHistorySelection(id: string) {
    activeHistoryId = id;
    const entry = historyEntries.find((item) => item.id === id);
    hydrateFromEntry(entry);
  }

  function clearHistory() {
    dhtSearchHistory.clear();
    historyEntries = [];
    activeHistoryId = null;
    latestMetadata = null;
    latestStatus = 'pending';
    searchError = null;
    hasSearched = false;
  }

  async function retryActiveSearch() {
    if (!activeHistoryId) return;
    const entry = historyEntries.find((item) => item.id === activeHistoryId);
    if (!entry) return;
    searchHash = entry.hash;
    await searchForFile();
  }

  function handleDownload(event: CustomEvent<FileMetadata>) {
    dispatch('download', event.detail);
  }

  function handleCopy(event: CustomEvent<string>) {
    pushMessage(
      tr('download.search.notifications.copied', { values: { value: event.detail } }),
      'info',
      2000,
    );
  }
</script>

<Card class="p-6">
  <div class="space-y-4">
    <div>
      <Label for="hash-input" class="text-base font-medium">{tr('download.addNew')}</Label>
      <p class="text-sm text-muted-foreground mt-1 mb-3">
        {tr('download.addNewSubtitle')}
      </p>
      <div class="flex flex-col sm:flex-row gap-3">
        <div class="relative flex-1">
          <Input
            id="hash-input"
            bind:value={searchHash}
            placeholder={tr('download.placeholder')}
            class="pr-10 h-10"
          />
          {#if searchHash}
            <button
              on:click={clearSearch}
              class="absolute right-2 top-1/2 transform -translate-y-1/2 p-1 hover:bg-muted rounded-full transition-colors"
              type="button"
              aria-label={tr('download.clearInput')}
            >
              <X class="h-4 w-4 text-muted-foreground hover:text-foreground" />
            </button>
          {/if}
        </div>
        <Button
          on:click={searchForFile}
          disabled={!searchHash.trim() || isSearching}
          class="h-10 px-6"
        >
          <Search class="h-4 w-4 mr-2" />
          {isSearching ? tr('download.search.status.searching') : tr('download.search.button')}
        </Button>
      </div>
    </div>

    {#if hasSearched}
      <div class="pt-6 border-t">
        <div class="grid gap-4 lg:grid-cols-[minmax(0,2fr)_minmax(0,1fr)]">
          <div class="space-y-4">
            {#if isSearching}
              <div class="rounded-md border border-dashed border-muted p-5 text-sm text-muted-foreground text-center">
                {tr('download.search.status.searching')}
              </div>
            {:else if latestStatus === 'found' && latestMetadata}
              <SearchResultCard
                metadata={latestMetadata}
                on:download={handleDownload}
                on:copy={handleCopy}
              />
              <p class="text-xs text-muted-foreground">
                {tr('download.search.status.completedIn', { values: { seconds: (lastSearchDuration / 1000).toFixed(1) } })}
              </p>
            {:else if latestStatus === 'not_found'}
              <div class="rounded-md border border-amber-300/70 bg-amber-100/50 dark:bg-amber-900/20 p-5 text-sm text-amber-700 dark:text-amber-300">
                {tr('download.search.status.notFoundDetail')}
              </div>
            {:else if latestStatus === 'error'}
              <div class="rounded-md border border-destructive/40 bg-destructive/10 p-5 text-sm text-destructive">
                <p class="font-medium mb-1">{tr('download.search.status.errorHeadline')}</p>
                <p>{searchError}</p>
              </div>
            {:else}
              <div class="rounded-md border border-dashed border-muted p-5 text-sm text-muted-foreground text-center">
                {tr('download.search.status.placeholder')}
              </div>
            {/if}

            <div class="flex items-center gap-2">
              <Button
                variant="outline"
                size="sm"
                on:click={retryActiveSearch}
                disabled={isSearching || !activeHistoryId}
                class="h-8 px-3"
              >
                <RefreshCcw class="h-3.5 w-3.5 mr-2" />
                {tr('download.search.history.retry')}
              </Button>
            </div>
          </div>

          <SearchHistoryPanel
            entries={historyEntries}
            {isSearching}
            activeId={activeHistoryId}
            on:select={(event) => handleHistorySelection(event.detail)}
            on:clear={clearHistory}
          />
        </div>
      </div>
    {/if}
  </div>
</Card>
