<script lang="ts">
  import Card from '$lib/components/ui/card.svelte';
  import Input from '$lib/components/ui/input.svelte';
  import Label from '$lib/components/ui/label.svelte';
  import Button from '$lib/components/ui/button.svelte';
  import { Search, X, History, RotateCcw, AlertCircle, CheckCircle2, Loader } from 'lucide-svelte';
  import { createEventDispatcher, onDestroy, onMount } from 'svelte';
  import { get } from 'svelte/store';
  import { t } from 'svelte-i18n';
  import { dhtService } from '$lib/dht';
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

  // 40 second timeout gives backend (35s) enough time, which gives Kademlia (30s) enough time
  // Timeout hierarchy: Frontend (40s) > Backend (35s) > Kademlia (30s) + Provider delay (3-5s)
  // This prevents premature timeouts that would kill queries that would eventually succeed
  const SEARCH_TIMEOUT_MS = 40_000;

  let searchHash = '';
  let searchMode = 'merkle_hash'; // 'merkle_hash', 'magnet', 'torrent', 'ed2k', 'ftp'
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

  // Protocol selection modal state
  let showProtocolSelectionModal = false;
  let availableProtocols: Array<{id: string, name: string, description: string, available: boolean}> = [];

  // Peer selection modal state
  let showPeerSelectionModal = false;
  let selectedFile: FileMetadata | null = null;
  let peerSelectionMode: 'auto' | 'manual' = 'auto';
  let selectedProtocol: 'http' | 'webrtc' | 'bitswap' | 'bittorrent' | 'ed2k' | 'ftp' = 'http';
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
    isSearching = true

    // Handle BitTorrent downloads - show confirmation instead of immediately downloading
    if (searchMode === 'magnet' || searchMode === 'torrent' || searchMode === 'ed2k' || searchMode === 'ftp') {
      let identifier: string | null = null

      if (searchMode === 'magnet') {
        identifier = searchHash.trim()
        if (!identifier) {
          pushMessage('Please enter a magnet link', 'warning')
          isSearching = false
          return
        }

        // For magnet links, extract info_hash and search DHT directly
        const urlParams = new URLSearchParams(identifier.split('?')[1])
        const infoHash = urlParams.get('xt')?.replace('urn:btih:', '')
        if (infoHash) {
          try {
            // Search DHT using the info_hash as the key (BitTorrent files are stored with info_hash as merkle_root)
            const metadata = await dhtService.searchFileMetadata(infoHash, SEARCH_TIMEOUT_MS)
            if (metadata) {
              // Found the file! Show it instead of the placeholder
              metadata.fileHash = metadata.merkleRoot || ""
              latestMetadata = metadata
              latestStatus = 'found'
              hasSearched = true
              pushMessage(`Found file: ${metadata.fileName}`, 'success')
              isSearching = false
              return
            }
          } catch (error) {
            console.log('DHT search failed, falling back to magnet download:', error)
          }
        }

        // If not found in DHT or no info_hash, proceed with magnet download
      } else if (searchMode === 'torrent') {
        if (!torrentFileName) {
          pushMessage('Please select a .torrent file', 'warning')
          isSearching = false
          return
        }
        // Use the file input to get the actual file
        const file = torrentFileInput?.files?.[0]
        if (file) {
          // Try to parse torrent file and search for it first
          try {
            // For now, we'll search using a placeholder - ideally we'd parse the torrent
            // to extract the info hash and search DHT. For simplicity, fall back to placeholder.
          identifier = torrentFileName
          } catch (error) {
            console.log('Failed to parse torrent file:', error)
            identifier = torrentFileName
          }
        } else {
          pushMessage('Please select a .torrent file', 'warning')
          return
        }
      } else if (searchMode === 'ed2k') {
        identifier = searchHash.trim()
        if (!identifier) {
          pushMessage('Please enter an ED2K link', 'warning')
          isSearching = false
          return
        }
        // Basic ED2K link validation
        if (!identifier.startsWith('ed2k://')) {
          pushMessage('Please enter a valid ED2K link starting with ed2k://', 'warning')
          isSearching = false
          return
        }

        // For ED2K links, extract hash and search DHT first
        const parts = identifier.split('|')
        if (parts.length >= 5) {
          const ed2kHash = parts[4]
          try {
            // Search DHT using the ED2K hash as the key
            const metadata = await dhtService.searchFileMetadata(ed2kHash, SEARCH_TIMEOUT_MS)
            if (metadata) {
              // Found the file! Show it instead of the placeholder
              metadata.fileHash = metadata.merkleRoot || ""
              latestMetadata = metadata
              latestStatus = 'found'
              hasSearched = true
              pushMessage(`Found file: ${metadata.fileName}`, 'success')
              isSearching = false
              return
            }
          } catch (error) {
            console.log('DHT search failed, falling back to ED2K download:', error)
          }
        }
      } else if (searchMode === 'ftp') {
        identifier = searchHash.trim()
        if (!identifier) {
          pushMessage('Please enter an FTP URL', 'warning')
          isSearching = false
          return
        }
        // Basic FTP URL validation
        if (!identifier.startsWith('ftp://') && !identifier.startsWith('ftps://')) {
          pushMessage('Please enter a valid FTP URL starting with ftp:// or ftps://', 'warning')
          isSearching = false
          return
        }
      }

      if (identifier) {
        try {
          
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
            price: 0,
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

    // Original DHT search logic for merkle_hash
    const trimmed = searchHash.trim();
    if (!trimmed) {
      const message = searchMode === 'merkle_hash' ? tr('download.notifications.enterHash') :
                     searchMode === 'magnet' ? 'Please enter a magnet link' :
                     searchMode === 'ed2k' ? 'Please enter an ED2K link' :
                     searchMode === 'ftp' ? 'Please enter an FTP URL' :
                     'Please enter a search term';
      pushMessage(message, 'warning');
      isSearching = false; // Reset searching state
      return;
    }

    hasSearched = true;
    latestMetadata = null;
    latestStatus = 'pending';
    searchError = null;

    const startedAt = performance.now();

    try {
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
    } catch (error) {
      const message = error instanceof Error ? error.message : tr('download.search.status.unknownError');
      const elapsed = Math.round(performance.now() - startedAt);
      lastSearchDuration = elapsed;
      latestStatus = 'error';
      searchError = message;

      if (searchMode === 'merkle_hash' && activeHistoryId) {
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

  // Helper function to determine available protocols for a file
  function getAvailableProtocols(metadata: FileMetadata): Array<{id: string, name: string, description: string, available: boolean}> {
    return [
      {
        id: 'bitswap',
        name: 'Bitswap',
        description: 'IPFS Bitswap protocol',
        available: !!(metadata.cids && metadata.cids.length > 0)
      },
      {
        id: 'webrtc',
        name: 'WebRTC',
        description: 'Peer-to-peer via WebRTC',
        available: !!(metadata.seeders && metadata.seeders.length > 0)
      },
      {
        id: 'http',
        name: 'HTTP',
        description: 'Direct HTTP/FTP download',
        available: !!(metadata.httpSources && metadata.httpSources.length > 0) || !!(metadata.ftpSources && metadata.ftpSources.length > 0)
      },
      {
        id: 'bittorrent',
        name: 'BitTorrent',
        description: 'BitTorrent protocol',
        available: !!metadata.infoHash
      },
      {
        id: 'ed2k',
        name: 'ED2K',
        description: 'ED2K protocol',
        available: !!(metadata.ed2kSources && metadata.ed2kSources.length > 0)
      },
      {
        id: 'ftp',
        name: 'FTP',
        description: 'FTP protocol',
        available: !!(metadata.ftpSources && metadata.ftpSources.length > 0)
      }
    ];
  }

  // Handle file download - show protocol selection modal first if multiple protocols available
  async function handleFileDownload(metadata: FileMetadata) {
    // Handle BitTorrent downloads (magnet/torrent) - skip protocol selection, go directly to peer selection
    if (pendingTorrentType && pendingTorrentIdentifier) {
      selectedFile = metadata;
      selectedProtocol = 'bittorrent';
      showPeerSelectionModal = true;
      return;
    }

    // Get available protocols for this file
    availableProtocols = getAvailableProtocols(metadata);
    const availableProtocolList = availableProtocols.filter(p => p.available);

    // If only one protocol available, use it directly
    if (availableProtocolList.length === 1) {
      selectedProtocol = availableProtocolList[0].id as any;
      await proceedWithProtocolSelection(metadata, availableProtocolList[0].id);
      return;
    }

    // If multiple protocols available, show selection modal
    if (availableProtocolList.length > 1) {
      selectedFile = metadata;
      showProtocolSelectionModal = true;
      return;
    }

    // No protocols available
    pushMessage('No download protocols available for this file', 'warning');
  }

  // Handle protocol selection and proceed to download
  async function selectProtocol(protocolId: string) {
    if (!selectedFile) return;

    selectedProtocol = protocolId as any;
    showProtocolSelectionModal = false;

    await proceedWithProtocolSelection(selectedFile, protocolId);
  }

  // Proceed with download using selected protocol
  async function proceedWithProtocolSelection(metadata: FileMetadata, protocolId: string) {
    // Handle protocols that don't need peer selection (direct downloads)
    if (protocolId === 'http' || protocolId === 'ftp' || protocolId === 'ed2k') {
      // For HTTP, FTP, ED2K - proceed directly to download
      await startDirectDownload(metadata, protocolId);
      return;
    }

    // For P2P protocols (WebRTC, Bitswap, BitTorrent) - need peer selection
    if (protocolId === 'webrtc' || protocolId === 'bitswap' || protocolId === 'bittorrent') {
      // Check if there are any seeders
      if (!metadata.seeders || metadata.seeders.length === 0) {
        pushMessage('No seeders available for this file', 'warning');
        return;
      }

      // Proceed with peer selection for P2P protocols
      await proceedWithPeerSelection(metadata);
    }
  }

  // Start direct download for HTTP/FTP/ED2K protocols
  async function startDirectDownload(metadata: FileMetadata, protocolId: string) {
    try {
      const { invoke } = await import("@tauri-apps/api/core");

      if (protocolId === 'http' && metadata.httpSources && metadata.httpSources.length > 0) {
        await invoke('download_file_http', {
          seeder_url: metadata.httpSources[0],
          merkle_root: metadata.merkleRoot || metadata.fileHash,
          output_path: `./downloads/${metadata.fileName}`,
          peer_id: null
        });
        pushMessage('HTTP download started', 'success');
      } else if (protocolId === 'ftp' && metadata.ftpSources && metadata.ftpSources.length > 0) {
        await invoke('download_ftp', { url: metadata.ftpSources[0] });
        pushMessage('FTP download started', 'success');
      } else if (protocolId === 'ed2k' && metadata.ed2kSources && metadata.ed2kSources.length > 0) {
        await invoke('download_ed2k', { link: metadata.ed2kSources[0] });
        pushMessage('ED2K download started', 'success');
      } else {
        pushMessage(`No ${protocolId.toUpperCase()} sources available`, 'warning');
      }
    } catch (error) {
      console.error(`Failed to start ${protocolId} download:`, error);
      pushMessage(`Failed to start ${protocolId.toUpperCase()} download: ${String(error)}`, 'error');
    }
  }

  // Proceed with peer selection for P2P protocols
  async function proceedWithPeerSelection(metadata: FileMetadata) {

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

    // Handle direct downloads (HTTP, FTP, ED2K) that skip peer selection
    if (selectedProtocol === 'http' || selectedProtocol === 'ftp' || selectedProtocol === 'ed2k') {
      // This shouldn't happen since direct downloads bypass peer selection
      return;
    }

    // Handle BitTorrent downloads from search
    if ((pendingTorrentType && pendingTorrentIdentifier) || selectedProtocol === 'bittorrent') {
      try {
        const { invoke } = await import("@tauri-apps/api/core")

        if (pendingTorrentType === 'file' && pendingTorrentBytes) {
          // For torrent files, pass the file bytes
          await invoke('download_torrent_from_bytes', { bytes: pendingTorrentBytes })
        } else if (pendingTorrentType === 'magnet') {
          // For magnet links
          await invoke('download_torrent', { identifier: pendingTorrentIdentifier })
        } else {
          // For BitTorrent from metadata
          await invoke('download_torrent', { identifier: selectedFile?.infoHash })
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

        pushMessage('BitTorrent download started', 'success')
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
    if (selectedProtocol === 'webrtc' || selectedProtocol === 'bitswap' || selectedProtocol === 'bittorrent') {
      // P2P download flow (WebRTC, Bitswap, BitTorrent)
      console.log(`🔍 DEBUG: Initiating ${selectedProtocol} download for file: ${selectedFile.fileName}`);

      const fileWithSelectedPeers: FileMetadata & { peerAllocation?: any[]; selectedProtocol?: string } = {
        ...selectedFile,
        seeders: selectedPeers,  // Override with selected peers
        peerAllocation,
        selectedProtocol: selectedProtocol  // Pass the user's protocol selection
      };

      // Dispatch to parent (Download.svelte)
      dispatch('download', fileWithSelectedPeers);
    } else {
      // This shouldn't happen - direct downloads bypass peer selection
      console.error(`Unexpected protocol in peer selection: ${selectedProtocol}`);
      pushMessage(`Protocol ${selectedProtocol} should not require peer selection`, 'error');
      return;
    }

    // Close modal and reset state
    showPeerSelectionModal = false;
    selectedFile = null;
    pushMessage(`Starting ${selectedProtocol.toUpperCase()} download with ${selectedPeers.length} selected peer${selectedPeers.length === 1 ? '' : 's'}`, 'info', 3000);
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
            <option value="magnet">Search by Magnet Link</option>
            <option value="torrent">Search by .torrent File</option>
            <option value="ed2k">Search by ED2K Link</option>
            <option value="ftp">Search by FTP URL</option>
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
                searchMode === 'magnet' ? 'magnet:?xt=urn:btih:...' :
                searchMode === 'ed2k' ? 'ed2k://|file|filename|size|hash|/' :
                searchMode === 'ftp' ? 'ftp://user:pass@server.com/path/file' :
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
          {#if isSearching}
            <Loader class="h-4 w-4 mr-2 animate-spin" />
            {tr('download.search.status.searching')}
          {:else}
            <Search class="h-4 w-4 mr-2" />
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
                on:download={(event: any) => handleFileDownload(event.detail)}
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

<!-- Protocol Selection Modal -->
{#if showProtocolSelectionModal}
  <div class="fixed inset-0 bg-black bg-opacity-50 flex items-center justify-center z-50">
    <div class="bg-background border border-border rounded-lg p-6 max-w-md w-full mx-4">
      <h3 class="text-lg font-semibold mb-4">Choose Download Protocol</h3>
      <p class="text-sm text-muted-foreground mb-4">
        Multiple download protocols are available for "{selectedFile?.fileName}". Choose your preferred method:
      </p>

      <div class="space-y-2">
        {#each availableProtocols.filter(p => p.available) as protocol}
          <button
            class="w-full text-left p-3 rounded-md border border-border hover:bg-muted transition-colors"
            on:click={() => selectProtocol(protocol.id)}
          >
            <div class="flex items-center justify-between">
              <div>
                <div class="font-medium">{protocol.name}</div>
                <div class="text-sm text-muted-foreground">{protocol.description}</div>
              </div>
              <div class="text-primary">→</div>
            </div>
          </button>
        {/each}
      </div>

      <div class="flex justify-end mt-6">
        <Button variant="outline" on:click={() => { showProtocolSelectionModal = false; selectedFile = null; }}>
          Cancel
        </Button>
      </div>
    </div>
  </div>
{/if}

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
