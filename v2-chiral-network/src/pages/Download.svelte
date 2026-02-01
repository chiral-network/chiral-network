<script lang="ts">
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
    Loader2
  } from 'lucide-svelte';
  import { networkConnected } from '$lib/stores';
  import { showToast } from '$lib/toastStore';

  // Check if running in Tauri environment
  const isTauri = typeof window !== 'undefined' && '__TAURI__' in window;

  // Types
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
    status: 'queued' | 'downloading' | 'paused' | 'completed' | 'failed';
    progress: number;
    speed: string;
    eta: string;
    seeders: number;
  }

  interface HistoryItem {
    hash: string;
    fileName: string;
    searchedAt: number;
    found: boolean;
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

  // State
  let searchQuery = $state('');
  let isSearching = $state(false);
  let searchResult = $state<SearchResult | null>(null);
  let searchError = $state<string | null>(null);
  let downloads = $state<DownloadItem[]>([]);
  let searchHistory = $state<HistoryItem[]>([]);
  let showHistory = $state(false);

  // Load search history from localStorage
  function loadHistory() {
    try {
      const stored = localStorage.getItem('download_search_history');
      if (stored) {
        searchHistory = JSON.parse(stored);
      }
    } catch (e) {
      console.error('Failed to load search history:', e);
    }
  }

  function saveHistory() {
    try {
      localStorage.setItem('download_search_history', JSON.stringify(searchHistory.slice(0, 20)));
    } catch (e) {
      console.error('Failed to save search history:', e);
    }
  }

  function addToHistory(hash: string, fileName: string, found: boolean) {
    const entry: HistoryItem = {
      hash,
      fileName,
      searchedAt: Date.now(),
      found
    };
    // Remove duplicate if exists
    searchHistory = searchHistory.filter(h => h.hash !== hash);
    // Add to front
    searchHistory = [entry, ...searchHistory].slice(0, 20);
    saveHistory();
  }

  // Search for file by hash
  async function searchFile() {
    if (!searchQuery.trim()) {
      showToast('Please enter a file hash', 'error');
      return;
    }

    if (!$networkConnected) {
      showToast('Please connect to the network first', 'error');
      return;
    }

    isSearching = true;
    searchResult = null;
    searchError = null;

    try {
      if (isTauri) {
        const { invoke } = await import('@tauri-apps/api/core');
        const result = await invoke<SearchResult>('search_file', {
          fileHash: searchQuery.trim()
        });

        if (result) {
          searchResult = result;
          addToHistory(result.hash, result.fileName, true);
          showToast(`Found: ${result.fileName}`, 'success');
        } else {
          searchError = 'File not found on the network';
          addToHistory(searchQuery.trim(), 'Unknown', false);
        }
      } else {
        // Web fallback - simulate search
        await new Promise(resolve => setTimeout(resolve, 1000));
        searchError = 'Search requires the desktop application';
      }
    } catch (error) {
      console.error('Search failed:', error);
      searchError = `Search failed: ${error}`;
      addToHistory(searchQuery.trim(), 'Unknown', false);
    } finally {
      isSearching = false;
    }
  }

  // Start download
  async function startDownload(result: SearchResult) {
    if (!isTauri) {
      showToast('Download requires the desktop app', 'error');
      return;
    }

    // Check if already downloading
    if (downloads.some(d => d.hash === result.hash && d.status !== 'completed' && d.status !== 'failed')) {
      showToast('This file is already being downloaded', 'warning');
      return;
    }

    const newDownload: DownloadItem = {
      id: `download-${Date.now()}`,
      hash: result.hash,
      name: result.fileName,
      size: result.fileSize,
      status: 'downloading',
      progress: 0,
      speed: '0 B/s',
      eta: 'Calculating...',
      seeders: result.seeders.length
    };

    downloads = [...downloads, newDownload];

    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('start_download', {
        fileHash: result.hash,
        fileName: result.fileName,
        seeders: result.seeders
      });
      showToast(`Download started: ${result.fileName}`, 'info');
    } catch (error) {
      console.error('Download failed:', error);
      downloads = downloads.map(d =>
        d.id === newDownload.id ? { ...d, status: 'failed' as const } : d
      );
      showToast(`Download failed: ${error}`, 'error');
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
  }

  // Cancel download
  function cancelDownload(downloadId: string) {
    const download = downloads.find(d => d.id === downloadId);
    downloads = downloads.filter(d => d.id !== downloadId);
    if (download) {
      showToast(`Cancelled: ${download.name}`, 'info');
    }
  }

  // Clear completed downloads
  function clearCompleted() {
    const completedCount = downloads.filter(d => d.status === 'completed').length;
    downloads = downloads.filter(d => d.status !== 'completed');
    if (completedCount > 0) {
      showToast(`Cleared ${completedCount} completed download${completedCount > 1 ? 's' : ''}`, 'info');
    }
  }

  // Handle history item click
  function selectHistoryItem(item: HistoryItem) {
    searchQuery = item.hash;
    showHistory = false;
    searchFile();
  }

  // Initialize
  $effect(() => {
    loadHistory();
  });

  // Get status color
  function getStatusColor(status: DownloadItem['status']): string {
    switch (status) {
      case 'downloading': return 'text-blue-500';
      case 'paused': return 'text-yellow-500';
      case 'completed': return 'text-green-500';
      case 'failed': return 'text-red-500';
      case 'queued': return 'text-gray-500';
      default: return 'text-gray-500';
    }
  }

  // Get status icon
  function getStatusIcon(status: DownloadItem['status']) {
    switch (status) {
      case 'downloading': return Loader2;
      case 'paused': return Pause;
      case 'completed': return CheckCircle;
      case 'failed': return AlertCircle;
      case 'queued': return Clock;
      default: return Clock;
    }
  }
</script>

<div class="p-6 space-y-6">
  <div>
    <h1 class="text-3xl font-bold">Download</h1>
    <p class="text-gray-600 mt-2">Search and download files from the Chiral Network</p>
  </div>

  <!-- Network Status Warning -->
  {#if !$networkConnected}
    <div class="bg-yellow-50 border border-yellow-200 rounded-lg p-4">
      <div class="flex items-start gap-3">
        <div class="text-yellow-600 mt-0.5">!</div>
        <div>
          <p class="text-sm font-semibold text-yellow-800">Network Not Connected</p>
          <p class="text-sm text-yellow-700">
            Please connect to the DHT network from the Network page before downloading files.
          </p>
        </div>
      </div>
    </div>
  {/if}

  <!-- Search Section -->
  <div class="bg-white rounded-lg border border-gray-200 p-6">
    <h2 class="text-lg font-semibold mb-4">Search by File Hash</h2>

    <div class="relative">
      <div class="flex gap-3">
        <div class="flex-1 relative">
          <input
            type="text"
            bind:value={searchQuery}
            placeholder="Enter file hash (SHA-256)"
            class="w-full px-4 py-3 border border-gray-300 rounded-lg focus:ring-2 focus:ring-blue-500 focus:border-blue-500 font-mono text-sm"
            onkeydown={(e) => e.key === 'Enter' && searchFile()}
            onfocus={() => showHistory = true}
            onblur={() => setTimeout(() => showHistory = false, 200)}
          />

          <!-- Search History Dropdown -->
          {#if showHistory && searchHistory.length > 0}
            <div class="absolute z-10 w-full mt-1 bg-white border border-gray-200 rounded-lg shadow-lg max-h-60 overflow-y-auto">
              <div class="p-2 border-b border-gray-100">
                <div class="flex items-center gap-2 text-xs text-gray-500">
                  <History class="w-3 h-3" />
                  Recent Searches
                </div>
              </div>
              {#each searchHistory as item}
                <button
                  class="w-full px-3 py-2 text-left hover:bg-gray-50 flex items-center justify-between"
                  onmousedown={() => selectHistoryItem(item)}
                >
                  <div class="flex-1 min-w-0">
                    <p class="text-sm font-mono truncate">{item.hash.slice(0, 16)}...{item.hash.slice(-8)}</p>
                    <p class="text-xs text-gray-500">{item.fileName}</p>
                  </div>
                  <span class={item.found ? 'text-green-500' : 'text-red-500'}>
                    {item.found ? 'Found' : 'Not found'}
                  </span>
                </button>
              {/each}
            </div>
          {/if}
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

    <p class="text-xs text-gray-500 mt-2">
      Enter a 64-character SHA-256 hash to search for files on the network
    </p>

    <!-- Search Result -->
    {#if searchResult}
      <div class="mt-6 bg-gray-50 rounded-lg p-4 border border-gray-200">
        <div class="flex items-start gap-4">
          <div class="flex items-center justify-center w-14 h-14 bg-white rounded-lg border border-gray-200">
            <svelte:component this={getFileIcon(searchResult.fileName)} class="w-7 h-7 {getFileColor(searchResult.fileName)}" />
          </div>

          <div class="flex-1 min-w-0">
            <h3 class="text-lg font-semibold truncate">{searchResult.fileName}</h3>
            <div class="flex items-center gap-4 text-sm text-gray-600 mt-1">
              <span>{formatFileSize(searchResult.fileSize)}</span>
              <span class="text-green-600">{searchResult.seeders.length} seeder{searchResult.seeders.length !== 1 ? 's' : ''}</span>
            </div>
            <p class="text-xs text-gray-500 font-mono mt-2 truncate">
              {searchResult.hash}
            </p>
          </div>

          <button
            onclick={() => startDownload(searchResult!)}
            disabled={!isTauri}
            class="px-4 py-2 bg-green-600 text-white rounded-lg hover:bg-green-700 disabled:opacity-50 flex items-center gap-2 transition-all"
          >
            <Download class="w-4 h-4" />
            Download
          </button>
        </div>
      </div>
    {/if}

    <!-- Search Error -->
    {#if searchError}
      <div class="mt-6 bg-red-50 rounded-lg p-4 border border-red-200">
        <div class="flex items-center gap-3">
          <AlertCircle class="w-5 h-5 text-red-500" />
          <p class="text-sm text-red-700">{searchError}</p>
        </div>
      </div>
    {/if}
  </div>

  <!-- Active Downloads -->
  <div class="bg-white rounded-lg border border-gray-200 p-6">
    <div class="flex items-center justify-between mb-4">
      <h2 class="text-lg font-semibold">Downloads</h2>

      {#if downloads.some(d => d.status === 'completed')}
        <button
          onclick={clearCompleted}
          class="text-sm text-gray-600 hover:text-gray-900"
        >
          Clear Completed
        </button>
      {/if}
    </div>

    {#if downloads.length === 0}
      <div class="text-center py-12">
        <Download class="w-16 h-16 mx-auto text-gray-300 mb-4" />
        <p class="text-gray-600">No active downloads</p>
        <p class="text-sm text-gray-500 mt-1">Search for a file to start downloading</p>
      </div>
    {:else}
      <div class="space-y-4">
        {#each downloads as download (download.id)}
          <div class="border border-gray-200 rounded-lg p-4">
            <div class="flex items-center gap-4">
              <!-- File Icon -->
              <div class="flex items-center justify-center w-12 h-12 bg-gray-100 rounded-lg">
                <svelte:component this={getFileIcon(download.name)} class="w-6 h-6 {getFileColor(download.name)}" />
              </div>

              <!-- File Info -->
              <div class="flex-1 min-w-0">
                <div class="flex items-center gap-2">
                  <p class="text-sm font-semibold truncate">{download.name}</p>
                  <svelte:component
                    this={getStatusIcon(download.status)}
                    class="w-4 h-4 {getStatusColor(download.status)} {download.status === 'downloading' ? 'animate-spin' : ''}"
                  />
                </div>

                <div class="flex items-center gap-4 text-xs text-gray-500 mt-1">
                  <span>{formatFileSize(download.size)}</span>
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
                      <span class="text-gray-600">{download.progress.toFixed(1)}%</span>
                    </div>
                    <div class="h-2 bg-gray-200 rounded-full overflow-hidden">
                      <div
                        class="h-full bg-blue-500 transition-all duration-300"
                        style="width: {download.progress}%"
                      ></div>
                    </div>
                  </div>
                {/if}
              </div>

              <!-- Actions -->
              <div class="flex items-center gap-2">
                {#if download.status === 'downloading' || download.status === 'paused'}
                  <button
                    onclick={() => togglePause(download.id)}
                    class="p-2 hover:bg-gray-100 rounded-lg transition-colors"
                    title={download.status === 'downloading' ? 'Pause' : 'Resume'}
                  >
                    {#if download.status === 'downloading'}
                      <Pause class="w-4 h-4 text-gray-600" />
                    {:else}
                      <Play class="w-4 h-4 text-gray-600" />
                    {/if}
                  </button>
                {/if}

                {#if download.status !== 'completed'}
                  <button
                    onclick={() => cancelDownload(download.id)}
                    class="p-2 hover:bg-red-50 rounded-lg transition-colors"
                    title="Cancel"
                  >
                    <X class="w-4 h-4 text-gray-400 hover:text-red-500" />
                  </button>
                {/if}
              </div>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
