<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import {
    Search,
    Download,
    File as FileIcon,
    Image,
    Video,
    Music,
    Archive,
    Code,
    FileText,
    FileSpreadsheet,
    Pause,
    Play,
    X,
    CheckCircle,
    AlertCircle,
    AlertTriangle,
    History,
    Loader2,
    Link,
    FileUp,
    Plus,
    Trash2,
    FolderOpen,
    ExternalLink,
    Eye
  } from 'lucide-svelte';
  import { Zap, Gauge, Rocket, Star } from 'lucide-svelte';
  import { networkConnected, walletAccount, blacklist, type BlacklistEntry } from '$lib/stores';
  import { get } from 'svelte/store';
  import BlacklistWarningModal from '$lib/components/BlacklistWarningModal.svelte';
  import RateSeederModal from '$lib/components/RateSeederModal.svelte';
  import { walletService } from '$lib/services/walletService';
  import { ratingApi, setRatingOwner, type BatchRatingEntry } from '$lib/services/ratingApiService';
  import { TIERS, calculateCost, formatCost, formatSpeed, type SpeedTier } from '$lib/speedTiers';
  import { toasts } from '$lib/toastStore';
  import { logger } from '$lib/logger';
  const log = logger('Download');

  // Check if running in Tauri environment (reactive)
  let isTauri = $state(false);
  
  // Check Tauri availability
  function checkTauriAvailability(): boolean {
    return typeof window !== 'undefined' && ('__TAURI__' in window || '__TAURI_INTERNALS__' in window);
  }

  // Event listener cleanup functions
  let unlistenDownloadComplete: (() => void) | null = null;
  let unlistenDownloadFailed: (() => void) | null = null;
  let unlistenDownloadProgress: (() => void) | null = null;
  let unlistenPaymentProcessing: (() => void) | null = null;
  let unlistenSpeedTierPayment: (() => void) | null = null;

  // Types
  type SearchMode = 'hash' | 'magnet' | 'torrent';
  type DownloadStatus = 'queued' | 'downloading' | 'paused' | 'completed' | 'cancelled' | 'failed';
  type PreviewType = 'video' | 'audio' | 'image' | 'pdf' | 'unsupported';

  interface SearchResult {
    hash: string;
    fileName: string;
    fileSize: number;
    seeders: string[];
    createdAt: number;
    priceWei: string;
    walletAddress: string;
  }

  interface DownloadItem {
    id: string;
    hash: string;
    name: string;
    size: number;
    status: DownloadStatus;
    progress: number;
    speed: string;
    eta: string;
    seeders: number;
    startedAt: Date;
    completedAt?: Date;
    speedTier?: SpeedTier;
    filePath?: string;
    balanceBefore?: string;
    balanceAfter?: string;
  }

  interface HistoryEntry {
    id: string;
    hash: string;
    fileName: string;
    fileSize: number;
    completedAt: Date;
    startedAt?: Date;
    status: 'completed' | 'cancelled' | 'failed';
    speedTier?: SpeedTier;
    seeders?: number;
    filePath?: string;
    balanceBefore?: string;
    balanceAfter?: string;
  }

  // File type detection
  function getFileIcon(fileName: string) {
    const ext = fileName.split('.').pop()?.toLowerCase() || '';

    if (['jpg', 'jpeg', 'png', 'gif', 'webp', 'svg', 'bmp', 'ico'].includes(ext)) return Image;
    if (['mp4', 'avi', 'mkv', 'mov', 'wmv', 'webm', 'flv', 'm4v'].includes(ext)) return Video;
    if (['mp3', 'wav', 'flac', 'aac', 'ogg', 'm4a', 'wma'].includes(ext)) return Music;
    if (['zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz'].includes(ext)) return Archive;
    if (['js', 'ts', 'html', 'css', 'py', 'java', 'cpp', 'c', 'php', 'rb', 'go', 'rs'].includes(ext)) return Code;
    if (['txt', 'md', 'pdf', 'doc', 'docx', 'rtf'].includes(ext)) return FileText;
    if (['xls', 'xlsx', 'csv', 'ods'].includes(ext)) return FileSpreadsheet;

    return FileIcon;
  }

  function getFileColor(fileName: string) {
    const ext = fileName.split('.').pop()?.toLowerCase() || '';

    if (['jpg', 'jpeg', 'png', 'gif', 'webp', 'svg', 'bmp', 'ico'].includes(ext)) return 'text-blue-500';
    if (['mp4', 'avi', 'mkv', 'mov', 'wmv', 'webm', 'flv', 'm4v'].includes(ext)) return 'text-purple-500';
    if (['mp3', 'wav', 'flac', 'aac', 'ogg', 'm4a', 'wma'].includes(ext)) return 'text-green-500';
    if (['zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz'].includes(ext)) return 'text-orange-500';
    if (['js', 'ts', 'html', 'css', 'py', 'java', 'cpp', 'c', 'php', 'rb', 'go', 'rs'].includes(ext)) return 'text-red-500';
    if (['txt', 'md', 'pdf', 'doc', 'docx', 'rtf'].includes(ext)) return 'text-gray-600';
    if (['xls', 'xlsx', 'csv', 'ods'].includes(ext)) return 'text-emerald-500';

    return 'text-gray-400';
  }

  function getFileExtension(fileNameOrPath: string): string {
    const normalized = fileNameOrPath.split('?')[0];
    const ext = normalized.split('.').pop();
    return ext?.toLowerCase() || '';
  }

  function getPreviewType(fileNameOrPath: string): PreviewType {
    const ext = getFileExtension(fileNameOrPath);

    if (['mp4', 'webm', 'mov', 'm4v', 'ogg', 'ogv'].includes(ext)) return 'video';
    if (['mp3', 'wav', 'flac', 'aac', 'ogg', 'm4a'].includes(ext)) return 'audio';
    if (['jpg', 'jpeg', 'png', 'gif', 'webp', 'bmp', 'svg'].includes(ext)) return 'image';
    if (ext === 'pdf') return 'pdf';

    return 'unsupported';
  }

  function canPreviewFile(fileNameOrPath: string): boolean {
    return getPreviewType(fileNameOrPath) !== 'unsupported';
  }

  // Format file size
  function formatFileSize(bytes: number): string {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
  }

  // Format date
  function formatDate(date: Date): string {
    return new Intl.DateTimeFormat('en-US', {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit'
    }).format(date);
  }

  // Format wei price as CHI string
  function formatPriceWei(weiStr: string): string {
    if (!weiStr || weiStr === '0') return '0 CHI';
    try {
      const wei = BigInt(weiStr);
      if (wei === 0n) return '0 CHI';
      const whole = wei / 1_000_000_000_000_000_000n;
      const frac = wei % 1_000_000_000_000_000_000n;
      if (frac === 0n) return `${whole} CHI`;
      const fracStr = frac.toString().padStart(18, '0').replace(/0+$/, '');
      const decimals = fracStr.length > 6 ? fracStr.slice(0, 6) : fracStr;
      return `${whole}.${decimals} CHI`;
    } catch {
      return '0 CHI';
    }
  }

  // State
  let searchMode = $state<SearchMode>('hash');
  let searchQuery = $state('');
  let isSearching = $state(false);
  let searchResult = $state<SearchResult | null>(null);
  let searchError = $state<string | null>(null);
  let downloads = $state<DownloadItem[]>([]);
  let downloadHistory = $state<HistoryEntry[]>([]);
  let showSearchHistory = $state(false);
  let downloadsTab = $state<'active' | 'history'>('active');
  let isViewerOpen = $state(false);
  let viewerSource = $state('');
  let viewerType = $state<PreviewType>('unsupported');
  let viewerName = $state('');
  let viewerError = $state<string | null>(null);

  // Speed tier state
  let selectedTier = $state<SpeedTier>('standard');
  let walletBalance = $state<string>('0');
  let isProcessingPayment = $state(false);

  // Blacklist warning modal
  let blacklistWarning = $state<{ match: BlacklistEntry; result: SearchResult } | null>(null);

  // Download confirmation modal
  let pendingDownload = $state<{ result: SearchResult; tierCost: number; seederPriceChi: number; totalCost: number } | null>(null);

  // Seeder reputation
  let seederRatings = $state<Record<string, BatchRatingEntry>>({});
  let showRatingModal = $state<{ seederWallet: string; fileHash: string; fileName: string } | null>(null);

  // Persistence keys
  const DOWNLOAD_HISTORY_KEY = 'chiral_download_history';
  const ACTIVE_DOWNLOADS_KEY = 'chiral_active_downloads';

  function normalizeUniqueIds<T extends { id: string }>(
    items: T[],
    fallbackPrefix: string
  ): { items: T[]; changed: boolean } {
    const seen = new Set<string>();
    let changed = false;

    const normalized = items.map((item, index) => {
      let nextId = (item.id || '').trim();
      if (!nextId) {
        nextId = `${fallbackPrefix}-${Date.now()}-${index}`;
        changed = true;
      }

      if (seen.has(nextId)) {
        let suffix = 1;
        let candidate = `${nextId}-${suffix}`;
        while (seen.has(candidate)) {
          suffix += 1;
          candidate = `${nextId}-${suffix}`;
        }
        nextId = candidate;
        changed = true;
      }

      seen.add(nextId);
      return nextId === item.id ? item : { ...item, id: nextId };
    });

    return { items: normalized, changed };
  }

  // Load download history from localStorage
  function loadDownloadHistory() {
    try {
      let idsChanged = false;

      const stored = localStorage.getItem(DOWNLOAD_HISTORY_KEY);
      if (stored) {
        const parsed = JSON.parse(stored);
        const mappedHistory = parsed.map((h: any) => ({
          ...h,
          completedAt: new Date(h.completedAt)
        }));
        const normalizedHistory = normalizeUniqueIds(mappedHistory, 'history');
        downloadHistory = normalizedHistory.items;
        idsChanged = idsChanged || normalizedHistory.changed;
      }

      // Load active downloads
      const activeStored = localStorage.getItem(ACTIVE_DOWNLOADS_KEY);
      if (activeStored) {
        const parsed = JSON.parse(activeStored);
        const mappedDownloads = parsed.map((d: any) => ({
          ...d,
          startedAt: new Date(d.startedAt),
          completedAt: d.completedAt ? new Date(d.completedAt) : undefined
        }));
        const normalizedDownloads = normalizeUniqueIds(mappedDownloads, 'download');
        downloads = normalizedDownloads.items;
        idsChanged = idsChanged || normalizedDownloads.changed;
      }

      // Persist normalized IDs so old duplicates don't come back on next reload
      if (idsChanged) {
        saveDownloadHistory();
      }
    } catch (e) {
      log.error('Failed to load download history:', e);
    }
  }

  function saveDownloadHistory() {
    try {
      const normalizedDownloads = normalizeUniqueIds(downloads, 'download');
      if (normalizedDownloads.changed) {
        downloads = normalizedDownloads.items;
      }
      const normalizedHistory = normalizeUniqueIds(downloadHistory, 'history');
      if (normalizedHistory.changed) {
        downloadHistory = normalizedHistory.items;
      }
      localStorage.setItem(DOWNLOAD_HISTORY_KEY, JSON.stringify(downloadHistory));
      localStorage.setItem(ACTIVE_DOWNLOADS_KEY, JSON.stringify(downloads));
    } catch (e) {
      log.error('Failed to save download history:', e);
    }
  }

  function addToDownloadHistory(download: DownloadItem) {
    let entryId = download.id;
    const existingIds = new Set(downloadHistory.map(h => h.id));
    if (existingIds.has(entryId)) {
      let suffix = 1;
      let candidate = `${entryId}-${suffix}`;
      while (existingIds.has(candidate)) {
        suffix += 1;
        candidate = `${entryId}-${suffix}`;
      }
      entryId = candidate;
    }

    const entry: HistoryEntry = {
      id: entryId,
      hash: download.hash,
      fileName: download.name,
      fileSize: download.size,
      completedAt: new Date(),
      startedAt: download.startedAt,
      status: download.status as 'completed' | 'cancelled' | 'failed',
      speedTier: download.speedTier,
      seeders: download.seeders,
      filePath: download.filePath,
      balanceBefore: download.balanceBefore,
      balanceAfter: download.balanceAfter,
    };
    downloadHistory = [entry, ...downloadHistory].slice(0, 50);
    saveDownloadHistory();
  }

  // Extract info hash from magnet link
  function extractInfoHashFromMagnet(magnetLink: string): string | null {
    // Match SHA-256 (64 chars), SHA-1 (40 chars), or Base32 (32 chars)
    const match = magnetLink.match(/urn:btih:([a-fA-F0-9]{64}|[a-fA-F0-9]{40}|[a-zA-Z2-7]{32})/i);
    if (match) {
      let hash = match[1];
      // Convert Base32 to hex if needed (32 chars)
      if (hash.length === 32 && !/^[a-fA-F0-9]+$/.test(hash)) {
        // Base32 decode would go here - for now just use as-is
        return hash.toLowerCase();
      }
      return hash.toLowerCase();
    }
    return null;
  }

  // Extract name from magnet link
  function extractNameFromMagnet(magnetLink: string): string {
    const match = magnetLink.match(/dn=([^&]+)/);
    if (match) {
      return decodeURIComponent(match[1]);
    }
    return 'Unknown';
  }

  // Handle torrent file upload
  async function handleTorrentFile() {
    const tauriAvailable = checkTauriAvailability();
    if (!tauriAvailable) {
      toasts.show('Torrent file upload requires the desktop app', 'error');
      return;
    }

    try {
      const { invoke } = await import('@tauri-apps/api/core');

      // Use our custom file dialog command instead of plugin-dialog
      const selectedPaths = await invoke<string[]>('open_file_dialog', {
        multiple: false
      });

      if (selectedPaths && selectedPaths.length > 0) {
        const selectedPath = selectedPaths[0];

        // Check if it's a .torrent file
        if (!selectedPath.toLowerCase().endsWith('.torrent')) {
          toasts.show('Please select a .torrent file', 'error');
          return;
        }

        const result = await invoke<{ infoHash: string; name: string; size: number }>('parse_torrent_file', {
          filePath: selectedPath
        });

        if (result) {
          // Search DHT for seeders using the parsed hash
          const dhtResult = await invoke<SearchResult | null>('search_file', {
            fileHash: result.infoHash
          });

          searchResult = {
            hash: result.infoHash,
            fileName: result.name,
            fileSize: result.size || dhtResult?.fileSize || 0,
            seeders: dhtResult?.seeders || [],
            createdAt: dhtResult?.createdAt || Date.now(),
            priceWei: dhtResult?.priceWei || '0',
            walletAddress: dhtResult?.walletAddress || '',
          };

          if (searchResult.seeders.length > 0) {
            toasts.show(`Loaded torrent: ${result.name} (${searchResult.seeders.length} seeder${searchResult.seeders.length !== 1 ? 's' : ''})`, 'success');
          } else {
            toasts.show(`Loaded torrent: ${result.name} - searching for seeders...`, 'info');
          }
        }
      }
    } catch (error) {
      log.error('Failed to parse torrent file:', error);
      toasts.show(`Failed to parse torrent file: ${error}`, 'error');
    }
  }

  // Search for file
  async function searchFile() {
    if (!searchQuery.trim()) {
      toasts.show('Please enter a search query', 'error');
      return;
    }

    if (!$networkConnected) {
      toasts.show('Please connect to the network first', 'error');
      return;
    }

    isSearching = true;
    searchResult = null;
    searchError = null;

    try {
      let fileHash = searchQuery.trim();
      let fileName = 'Unknown';

      // Handle magnet link
      if (searchMode === 'magnet' || searchQuery.startsWith('magnet:')) {
        const extractedHash = extractInfoHashFromMagnet(searchQuery);
        if (!extractedHash) {
          searchError = 'Invalid magnet link';
          isSearching = false;
          return;
        }
        fileHash = extractedHash;
        fileName = extractNameFromMagnet(searchQuery);
      }

      if (isTauri) {
        const { invoke } = await import('@tauri-apps/api/core');
        const result = await invoke<SearchResult | null>('search_file', {
          fileHash
        });

        if (result) {
          // For magnet links, use the name from the magnet link if available
          if ((searchMode === 'magnet' || searchQuery.startsWith('magnet:')) && fileName !== 'Unknown') {
            result.fileName = fileName;
          }
          // If result has empty fileName (from DHT fallback), use the one from magnet/torrent
          if (!result.fileName && fileName !== 'Unknown') {
            result.fileName = fileName;
          }
          // Fallback file name
          if (!result.fileName) {
            result.fileName = `file-${fileHash.slice(0, 8)}`;
          }
          searchResult = result;
          // Fetch seeder reputation ratings
          if (result.seeders.length > 0) {
            fetchSeederRatings(result.seeders, result.walletAddress);
          }
          if (result.seeders.length > 0) {
            toasts.show(`Found ${result.seeders.length} potential seeder${result.seeders.length !== 1 ? 's' : ''} for: ${result.fileName}`, 'success');
          } else {
            toasts.show(`Found: ${result.fileName} - no seeders currently available`, 'warning');
          }
        } else {
          // For magnet links, create a result even if not in DHT but show warning
          if (searchMode === 'magnet' || searchQuery.startsWith('magnet:')) {
            searchResult = {
              hash: fileHash,
              fileName,
              fileSize: 0,
              seeders: [],
              createdAt: Date.now(),
              priceWei: '0',
              walletAddress: '',
            };
            toasts.show(`Magnet link parsed but file not found in DHT. The seeder may be offline.`, 'warning');
          } else {
            searchError = 'File not found in DHT. The seeder may be offline or the hash may be incorrect.';
          }
        }
      } else {
        await new Promise(resolve => setTimeout(resolve, 1000));
        searchError = 'Search requires the desktop application';
      }
    } catch (error) {
      log.error('Search failed:', error);
      searchError = `Search failed: ${error}`;
    } finally {
      isSearching = false;
    }
  }

  // Fetch seeder reputation ratings (non-blocking)
  async function fetchSeederRatings(seeders: string[], walletAddress?: string) {
    try {
      const wallets = [...new Set([...seeders, walletAddress].filter(Boolean) as string[])];
      if (wallets.length === 0) return;
      const ratings = await ratingApi.getBatchRatings(wallets);
      seederRatings = ratings;
    } catch (err) {
      log.warn('Failed to fetch seeder ratings:', err);
    }
  }

  // Fetch wallet balance
  async function refreshWalletBalance() {
    if ($walletAccount?.address) {
      walletBalance = await walletService.getBalance($walletAccount.address);
    } else {
      walletBalance = '0';
    }
  }

  // Get tier icon component
  function getTierIcon(tier: SpeedTier) {
    switch (tier) {
      case 'standard': return Zap;
      case 'premium': return Gauge;
      case 'ultra': return Rocket;
      default: return Zap;
    }
  }

  // Start download (validates, shows confirmation if paid, then proceeds)
  async function startDownload(result: SearchResult, skipBlacklistCheck = false, skipCostConfirm = false) {
    const tauriAvailable = checkTauriAvailability();
    if (!tauriAvailable) {
      toasts.show('Download requires the desktop app', 'error');
      return;
    }

    // Check if already downloading
    if (downloads.some(d => d.hash === result.hash && !['completed', 'failed', 'cancelled'].includes(d.status))) {
      toasts.show('This file is already being downloaded', 'warning');
      return;
    }

    // Check if we have any seeders
    if (result.seeders.length === 0) {
      toasts.show('No seeders available. The file owner may be offline or the file was not found in DHT.', 'error');
      return;
    }

    // Check blacklist - compare against wallet address AND peer IDs (seeders)
    const bl = get(blacklist);
    if (!skipBlacklistCheck && bl.length > 0) {
      const candidates = [result.walletAddress, ...result.seeders]
        .filter(Boolean)
        .map(a => a.trim().toLowerCase());
      const blacklistedMatch = bl.find(entry => {
        const entryAddr = entry.address.trim().toLowerCase();
        return candidates.some(addr => addr === entryAddr || addr.includes(entryAddr) || entryAddr.includes(addr));
      });
      if (blacklistedMatch) {
        blacklistWarning = { match: blacklistedMatch, result };
        return;
      }
    }

    // Calculate total cost: speed tier + seeder file price
    const tierCost = calculateCost(selectedTier, result.fileSize);
    const seederPriceWei = result.priceWei || '0';
    const seederPriceChi = seederPriceWei !== '0'
      ? Number(BigInt(seederPriceWei)) / 1e18
      : 0;
    const totalCost = tierCost + seederPriceChi;

    if (totalCost > 0) {
      if (!$walletAccount) {
        toasts.show('Please log in with your wallet to download paid files', 'error');
        return;
      }
      if (parseFloat(walletBalance) < totalCost) {
        toasts.show(`Insufficient balance. Need ${totalCost.toFixed(6)} CHI, have ${walletBalance} CHI`, 'error');
        return;
      }
      // Show confirmation modal before spending CHI
      if (!skipCostConfirm) {
        pendingDownload = { result, tierCost, seederPriceChi, totalCost };
        return;
      }
    }

    isProcessingPayment = totalCost > 0;

    const newDownload: DownloadItem = {
      id: `download-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
      hash: result.hash,
      name: result.fileName,
      size: result.fileSize,
      status: 'downloading',
      progress: 0,
      speed: totalCost > 0 ? 'Processing payment...' : 'Connecting...',
      eta: 'Requesting file...',
      seeders: result.seeders.length,
      startedAt: new Date(),
      speedTier: selectedTier
    };

    downloads = [...downloads, newDownload];
    downloadsTab = 'active';
    saveDownloadHistory();

    try {
      const { invoke } = await import('@tauri-apps/api/core');

      // Build params - only include wallet fields when they have values
      const params: Record<string, unknown> = {
        fileHash: result.hash,
        fileName: result.fileName,
        seeders: result.seeders,
        speedTier: selectedTier,
        fileSize: result.fileSize || 0,
      };
      if ($walletAccount?.address) {
        params.walletAddress = $walletAccount.address;
        params.privateKey = $walletAccount.privateKey;
      }
      // Pass seeder pricing info
      if (seederPriceWei !== '0') {
        params.seederPriceWei = seederPriceWei;
        params.seederWalletAddress = result.walletAddress;
      }

      const response = await invoke<{ requestId: string; status: string }>('start_download', params);

      log.info('Download request sent:', response);
      if (tierCost > 0) {
        toasts.show(`Speed tier payment processed! Requesting file from seeder...`, 'success');
        refreshWalletBalance();
      } else if (seederPriceChi > 0) {
        toasts.show(`Requesting file from seeder (payment will be sent automatically)...`, 'info');
      } else {
        toasts.show(`Requesting file from seeder...`, 'info');
      }

      // Update download with request ID
      let resolvedRequestId = response.requestId;
      if (downloads.some(d => d.id === resolvedRequestId && d.id !== newDownload.id)) {
        resolvedRequestId = `${response.requestId}-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
      }

      downloads = downloads.map(d =>
        d.id === newDownload.id ? { ...d, id: resolvedRequestId, speed: 'Connecting...' } : d
      );
      saveDownloadHistory();
    } catch (error) {
      log.error('Download failed:', error);
      downloads = downloads.map(d =>
        d.id === newDownload.id ? { ...d, status: 'failed' as const } : d
      );
      saveDownloadHistory();
      toasts.show(`Download failed: ${error}`, 'error');
    } finally {
      isProcessingPayment = false;
    }
  }

  // Pause/Resume download
  function togglePause(downloadId: string) {
    downloads = downloads.map(d => {
      if (d.id === downloadId) {
        if (d.status === 'downloading') {
          return { ...d, status: 'paused' as const };
        } else if (d.status === 'paused') {
          return { ...d, status: 'downloading' as const };
        }
      }
      return d;
    });
    saveDownloadHistory();
  }

  // Cancel download
  function cancelDownload(downloadId: string) {
    const download = downloads.find(d => d.id === downloadId);
    if (download) {
      download.status = 'cancelled';
      addToDownloadHistory(download);
      downloads = downloads.filter(d => d.id !== downloadId);
      saveDownloadHistory();
      toasts.show(`Cancelled: ${download.name}`, 'info');
    }
  }

  // Move completed/failed downloads to history
  function moveToHistory(downloadId: string) {
    const download = downloads.find(d => d.id === downloadId);
    if (download && ['completed', 'failed', 'cancelled'].includes(download.status)) {
      addToDownloadHistory(download);
      downloads = downloads.filter(d => d.id !== downloadId);
      saveDownloadHistory();
    }
  }

  // Clear download history
  function clearDownloadHistory() {
    const count = downloadHistory.length;
    downloadHistory = [];
    saveDownloadHistory();
    toasts.show(`Cleared ${count} item${count !== 1 ? 's' : ''} from history`, 'info');
  }

  // Get active downloads (not completed/failed/cancelled)
  function getActiveDownloads(): DownloadItem[] {
    return downloads.filter(d => !['completed', 'failed', 'cancelled'].includes(d.status));
  }

  // Get finished downloads
  function getFinishedDownloads(): DownloadItem[] {
    return downloads.filter(d => ['completed', 'failed', 'cancelled'].includes(d.status));
  }

  // Setup event listeners for download events from Tauri backend
  async function setupEventListeners() {
    if (!isTauri) return;

    try {
      const { listen } = await import('@tauri-apps/api/event');

      // Listen for successful file downloads
      unlistenDownloadComplete = await listen<{
        requestId: string;
        fileHash: string;
        fileName: string;
        filePath: string;
        fileSize: number;
        status: string;
      }>('file-download-complete', (event) => {
        log.info('Download complete:', event.payload);
        const { fileHash, fileName, filePath, fileSize } = event.payload;

        // Update the download status
        downloads = downloads.map(d => {
          if (d.hash === fileHash) {
            return {
              ...d,
              status: 'completed' as const,
              progress: 100,
              size: fileSize || d.size,
              completedAt: new Date(),
              filePath: filePath || undefined
            };
          }
          return d;
        });
        saveDownloadHistory();

        toasts.show(`Downloaded: ${fileName}`, 'success');

        // Show rating modal for the seeder
        if (searchResult && searchResult.hash === fileHash) {
          const seederWallet = searchResult.walletAddress || searchResult.seeders[0] || '';
          if (seederWallet) {
            showRatingModal = { seederWallet, fileHash, fileName };
          }
        }
      });

      // Listen for failed downloads
      unlistenDownloadFailed = await listen<{
        requestId: string;
        fileHash: string;
        error: string;
      }>('file-download-failed', (event) => {
        log.error('Download failed:', event.payload);
        const { fileHash, error } = event.payload;

        // Update the download status
        downloads = downloads.map(d => {
          if (d.hash === fileHash) {
            return {
              ...d,
              status: 'failed' as const
            };
          }
          return d;
        });
        saveDownloadHistory();

        toasts.show(`Download failed: ${error}`, 'error');
      });

      // Listen for download progress (rate-limited writes)
      unlistenDownloadProgress = await listen<{
        requestId: string;
        fileHash: string;
        fileName: string;
        bytesWritten: number;
        totalBytes: number;
        speedBps: number;
        progress: number;
      }>('download-progress', (event) => {
        const { requestId, fileHash, bytesWritten, totalBytes, speedBps, progress } = event.payload;

        downloads = downloads.map(d => {
          if (d.hash === fileHash || d.id === requestId) {
            return {
              ...d,
              progress,
              speed: formatSpeed(speedBps),
              size: totalBytes || d.size,
              eta: speedBps > 0
                ? `${Math.ceil((totalBytes - bytesWritten) / speedBps)}s remaining`
                : 'Calculating...'
            };
          }
          return d;
        });
      });

      // Listen for file payment processing
      unlistenPaymentProcessing = await listen<{
        requestId: string;
        fileHash: string;
        priceWei: string;
        walletAddress: string;
      }>('file-payment-processing', (event) => {
        const { fileHash, priceWei } = event.payload;
        log.info('Processing seeder payment:', event.payload);

        downloads = downloads.map(d => {
          if (d.hash === fileHash) {
            return { ...d, speed: `Paying seeder ${formatPriceWei(priceWei)}...` };
          }
          return d;
        });
      });

      // Listen for speed tier payment completion with balance data
      unlistenSpeedTierPayment = await listen<{
        txHash: string;
        fileHash: string;
        fileName: string;
        speedTier: string;
        balanceBefore: string;
        balanceAfter: string;
      }>('speed-tier-payment-complete', (event) => {
        const { fileHash, balanceBefore, balanceAfter } = event.payload;
        log.info('Speed tier payment complete:', event.payload);

        // Store balance data on the active download so it transfers to history
        downloads = downloads.map(d => {
          if (d.hash === fileHash) {
            return { ...d, balanceBefore, balanceAfter };
          }
          return d;
        });
      });

      log.info('Download event listeners registered');
    } catch (error) {
      log.error('Failed to setup event listeners:', error);
    }
  }

  // Cleanup event listeners
  function cleanupEventListeners() {
    if (unlistenDownloadComplete) {
      unlistenDownloadComplete();
      unlistenDownloadComplete = null;
    }
    if (unlistenDownloadFailed) {
      unlistenDownloadFailed();
      unlistenDownloadFailed = null;
    }
    if (unlistenDownloadProgress) {
      unlistenDownloadProgress();
      unlistenDownloadProgress = null;
    }
    if (unlistenPaymentProcessing) {
      unlistenPaymentProcessing();
      unlistenPaymentProcessing = null;
    }
    if (unlistenSpeedTierPayment) {
      unlistenSpeedTierPayment();
      unlistenSpeedTierPayment = null;
    }
  }

  // Initialize
  onMount(() => {
    isTauri = checkTauriAvailability();
    loadDownloadHistory();
    setupEventListeners();
    refreshWalletBalance();
  });

  // Refresh balance when wallet changes
  $effect(() => {
    if ($walletAccount?.address) {
      refreshWalletBalance();
    } else {
      walletBalance = '0';
    }
  });

  onDestroy(() => {
    cleanupEventListeners();
  });

  // Get status badge color
  function getStatusBadgeColor(status: string): string {
    switch (status) {
      case 'completed': return 'bg-green-100 text-green-800 dark:bg-green-900/40 dark:text-green-400';
      case 'downloading': return 'bg-blue-100 text-blue-800 dark:bg-blue-900/40 dark:text-blue-400';
      case 'paused': return 'bg-yellow-100 text-yellow-800 dark:bg-yellow-900/40 dark:text-yellow-400';
      case 'failed': return 'bg-red-100 text-red-800 dark:bg-red-900/40 dark:text-red-400';
      case 'cancelled': return 'bg-gray-100 text-gray-800 dark:bg-gray-700 dark:text-gray-400';
      case 'queued': return 'bg-gray-100 text-gray-800 dark:bg-gray-700 dark:text-gray-400';
      default: return 'bg-gray-100 text-gray-800 dark:bg-gray-700 dark:text-gray-400';
    }
  }

  // Open a downloaded file with the system default application
  async function handleOpenFile(filePath: string) {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('open_file', { path: filePath });
    } catch (error) {
      log.error('Failed to open file:', error);
      toasts.show(`Failed to open file: ${error}`, 'error');
    }
  }

  // Show a downloaded file in the system file manager
  async function handleShowInFolder(filePath: string) {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('show_in_folder', { path: filePath });
    } catch (error) {
      log.error('Failed to show in folder:', error);
      toasts.show(`Failed to show in folder: ${error}`, 'error');
    }
  }

  async function handlePreviewFile(filePath: string, fileName: string) {
    if (!isTauri) {
      toasts.show('In-app preview requires the desktop app', 'error');
      return;
    }

    const previewType = getPreviewType(fileName || filePath);
    if (previewType === 'unsupported') {
      toasts.show('Preview is not supported for this file type', 'warning');
      return;
    }

    try {
      const { convertFileSrc } = await import('@tauri-apps/api/core');
      viewerType = previewType;
      viewerSource = convertFileSrc(filePath);
      viewerName = fileName || filePath.split(/[\\/]/).pop() || 'Preview';
      viewerError = null;
      isViewerOpen = true;
    } catch (error) {
      log.error('Failed to preview file:', error);
      toasts.show(`Failed to preview file: ${error}`, 'error');
    }
  }

  function closeViewer() {
    isViewerOpen = false;
    viewerSource = '';
    viewerType = 'unsupported';
    viewerName = '';
    viewerError = null;
  }

  function getTierLabel(tier?: SpeedTier): string {
    switch (tier) {
      case 'standard': return 'Standard';
      case 'premium': return 'Premium';
      case 'ultra': return 'Ultra';
      default: return 'Standard';
    }
  }

  function getTierBadgeColor(tier?: SpeedTier): string {
    switch (tier) {
      case 'ultra': return 'bg-purple-100 text-purple-700 dark:bg-purple-900/40 dark:text-purple-400';
      case 'premium': return 'bg-amber-100 text-amber-700 dark:bg-amber-900/40 dark:text-amber-400';
      default: return 'bg-blue-100 text-blue-600 dark:bg-blue-900/40 dark:text-blue-400';
    }
  }

  function formatDuration(start: Date, end: Date): string {
    const seconds = Math.round((end.getTime() - start.getTime()) / 1000);
    if (seconds < 60) return `${seconds}s`;
    const minutes = Math.floor(seconds / 60);
    const remainingSecs = seconds % 60;
    if (minutes < 60) return `${minutes}m ${remainingSecs}s`;
    const hours = Math.floor(minutes / 60);
    return `${hours}h ${minutes % 60}m`;
  }
