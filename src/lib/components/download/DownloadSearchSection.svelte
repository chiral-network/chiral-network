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
import { files } from '$lib/stores';
import { paymentService } from '$lib/services/paymentService';
  import type { FileMetadata } from '$lib/dht';
  import SearchResultCard from './SearchResultCard.svelte';
  import { dhtSearchHistory, type SearchHistoryEntry, type SearchStatus } from '$lib/stores/searchHistory';
  import PeerSelectionModal, { type PeerInfo } from './PeerSelectionModal.svelte';
  import PeerSelectionService from '$lib/services/peerSelectionService';

  type ToastType = 'success' | 'error' | 'info' | 'warning';
  type ToastPayload = { message: string; type?: ToastType; duration?: number; };

const dispatch = createEventDispatcher<{ download: FileMetadata; message: ToastPayload }>();
const tr = (key: string, params?: Record<string, unknown>) => (get(t) as any)(key, params);

const SEARCH_TIMEOUT_MS = 10_000; // 10 seconds for DHT searches to find peers
export let isBitswap: boolean = false;

function buildSaveDialogOptions(fileName: string) {
  const defaultPath =
    typeof fileName === 'string' && fileName.trim().length > 0
      ? fileName
      : 'downloaded-file';

  const lastDot = defaultPath.lastIndexOf('.');
  const rawExt =
    lastDot > 0 && lastDot < defaultPath.length - 1
      ? defaultPath.slice(lastDot + 1)
      : '';
  const sanitizedExt = rawExt.replace(/[^a-zA-Z0-9]/g, '').toLowerCase();

  if (sanitizedExt.length === 0) {
    return { defaultPath };
  }

  return {
    defaultPath,
    filters: [
      {
        name: `${sanitizedExt.toUpperCase()} Files`,
        extensions: [sanitizedExt]
      }
    ]
  };
}

  let searchHash = '';
  let searchMode = 'merkle_hash'; // 'merkle_hash' or 'cid'
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

  // Peer selection modal state
  let showPeerSelectionModal = false;
  let selectedFile: FileMetadata | null = null;
  let peerSelectionMode: 'auto' | 'manual' = 'auto';
  let selectedProtocol: 'http' | 'webrtc' = 'http';
  let availablePeers: PeerInfo[] = [];
  let autoSelectionInfo: Array<{peerId: string; score: number; metrics: any}> | null = null;

  const unsubscribe = dhtSearchHistory.subscribe((entries) => {
    historyEntries = entries;
    // if (!activeHistoryId && entries.length > 0) {
    //   activeHistoryId = entries[0].id;
    //   latestStatus = entries[0].status;
    //   latestMetadata = entries[0].metadata ?? null;
    //   searchError = entries[0].errorMessage ?? null;
    //   hasSearched = entries.length > 0;
    // }
    if (entries.length > 0) {
      // 1. Always set the active ID from the most recent entry for the history dropdown.
      activeHistoryId = entries[0].id;

      // 2. Control the main UI state based on whether a search has been initiated in this session.
      if (!hasSearched) {
        // If it's a fresh load (hasSearched is false):
        // Keep the input clear, and the result panel empty.
        searchHash = '';
        latestStatus = 'pending';
        latestMetadata = null;
        searchError = null;
      } else {
        // If the user has searched in this session, ensure the current search results are displayed.
        const entry = entries.find(e => e.id === activeHistoryId) || entries[0];
        if (entry) {
          latestStatus = entry.status;
          latestMetadata = entry.metadata ?? null;
          searchError = entry.errorMessage ?? null;
          searchHash = entry.hash;
        }
      }
    } else {
      activeHistoryId = null;
      // On empty history, ensure the main state is also reset.
      if (!hasSearched) {
        searchHash = '';
        latestStatus = 'pending';
        latestMetadata = null;
        searchError = null;
      }
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
        // This mode is now deprecated in favor of Merkle Hash and CID
        pushMessage('Searching for file versions...', 'info', 2000);

        try {
          // Import invoke function for backend calls
          const { invoke } = await import("@tauri-apps/api/core");

          console.log('🔍 Starting search for file versions with name:', trimmed);
          console.log('⏱️ Search timeout set to:', SEARCH_TIMEOUT_MS, 'ms');
          console.log('📡 About to call invoke with get_file_versions_by_name');
          
          // Test if invoke is working at all
          console.log('🧪 Testing invoke function availability...');
          
          // Test backend connection first
          try {
            console.log('🔌 Testing backend connection...');
            const connectionTest = await invoke('test_backend_connection') as string;
            console.log('✅ Backend connection test result:', connectionTest);
          } catch (connectionError) {
            console.error('❌ Backend connection test failed:', connectionError);
            throw new Error(`Backend connection failed: ${connectionError}`);
          }
          
          // Try a simpler approach - check local files first
          console.log('🔍 Checking local files first...');
          const localFiles = get(files);
          const localMatches = localFiles.filter(f => f.name === trimmed);
          if (localMatches.length > 0) {
            console.log('✅ Found local files:', localMatches.length);
            versionResults = localMatches.map(file => ({
              fileHash: file.hash,
              fileName: file.name,
              fileSize: file.size,
              version: file.version || 1,
              createdAt: file.uploadDate ? Math.floor(file.uploadDate.getTime() / 1000) : Date.now() / 1000,
              seeders: [],
              is_encrypted: file.isEncrypted || false,
              price: file.price ?? 0
            })).sort((a, b) => b.version - a.version);
            
            latestStatus = 'found';
            pushMessage(`Found ${versionResults.length} local version(s) of "${trimmed}"`, 'success');
            return;
          }
          
          // Add timeout for name search with multiple fallback mechanisms
          const searchPromise = invoke('get_file_versions_by_name', { fileName: trimmed }) as Promise<any[]>;
          const timeoutPromise = new Promise((_, reject) => 
            setTimeout(() => {
              console.log('⏰ Search timeout reached!');
              reject(new Error('Search timeout'));
            }, SEARCH_TIMEOUT_MS)
          );
          
          console.log('🏁 Starting Promise.race between search and timeout');
          console.log('⏱️ Current time:', new Date().toISOString());
          
          // Add a progress indicator
          const progressInterval = setInterval(() => {
            console.log('⏳ Search still in progress...');
          }, 500);
          
          // Add a force-complete mechanism
          const forceCompleteTimeout = setTimeout(() => {
            console.log('🚨 Force completing search due to background network issues');
            clearInterval(progressInterval);
            // Force the search to complete even if there are background issues
            latestStatus = 'not_found';
            searchError = 'Search completed but may have background network issues';
            pushMessage('Search completed with potential network issues. Try again if needed.', 'warning', 6000);
            isSearching = false;
          }, SEARCH_TIMEOUT_MS + 1000);
          
          try {
            const versions = await Promise.race([searchPromise, timeoutPromise]) as any[];
            clearTimeout(forceCompleteTimeout);
            clearInterval(progressInterval);
            console.log('✅ Search results received:', versions);
            console.log('⏱️ Search completed in:', Math.round(performance.now() - startedAt), 'ms');
            
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
          } catch (error) {
            clearTimeout(forceCompleteTimeout);
            clearInterval(progressInterval);
            throw error;
          }
        } catch (nameSearchError) {
          console.error('❌ Search by name failed:', nameSearchError);
          latestStatus = 'error';
          const errorMessage = nameSearchError instanceof Error ? nameSearchError.message : 'Search failed';
          searchError = errorMessage;
          
          if (errorMessage === 'Search timeout') {
            pushMessage(`Search timed out after ${SEARCH_TIMEOUT_MS / 1000} seconds. Try again or use hash search.`, 'error', 8000);
          } else if (errorMessage.includes('DHT not running')) {
            pushMessage('DHT service is not running. Please restart the application.', 'error', 8000);
          } else {
            pushMessage(`Search failed: ${errorMessage}`, 'error', 6000);
          }
        }
      } else if (searchMode === 'cid') {
        const entry = dhtSearchHistory.addPending(trimmed);
        activeHistoryId = entry.id;
        pushMessage('Searching for providers by CID...', 'info', 2000);
        await dhtService.searchFileByCid(trimmed);
        // The result will come via a `found_file` event, which is handled by the search history store.
        // We just need to wait and see.
        setTimeout(() => {
          isSearching = false;
        }, SEARCH_TIMEOUT_MS);
      } else {
        // Skip local file lookup - always search DHT for peer information
        // This ensures we get proper seeder lists for peer selection
        console.log('🔍 Searching DHT for file hash:', trimmed);

        // Original hash search
        const entry = dhtSearchHistory.addPending(trimmed);
        activeHistoryId = entry.id;

        pushMessage(tr('download.search.status.started'), 'info', 2000);
        const metadata = await dhtService.searchFileMetadata(trimmed, SEARCH_TIMEOUT_MS);
        const elapsed = Math.round(performance.now() - startedAt);
        lastSearchDuration = elapsed;

        if (metadata) {
          metadata.fileHash = metadata.merkleRoot || "";
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
          isSearching = false;
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
      // Ensure isSearching is always set to false
      setTimeout(() => {
        isSearching = false;
        console.log('🔒 Forced isSearching to false');
      }, 100);
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

  function handleCopy(event: CustomEvent<string>) {
    pushMessage(
      tr('download.search.notifications.copied', { values: { value: event.detail } }),
      'info',
      2000,
    );
  }

  async function downloadVersion(version: any) {
    const rawPrice =
      typeof version.price === 'number'
        ? version.price
        : typeof version.price === 'string'
          ? Number.parseFloat(version.price)
          : undefined;
    const price =
      typeof rawPrice === 'number' && Number.isFinite(rawPrice)
        ? rawPrice
        : undefined;

    // Convert version data to FileMetadata format for download
    const metadata: FileMetadata = {
      fileHash: version.fileHash,
      fileName: version.fileName,
      fileSize: version.fileSize,
      seeders: version.seeders || [],
      createdAt: version.createdAt * 1000, // Convert to milliseconds
      isEncrypted: version.is_encrypted || false,
      mimeType: version.mime_type,
      encryptionMethod: version.encryption_method,
      keyFingerprint: version.key_fingerprint,
      version: version.version,
      price,
      uploaderAddress: version.uploader_address ?? version.uploaderAddress
    };

    // Show peer selection modal instead of direct download
    await handleFileDownload(metadata);
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

  // Handle file download - show peer selection modal first
  async function handleFileDownload(metadata: FileMetadata) {
    // Check if there are any seeders
    if (!metadata.seeders || metadata.seeders.length === 0) {
      pushMessage('No seeders available for this file', 'warning');
      dispatch('download', metadata);
      return;
    }

    selectedFile = metadata;
    autoSelectionInfo = null;  // Clear previous auto-selection info

    // Fetch peer metrics for each seeder
    try {
      const allMetrics = await PeerSelectionService.getPeerMetrics();

      const sizeInMb = metadata.fileSize > 0 ? metadata.fileSize / (1024 * 1024) : 0;
      let perMbPrice =
        metadata.price && sizeInMb > 0
          ? metadata.price / sizeInMb
          : 0;

      if (!Number.isFinite(perMbPrice) || perMbPrice <= 0) {
        try {
          perMbPrice = await paymentService.getDynamicPricePerMB(1.2);
        } catch (pricingError) {
          console.warn('Falling back to static per MB price:', pricingError);
          perMbPrice = 0.001;
        }
      }

      availablePeers = metadata.seeders.map(seederId => {
        const metrics = allMetrics.find(m => m.peer_id === seederId);

        return {
          peerId: seederId,
          latency_ms: metrics?.latency_ms,
          bandwidth_kbps: metrics?.bandwidth_kbps,
          reliability_score: metrics?.reliability_score ?? 0.5,
          price_per_mb: perMbPrice,
          selected: true,  // All selected by default
          percentage: Math.round(100 / metadata.seeders.length)  // Equal split
        };
      });

      // If in auto mode, pre-calculate the selection for transparency
      if (peerSelectionMode === 'auto') {
        await calculateAutoSelection(metadata, allMetrics);
      }

      showPeerSelectionModal = true;
    } catch (error) {
      console.error('Failed to fetch peer metrics:', error);
      // Fall back to direct download without peer selection
      pushMessage('Failed to load peer selection, proceeding with default download', 'warning');
      dispatch('download', metadata);
    }
  }

  // Calculate auto-selection for transparency display
  async function calculateAutoSelection(metadata: FileMetadata, allMetrics: any[]) {
    try {
      // Auto-select best peers using backend algorithm
      const autoPeers = await PeerSelectionService.getPeersForParallelDownload(
        metadata.seeders,
        metadata.fileSize,
        3,  // Max 3 peers
        metadata.isEncrypted
      );

      // Get metrics for selected peers
      const selectedMetrics = autoPeers.map(peerId =>
        allMetrics.find(m => m.peer_id === peerId)
      ).filter(m => m !== undefined);

      if (selectedMetrics.length > 0) {
        // Calculate composite scores for each peer
        const peerScores = selectedMetrics.map(m => ({
          peerId: m!.peer_id,
          score: PeerSelectionService.compositeScoreFromMetrics(m!)
        }));

        // Calculate total score
        const totalScore = peerScores.reduce((sum, p) => sum + p.score, 0);

        // Store selection info for transparency display
        autoSelectionInfo = peerScores.map((p, index) => ({
          peerId: p.peerId,
          score: p.score,
          metrics: selectedMetrics[index]!
        }));

        // Update availablePeers with score-weighted percentages
        availablePeers = availablePeers.map(peer => {
          const peerScore = peerScores.find(ps => ps.peerId === peer.peerId);
          if (peerScore) {
            const percentage = Math.round((peerScore.score / totalScore) * 100);
            return {
              ...peer,
              selected: true,
              percentage
            };
          }
          return {
            ...peer,
            selected: false,
            percentage: 0
          };
        });

        // Adjust for rounding to ensure selected peers total 100%
        const selectedPeers = availablePeers.filter(p => p.selected);
        const totalPercentage = selectedPeers.reduce((sum, p) => sum + p.percentage, 0);
        if (totalPercentage !== 100 && selectedPeers.length > 0) {
          selectedPeers[0].percentage += (100 - totalPercentage);
        }
      }
    } catch (error) {
      console.error('Failed to calculate auto-selection:', error);
    }
  }

  // Confirm peer selection and start download
  async function confirmPeerSelection() {
    if (!selectedFile) return;

    // Get selected peers and their allocations from availablePeers
    const selectedPeers = availablePeers
      .filter(p => p.selected)
      .map(p => p.peerId);

    const peerAllocation = availablePeers
      .filter(p => p.selected)
      .map(p => ({
        peerId: p.peerId,
        percentage: p.percentage
      }));

    // Log transparency info for auto-selection
    if (peerSelectionMode === 'auto' && autoSelectionInfo) {
      autoSelectionInfo.forEach((info, index) => {
        console.log(`📊 Auto-selected peer ${index + 1}:`, {
          peerId: info.peerId.slice(0, 12),
          score: info.score.toFixed(3),
          allocation: `${availablePeers.find(p => p.peerId === info.peerId)?.percentage}%`,
          metrics: info.metrics
        });
      });

      pushMessage(
        `Auto-selected ${selectedPeers.length} peers with score-weighted distribution`,
        'success',
        3000
      );
    }

    // Route download based on selected protocol
    if (selectedProtocol === 'http') {
      // HTTP download flow
      await handleHttpDownload(selectedFile, selectedPeers);
    } else {
      // WebRTC download flow (existing)
      console.log(`🔍 DEBUG: Initiating WebRTC download for file: ${selectedFile.fileName}`);

      const fileWithSelectedPeers: FileMetadata & { peerAllocation?: any[] } = {
        ...selectedFile,
        seeders: selectedPeers,  // Override with selected peers
        peerAllocation
      };

      // Dispatch to parent (Download.svelte)
      dispatch('download', fileWithSelectedPeers);
    }

    // Close modal and reset state
    showPeerSelectionModal = false;
    selectedFile = null;
    pushMessage(`Starting ${selectedProtocol.toUpperCase()} download with ${selectedPeers.length} selected peer${selectedPeers.length === 1 ? '' : 's'}`, 'info', 3000);
  }

  // Handle HTTP download
  async function handleHttpDownload(file: FileMetadata, selectedPeerIds: string[]) {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const { save } = await import('@tauri-apps/plugin-dialog');

      // Show file save dialog
      const outputPath = await save(buildSaveDialogOptions(file.fileName));

      if (!outputPath) {
        pushMessage('Download cancelled by user', 'info');
        return;
      }

      // For HTTP, use the first selected peer
      // Get HTTP URL from DHT metadata
      const firstPeer = selectedPeerIds[0];
      if (!firstPeer) {
        throw new Error('No peers selected for HTTP download');
      }

      // Get HTTP URL from file metadata (published to DHT)
      const seederUrl = file.httpSources?.[0]?.url || `http://localhost:8080`;
      const merkleRoot = file.fileHash || file.merkleRoot || '';

      console.log(`📡 Starting HTTP download from ${seederUrl}`);
      await invoke('download_file_http', {
        seederUrl,
        merkleRoot,
        outputPath
      });

      pushMessage(`HTTP download started successfully`, 'success');
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : 'Unknown error';
      pushMessage(`HTTP download failed: ${errorMessage}`, 'error', 6000);
      console.error('HTTP download failed:', error);
    }
  }

  // Cancel peer selection
  function cancelPeerSelection() {
    showPeerSelectionModal = false;
    selectedFile = null;
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
        <select bind:value={searchMode} class="px-3 py-1 text-sm rounded-md border transition-colors bg-muted/50 hover:bg-muted border-border">
            <option value="merkle_hash">Search by Merkle Hash</option>
            <option value="cid">Search by CID</option>
        </select>
      </div>

      <div class="flex flex-col sm:flex-row gap-3">
        <div class="relative flex-1 search-input-container">
          <Input
            id="hash-input"
            bind:value={searchHash}
            placeholder={searchMode === 'merkle_hash' ? 'Enter Merkle Hash...' : 'Enter CID...'}
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

          {#if showHistoryDropdown}
            <div class="absolute top-full left-0 right-0 mt-1 bg-background border border-border rounded-md shadow-lg z-50 max-h-80 overflow-auto">
              {#if historyEntries.length > 0}
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
              {:else}
                <div class="p-4 text-center">
                  <p class="text-sm text-muted-foreground">No search history yet</p>
                </div>
              {/if}
            </div>
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
                          <div class="font-medium text-sm truncate">{version.fileName}</div>
                          <div class="flex items-center gap-2 text-xs text-muted-foreground">
                            <span>Hash: {version.fileHash.slice(0, 8)}...</span>
                            <span>•</span>
                            <span>{(version.fileSize / 1048576).toFixed(2)} MB</span>
                            <span>•</span>
                            <span>{new Date(version.createdAt * 1000).toLocaleDateString()}</span>
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
                on:copy={handleCopy}
                on:download={event => handleFileDownload(event.detail)}
                isBitswap={isBitswap}
              />
              <p class="text-xs text-muted-foreground">
                {tr('download.search.status.completedIn', { values: { seconds: (lastSearchDuration / 1000).toFixed(1) } })}
              </p>
            {:else if latestStatus === 'not_found'}
              <div class="text-center py-8">
                {#if searchError}
                   <p class="text-sm text-red-500">{searchError}</p>
                {:else}
                   <p class="text-sm text-muted-foreground">{tr('download.search.status.notFoundDetail')}</p>
                {/if}
              </div>
            {:else if latestStatus === 'error'}
              <div class="text-center py-8">
                <p class="text-sm font-medium text-muted-foreground mb-1">{tr('download.search.status.errorHeadline')}</p>
                <p class="text-sm text-muted-foreground">{searchError}</p>
              </div>
            {:else}
              <div class="rounded-md border border-dashed border-muted p-5 text-sm text-muted-foreground text-center">
                {tr('download.search.status.placeholder')}
              </div>
            {/if}
        </div>
      </div>
    {/if}
  </div>
</Card>

<!-- Peer Selection Modal -->
<PeerSelectionModal
  show={showPeerSelectionModal}
  fileName={selectedFile?.fileName || ''}
  fileSize={selectedFile?.fileSize || 0}
  bind:peers={availablePeers}
  bind:mode={peerSelectionMode}
  bind:protocol={selectedProtocol}
  autoSelectionInfo={autoSelectionInfo}
  on:confirm={confirmPeerSelection}
  on:cancel={cancelPeerSelection}
/>
