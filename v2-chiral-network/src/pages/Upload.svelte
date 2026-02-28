<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import {
    Upload,
    FolderOpen,
    Image,
    Video,
    Music,
    Archive,
    Code,
    X,
    Plus,
    Copy,
    Globe,
    Share2,
    History,
    Trash2,
    Link,
    Download,
    ExternalLink
  } from 'lucide-svelte';
  import { getFileIcon, getFileColor } from '$lib/utils/fileIcons';
  import { networkConnected, walletAccount } from '$lib/stores';
  import { toasts } from '$lib/toastStore';
  import { logger } from '$lib/logger';
  const log = logger('Upload');

  // Check if running in Tauri environment (reactive)
  let isTauri = $state(false);
  
  // Check Tauri availability
  function checkTauriAvailability(): boolean {
    return typeof window !== 'undefined' && ('__TAURI__' in window || '__TAURI_INTERNALS__' in window);
  }

  // Protocol types
  type Protocol = 'WebRTC' | 'BitTorrent';

  // File type detection - imported from $lib/utils/fileIcons

  function getFileType(fileName: string): string {
    const ext = fileName.split('.').pop()?.toLowerCase() || '';

    if (['jpg', 'jpeg', 'png', 'gif', 'webp', 'svg', 'bmp', 'ico'].includes(ext)) return 'Image';
    if (['mp4', 'avi', 'mkv', 'mov', 'wmv', 'webm', 'flv', 'm4v'].includes(ext)) return 'Video';
    if (['mp3', 'wav', 'flac', 'aac', 'ogg', 'm4a', 'wma'].includes(ext)) return 'Audio';
    if (['zip', 'rar', '7z', 'tar', 'gz', 'bz2', 'xz'].includes(ext)) return 'Archive';
    if (['js', 'ts', 'html', 'css', 'py', 'java', 'cpp', 'c', 'php', 'rb', 'go', 'rs'].includes(ext)) return 'Code';
    if (['txt', 'md', 'pdf', 'doc', 'docx', 'rtf'].includes(ext)) return 'Document';
    if (['xls', 'xlsx', 'csv', 'ods'].includes(ext)) return 'Spreadsheet';

    return 'File';
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

  // Shared file interface
  interface SharedFile {
    id: string;
    name: string;
    size: number;
    hash: string;
    protocol: Protocol;
    fileType: string;
    seeders: number;
    uploadDate: Date;
    filePath: string;
    priceChi: string;
  }

  // State
  let isDragging = $state(false);
  let isUploading = $state(false);
  let selectedProtocol = $state<Protocol>('WebRTC');
  let sharedFiles = $state<SharedFile[]>([]);
  let showUploadHistory = $state(true);
  let filePrice = $state('');

  // Persistence keys
  const UPLOAD_HISTORY_KEY = 'chiral_upload_history';

  // Load upload history from localStorage
  function loadUploadHistory() {
    try {
      const stored = localStorage.getItem(UPLOAD_HISTORY_KEY);
      if (stored) {
        const parsed = JSON.parse(stored);
        sharedFiles = parsed.map((f: any) => ({
          ...f,
          uploadDate: new Date(f.uploadDate)
        }));
      }
    } catch (e) {
      log.error('Failed to load upload history:', e);
    }
  }

  function saveUploadHistory() {
    try {
      localStorage.setItem(UPLOAD_HISTORY_KEY, JSON.stringify(sharedFiles));
    } catch (e) {
      log.error('Failed to save upload history:', e);
    }
  }

  // Hidden file input element reference (used as fallback when native dialog fails)
  let fileInputEl: HTMLInputElement | undefined = $state(undefined);

  // Handle file selection
  async function openFileDialog() {
    // Check Tauri availability at runtime
    const tauriAvailable = checkTauriAvailability();

    if (!tauriAvailable) {
      log.error('Tauri not detected. Window properties:', Object.keys(window).filter(k => k.includes('TAURI')));
      toasts.show('File upload requires the desktop app. Please run with: npm run tauri:dev', 'error');
      return;
    }

    if (!$networkConnected) {
      toasts.show('Please connect to the network first', 'error');
      return;
    }

    if (isUploading) return;

    // macOS: skip native dialog entirely (NSOpenPanel crashes in Tauri's objc2-app-kit)
    const isMac = navigator.platform?.startsWith('Mac') || navigator.userAgent?.includes('Macintosh');
    if (isMac) {
      fileInputEl?.click();
      return;
    }

    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const selectedPaths = await invoke<string[]>('open_file_dialog', {
        multiple: true
      });

      if (selectedPaths && selectedPaths.length > 0) {
        isUploading = true;
        await processFiles(selectedPaths);
      }
    } catch (error) {
      log.warn('Native file dialog failed, using HTML fallback:', error);
      fileInputEl?.click();
    } finally {
      isUploading = false;
    }
  }

  // Handle files selected via the hidden HTML file input (macOS fallback)
  async function handleFileInputChange(e: Event) {
    const input = e.target as HTMLInputElement;
    const files = input.files;
    if (!files || files.length === 0) return;

    if (!$networkConnected) {
      toasts.show('Please connect to the network first', 'error');
      input.value = '';
      return;
    }

    isUploading = true;
    try {
      await processFileObjects(Array.from(files));
    } finally {
      isUploading = false;
      input.value = ''; // reset so the same file can be selected again
    }
  }

  // Process File objects from the HTML input — reads bytes and uses publish_file_data
  async function processFileObjects(files: File[]) {
    const { invoke } = await import('@tauri-apps/api/core');

    for (const file of files) {
      try {
        const priceChi = filePrice && parseFloat(String(filePrice)) > 0 ? String(filePrice) : undefined;
        const walletAddr = $walletAccount?.address;

        if (priceChi && !walletAddr) {
          toasts.show('Connect your wallet to set a file price', 'error');
          continue;
        }

        // Read file bytes
        const arrayBuffer = await file.arrayBuffer();
        const fileData = Array.from(new Uint8Array(arrayBuffer));

        const result = await invoke<{ merkleRoot: string }>('publish_file_data', {
          fileName: file.name,
          fileData,
          priceChi: priceChi || null,
          walletAddress: walletAddr || null,
        });

        const newFile: SharedFile = {
          id: `file-${Date.now()}-${Math.random()}`,
          name: file.name,
          size: file.size,
          hash: result.merkleRoot,
          protocol: selectedProtocol,
          fileType: getFileType(file.name),
          seeders: 1,
          uploadDate: new Date(),
          filePath: `memory:${result.merkleRoot}`,
          priceChi: priceChi || '0',
        };

        sharedFiles = [...sharedFiles, newFile];
        saveUploadHistory();
        toasts.show(`${file.name} is now being shared via ${selectedProtocol}`, 'success');
      } catch (error) {
        log.error('Failed to process file:', error);
        toasts.show(`Failed to share file: ${error}`, 'error');
      }
    }
  }

  // Process file paths from native dialog (Linux/Windows)
  async function processFiles(paths: string[]) {
    const { invoke } = await import('@tauri-apps/api/core');

    for (const filePath of paths) {
      try {
        const fileName = filePath.split(/[/\\]/).pop() || 'Unknown';

        // Get file size
        let fileSize = 0;
        try {
          fileSize = await invoke<number>('get_file_size', { filePath });
        } catch (e) {
          log.warn('Could not get file size:', e);
        }

        // Publish to DHT with selected protocol and pricing
        const priceChi = filePrice && parseFloat(String(filePrice)) > 0 ? String(filePrice) : undefined;
        const walletAddr = $walletAccount?.address;

        if (priceChi && !walletAddr) {
          toasts.show('Connect your wallet to set a file price', 'error');
          continue;
        }

        const result = await invoke<{ merkleRoot: string }>('publish_file', {
          filePath,
          fileName,
          protocol: selectedProtocol,
          priceChi: priceChi || null,
          walletAddress: walletAddr || null,
        });

        const newFile: SharedFile = {
          id: `file-${Date.now()}-${Math.random()}`,
          name: fileName,
          size: fileSize,
          hash: result.merkleRoot,
          protocol: selectedProtocol,
          fileType: getFileType(fileName),
          seeders: 1,
          uploadDate: new Date(),
          filePath,
          priceChi: priceChi || '0',
        };

        sharedFiles = [...sharedFiles, newFile];
        saveUploadHistory();
        toasts.show(`${fileName} is now being shared via ${selectedProtocol}`, 'success');
      } catch (error) {
        log.error('Failed to process file:', error);
        toasts.show(`Failed to share file: ${error}`, 'error');
      }
    }
  }

  function removeFile(id: string) {
    const file = sharedFiles.find(f => f.id === id);
    sharedFiles = sharedFiles.filter(f => f.id !== id);
    saveUploadHistory();
    if (file) {
      toasts.show(`Stopped sharing ${file.name}`, 'info');
    }
  }

  function clearAllHistory() {
    const count = sharedFiles.length;
    sharedFiles = [];
    saveUploadHistory();
    toasts.show(`Cleared ${count} file${count !== 1 ? 's' : ''} from history`, 'info');
  }

  async function copyHash(hash: string) {
    await navigator.clipboard.writeText(hash);
    toasts.show('Hash copied to clipboard', 'success');
  }

  // Generate magnet link from file info
  function generateMagnetLink(file: SharedFile): string {
    const encodedName = encodeURIComponent(file.name);
    // Using btih (BitTorrent Info Hash) format with our merkle root hash
    return `magnet:?xt=urn:btih:${file.hash}&dn=${encodedName}&xl=${file.size}`;
  }

  async function copyMagnetLink(file: SharedFile) {
    const magnetLink = generateMagnetLink(file);
    await navigator.clipboard.writeText(magnetLink);
    toasts.show('Magnet link copied to clipboard', 'success');
  }

  // Export .torrent file
  async function exportTorrentFile(file: SharedFile) {
    const tauriAvailable = checkTauriAvailability();
    if (!tauriAvailable) {
      toasts.show('Torrent export requires the desktop app', 'error');
      return;
    }

    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const result = await invoke<{ path: string }>('export_torrent_file', {
        fileHash: file.hash,
        fileName: file.name,
        fileSize: file.size,
        filePath: file.filePath
      });
      toasts.show(`Torrent file saved to ${result.path}`, 'success');
    } catch (error) {
      log.error('Failed to export torrent:', error);
      toasts.show(`Failed to export torrent: ${error}`, 'error');
    }
  }

  // State for showing share options
  let expandedFileId = $state<string | null>(null);

  function toggleShareOptions(fileId: string) {
    expandedFileId = expandedFileId === fileId ? null : fileId;
  }

  // Drag and drop handlers
  function handleDragOver(e: DragEvent) {
    e.preventDefault();
    isDragging = true;
  }

  function handleDragLeave(e: DragEvent) {
    e.preventDefault();
    isDragging = false;
  }

  function handleDrop(e: DragEvent) {
    e.preventDefault();
    isDragging = false;
    // Actual file processing is handled by Tauri's onDragDropEvent listener
  }

  // Get protocol badge color
  function getProtocolColor(protocol: Protocol): string {
    return protocol === 'WebRTC' ? 'bg-blue-100 text-blue-800' : 'bg-green-100 text-green-800';
  }

  // Track if we've already registered files for this network session
  let hasRegisteredFiles = $state(false);

  // Re-register previously shared files with the backend
  async function reregisterSharedFiles(filesToRegister: SharedFile[]) {
    if (!isTauri || filesToRegister.length === 0) return;

    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const filesToRemove: string[] = [];

      for (const file of filesToRegister) {
        try {
          await invoke('register_shared_file', {
            fileHash: file.hash,
            filePath: file.filePath,
            fileName: file.name,
            fileSize: file.size,
            priceChi: file.priceChi && file.priceChi !== '0' ? file.priceChi : null,
            walletAddress: file.priceChi && file.priceChi !== '0' ? $walletAccount?.address : null,
          });
          log.info(`Re-registered shared file: ${file.name}`);
        } catch (e) {
          log.warn(`Failed to re-register file ${file.name}:`, e);
          // File might not exist anymore, mark for removal
          if (String(e).includes('no longer exists')) {
            filesToRemove.push(file.id);
          }
        }
      }

      // Remove files that no longer exist
      if (filesToRemove.length > 0) {
        sharedFiles = sharedFiles.filter(f => !filesToRemove.includes(f.id));
        saveUploadHistory();
      }
    } catch (e) {
      log.error('Failed to re-register shared files:', e);
    }
  }

  // Watch for network connection changes to re-register files
  $effect(() => {
    const connected = $networkConnected;
    if (connected && isTauri && !hasRegisteredFiles && sharedFiles.length > 0) {
      hasRegisteredFiles = true;
      // Create a copy of the files array to avoid reactive loop
      const filesToRegister = [...sharedFiles];
      reregisterSharedFiles(filesToRegister);
    }
    // Reset when disconnected so we re-register on next connect
    if (!connected) {
      hasRegisteredFiles = false;
    }
  });

  // Initialize on mount (runs once)
  onMount(() => {
    isTauri = checkTauriAvailability();
    loadUploadHistory();
  });

  // Set up Tauri drag-drop listener
  let unlistenDragDrop: (() => void) | undefined;

  onMount(async () => {
    const tauriAvailable = checkTauriAvailability();
    if (tauriAvailable) {
      try {
        const { getCurrentWindow } = await import('@tauri-apps/api/window');
        const appWindow = getCurrentWindow();

        unlistenDragDrop = await appWindow.onDragDropEvent((event) => {
          if (event.payload.type === 'drop') {
            const paths = event.payload.paths;
            if (paths && paths.length > 0) {
              if (!$networkConnected) {
                toasts.show('Please connect to the network first', 'error');
                return;
              }
              isUploading = true;
              processFiles(paths).finally(() => {
                isUploading = false;
              });
            }
          } else if (event.payload.type === 'enter') {
            isDragging = true;
          } else if (event.payload.type === 'leave') {
            isDragging = false;
          }
        });
      } catch (error) {
        log.error('Failed to setup drag-drop listener:', error);
      }
    }
  });

  onDestroy(() => {
    unlistenDragDrop?.();
  });
