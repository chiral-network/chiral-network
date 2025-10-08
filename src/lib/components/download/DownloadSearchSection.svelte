<script lang="ts">
  import Card from '$lib/components/ui/card.svelte';
  import Input from '$lib/components/ui/input.svelte';
  import Label from '$lib/components/ui/label.svelte';
  import Button from '$lib/components/ui/button.svelte';
  import Badge from '$lib/components/ui/badge.svelte';
  import { Search, X, History, RotateCcw, AlertCircle, CheckCircle2 } from 'lucide-svelte';
  import { createEventDispatcher, onDestroy, onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { t } from 'svelte-i18n';
  import { dhtService } from '$lib/dht';
  import type { FileMetadata } from '$lib/dht';
  import SearchResultCard from './SearchResultCard.svelte';
  import { dhtSearchHistory, type SearchHistoryEntry, type SearchStatus } from '$lib/stores/searchHistory';

  type ToastType = 'success' | 'error' | 'info' | 'warning';
  type ToastPayload = { message: string; type?: ToastType; duration?: number };

  const dispatch = createEventDispatcher<{ download: FileMetadata; message: ToastPayload }>();
  const tr = (key: string, params?: Record<string, unknown>) => (get(t) as any)(key, params);

  const SEARCH_TIMEOUT_MS = 12_000;

  let searchHash = '';
  let searchMode = 'hash'; // 'hash' or 'name'
  let isSearching = false;
  let hasSearched = false;
  let latestStatus: SearchStatus = 'pending';
  let latestMetadata: FileMetadata | null = null;
  let searchError: string | null = null;
  let lastSearchDuration = 0;
  let historyEntries: SearchHistoryEntry[] = [];
  let activeHistoryId: string | null = null;
  let versionResults: any[] = [];
  let showHistoryDropdown = false;

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

  onMount(() => {
    document.addEventListener('click', handleClickOutside);
  });

  onDestroy(() => {
    document.removeEventListener('click', handleClickOutside);
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
      pushMessage(searchMode === 'hash' ? tr('download.notifications.enterHash') : 'Please enter a file name', 'warning');
      return;
    }

    isSearching = true;
    hasSearched = true;
    latestMetadata = null;
    latestStatus = 'pending';
    searchError = null;
    versionResults = [];

    const startedAt = performance.now();

    try {
      if (searchMode === 'name') {
        // Search for file versions by name
        pushMessage('Searching for file versions...', 'info', 2000);
        
        // Import invoke function for backend calls
        const { invoke } = await import("@tauri-apps/api/core");
        
        const versions = await invoke('get_file_versions_by_name', { fileName: trimmed }) as any[];
        const elapsed = Math.round(performance.now() - startedAt);
        lastSearchDuration = elapsed;

        if (versions && versions.length > 0) {
          versionResults = versions.sort((a, b) => b.version - a.version); // Sort by version descending
          latestStatus = 'found';
          pushMessage(`Found ${versions.length} version(s) of "${trimmed}"`, 'success');
        } else {
          latestStatus = 'not_found';
          pushMessage(`No versions found for "${trimmed}"`, 'warning', 6000);
        }
      } else {
        // Original hash search
        const entry = dhtSearchHistory.addPending(trimmed);
        activeHistoryId = entry.id;
        
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
      }
    } catch (error) {
      const message = error instanceof Error ? error.message : tr('download.search.status.unknownError');
      const elapsed = Math.round(performance.now() - startedAt);
      lastSearchDuration = elapsed;
      latestStatus = 'error';
      searchError = message;
      
      if (searchMode === 'hash' && activeHistoryId) {
        dhtSearchHistory.updateEntry(activeHistoryId, {
          status: 'error',
          errorMessage: message,
          elapsedMs: elapsed,
        });
      }
      
      console.error('Search failed:', error);
      pushMessage(`${tr('download.search.status.errorNotification')}: ${message}`, 'error', 6000);
    } finally {
      isSearching = false;
    }
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

  async function downloadVersion(version: any) {
    // Convert version data to FileMetadata format for download
    const metadata: FileMetadata = {
      fileHash: version.file_hash,
      fileName: version.file_name,
      fileSize: version.file_size,
      seeders: version.seeders || [],
      createdAt: version.created_at * 1000, // Convert to milliseconds
      isEncrypted: version.is_encrypted || false,
      mimeType: version.mime_type,
      encryptionMethod: version.encryption_method,
      keyFingerprint: version.key_fingerprint,
      version: version.version
    };
    
    dispatch('download', metadata);
    pushMessage(`Starting download of ${version.file_name} v${version.version}`, 'info', 3000);
  }

  function statusLabel(status: string) {
    switch (status) {
      case 'found':
        return tr('download.search.status.found');
      case 'not_found':
        return tr('download.search.history.notFound');
      case 'error':
        return tr('download.search.status.error');
      default:
        return tr('download.search.history.pending');
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

  function toggleHistoryDropdown() {
    showHistoryDropdown = !showHistoryDropdown;
  }

  function selectHistoryEntry(entry: SearchHistoryEntry) {
    searchHash = entry.hash;
    activeHistoryId = entry.id;
    hydrateFromEntry(entry);
    showHistoryDropdown = false;
  }

  function handleClickOutside(event: MouseEvent) {
    const target = event.target as HTMLElement;
    if (!target.closest('.search-input-container')) {
      showHistoryDropdown = false;
    }
  }
</script>

<Card class="p-6">
  <div class="space-y-4">
    <div>
      <Label for="hash-input" class="text-base font-medium">{tr('download.addNew')}</Label>
      <p class="text-sm text-muted-foreground mt-1 mb-3">
        {tr('download.addNewSubtitle')}
      </p>
      
      <!-- Search Mode Switcher -->
      <div class="flex gap-2 mb-3">
        <button
          on:click={() => { searchMode = 'hash'; versionResults = []; }}
          class="px-3 py-1 text-sm rounded-md border transition-colors {searchMode === 'hash' ? 'bg-primary text-primary-foreground border-primary' : 'bg-muted/50 hover:bg-muted border-border'}"
        >
          Search by Hash
        </button>
        <button
          on:click={() => { searchMode = 'name'; latestMetadata = null; }}
          class="px-3 py-1 text-sm rounded-md border transition-colors {searchMode === 'name' ? 'bg-primary text-primary-foreground border-primary' : 'bg-muted/50 hover:bg-muted border-border'}"
        >
          Search by Name (Versions)
        </button>
      </div>
      
      <div class="flex flex-col sm:flex-row gap-3">
        <div class="relative flex-1 search-input-container">
          <Input
            id="hash-input"
            bind:value={searchHash}
            placeholder={searchMode === 'hash' ? tr('download.placeholder') : 'Enter file name (e.g., "document.pdf")'}
            class="pr-20 h-10"
            on:focus={toggleHistoryDropdown}
          />
          {#if searchHash}
            <button
              on:click={clearSearch}
              class="absolute right-10 top-1/2 transform -translate-y-1/2 p-1 hover:bg-muted rounded-full transition-colors"
              type="button"
              aria-label={tr('download.clearInput')}
            >
              <X class="h-4 w-4 text-muted-foreground hover:text-foreground" />
            </button>
          {/if}
          <button
            on:click={toggleHistoryDropdown}
            class="absolute right-2 top-1/2 transform -translate-y-1/2 p-1 hover:bg-muted rounded-full transition-colors"
            type="button"
            aria-label="Toggle search history"
          >
            <History class="h-4 w-4 text-muted-foreground hover:text-foreground" />
          </button>

          {#if showHistoryDropdown && historyEntries.length > 0}
            <div class="absolute top-full left-0 right-0 mt-1 bg-background border border-border rounded-md shadow-lg z-50 max-h-80 overflow-auto">
              <div class="p-2 border-b border-border">
                <div class="flex items-center justify-between">
                  <span class="text-sm font-medium text-muted-foreground">Search History</span>
                  <Button
                    variant="ghost"
                    size="sm"
                    class="h-6 px-2 text-xs"
                    on:click={clearHistory}
                  >
                    <RotateCcw class="h-3 w-3 mr-1" />
                    Clear
                  </Button>
                </div>
              </div>
              <div class="py-1">
                {#each historyEntries as entry}
                  <button
                    type="button"
                    class="w-full px-3 py-2 text-left hover:bg-muted/60 transition-colors flex items-center justify-between"
                    on:click={() => selectHistoryEntry(entry)}
                  >
                    <div class="flex items-center gap-2 flex-1 min-w-0">
                      <span class="text-sm font-medium truncate">{entry.hash}</span>
                      <Badge variant="outline" class="text-xs">
                        {statusLabel(entry.status)}
                      </Badge>
                    </div>
                    <div class="flex items-center gap-2 text-xs text-muted-foreground">
                      <svelte:component this={statusIcon(entry.status)} class={`h-3 w-3 ${statusClass(entry.status)}`} />
                      {#if entry.elapsedMs}
                        <span>{(entry.elapsedMs / 1000).toFixed(1)}s</span>
                      {/if}
                    </div>
                  </button>
                  {#if entry.metadata?.fileName}
                    <div class="px-3 pb-2 text-xs text-muted-foreground truncate">
                      {entry.metadata.fileName}
                    </div>
                  {/if}
                {/each}
              </div>
            </div>
          {/if}
        </div>
        <Button
          on:click={searchForFile}
          disabled={!searchHash.trim() || isSearching}
          class="h-10 px-6"
        >
          <Search class="h-4 w-4 mr-2" />
          {isSearching ? tr('download.search.status.searching') : (searchMode === 'name' ? 'Search Versions' : tr('download.search.button'))}
        </Button>
      </div>
    </div>

    {#if hasSearched}
      <div class="pt-6 border-t">
        <div class="space-y-4">
            {#if isSearching}
              <div class="rounded-md border border-dashed border-muted p-5 text-sm text-muted-foreground text-center">
                {tr('download.search.status.searching')}
              </div>
            {:else if latestStatus === 'found' && versionResults.length > 0}
              <!-- Version Results Display -->
              <div class="space-y-3">
                <div class="flex items-center justify-between">
                  <h3 class="font-medium text-sm">Found {versionResults.length} version{versionResults.length === 1 ? '' : 's'}</h3>
                  <p class="text-xs text-muted-foreground">
                    Search completed in {(lastSearchDuration / 1000).toFixed(1)}s
                  </p>
                </div>
                
                <div class="space-y-2 max-h-80 overflow-y-auto">
                  {#each versionResults as version}
                    <div class="flex items-center justify-between p-3 bg-muted/50 rounded-lg hover:bg-muted/70 transition-colors">
                      <div class="flex items-center gap-3 flex-1 min-w-0">
                        <Badge class="bg-blue-100 text-blue-800 text-xs">
                          v{version.version}
                        </Badge>
                        <div class="flex-1 min-w-0">
                          <div class="font-medium text-sm truncate">{version.file_name}</div>
                          <div class="flex items-center gap-2 text-xs text-muted-foreground">
                            <span>Hash: {version.file_hash.slice(0, 8)}...</span>
                            <span>•</span>
                            <span>{(version.file_size / 1048576).toFixed(2)} MB</span>
                            <span>•</span>
                            <span>{new Date(version.created_at * 1000).toLocaleDateString()}</span>
                          </div>
                        </div>
                      </div>
                      <Button
                        size="sm"
                        on:click={() => downloadVersion(version)}
                        class="h-8 px-3"
                      >
                        Download
                      </Button>
                    </div>
                  {/each}
                </div>
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
              <div class="text-center py-8">
                <p class="text-sm text-muted-foreground">
                  {searchMode === 'name' ? `No versions found for "${searchHash}"` : tr('download.search.status.notFoundDetail')}
                </p>
              </div>
            {:else if latestStatus === 'error'}
              <div class="text-center py-8">
                <p class="text-sm font-medium text-muted-foreground mb-1">{tr('download.search.status.errorHeadline')}</p>
                <p class="text-sm text-muted-foreground">{searchError}</p>
              </div>
            {:else}
              <div class="rounded-md border border-dashed border-muted p-5 text-sm text-muted-foreground text-center">
                {searchMode === 'name' ? 'Enter a file name to search for versions' : tr('download.search.status.placeholder')}
              </div>
            {/if}

        </div>
      </div>
    {/if}
  </div>
</Card>