</script>

<div class="p-6 space-y-6">
  <div>
    <h1 class="text-3xl font-bold dark:text-white">Download</h1>
    <p class="text-gray-600 dark:text-gray-400 mt-2">Search and download files from the Chiral Network</p>
  </div>

  <!-- Network Status Warning -->
  {#if !$networkConnected}
    <div class="bg-yellow-50 dark:bg-yellow-900/30 border border-yellow-200 dark:border-yellow-800 rounded-lg p-4">
      <div class="flex items-start gap-3">
        <div class="text-yellow-600 dark:text-yellow-400 mt-0.5">!</div>
        <div>
          <p class="text-sm font-semibold text-yellow-800 dark:text-yellow-300">Network Not Connected</p>
          <p class="text-sm text-yellow-700 dark:text-yellow-400">
            Please connect to the DHT network from the Network page before downloading files.
          </p>
        </div>
      </div>
    </div>
  {/if}

  <!-- Add New Download Section -->
  <div class="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-6">
    <div class="flex items-center gap-2 mb-4">
      <Plus class="w-5 h-5 text-gray-600 dark:text-gray-400" />
      <h2 class="text-lg font-semibold dark:text-white">Add New Download</h2>
    </div>

    <!-- Search Mode Tabs -->
    <div class="flex gap-2 mb-4">
      <button
        onclick={() => { searchMode = 'hash'; searchQuery = ''; searchResult = null; searchError = null; }}
        class="flex items-center gap-2 px-4 py-2 rounded-lg border transition-all {searchMode === 'hash' ? 'border-primary-500 bg-primary-50 dark:bg-primary-900/30 text-primary-700 dark:text-primary-400' : 'border-gray-300 dark:border-gray-600 text-gray-700 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-gray-700'}"
      >
        <Search class="w-4 h-4" />
        Merkle Hash
      </button>
      <button
        onclick={() => { searchMode = 'magnet'; searchQuery = ''; searchResult = null; searchError = null; }}
        class="flex items-center gap-2 px-4 py-2 rounded-lg border transition-all {searchMode === 'magnet' ? 'border-purple-500 bg-purple-50 dark:bg-purple-900/30 text-purple-700 dark:text-purple-400' : 'border-gray-300 dark:border-gray-600 text-gray-700 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-gray-700'}"
      >
        <Link class="w-4 h-4" />
        Magnet Link
      </button>
      <button
        onclick={() => { searchMode = 'torrent'; searchQuery = ''; searchResult = null; searchError = null; }}
        class="flex items-center gap-2 px-4 py-2 rounded-lg border transition-all {searchMode === 'torrent' ? 'border-green-500 bg-green-50 dark:bg-green-900/30 text-green-700 dark:text-green-400' : 'border-gray-300 dark:border-gray-600 text-gray-700 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-gray-700'}"
      >
        <FileUp class="w-4 h-4" />
        .torrent File
      </button>
    </div>

    <!-- Search Input -->
    {#if searchMode === 'torrent'}
      <div class="text-center py-8 border-2 border-dashed border-gray-300 dark:border-gray-600 rounded-lg">
        <FileUp class="w-12 h-12 mx-auto text-gray-400 mb-3" />
        <p class="text-gray-600 dark:text-gray-400 mb-4">Upload a .torrent file to start downloading</p>
        <button
          onclick={handleTorrentFile}
          disabled={!$networkConnected}
          class="px-6 py-3 bg-green-600 text-white rounded-lg hover:bg-green-700 disabled:opacity-50 disabled:cursor-not-allowed transition-all"
        >
          Select .torrent File
        </button>
      </div>
    {:else}
      <div class="relative">
        <div class="flex gap-3">
          <div class="flex-1 relative">
            <input
              type="text"
              bind:value={searchQuery}
              placeholder={searchMode === 'hash' ? 'Enter SHA-256 hash (64 characters)' : 'Paste magnet link (magnet:?xt=urn:btih:...)'}
              class="w-full px-4 py-3 border border-gray-300 dark:border-gray-600 rounded-lg focus:ring-2 focus:ring-primary-500 focus:border-primary-500 font-mono text-sm dark:bg-gray-700 dark:text-gray-200"
              onkeydown={(e) => e.key === 'Enter' && searchFile()}
              onfocus={() => showSearchHistory = true}
              onblur={() => setTimeout(() => showSearchHistory = false, 200)}
            />
          </div>

          <button
            onclick={searchFile}
            disabled={isSearching || !$networkConnected}
            class="px-6 py-3 bg-primary-600 text-white rounded-lg hover:bg-primary-700 disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2 transition-all"
          >
            {#if isSearching}
              <Loader2 class="w-5 h-5 animate-spin" />
              Searching...
            {:else}
              <Search class="w-5 h-5" />
              Search
            {/if}
          </button>
        </div>
      </div>

      <p class="text-xs text-gray-500 dark:text-gray-400 mt-2">
        {#if searchMode === 'hash'}
          Enter a 64-character SHA-256 Merkle root hash to search for files
        {:else}
          Paste a magnet link starting with "magnet:?xt=urn:btih:"
        {/if}
      </p>
    {/if}

    <!-- Search Result -->
    {#if searchResult}
      {@const ResultFileIcon = getFileIcon(searchResult.fileName)}
      <div class="mt-6 bg-gray-50 dark:bg-gray-700 rounded-lg p-4 border border-gray-200 dark:border-gray-600">
        <!-- File info row -->
        <div class="flex items-start gap-4">
          <div class="flex items-center justify-center w-14 h-14 bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-600 flex-shrink-0">
            <ResultFileIcon class="w-7 h-7 {getFileColor(searchResult.fileName)}" />
          </div>

          <div class="flex-1 min-w-0">
            <h3 class="text-lg font-semibold truncate dark:text-white">{searchResult.fileName}</h3>
            <div class="flex items-center gap-4 text-sm text-gray-600 dark:text-gray-400 mt-1">
              {#if searchResult.fileSize > 0}
                <span>{formatFileSize(searchResult.fileSize)}</span>
              {/if}
              <span class="{searchResult.seeders.length > 0 ? 'text-green-600 dark:text-green-400' : 'text-amber-600 dark:text-amber-400'}">
                {searchResult.seeders.length > 0 ? `${searchResult.seeders.length} seeder${searchResult.seeders.length !== 1 ? 's' : ''} found` : 'No seeders available'}
              </span>
              {#if searchResult.walletAddress && seederRatings[searchResult.walletAddress]?.count > 0}
                {@const rating = seederRatings[searchResult.walletAddress]}
                <span class="inline-flex items-center gap-1 px-2 py-0.5 text-xs font-medium rounded bg-yellow-100 text-yellow-800 dark:bg-yellow-900/30 dark:text-yellow-400">
                  <Star class="w-3 h-3 fill-current" />
                  {rating.average.toFixed(1)} ({rating.count})
                </span>
              {/if}
              <span class="px-2 py-0.5 text-xs font-medium rounded bg-amber-100 text-amber-800 dark:bg-amber-900/30 dark:text-amber-400">
                {formatPriceWei(searchResult.priceWei || '0')}
              </span>
            </div>
            <p class="text-xs text-gray-500 dark:text-gray-400 font-mono mt-2 truncate">
              {searchResult.hash}
            </p>
            {#if searchResult.seeders.length > 0}
              <p class="text-xs text-gray-400 dark:text-gray-500 mt-1">
                Seeder availability is verified when download starts
              </p>
            {/if}
          </div>
        </div>

        <!-- Speed Tier Selector -->
        <div class="mt-4 pt-4 border-t border-gray-200 dark:border-gray-600">
          <p class="text-sm font-medium text-gray-700 dark:text-gray-300 mb-3">⚡ Select Download Speed</p>
          <div class="grid grid-cols-3 gap-3">
            {#each TIERS as tier}
              {@const fileSizeKnown = searchResult.fileSize > 0}
              {@const TierIcon = getTierIcon(tier.id)}
              {@const cost = fileSizeKnown ? calculateCost(tier.id, searchResult.fileSize) : 0}
              {@const isPaid = tier.costPerMb > 0}
              {@const isSelected = selectedTier === tier.id}
              {@const needsWallet = isPaid && !$walletAccount}
              {@const insufficientBalance = fileSizeKnown && cost > 0 && parseFloat(walletBalance) < cost}
              {@const isDisabled = needsWallet || insufficientBalance}
              <button
                onclick={() => { if (!isDisabled) selectedTier = tier.id; }}
                disabled={isDisabled}
                class="relative p-3 rounded-lg border-2 text-left transition-all
                  {isSelected
                    ? 'border-primary-500 bg-primary-50 dark:bg-primary-900/30 ring-1 ring-primary-500'
                    : isDisabled
                      ? 'border-gray-200 dark:border-gray-600 opacity-50 cursor-not-allowed'
                      : 'border-gray-200 dark:border-gray-600 hover:border-gray-400 dark:hover:border-gray-500 cursor-pointer'
                  }"
              >
                <div class="flex items-center gap-2 mb-1">
                  <TierIcon class="w-4 h-4 {isSelected ? 'text-primary-600 dark:text-primary-400' : 'text-gray-500 dark:text-gray-400'}" />
                  <span class="text-sm font-semibold {isSelected ? 'text-primary-700 dark:text-primary-300' : 'dark:text-white'}">{tier.name}</span>
                </div>
                <p class="text-xs text-gray-500 dark:text-gray-400">{tier.speedLabel}</p>
                <p class="text-xs font-medium mt-1 text-amber-600 dark:text-amber-400">
                  {#if fileSizeKnown}
                    {formatCost(cost)}
                  {:else}
                    {tier.costPerMb} CHI/MB
                  {/if}
                </p>
                {#if needsWallet}
                  <p class="text-xs text-red-500 mt-1">Wallet required</p>
                {:else if insufficientBalance}
                  <p class="text-xs text-red-500 mt-1">Low balance</p>
                {/if}
              </button>
            {/each}
          </div>

          <!-- Download button -->
          {#if searchResult}
            {@const seederPrice = searchResult.priceWei && searchResult.priceWei !== '0' ? formatPriceWei(searchResult.priceWei) : null}
            {@const tierCostVal = searchResult.fileSize > 0 ? calculateCost(selectedTier, searchResult.fileSize) : 0}
            {@const hasCost = seederPrice || tierCostVal > 0}
            <div class="mt-4 flex items-center justify-between">
              <div class="text-sm text-gray-600 dark:text-gray-400">
                Cost:
                {#if seederPrice}
                  <span class="font-medium text-amber-600 dark:text-amber-400">{seederPrice}</span> (file)
                {/if}
                {#if seederPrice && tierCostVal > 0}
                  <span class="mx-1">+</span>
                {/if}
                {#if tierCostVal > 0}
                  <span class="font-medium text-amber-600 dark:text-amber-400">{formatCost(tierCostVal)}</span> (speed tier)
                {/if}
                {#if $walletAccount}
                  <span class="text-gray-400 mx-1">•</span>
                  Balance: <span class="font-medium">{parseFloat(walletBalance).toFixed(4)} CHI</span>
                {/if}
              </div>
            <button
              onclick={() => startDownload(searchResult!)}
              disabled={!isTauri || isProcessingPayment}
              class="px-5 py-2.5 bg-green-600 text-white rounded-lg hover:bg-green-700 disabled:opacity-50 flex items-center gap-2 transition-all font-medium"
            >
              {#if isProcessingPayment}
                <Loader2 class="w-4 h-4 animate-spin" />
                Processing...
              {:else}
                <Download class="w-4 h-4" />
                Download
              {/if}
            </button>
            </div>
          {/if}
        </div>
      </div>
    {/if}

    <!-- Search Error -->
    {#if searchError}
      <div class="mt-6 bg-red-50 dark:bg-red-900/30 rounded-lg p-4 border border-red-200 dark:border-red-800">
        <div class="flex items-center gap-3">
          <AlertCircle class="w-5 h-5 text-red-500 dark:text-red-400" />
          <p class="text-sm text-red-700 dark:text-red-300">{searchError}</p>
        </div>
      </div>
    {/if}
  </div>

  <!-- Downloads -->
  <div class="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700">
    <!-- Tabs -->
    <div class="flex items-center justify-between border-b border-gray-200 dark:border-gray-700 px-4">
      <div class="flex">
        <button
          onclick={() => downloadsTab = 'active'}
          class="flex items-center gap-2 px-4 py-3 text-sm font-medium border-b-2 transition-colors
            {downloadsTab === 'active'
              ? 'border-primary-500 text-primary-600 dark:text-primary-400'
              : 'border-transparent text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300'}"
        >
          <Download class="w-4 h-4" />
          Active
          {#if getActiveDownloads().length > 0}
            <span class="px-1.5 py-0.5 text-xs font-semibold bg-primary-100 text-primary-700 dark:bg-primary-900/50 dark:text-primary-400 rounded-full">
              {getActiveDownloads().length}
            </span>
          {/if}
        </button>
        <button
          onclick={() => downloadsTab = 'history'}
          class="flex items-center gap-2 px-4 py-3 text-sm font-medium border-b-2 transition-colors
            {downloadsTab === 'history'
              ? 'border-primary-500 text-primary-600 dark:text-primary-400'
              : 'border-transparent text-gray-500 dark:text-gray-400 hover:text-gray-700 dark:hover:text-gray-300'}"
        >
          <History class="w-4 h-4" />
          History
          {#if downloadHistory.length > 0}
            <span class="px-1.5 py-0.5 text-xs font-semibold bg-gray-100 text-gray-600 dark:bg-gray-700 dark:text-gray-400 rounded-full">
              {downloadHistory.length}
            </span>
          {/if}
        </button>
      </div>

      {#if downloadsTab === 'history' && downloadHistory.length > 0}
        <button
          onclick={clearDownloadHistory}
          class="flex items-center gap-1 px-3 py-1.5 text-sm text-red-600 dark:text-red-400 hover:bg-red-50 dark:hover:bg-red-900/30 rounded-lg transition-colors"
        >
          <Trash2 class="w-3.5 h-3.5" />
          Clear
        </button>
      {/if}
    </div>

    <!-- Active Downloads Tab -->
    {#if downloadsTab === 'active'}
      {#if downloads.length === 0}
        <div class="text-center py-16 px-6">
          <Download class="w-12 h-12 mx-auto text-gray-300 dark:text-gray-600 mb-3" />
          <p class="text-gray-500 dark:text-gray-400">No active downloads</p>
          <p class="text-sm text-gray-400 dark:text-gray-500 mt-1">Search for a file above to start downloading</p>
        </div>
      {:else}
        <div class="divide-y divide-gray-100 dark:divide-gray-700">
          {#each downloads as download (download.id)}
            {@const DownloadIcon = getFileIcon(download.name)}
            {@const TierIcon = getTierIcon(download.speedTier || 'standard')}
            {@const isActive = download.status === 'downloading' || download.status === 'paused'}
            {@const isFinished = ['completed', 'failed', 'cancelled'].includes(download.status)}
            <div class="p-4 hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors">
              <!-- Top row: icon, name, badges, actions -->
              <div class="flex items-start gap-3">
                <div class="flex items-center justify-center w-10 h-10 rounded-lg flex-shrink-0
                  {download.status === 'completed' ? 'bg-green-50 dark:bg-green-900/20' : 'bg-gray-100 dark:bg-gray-700'}">
                  <DownloadIcon class="w-5 h-5 {download.status === 'completed' ? 'text-green-500' : getFileColor(download.name)}" />
                </div>

                <div class="flex-1 min-w-0">
                  <div class="flex items-center gap-2 flex-wrap">
                    <p class="text-sm font-semibold truncate dark:text-white">{download.name}</p>
                    <span class="px-2 py-0.5 text-xs font-medium rounded-full capitalize {getStatusBadgeColor(download.status)}">
                      {download.status}
                    </span>
                    <span class="px-2 py-0.5 text-xs font-medium rounded-full {getTierBadgeColor(download.speedTier)}">
                      <TierIcon class="w-3 h-3 inline -mt-0.5" />
                      {getTierLabel(download.speedTier)}
                    </span>
                  </div>

                  <!-- Stats row -->
                  <div class="flex items-center gap-3 mt-1.5 text-xs text-gray-500 dark:text-gray-400">
                    {#if download.size > 0}
                      <span class="flex items-center gap-1">
                        {formatFileSize(download.size)}
                      </span>
                    {/if}
                    {#if isActive}
                      <span class="text-primary-600 dark:text-primary-400 font-medium">{download.speed}</span>
                      <span>{download.eta}</span>
                    {/if}
                    {#if download.status === 'completed' && download.startedAt && download.completedAt}
                      <span>Took {formatDuration(download.startedAt, download.completedAt)}</span>
                    {/if}
                    <span>{download.seeders} seeder{download.seeders !== 1 ? 's' : ''}</span>
                    <span class="text-gray-400 dark:text-gray-500">Started {formatDate(download.startedAt)}</span>
                  </div>

                  <!-- Hash (truncated) -->
                  <p class="text-xs text-gray-400 dark:text-gray-500 font-mono mt-1 truncate">{download.hash}</p>
                </div>

                <!-- Actions -->
                <div class="flex items-center gap-1 flex-shrink-0">
                  {#if download.status === 'downloading' || download.status === 'paused'}
                    <button
                      onclick={() => togglePause(download.id)}
                      class="p-1.5 hover:bg-gray-200 dark:hover:bg-gray-600 rounded-lg transition-colors"
                      title={download.status === 'downloading' ? 'Pause' : 'Resume'}
                    >
                      {#if download.status === 'downloading'}
                        <Pause class="w-4 h-4 text-gray-500 dark:text-gray-400" />
                      {:else}
                        <Play class="w-4 h-4 text-green-500" />
                      {/if}
                    </button>
                    <button
                      onclick={() => cancelDownload(download.id)}
                      class="p-1.5 hover:bg-red-50 dark:hover:bg-red-900/30 rounded-lg transition-colors"
                      title="Cancel"
                    >
                      <X class="w-4 h-4 text-gray-400 hover:text-red-500" />
                    </button>
                  {:else if download.status === 'queued'}
                    <button
                      onclick={() => cancelDownload(download.id)}
                      class="p-1.5 hover:bg-red-50 dark:hover:bg-red-900/30 rounded-lg transition-colors"
                      title="Cancel"
                    >
                      <X class="w-4 h-4 text-gray-400 hover:text-red-500" />
                    </button>
                  {:else if isFinished}
                    {#if download.status === 'completed' && download.filePath}
                      {#if canPreviewFile(download.name)}
                        <button
                          onclick={() => handlePreviewFile(download.filePath!, download.name)}
                          class="p-1.5 hover:bg-indigo-50 dark:hover:bg-indigo-900/30 rounded-lg transition-colors"
                          title="Preview in app"
                        >
                          <Eye class="w-4 h-4 text-indigo-500" />
                        </button>
                      {/if}
                      <button
                        onclick={() => handleOpenFile(download.filePath!)}
                        class="p-1.5 hover:bg-primary-50 dark:hover:bg-primary-900/30 rounded-lg transition-colors"
                        title="Open file"
                      >
                        <ExternalLink class="w-4 h-4 text-primary-500" />
                      </button>
                      <button
                        onclick={() => handleShowInFolder(download.filePath!)}
                        class="p-1.5 hover:bg-gray-200 dark:hover:bg-gray-600 rounded-lg transition-colors"
                        title="Show in folder"
                      >
                        <FolderOpen class="w-4 h-4 text-gray-500 dark:text-gray-400" />
                      </button>
                    {/if}
                    <button
                      onclick={() => moveToHistory(download.id)}
                      class="px-2.5 py-1 text-xs text-gray-500 dark:text-gray-400 hover:bg-gray-200 dark:hover:bg-gray-600 rounded-lg transition-colors"
                      title="Dismiss"
                    >
                      Dismiss
                    </button>
                  {/if}
                </div>
              </div>

              <!-- Progress Bar (for active downloads) -->
              {#if isActive}
                <div class="mt-3 ml-13">
                  <div class="flex items-center gap-3">
                    <div class="flex-1 h-2 bg-gray-200 dark:bg-gray-600 rounded-full overflow-hidden">
                      <div
                        class="h-full rounded-full transition-all duration-300 {download.status === 'paused' ? 'bg-yellow-500' : 'bg-primary-500'}"
                        style="width: {download.progress}%"
                      ></div>
                    </div>
                    <span class="text-xs font-medium text-gray-600 dark:text-gray-400 w-12 text-right">{(download.progress ?? 0).toFixed(1)}%</span>
                  </div>
                </div>
              {/if}

              <!-- Completed progress bar (full green) -->
              {#if download.status === 'completed'}
                <div class="mt-3 ml-13">
                  <div class="flex items-center gap-3">
                    <div class="flex-1 h-1.5 bg-green-200 dark:bg-green-900/30 rounded-full overflow-hidden">
                      <div class="h-full rounded-full bg-green-500 w-full"></div>
                    </div>
                    <CheckCircle class="w-4 h-4 text-green-500 flex-shrink-0" />
                  </div>
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {/if}

    <!-- History Tab -->
    {:else}
      {#if downloadHistory.length === 0}
        <div class="text-center py-16 px-6">
          <History class="w-12 h-12 mx-auto text-gray-300 dark:text-gray-600 mb-3" />
          <p class="text-gray-500 dark:text-gray-400">No download history</p>
          <p class="text-sm text-gray-400 dark:text-gray-500 mt-1">Completed and finished downloads will appear here</p>
        </div>
      {:else}
        <div class="divide-y divide-gray-100 dark:divide-gray-700">
          {#each downloadHistory as entry (entry.id)}
            {@const EntryIcon = getFileIcon(entry.fileName)}
            {@const EntryTierIcon = getTierIcon(entry.speedTier || 'standard')}
            <div class="p-4 hover:bg-gray-50 dark:hover:bg-gray-700/50 transition-colors">
              <div class="flex items-start gap-3">
                <div class="flex items-center justify-center w-10 h-10 rounded-lg flex-shrink-0
                  {entry.status === 'completed' ? 'bg-green-50 dark:bg-green-900/20' :
                   entry.status === 'failed' ? 'bg-red-50 dark:bg-red-900/20' :
                   'bg-gray-100 dark:bg-gray-700'}">
                  <EntryIcon class="w-5 h-5 {
                    entry.status === 'completed' ? 'text-green-500' :
                    entry.status === 'failed' ? 'text-red-400' :
                    getFileColor(entry.fileName)
                  }" />
                </div>

                <div class="flex-1 min-w-0">
                  <div class="flex items-center gap-2 flex-wrap">
                    <p class="text-sm font-semibold truncate dark:text-white">{entry.fileName}</p>
                    <span class="px-2 py-0.5 text-xs font-medium rounded-full capitalize {getStatusBadgeColor(entry.status)}">
                      {entry.status}
                    </span>
                    {#if entry.speedTier}
                      <span class="px-2 py-0.5 text-xs font-medium rounded-full {getTierBadgeColor(entry.speedTier)}">
                        <EntryTierIcon class="w-3 h-3 inline -mt-0.5" />
                        {getTierLabel(entry.speedTier)}
                      </span>
                    {/if}
                  </div>

                  <div class="flex items-center gap-3 mt-1.5 text-xs text-gray-500 dark:text-gray-400">
                    {#if entry.fileSize > 0}
                      <span>{formatFileSize(entry.fileSize)}</span>
                    {/if}
                    {#if entry.status === 'completed' && entry.startedAt && entry.completedAt}
                      <span>Took {formatDuration(new Date(entry.startedAt), new Date(entry.completedAt))}</span>
                    {/if}
                    {#if entry.seeders}
                      <span>{entry.seeders} seeder{entry.seeders !== 1 ? 's' : ''}</span>
                    {/if}
                    <span>{formatDate(entry.completedAt)}</span>
                  </div>

                  {#if entry.balanceBefore && entry.balanceAfter}
                    <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
                      Balance: {entry.balanceBefore} → {entry.balanceAfter} CHI
                    </p>
                  {/if}
                  <p class="text-xs text-gray-400 dark:text-gray-500 font-mono mt-1 truncate">{entry.hash}</p>
                </div>

                <!-- File actions for completed entries -->
                {#if entry.status === 'completed'}
                  <div class="flex items-center gap-1 flex-shrink-0">
                    {#if entry.filePath}
                      {#if canPreviewFile(entry.fileName)}
                        <button
                          onclick={() => handlePreviewFile(entry.filePath!, entry.fileName)}
                          class="p-1.5 hover:bg-indigo-50 dark:hover:bg-indigo-900/30 rounded-lg transition-colors"
                          title="Preview in app"
                        >
                          <Eye class="w-4 h-4 text-indigo-500" />
                        </button>
                      {/if}
                      <button
                        onclick={() => handleOpenFile(entry.filePath!)}
                        class="p-1.5 hover:bg-primary-50 dark:hover:bg-primary-900/30 rounded-lg transition-colors"
                        title="Open file"
                      >
                        <ExternalLink class="w-4 h-4 text-primary-500" />
                      </button>
                      <button
                        onclick={() => handleShowInFolder(entry.filePath!)}
                        class="p-1.5 hover:bg-gray-200 dark:hover:bg-gray-600 rounded-lg transition-colors"
                        title="Show in folder"
                      >
                        <FolderOpen class="w-4 h-4 text-gray-500 dark:text-gray-400" />
                      </button>
                    {/if}
                    <button
                      onclick={() => { searchQuery = entry.hash; searchMode = 'hash'; searchFile(); }}
                      class="p-1.5 hover:bg-gray-200 dark:hover:bg-gray-600 rounded-lg transition-colors"
                      title="Download again"
                    >
                      <Download class="w-4 h-4 text-gray-400" />
                    </button>
                  </div>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      {/if}
    {/if}
  </div>
</div>

{#if isViewerOpen}
  <div
    class="fixed inset-0 z-50 bg-black/70 flex items-center justify-center p-4"
    role="dialog"
    aria-modal="true"
    tabindex="0"
    onclick={(e) => e.target === e.currentTarget && closeViewer()}
    onkeydown={(e) => e.key === 'Escape' && closeViewer()}
  >
    <div class="w-full max-w-5xl max-h-[90vh] bg-white dark:bg-gray-900 rounded-xl border border-gray-200 dark:border-gray-700 shadow-2xl flex flex-col">
      <div class="flex items-center justify-between px-4 py-3 border-b border-gray-200 dark:border-gray-700">
        <div class="min-w-0">
          <p class="text-sm font-semibold truncate dark:text-white">{viewerName}</p>
          <p class="text-xs text-gray-500 dark:text-gray-400 capitalize">{viewerType} preview</p>
        </div>
        <button
          onclick={closeViewer}
          class="p-1.5 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
          title="Close preview"
        >
          <X class="w-5 h-5 text-gray-500 dark:text-gray-300" />
        </button>
      </div>

      <div class="flex-1 p-4 overflow-auto bg-gray-50 dark:bg-gray-950">
        {#if viewerError}
          <div class="h-full flex items-center justify-center">
            <p class="text-sm text-red-600 dark:text-red-400">{viewerError}</p>
          </div>
        {:else if viewerType === 'video'}
          <video
            class="w-full h-full max-h-[75vh] rounded-lg bg-black"
            controls
            src={viewerSource}
            onerror={() => viewerError = 'Video preview failed to load'}
          >
            <track kind="captions" srclang="en" label="English captions" />
          </video>
        {:else if viewerType === 'audio'}
          <div class="h-full flex items-center justify-center">
            <audio
              class="w-full max-w-2xl"
              controls
              src={viewerSource}
              onerror={() => viewerError = 'Audio preview failed to load'}
            ></audio>
          </div>
        {:else if viewerType === 'image'}
          <img
            class="max-h-[75vh] mx-auto object-contain rounded-lg"
            src={viewerSource}
            alt={viewerName}
            onerror={() => viewerError = 'Image preview failed to load'}
          />
        {:else if viewerType === 'pdf'}
          <iframe
            class="w-full h-[75vh] rounded-lg border border-gray-200 dark:border-gray-700 bg-white"
            src={viewerSource}
            title={viewerName}
          ></iframe>
        {:else}
          <div class="h-full flex items-center justify-center">
            <p class="text-sm text-gray-600 dark:text-gray-400">Preview is not supported for this file type.</p>
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}

{#if blacklistWarning}
  <BlacklistWarningModal
    address={blacklistWarning.match.address}
    reason={blacklistWarning.match.reason}
    action="download this file"
    onconfirm={() => {
      const result = blacklistWarning!.result;
      blacklistWarning = null;
      startDownload(result, true);
    }}
    oncancel={() => { blacklistWarning = null; }}
  />
{/if}

{#if pendingDownload}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 bg-black/50 flex items-center justify-center z-50"
    onkeydown={(e: KeyboardEvent) => { if (e.key === 'Escape') pendingDownload = null; }}
    onclick={(e: MouseEvent) => { if (e.target === e.currentTarget) pendingDownload = null; }}
  >
    <div class="bg-white dark:bg-gray-800 rounded-xl shadow-xl border border-gray-200 dark:border-gray-700 p-6 max-w-md w-full mx-4">
      <div class="flex items-center gap-3 mb-4">
        <div class="p-2.5 bg-amber-100 dark:bg-amber-900/30 rounded-lg">
          <AlertTriangle class="w-6 h-6 text-amber-600 dark:text-amber-400" />
        </div>
        <h3 class="text-lg font-semibold dark:text-white">Confirm Download</h3>
      </div>

      <div class="space-y-3 mb-5">
        <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-3">
          <p class="text-sm text-gray-500 dark:text-gray-400">File</p>
          <p class="font-medium dark:text-white truncate">{pendingDownload.result.fileName}</p>
          {#if pendingDownload.result.fileSize > 0}
            <p class="text-xs text-gray-400 mt-0.5">{formatFileSize(pendingDownload.result.fileSize)}</p>
          {/if}
        </div>

        <div class="bg-gray-50 dark:bg-gray-700/50 rounded-lg p-3 space-y-2">
          <p class="text-sm text-gray-500 dark:text-gray-400">Cost Breakdown</p>
          {#if pendingDownload.seederPriceChi > 0}
            <div class="flex justify-between text-sm">
              <span class="text-gray-600 dark:text-gray-300">File price</span>
              <span class="font-medium text-amber-600 dark:text-amber-400">{pendingDownload.seederPriceChi.toFixed(6)} CHI</span>
            </div>
          {/if}
          {#if pendingDownload.tierCost > 0}
            <div class="flex justify-between text-sm">
              <span class="text-gray-600 dark:text-gray-300">Speed tier ({selectedTier})</span>
              <span class="font-medium text-amber-600 dark:text-amber-400">{formatCost(pendingDownload.tierCost)}</span>
            </div>
          {/if}
          <div class="flex justify-between text-sm pt-2 border-t border-gray-200 dark:border-gray-600">
            <span class="font-semibold dark:text-white">Total</span>
            <span class="font-semibold text-amber-600 dark:text-amber-400">{pendingDownload.totalCost.toFixed(6)} CHI</span>
          </div>
        </div>

        <div class="flex justify-between text-sm text-gray-500 dark:text-gray-400 px-1">
          <span>Your balance</span>
          <span>{parseFloat(walletBalance).toFixed(4)} CHI</span>
        </div>
      </div>

      <div class="flex gap-3">
        <button
          onclick={() => { pendingDownload = null; }}
          class="flex-1 px-4 py-2.5 border border-gray-300 dark:border-gray-600 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors dark:text-gray-300 font-medium"
        >
          Cancel
        </button>
        <button
          onclick={() => {
            const result = pendingDownload!.result;
            pendingDownload = null;
            startDownload(result, true, true);
          }}
          class="flex-1 px-4 py-2.5 bg-green-600 text-white rounded-lg hover:bg-green-700 transition-colors flex items-center justify-center gap-2 font-medium"
        >
          <Download class="w-4 h-4" />
          Confirm & Pay
        </button>
      </div>
    </div>
  </div>
{/if}

{#if showRatingModal}
  <RateSeederModal
    seederWallet={showRatingModal.seederWallet}
    fileHash={showRatingModal.fileHash}
    fileName={showRatingModal.fileName}
    onclose={() => { showRatingModal = null; }}
  />
{/if}
