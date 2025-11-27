<script lang="ts">
  import Card from '$lib/components/ui/card.svelte';
  import Input from '$lib/components/ui/input.svelte';
  import Label from '$lib/components/ui/label.svelte';
  import Button from '$lib/components/ui/button.svelte';
  import { Search, X, History, RotateCcw, AlertCircle, CheckCircle2 } from 'lucide-svelte';
  import { createEventDispatcher, onDestroy, onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { t } from 'svelte-i18n';
  import { dhtService } from '$lib/dht';
  import { paymentService } from '$lib/services/paymentService';
  import type { FileMetadata } from '$lib/dht';
  import { buildSaveDialogOptions } from '$lib/utils/saveDialog';
  import SearchResultCard from './SearchResultCard.svelte';
  import { dhtSearchHistory, type SearchHistoryEntry, type SearchStatus } from '$lib/stores/searchHistory';
  import PeerSelectionModal, { type PeerInfo } from './PeerSelectionModal.svelte';
  import PeerSelectionService from '$lib/services/peerSelectionService';
  import { files, type FileItem } from '$lib/stores';

  type ToastType = 'success' | 'error' | 'info' | 'warning';
  type ToastPayload = { message: string; type?: ToastType; duration?: number; };

  const dispatch = createEventDispatcher<{ download: FileMetadata; message: ToastPayload }>();
  const tr = (key: string, params?: Record<string, unknown>) => (get(t) as any)(key, params);

  // 40 second timeout gives backend (35s) enough time, which gives Kademlia (30s) enough time
  // Timeout hierarchy: Frontend (40s) > Backend (35s) > Kademlia (30s) + Provider delay (3-5s)
  // This prevents premature timeouts that would kill queries that would eventually succeed
  const SEARCH_TIMEOUT_MS = 40_000;

  let searchHash = '';
  let searchMode = 'merkle_hash'; // 'merkle_hash', 'cid', 'magnet', or 'torrent'
  let isSearching = false;
  let torrentFileInput: HTMLInputElement;
  let torrentFileName: string | null = null;
  let hasSearched = false;
  let latestStatus: SearchStatus = 'pending';
  let latestMetadata: FileMetadata | null = null;
  let searchError: string | null = null;
  let lastSearchDuration = 0;
  let historyEntries: SearchHistoryEntry[] = [];
  let activeHistoryId: string | null = null;
  let showHistoryDropdown = false;

  // Peer selection modal state
  let showPeerSelectionModal = false;
  let selectedFile: FileMetadata | null = null;
  let peerSelectionMode: 'auto' | 'manual' = 'auto';
  let selectedProtocol: 'http' | 'webrtc' | 'bitswap' = 'http';
  let availablePeers: PeerInfo[] = [];
  let autoSelectionInfo: Array<{peerId: string; score: number; metrics: any}> | null = null;

  // Torrent confirmation state
  let pendingTorrentIdentifier: string | null = null;
  let pendingTorrentBytes: number[] | null = null;
  let pendingTorrentType: 'magnet' | 'file' | null = null;

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
    torrentFileName = null;
  }

  function handleTorrentFileSelect(event: Event) {
    const target = event.target as HTMLInputElement
    const file = target.files?.[0]
    if (file && file.name.endsWith('.torrent')) {
      // For Tauri, we'll handle this differently in the download function
      torrentFileName = file.name
    } else {
      torrentFileName = null
      pushMessage('Please select a valid .torrent file', 'warning')
    }
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
    // Handle BitTorrent downloads - show confirmation instead of immediately downloading
    if (searchMode === 'magnet' || searchMode === 'torrent') {
      let identifier: string | null = null

      if (searchMode === 'magnet') {
        identifier = searchHash.trim()
        if (!identifier) {
          pushMessage('Please enter a magnet link', 'warning')
          return
        }
      } else if (searchMode === 'torrent') {
        if (!torrentFileName) {
          pushMessage('Please select a .torrent file', 'warning')
          return
        }
        // Use the file input to get the actual file
        const file = torrentFileInput?.files?.[0]
        if (file) {
          // Read the file and pass it to the backend
          // For now, we'll just use the filename approach
          identifier = torrentFileName
        } else {
          pushMessage('Please select a .torrent file', 'warning')
          return
        }
      }

      if (identifier) {
        try {
          isSearching = true
          
          // Store the pending torrent info for confirmation
          if (searchMode === 'torrent') {
            const file = torrentFileInput?.files?.[0]
            if (file) {
              const arrayBuffer = await file.arrayBuffer()
              const bytes = new Uint8Array(arrayBuffer)
              pendingTorrentBytes = Array.from(bytes)
              pendingTorrentType = 'file'
              pendingTorrentIdentifier = torrentFileName
            }
          } else {
            // For magnet links
            pendingTorrentIdentifier = identifier
            pendingTorrentType = 'magnet'
            pendingTorrentBytes = null
          }
          
          // Show confirmation (metadata display) instead of immediately downloading
          latestMetadata = {
            merkleRoot: '', // No merkle root for torrents
            fileHash: '',
            fileName: pendingTorrentType === 'magnet' ? 'Magnet Link Download' : (torrentFileName || 'Torrent Download'),
            fileSize: 0, // Unknown until torrent metadata is fetched
            seeders: [],
            createdAt: Date.now() / 1000,
            mimeType: undefined,
            isEncrypted: false,
            encryptionMethod: undefined,
            keyFingerprint: undefined,
            cids: undefined,
            isRoot: true,
            downloadPath: undefined,
            price: undefined,
            uploaderAddress: undefined,
            httpSources: undefined,
          }
          
          latestStatus = 'found'
          hasSearched = true
          isSearching = false
          pushMessage(`${pendingTorrentType === 'magnet' ? 'Magnet link' : 'Torrent file'} ready to download`, 'success')
        } catch (error) {
          console.error("Failed to prepare torrent:", error)
          pushMessage(`Failed to prepare download: ${String(error)}`, 'error')
          isSearching = false
        }
      }
      return
    }

    // Original DHT search logic for merkle_hash and cid
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

    const startedAt = performance.now();

    try {
      if (searchMode === 'cid') {
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
    // Handle BitTorrent downloads (magnet/torrent) - skip peer selection
    if (pendingTorrentType && pendingTorrentIdentifier) {
      selectedFile = metadata;
      showPeerSelectionModal = true;
      return;
    }

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

    // Handle BitTorrent downloads (magnet/torrent)
    if (pendingTorrentType && pendingTorrentIdentifier) {
      try {
        const { invoke } = await import("@tauri-apps/api/core")
        
        if (pendingTorrentType === 'file' && pendingTorrentBytes) {
          // For torrent files, pass the file bytes
          await invoke('download_torrent_from_bytes', { bytes: pendingTorrentBytes })
        } else if (pendingTorrentType === 'magnet') {
          // For magnet links
          await invoke('download_torrent', { identifier: pendingTorrentIdentifier })
        }
        
        // Clear state
        searchHash = ''
        torrentFileName = null
        if (torrentFileInput) torrentFileInput.value = ''
        pendingTorrentIdentifier = null
        pendingTorrentBytes = null
        pendingTorrentType = null
        
        showPeerSelectionModal = false
        selectedFile = null
        pushMessage('Torrent download started', 'success')
      } catch (error) {
        console.error("Failed to start torrent download:", error)
        pushMessage(`Failed to start download: ${String(error)}`, 'error')
      }
      return
    }

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
      console.log(`🔍 DEBUG: Initiating ${selectedProtocol} download for file: ${selectedFile.fileName}`);

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

      // For HTTP, use the first selected peer
      const firstPeer = selectedPeerIds[0];
      if (!firstPeer) {
        throw new Error('No peers selected for HTTP download');
      }

      // PAYMENT PROCESSING: Calculate and check payment before download
      const paymentAmount = await paymentService.calculateDownloadCost(file.fileSize);
      console.log(`💰 Payment required for HTTP download: ${paymentAmount.toFixed(6)} Chiral for ${file.fileName}`);

      // Check if user has sufficient balance
      if (paymentAmount > 0 && !paymentService.hasSufficientBalance(paymentAmount)) {
        throw new Error(`Insufficient balance. Need ${paymentAmount.toFixed(4)} Chiral`);
      }

      // Show file save dialog
      const outputPath = await save(buildSaveDialogOptions(file.fileName));

      if (!outputPath) {
        pushMessage('Download cancelled by user', 'info');
        return;
      }

      // Get HTTP URL from file metadata (published to DHT)
      const seederUrl = file.httpSources?.[0]?.url || `http://localhost:8080`;
      const merkleRoot = file.fileHash || file.merkleRoot || '';

      console.log(`📡 Starting HTTP download from ${seederUrl}`);
      console.log(`   File hash: ${merkleRoot}`);
      console.log(`   Output path: ${outputPath}`);
      
      pushMessage(`Starting HTTP download to ${outputPath}`, 'info');
      
      try {
        await invoke('download_file_http', {
          seederUrl,
          merkleRoot,
          outputPath,
          peerId: firstPeer  // Pass peer ID for metrics tracking
        });

        console.log(`✅ HTTP download completed: ${outputPath}`);

        // Process payment after successful download
        // Use uploaderAddress from file metadata (this is the wallet address of who uploaded the file)
        const seederWalletAddress = paymentService.isValidWalletAddress(file.uploaderAddress)
          ? file.uploaderAddress!
          : null;

        if (!seederWalletAddress) {
          console.warn('⚠️ Skipping HTTP download payment due to missing or invalid uploader wallet address', {
            file: file.fileName,
            uploaderAddress: file.uploaderAddress
          });
          pushMessage(`Download completed but payment skipped: missing uploader wallet address`, 'warning', 6000);
        } else {
          const paymentResult = await paymentService.processDownloadPayment(
            merkleRoot,
            file.fileName,
            file.fileSize,
            seederWalletAddress,
            firstPeer
          );

          if (paymentResult.success) {
            console.log(`✅ Payment processed: ${paymentAmount.toFixed(6)} Chiral to ${seederWalletAddress}`);
            pushMessage(`Download completed! Paid ${paymentAmount.toFixed(4)} Chiral to seeder`, 'success', 8000);
          } else {
            console.error('❌ Payment failed:', paymentResult.error);
            pushMessage(`Download completed but payment failed: ${paymentResult.error}`, 'warning', 6000);
          }
        }

        const completedHash = merkleRoot || file.fileHash || `http-${Date.now()}`;
        const manifestData =
          typeof file.manifest === 'string'
            ? (() => {
                try {
                  return JSON.parse(file.manifest as string);
                } catch {
                  return undefined;
                }
              })()
            : file.manifest;

        const completedEntry: FileItem = {
          id: `http-download-${Date.now()}`,
          name: file.fileName,
          hash: completedHash,
          size: file.fileSize ?? 0,
          status: 'completed',
          progress: 100,
          downloadPath: outputPath,
          isEncrypted: file.isEncrypted ?? file.encrypted ?? false,
          manifest: manifestData,
          cids: file.cids ?? [],
          seeders: file.seeders?.length ?? 0,
          seederAddresses: file.seeders ?? [],
          price: file.price ?? paymentAmount
        };

        files.update((current) => {
          const idx = current.findIndex((entry) => entry.hash === completedEntry.hash);
          if (idx >= 0) {
            return current.map((entry, entryIndex) =>
              entryIndex === idx ? { ...entry, ...completedEntry } : entry
            );
          }
          return [...current, completedEntry];
        });
      } catch (invokeError) {
        // Log the actual error from Rust
        console.error('❌ HTTP download invoke error:', invokeError);
        throw invokeError;
      }
    } catch (error) {
      const errorMessage = error instanceof Error ? error.message : String(error);
      pushMessage(`HTTP download failed: ${errorMessage}`, 'error', 6000);
      console.error('HTTP download failed:', error);
    }
  }

  // Cancel peer selection
  function cancelPeerSelection() {
    showPeerSelectionModal = false;
    selectedFile = null;
    // Clear torrent state if canceling a torrent download
    if (pendingTorrentType) {
      pendingTorrentIdentifier = null;
      pendingTorrentBytes = null;
      pendingTorrentType = null;
      latestMetadata = null;
      latestStatus = 'pending';
    }
  }
</script>

<Card class="p-6">
  <div class="space-y-4">
    <div>
      <Label for="hash-input" class="text-xl font-semibold">{tr('download.addNew')}</Label>

      <!-- Search Mode Switcher -->
      <div class="flex gap-2 mb-3 mt-3">
        <select bind:value={searchMode} class="px-3 py-1 text-sm rounded-md border transition-colors bg-muted/50 hover:bg-muted border-border">
            <option value="merkle_hash">Search by Merkle Hash</option>
            <option value="cid">Search by CID</option>
            <option value="magnet">Search by Magnet Link</option>
            <option value="torrent">Search by .torrent File</option>
        </select>
      </div>

      <div class="flex flex-col sm:flex-row gap-3">
        {#if searchMode === 'torrent'}
          <!-- File input for .torrent files -->
          <div class="flex-1">
            <input
              type="file"
              bind:this={torrentFileInput}
              accept=".torrent"
              class="hidden"
              on:change={handleTorrentFileSelect}
            />
            <Button
              variant="default"
              class="w-full h-10 justify-center font-medium cursor-pointer hover:opacity-90"
              on:click={() => torrentFileInput?.click()}
            >
              <svg xmlns="http://www.w3.org/2000/svg" width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" class="mr-2">
                <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
                <polyline points="17 8 12 3 7 8"></polyline>
                <line x1="12" y1="3" x2="12" y2="15"></line>
              </svg>
              {torrentFileName || 'Select .torrent File'}
            </Button>
          </div>
        {:else}
          <div class="relative flex-1 search-input-container">
            <Input
              id="hash-input"
              bind:value={searchHash}
              placeholder={
                searchMode === 'merkle_hash' ? 'Enter Merkle root hash (SHA-256)...' :
                searchMode === 'cid' ? 'Enter Content Identifier (CID)...' :
                searchMode === 'magnet' ? 'magnet:?xt=urn:btih:...' :
                ''
              }
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
        {/if}
        <Button
          on:click={searchForFile}
          disabled={(searchMode !== 'torrent' && !searchHash.trim()) || (searchMode === 'torrent' && !torrentFileName) || isSearching}
          class="h-10 px-6"
        >
          <Search class="h-4 w-4 mr-2" />
          {#if isSearching}
            {tr('download.search.status.searching')}
          {:else if searchMode === 'magnet' || searchMode === 'torrent'}
            Download
          {:else}
            {tr('download.search.button')}
          {/if}
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
            {:else if latestStatus === 'found' && latestMetadata}
              <SearchResultCard
                metadata={latestMetadata}
                on:copy={handleCopy}
                on:download={event => handleFileDownload(event.detail)}
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
  isTorrent={pendingTorrentType !== null}
  on:confirm={confirmPeerSelection}
  on:cancel={cancelPeerSelection}
/>
