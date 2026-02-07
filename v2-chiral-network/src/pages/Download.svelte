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
    Clock,
    CheckCircle,
    AlertCircle,
    History,
    Loader2,
    Link,
    FileUp,
    Plus,
    Trash2
  } from 'lucide-svelte';
  import { networkConnected } from '$lib/stores';
  import { toasts } from '$lib/toastStore';

  // Check if running in Tauri environment (reactive)
  let isTauri = $state(false);
  
  // Check Tauri availability
  function checkTauriAvailability(): boolean {
    return typeof window !== 'undefined' && ('__TAURI__' in window || '__TAURI_INTERNALS__' in window);
  }

  // Event listener cleanup functions
  let unlistenDownloadComplete: (() => void) | null = null;
  let unlistenDownloadFailed: (() => void) | null = null;

  // Types
  type SearchMode = 'hash' | 'magnet' | 'torrent';
  type DownloadStatus = 'queued' | 'downloading' | 'paused' | 'completed' | 'cancelled' | 'failed';

  interface SearchResult {
    hash: string;
    fileName: string;
    fileSize: number;
    seeders: string[];
    createdAt: number;
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
  }

  interface HistoryEntry {
    id: string;
    hash: string;
    fileName: string;
    fileSize: number;
    completedAt: Date;
    status: 'completed' | 'cancelled' | 'failed';
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

  // State
  let searchMode = $state<SearchMode>('hash');
  let searchQuery = $state('');
  let isSearching = $state(false);
  let searchResult = $state<SearchResult | null>(null);
  let searchError = $state<string | null>(null);
  let downloads = $state<DownloadItem[]>([]);
  let downloadHistory = $state<HistoryEntry[]>([]);
  let showSearchHistory = $state(false);
  let showDownloadHistory = $state(true);

  // Persistence keys
  const DOWNLOAD_HISTORY_KEY = 'chiral_download_history';
  const ACTIVE_DOWNLOADS_KEY = 'chiral_active_downloads';

  // Load download history from localStorage
  function loadDownloadHistory() {
    try {
      const stored = localStorage.getItem(DOWNLOAD_HISTORY_KEY);
      if (stored) {
        const parsed = JSON.parse(stored);
        downloadHistory = parsed.map((h: any) => ({
          ...h,
          completedAt: new Date(h.completedAt)
        }));
      }

      // Load active downloads
      const activeStored = localStorage.getItem(ACTIVE_DOWNLOADS_KEY);
      if (activeStored) {
        const parsed = JSON.parse(activeStored);
        downloads = parsed.map((d: any) => ({
          ...d,
          startedAt: new Date(d.startedAt),
          completedAt: d.completedAt ? new Date(d.completedAt) : undefined
        }));
      }
    } catch (e) {
      console.error('Failed to load download history:', e);
    }
  }

  function saveDownloadHistory() {
    try {
      localStorage.setItem(DOWNLOAD_HISTORY_KEY, JSON.stringify(downloadHistory));
      localStorage.setItem(ACTIVE_DOWNLOADS_KEY, JSON.stringify(downloads));
    } catch (e) {
      console.error('Failed to save download history:', e);
    }
  }

  function addToDownloadHistory(download: DownloadItem) {
    const entry: HistoryEntry = {
      id: download.id,
      hash: download.hash,
      fileName: download.name,
      fileSize: download.size,
      completedAt: new Date(),
      status: download.status as 'completed' | 'cancelled' | 'failed'
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
            createdAt: dhtResult?.createdAt || Date.now()
          };

          if (searchResult.seeders.length > 0) {
            toasts.show(`Loaded torrent: ${result.name} (${searchResult.seeders.length} seeder${searchResult.seeders.length !== 1 ? 's' : ''})`, 'success');
          } else {
            toasts.show(`Loaded torrent: ${result.name} - searching for seeders...`, 'info');
          }
        }
      }
    } catch (error) {
      console.error('Failed to parse torrent file:', error);
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
              createdAt: Date.now()
            };
            toasts.show(`Magnet link parsed but file not found in DHT. The seeder may be offline.`, 'warning');
          } else {
            searchError = 'File not found on the network';
          }
        }
      } else {
        await new Promise(resolve => setTimeout(resolve, 1000));
        searchError = 'Search requires the desktop application';
      }
    } catch (error) {
      console.error('Search failed:', error);
      searchError = `Search failed: ${error}`;
    } finally {
      isSearching = false;
    }
  }

  // Start download
  async function startDownload(result: SearchResult) {
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

    const newDownload: DownloadItem = {
      id: `download-${Date.now()}`,
      hash: result.hash,
      name: result.fileName,
      size: result.fileSize,
      status: 'downloading',
      progress: 0,
      speed: 'Connecting...',
      eta: 'Requesting file...',
      seeders: result.seeders.length,
      startedAt: new Date()
    };

    downloads = [...downloads, newDownload];
    saveDownloadHistory();

    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const response = await invoke<{ requestId: string; status: string }>('start_download', {
        fileHash: result.hash,
        fileName: result.fileName,
        seeders: result.seeders
      });

      console.log('Download request sent:', response);
      toasts.show(`Requesting file from seeder...`, 'info');

      // Update download with request ID
      downloads = downloads.map(d =>
        d.id === newDownload.id ? { ...d, id: response.requestId } : d
      );
      saveDownloadHistory();
    } catch (error) {
      console.error('Download failed:', error);
      downloads = downloads.map(d =>
        d.id === newDownload.id ? { ...d, status: 'failed' as const } : d
      );
      saveDownloadHistory();
      toasts.show(`Download failed: ${error}`, 'error');
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
        console.log('Download complete:', event.payload);
        const { fileHash, fileName, filePath, fileSize } = event.payload;

        // Update the download status
        downloads = downloads.map(d => {
          if (d.hash === fileHash) {
            return {
              ...d,
              status: 'completed' as const,
              progress: 100,
              size: fileSize || d.size,
              completedAt: new Date()
            };
          }
          return d;
        });
        saveDownloadHistory();

        toasts.show(`Downloaded: ${fileName}`, 'success');
      });

      // Listen for failed downloads
      unlistenDownloadFailed = await listen<{
        requestId: string;
        fileHash: string;
        error: string;
      }>('file-download-failed', (event) => {
        console.error('Download failed:', event.payload);
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

      console.log('Download event listeners registered');
    } catch (error) {
      console.error('Failed to setup event listeners:', error);
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
  }

  // Initialize
  onMount(() => {
    isTauri = checkTauriAvailability();
    loadDownloadHistory();
    setupEventListeners();
  });

  onDestroy(() => {
    cleanupEventListeners();
  });

  // Get status color
  function getStatusColor(status: DownloadStatus): string {
    switch (status) {
      case 'downloading': return 'text-blue-500';
      case 'paused': return 'text-yellow-500';
      case 'completed': return 'text-green-500';
      case 'failed': return 'text-red-500';
      case 'cancelled': return 'text-gray-500';
      case 'queued': return 'text-gray-500';
      default: return 'text-gray-500';
    }
  }

  // Get status icon
  function getStatusIcon(status: DownloadStatus) {
    switch (status) {
      case 'downloading': return Loader2;
      case 'paused': return Pause;
      case 'completed': return CheckCircle;
      case 'failed': return AlertCircle;
      case 'cancelled': return X;
      case 'queued': return Clock;
      default: return Clock;
    }
  }

  // Get status badge color
  function getStatusBadgeColor(status: string): string {
    switch (status) {
      case 'completed': return 'bg-green-100 text-green-800';
      case 'failed': return 'bg-red-100 text-red-800';
      case 'cancelled': return 'bg-gray-100 text-gray-800';
      default: return 'bg-gray-100 text-gray-800';
    }
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
        class="flex items-center gap-2 px-4 py-2 rounded-lg border transition-all {searchMode === 'hash' ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/30 text-blue-700 dark:text-blue-400' : 'border-gray-300 dark:border-gray-600 text-gray-700 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-gray-700'}"
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
              class="w-full px-4 py-3 border border-gray-300 dark:border-gray-600 rounded-lg focus:ring-2 focus:ring-blue-500 focus:border-blue-500 font-mono text-sm dark:bg-gray-700 dark:text-gray-200"
              onkeydown={(e) => e.key === 'Enter' && searchFile()}
              onfocus={() => showSearchHistory = true}
              onblur={() => setTimeout(() => showSearchHistory = false, 200)}
            />
          </div>

          <button
            onclick={searchFile}
            disabled={isSearching || !$networkConnected}
            class="px-6 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed flex items-center gap-2 transition-all"
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
      {@const FileIcon = getFileIcon(searchResult.fileName)}
      <div class="mt-6 bg-gray-50 dark:bg-gray-700 rounded-lg p-4 border border-gray-200 dark:border-gray-600">
        <div class="flex items-start gap-4">
          <div class="flex items-center justify-center w-14 h-14 bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-600 flex-shrink-0">
            <FileIcon class="w-7 h-7 {getFileColor(searchResult.fileName)}" />
          </div>

          <div class="flex-1 min-w-0">
            <h3 class="text-lg font-semibold truncate dark:text-white">{searchResult.fileName}</h3>
            <div class="flex items-center gap-4 text-sm text-gray-600 dark:text-gray-400 mt-1">
              {#if searchResult.fileSize > 0}
                <span>{formatFileSize(searchResult.fileSize)}</span>
              {/if}
              <span class="text-green-600 dark:text-green-400">
                {searchResult.seeders.length > 0 ? `${searchResult.seeders.length} seeder${searchResult.seeders.length !== 1 ? 's' : ''}` : 'Unknown seeders'}
              </span>
            </div>
            <p class="text-xs text-gray-500 dark:text-gray-400 font-mono mt-2 truncate">
              {searchResult.hash}
            </p>
          </div>

          <button
            onclick={() => startDownload(searchResult!)}
            disabled={!isTauri}
            class="px-4 py-2 bg-green-600 text-white rounded-lg hover:bg-green-700 disabled:opacity-50 flex items-center gap-2 transition-all flex-shrink-0"
          >
            <Download class="w-4 h-4" />
            Download
          </button>
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

  <!-- Download Tracker -->
  <div class="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-6">
    <div class="flex items-center justify-between mb-4">
      <div class="flex items-center gap-2">
        <Download class="w-5 h-5 text-gray-600 dark:text-gray-400" />
        <h2 class="text-lg font-semibold dark:text-white">Download Tracker</h2>
        <span class="text-sm text-gray-500 dark:text-gray-400">({getActiveDownloads().length} active)</span>
      </div>
    </div>

    {#if downloads.length === 0}
      <div class="text-center py-12">
        <Download class="w-16 h-16 mx-auto text-gray-300 dark:text-gray-600 mb-4" />
        <p class="text-gray-600 dark:text-gray-400">No downloads</p>
        <p class="text-sm text-gray-500 dark:text-gray-500 mt-1">Search for a file or add a magnet link to start downloading</p>
      </div>
    {:else}
      <div class="space-y-4">
        {#each downloads as download (download.id)}
          {@const DownloadIcon = getFileIcon(download.name)}
          <div class="border border-gray-200 dark:border-gray-700 rounded-lg p-4">
            <div class="flex items-center gap-4">
              <!-- File Icon -->
              <div class="flex items-center justify-center w-12 h-12 bg-gray-100 dark:bg-gray-700 rounded-lg flex-shrink-0">
                <DownloadIcon class="w-6 h-6 {getFileColor(download.name)}" />
              </div>

              <!-- File Info -->
              <div class="flex-1 min-w-0">
                <div class="flex items-center gap-2">
                  <p class="text-sm font-semibold truncate dark:text-white">{download.name}</p>
                  <span class="px-2 py-0.5 text-xs font-medium rounded capitalize {getStatusBadgeColor(download.status)}">
                    {download.status}
                  </span>
                </div>

                <div class="flex items-center gap-4 text-xs text-gray-500 dark:text-gray-400 mt-1">
                  {#if download.size > 0}
                    <span>{formatFileSize(download.size)}</span>
                  {/if}
                  {#if download.status === 'downloading'}
                    <span>{download.speed}</span>
                    <span>ETA: {download.eta}</span>
                  {/if}
                  <span>{download.seeders} seeder{download.seeders !== 1 ? 's' : ''}</span>
                </div>

                <!-- Progress Bar -->
                {#if download.status === 'downloading' || download.status === 'paused'}
                  <div class="mt-2">
                    <div class="flex items-center justify-between text-xs mb-1">
                      <span class="text-gray-600 dark:text-gray-400">{download.progress.toFixed(1)}%</span>
                    </div>
                    <div class="h-2 bg-gray-200 dark:bg-gray-600 rounded-full overflow-hidden">
                      <div
                        class="h-full transition-all duration-300 {download.status === 'paused' ? 'bg-yellow-500' : 'bg-blue-500'}"
                        style="width: {download.progress}%"
                      ></div>
                    </div>
                  </div>
                {/if}
              </div>

              <!-- Actions -->
              <div class="flex items-center gap-2 flex-shrink-0">
                {#if download.status === 'downloading' || download.status === 'paused'}
                  <button
                    onclick={() => togglePause(download.id)}
                    class="p-2 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition-colors"
                    title={download.status === 'downloading' ? 'Pause' : 'Resume'}
                  >
                    {#if download.status === 'downloading'}
                      <Pause class="w-4 h-4 text-gray-600 dark:text-gray-400" />
                    {:else}
                      <Play class="w-4 h-4 text-gray-600 dark:text-gray-400" />
                    {/if}
                  </button>
                  <button
                    onclick={() => cancelDownload(download.id)}
                    class="p-2 hover:bg-red-50 dark:hover:bg-red-900/30 rounded-lg transition-colors"
                    title="Cancel"
                  >
                    <X class="w-4 h-4 text-gray-400 hover:text-red-500" />
                  </button>
                {:else if download.status === 'queued'}
                  <button
                    onclick={() => cancelDownload(download.id)}
                    class="p-2 hover:bg-red-50 dark:hover:bg-red-900/30 rounded-lg transition-colors"
                    title="Cancel"
                  >
                    <X class="w-4 h-4 text-gray-400 hover:text-red-500" />
                  </button>
                {:else}
                  <button
                    onclick={() => moveToHistory(download.id)}
                    class="p-2 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition-colors"
                    title="Move to history"
                  >
                    <History class="w-4 h-4 text-gray-400" />
                  </button>
                {/if}
              </div>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>

  <!-- Download History -->
  <div class="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700">
    <div class="p-4 border-b border-gray-200 dark:border-gray-700 flex items-center justify-between">
      <button
        onclick={() => showDownloadHistory = !showDownloadHistory}
        class="flex items-center gap-2 text-lg font-semibold text-gray-900 dark:text-white"
      >
        <History class="w-5 h-5" />
        Download History
        <span class="text-sm font-normal text-gray-500 dark:text-gray-400">({downloadHistory.length})</span>
      </button>

      {#if downloadHistory.length > 0}
        <button
          onclick={clearDownloadHistory}
          class="flex items-center gap-1 px-3 py-1.5 text-sm text-red-600 dark:text-red-400 hover:bg-red-50 dark:hover:bg-red-900/30 rounded-lg transition-colors"
        >
          <Trash2 class="w-4 h-4" />
          Clear All
        </button>
      {/if}
    </div>

    {#if showDownloadHistory}
      {#if downloadHistory.length === 0}
        <div class="p-8 text-center">
          <History class="w-12 h-12 mx-auto text-gray-300 dark:text-gray-600 mb-3" />
          <p class="text-gray-600 dark:text-gray-400">No download history</p>
          <p class="text-sm text-gray-500 dark:text-gray-500 mt-1">Completed and finished downloads will appear here</p>
        </div>
      {:else}
        <div class="divide-y divide-gray-100 dark:divide-gray-700">
          {#each downloadHistory as entry (entry.id)}
            {@const EntryIcon = getFileIcon(entry.fileName)}
            <div class="p-4 hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors">
              <div class="flex items-center gap-4">
                <!-- File Icon -->
                <div class="flex items-center justify-center w-10 h-10 bg-gray-100 dark:bg-gray-700 rounded-lg flex-shrink-0">
                  <EntryIcon class="w-5 h-5 {getFileColor(entry.fileName)}" />
                </div>

                <!-- File Info -->
                <div class="flex-1 min-w-0">
                  <div class="flex items-center gap-2">
                    <p class="text-sm font-medium truncate text-gray-900 dark:text-white">{entry.fileName}</p>
                    <span class="px-2 py-0.5 text-xs font-medium rounded capitalize {getStatusBadgeColor(entry.status)}">
                      {entry.status}
                    </span>
                  </div>
                  <div class="flex items-center gap-4 text-xs text-gray-500 dark:text-gray-400 mt-1">
                    {#if entry.fileSize > 0}
                      <span>{formatFileSize(entry.fileSize)}</span>
                    {/if}
                    <span>{formatDate(entry.completedAt)}</span>
                  </div>
                </div>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    {/if}
  </div>
</div>