</script>

<div class="p-6 space-y-6">
  <div>
    <h1 class="text-3xl font-bold dark:text-white">Upload</h1>
    <p class="text-gray-600 dark:text-gray-400 mt-2">Share files with the Chiral Network</p>
  </div>

  <!-- Network Status Warning -->
  {#if !$networkConnected}
    <div class="bg-yellow-50 dark:bg-yellow-900/30 border border-yellow-200 dark:border-yellow-800 rounded-lg p-4">
      <div class="flex items-start gap-3">
        <div class="text-yellow-600 dark:text-yellow-400 mt-0.5">!</div>
        <div>
          <p class="text-sm font-semibold text-yellow-800 dark:text-yellow-300">Network Not Connected</p>
          <p class="text-sm text-yellow-700 dark:text-yellow-400">
            Please connect to the DHT network from the Network page before uploading files.
          </p>
        </div>
      </div>
    </div>
  {/if}

  <!-- Protocol Selection -->
  <div class="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4">
    <div class="flex items-center justify-between">
      <div>
        <p class="text-sm font-semibold text-gray-900 dark:text-white">Upload Protocol</p>
        <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">Choose the protocol for file sharing</p>
      </div>
      <div class="flex gap-2">
        <button
          onclick={() => selectedProtocol = 'WebRTC'}
          class="flex items-center gap-2 px-4 py-2 rounded-lg border transition-all {selectedProtocol === 'WebRTC' ? 'border-primary-500 bg-primary-50 dark:bg-primary-900/30 text-primary-700 dark:text-primary-400' : 'border-gray-300 dark:border-gray-600 text-gray-700 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-gray-700'}"
        >
          <Globe class="w-4 h-4" />
          WebRTC
        </button>
        <button
          onclick={() => selectedProtocol = 'BitTorrent'}
          class="flex items-center gap-2 px-4 py-2 rounded-lg border transition-all {selectedProtocol === 'BitTorrent' ? 'border-green-500 bg-green-50 dark:bg-green-900/30 text-green-700 dark:text-green-400' : 'border-gray-300 dark:border-gray-600 text-gray-700 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-gray-700'}"
        >
          <Share2 class="w-4 h-4" />
          BitTorrent
        </button>
      </div>
    </div>
  </div>

  <!-- File Price -->
  <div class="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700 p-4">
    <div class="flex items-center justify-between">
      <div>
        <p class="text-sm font-semibold text-gray-900 dark:text-white">File Price</p>
        <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">Set a price in CHI tokens (leave empty for free)</p>
      </div>
      <div class="flex items-center gap-2">
        <input
          type="number"
          min="0"
          step="0.001"
          placeholder="0 (free)"
          bind:value={filePrice}
          class="w-40 px-3 py-2 text-sm bg-white dark:bg-gray-700 border border-gray-300 dark:border-gray-600 rounded-lg text-gray-900 dark:text-white placeholder-gray-400 focus:ring-2 focus:ring-primary-500 focus:border-primary-500"
        />
        <span class="text-sm text-gray-500 dark:text-gray-400">CHI</span>
      </div>
    </div>
    {#if filePrice && parseFloat(filePrice) > 0 && !$walletAccount}
      <div class="mt-3 p-2 bg-amber-50 dark:bg-amber-900/30 border border-amber-200 dark:border-amber-800 rounded-lg">
        <p class="text-xs text-amber-700 dark:text-amber-400">
          Connect your wallet on the Account page to receive payments for file downloads.
        </p>
      </div>
    {/if}
  </div>

  <!-- Drop Zone -->
  <div
    role="button"
    tabindex="0"
    ondragover={handleDragOver}
    ondragleave={handleDragLeave}
    ondrop={handleDrop}
    class="relative border-2 border-dashed rounded-xl p-8 transition-all duration-200 {isDragging ? 'border-primary-500 bg-primary-50 dark:bg-primary-900/30' : 'border-gray-300 dark:border-gray-600 hover:border-gray-400 dark:hover:border-gray-500'}"
  >
    <div class="text-center py-8">
      <div class="mb-6">
        {#if isDragging}
          <Upload class="h-16 w-16 mx-auto text-primary-500" />
        {:else}
          <FolderOpen class="h-16 w-16 mx-auto text-gray-400" />
        {/if}
      </div>

      <h3 class="text-2xl font-bold mb-3 {isDragging ? 'text-primary-600 dark:text-primary-400' : 'text-gray-900 dark:text-white'}">
        {isDragging ? 'Drop files here' : 'Share Files'}
      </h3>

      <p class="text-gray-600 dark:text-gray-400 mb-4 text-lg">
        {#if isDragging}
          Release to upload files
        {:else}
          Drag and drop files here, or click the button below
        {/if}
      </p>

      <p class="text-sm text-gray-500 dark:text-gray-400 mb-8">
        Using <span class="font-semibold {selectedProtocol === 'WebRTC' ? 'text-primary-600 dark:text-primary-400' : 'text-green-600 dark:text-green-400'}">{selectedProtocol}</span> protocol
      </p>

      <div class="flex justify-center gap-4 mb-8 opacity-60">
        <Image class="h-8 w-8 text-blue-500" />
        <Video class="h-8 w-8 text-purple-500" />
        <Music class="h-8 w-8 text-green-500" />
        <Archive class="h-8 w-8 text-orange-500" />
        <Code class="h-8 w-8 text-red-500" />
      </div>

      {#if isTauri}
        <button
          onclick={openFileDialog}
          disabled={isUploading || !$networkConnected}
          class="inline-flex items-center gap-2 px-6 py-3 text-sm font-medium text-white bg-primary-600 rounded-xl hover:bg-primary-700 disabled:opacity-50 disabled:cursor-not-allowed transition-all"
        >
          <Plus class="w-5 h-5" />
          {isUploading ? 'Uploading...' : 'Add Files'}
        </button>
      {:else}
        <p class="text-sm text-gray-500 dark:text-gray-400">
          File upload requires the desktop application
        </p>
      {/if}

      <p class="text-xs text-gray-400 mt-4">
        Supports images, videos, audio, documents, archives, and more
      </p>
    </div>
  </div>

  <!-- Upload History -->
  <div class="bg-white dark:bg-gray-800 rounded-lg border border-gray-200 dark:border-gray-700">
    <div class="p-4 border-b border-gray-200 dark:border-gray-700 flex items-center justify-between">
      <button
        onclick={() => showUploadHistory = !showUploadHistory}
        class="flex items-center gap-2 text-lg font-semibold text-gray-900 dark:text-white"
      >
        <History class="w-5 h-5" />
        Upload History
        <span class="text-sm font-normal text-gray-500 dark:text-gray-400">({sharedFiles.length})</span>
      </button>

      {#if sharedFiles.length > 0}
        <button
          onclick={clearAllHistory}
          class="flex items-center gap-1 px-3 py-1.5 text-sm text-red-600 dark:text-red-400 hover:bg-red-50 dark:hover:bg-red-900/30 rounded-lg transition-colors"
        >
          <Trash2 class="w-4 h-4" />
          Clear All
        </button>
      {/if}
    </div>

    {#if showUploadHistory}
      {#if sharedFiles.length === 0}
        <div class="p-8 text-center">
          <History class="w-12 h-12 mx-auto text-gray-300 dark:text-gray-600 mb-3" />
          <p class="text-gray-600 dark:text-gray-400">No upload history</p>
          <p class="text-sm text-gray-500 dark:text-gray-500 mt-1">Files you share will appear here</p>
        </div>
      {:else}
        <div class="divide-y divide-gray-100 dark:divide-gray-700">
          {#each sharedFiles as file (file.id)}
            {@const FileTypeIcon = getFileIcon(file.name)}
            <div class="p-4 hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors">
              <div class="flex items-center gap-4">
                <!-- File Icon -->
                <div class="flex items-center justify-center w-12 h-12 bg-gray-100 dark:bg-gray-700 rounded-lg flex-shrink-0">
                  <FileTypeIcon class="w-6 h-6 {getFileColor(file.name)}" />
                </div>

                <!-- File Info -->
                <div class="flex-1 min-w-0">
                  <div class="flex items-center gap-2">
                    <p class="text-sm font-semibold truncate text-gray-900 dark:text-white">{file.name}</p>
                    <span class="px-2 py-0.5 text-xs font-medium rounded {getProtocolColor(file.protocol)}">
                      {file.protocol}
                    </span>
                    {#if file.priceChi && file.priceChi !== '0'}
                      <span class="px-2 py-0.5 text-xs font-medium rounded bg-amber-100 text-amber-800 dark:bg-amber-900/30 dark:text-amber-400">
                        {file.priceChi} CHI
                      </span>
                    {:else}
                      <span class="px-2 py-0.5 text-xs font-medium rounded bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-400">
                        Free
                      </span>
                    {/if}
                  </div>

                  <div class="flex items-center gap-4 text-xs text-gray-500 dark:text-gray-400 mt-1">
                    <span>{formatFileSize(file.size)}</span>
                    <span>{file.fileType}</span>
                    <span>{formatDate(file.uploadDate)}</span>
                    <span class="text-green-600 dark:text-green-400">{file.seeders} seeder{file.seeders !== 1 ? 's' : ''}</span>
                  </div>

                  <div class="flex items-center gap-2 mt-2">
                    <span class="text-xs text-gray-500 dark:text-gray-400">Merkle Hash:</span>
                    <code class="bg-gray-100 dark:bg-gray-700 px-2 py-0.5 rounded text-xs font-mono text-gray-600 dark:text-gray-300">
                      {file.hash.slice(0, 12)}...{file.hash.slice(-8)}
                    </code>
                    <button
                      onclick={() => copyHash(file.hash)}
                      class="p-1 hover:bg-gray-200 dark:hover:bg-gray-600 rounded transition-colors"
                      title="Copy full hash"
                    >
                      <Copy class="w-3 h-3 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300" />
                    </button>
                  </div>
                </div>

                <!-- Share & Actions -->
                <div class="flex items-center gap-2 flex-shrink-0">
                  <button
                    onclick={() => toggleShareOptions(file.id)}
                    class="flex items-center gap-1 px-3 py-1.5 text-sm font-medium text-primary-600 dark:text-primary-400 hover:bg-primary-50 dark:hover:bg-primary-900/30 rounded-lg transition-colors"
                    title="Share options"
                  >
                    <ExternalLink class="w-4 h-4" />
                    Share
                  </button>
                  <button
                    onclick={() => removeFile(file.id)}
                    class="p-2 hover:bg-red-50 dark:hover:bg-red-900/30 rounded-lg transition-colors"
                    title="Remove from history"
                  >
                    <X class="w-4 h-4 text-gray-400 hover:text-red-500" />
                  </button>
                </div>
              </div>

              <!-- Expanded Share Options -->
              {#if expandedFileId === file.id}
                <div class="mt-4 ml-16 p-4 bg-gray-50 dark:bg-gray-700 rounded-lg border border-gray-200 dark:border-gray-600">
                  <p class="text-sm font-semibold text-gray-700 dark:text-gray-300 mb-3">Share this file</p>

                  <!-- Magnet Link -->
                  <div class="mb-3">
                    <label for="magnet-{file.id}" class="text-xs text-gray-500 dark:text-gray-400 mb-1 block">Magnet Link</label>
                    <div class="flex items-center gap-2">
                      <input
                        id="magnet-{file.id}"
                        type="text"
                        readonly
                        value={generateMagnetLink(file)}
                        class="flex-1 px-3 py-2 text-xs font-mono bg-white dark:bg-gray-800 border border-gray-300 dark:border-gray-600 rounded-lg text-gray-600 dark:text-gray-300 truncate"
                      />
                      <button
                        onclick={() => copyMagnetLink(file)}
                        class="flex items-center gap-1 px-3 py-2 text-sm font-medium text-white bg-purple-600 hover:bg-purple-700 rounded-lg transition-colors"
                        title="Copy magnet link"
                      >
                        <Link class="w-4 h-4" />
                        Copy
                      </button>
                    </div>
                  </div>

                  <!-- Hash -->
                  <div class="mb-3">
                    <label for="hash-{file.id}" class="text-xs text-gray-500 dark:text-gray-400 mb-1 block">Merkle Hash (for direct search)</label>
                    <div class="flex items-center gap-2">
                      <input
                        id="hash-{file.id}"
                        type="text"
                        readonly
                        value={file.hash}
                        class="flex-1 px-3 py-2 text-xs font-mono bg-white dark:bg-gray-800 border border-gray-300 dark:border-gray-600 rounded-lg text-gray-600 dark:text-gray-300"
                      />
                      <button
                        onclick={() => copyHash(file.hash)}
                        class="flex items-center gap-1 px-3 py-2 text-sm font-medium text-white bg-primary-600 hover:bg-primary-700 rounded-lg transition-colors"
                        title="Copy hash"
                      >
                        <Copy class="w-4 h-4" />
                        Copy
                      </button>
                    </div>
                  </div>

                  <!-- Export Torrent -->
                  <div>
                    <span class="text-xs text-gray-500 dark:text-gray-400 mb-1 block">Torrent File</span>
                    <button
                      onclick={() => exportTorrentFile(file)}
                      class="flex items-center gap-2 px-4 py-2 text-sm font-medium text-white bg-green-600 hover:bg-green-700 rounded-lg transition-colors"
                    >
                      <Download class="w-4 h-4" />
                      Export .torrent File
                    </button>
                  </div>

                  <p class="text-xs text-gray-400 mt-3">
                    Share the magnet link or hash with others so they can download your file.
                  </p>
                </div>
              {/if}
            </div>
          {/each}
        </div>
      {/if}
    {/if}
  </div>

  <!-- Hidden file input for macOS fallback (WKWebView native picker) -->
  <input
    bind:this={fileInputEl}
    type="file"
    multiple
    onchange={handleFileInputChange}
    class="hidden"
  />
</div>
