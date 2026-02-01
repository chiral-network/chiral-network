<script lang="ts">
  import {
    Upload,
    FolderOpen,
    File as FileIcon,
    Image,
    Video,
    Music,
    Archive,
    Code,
    FileText,
    FileSpreadsheet,
    X,
    Plus,
    Copy,
    RefreshCw
  } from 'lucide-svelte';
  import { networkConnected } from '$lib/stores';
  import { showToast } from '$lib/toastStore';

  // Check if running in Tauri environment
  const isTauri = typeof window !== 'undefined' && '__TAURI__' in window;

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
  let isDragging = $state(false);
  let isUploading = $state(false);
  let sharedFiles = $state<Array<{
    id: string;
    name: string;
    size: number;
    hash: string;
    seeders: number;
    uploadDate: Date;
  }>>([]);

  // Storage info
  let availableStorage = $state<number | null>(null);
  let isRefreshingStorage = $state(false);

  async function refreshStorage() {
    if (!isTauri || isRefreshingStorage) return;

    isRefreshingStorage = true;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const storage = await invoke<number>('get_available_storage');
      availableStorage = storage;
    } catch (error) {
      console.error('Failed to get storage:', error);
    } finally {
      isRefreshingStorage = false;
    }
  }

  // Generate a simple hash for demo purposes
  function generateHash(): string {
    const chars = 'abcdef0123456789';
    let hash = '';
    for (let i = 0; i < 64; i++) {
      hash += chars[Math.floor(Math.random() * chars.length)];
    }
    return hash;
  }

  // Handle file selection
  async function openFileDialog() {
    if (!isTauri) {
      showToast('File upload requires the desktop app', 'error');
      return;
    }

    if (!$networkConnected) {
      showToast('Please connect to the network first', 'error');
      return;
    }

    if (isUploading) return;

    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const selectedPaths = await open({ multiple: true }) as string[] | null;

      if (selectedPaths && selectedPaths.length > 0) {
        isUploading = true;
        await processFiles(selectedPaths);
      }
    } catch (error) {
      console.error('File dialog error:', error);
      showToast('Failed to open file dialog', 'error');
    } finally {
      isUploading = false;
    }
  }

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
          console.warn('Could not get file size:', e);
        }

        // Publish to DHT
        const result = await invoke<{ merkleRoot: string }>('publish_file', {
          filePath,
          fileName
        });

        const newFile = {
          id: `file-${Date.now()}-${Math.random()}`,
          name: fileName,
          size: fileSize,
          hash: result.merkleRoot || generateHash(),
          seeders: 1,
          uploadDate: new Date()
        };

        sharedFiles = [...sharedFiles, newFile];
        showToast(`${fileName} is now being shared`, 'success');
      } catch (error) {
        console.error('Failed to process file:', error);
        showToast(`Failed to share file: ${error}`, 'error');
      }
    }
  }

  function removeFile(id: string) {
    const file = sharedFiles.find(f => f.id === id);
    sharedFiles = sharedFiles.filter(f => f.id !== id);
    if (file) {
      showToast(`Stopped sharing ${file.name}`, 'info');
    }
  }

  async function copyHash(hash: string) {
    await navigator.clipboard.writeText(hash);
    showToast('Hash copied to clipboard', 'success');
  }

  // Drag and drop handlers
  function handleDragOver(e: DragEvent) {
    e.preventDefault();
    if (!$networkConnected) return;
    isDragging = true;
  }

  function handleDragLeave(e: DragEvent) {
    e.preventDefault();
    isDragging = false;
  }

  async function handleDrop(e: DragEvent) {
    e.preventDefault();
    isDragging = false;

    if (!isTauri) {
      showToast('File upload requires the desktop app', 'error');
      return;
    }

    if (!$networkConnected) {
      showToast('Please connect to the network first', 'error');
      return;
    }

    // Note: In Tauri, we need to use the onDragDropEvent API for proper file paths
    showToast('Please use the Add Files button to select files', 'info');
  }

  // Initialize
  $effect(() => {
    if (isTauri) {
      refreshStorage();
    }
  });
</script>

<div class="p-6 space-y-6">
  <div>
    <h1 class="text-3xl font-bold">Upload</h1>
    <p class="text-gray-600 mt-2">Share files with the Chiral Network</p>
  </div>

  <!-- Network Status Warning -->
  {#if !$networkConnected}
    <div class="bg-yellow-50 border border-yellow-200 rounded-lg p-4">
      <div class="flex items-start gap-3">
        <div class="text-yellow-600 mt-0.5">!</div>
        <div>
          <p class="text-sm font-semibold text-yellow-800">Network Not Connected</p>
          <p class="text-sm text-yellow-700">
            Please connect to the DHT network from the Network page before uploading files.
          </p>
        </div>
      </div>
    </div>
  {/if}

  <!-- Storage Info -->
  {#if isTauri}
    <div class="bg-white rounded-lg border border-gray-200 p-4 flex items-center justify-between">
      <div>
        <p class="text-sm font-semibold text-gray-900">Storage</p>
        <p class="text-sm text-gray-600">
          {#if availableStorage !== null}
            {formatFileSize(availableStorage * 1024 * 1024)} available
          {:else}
            Checking storage...
          {/if}
        </p>
      </div>
      <button
        onclick={refreshStorage}
        disabled={isRefreshingStorage}
        class="flex items-center gap-2 px-3 py-2 text-sm font-medium text-gray-700 bg-white border border-gray-300 rounded-lg hover:bg-gray-50 disabled:opacity-50"
      >
        <RefreshCw class="w-4 h-4 {isRefreshingStorage ? 'animate-spin' : ''}" />
        Refresh
      </button>
    </div>
  {/if}

  <!-- Drop Zone -->
  <div
    role="button"
    tabindex="0"
    ondragover={handleDragOver}
    ondragleave={handleDragLeave}
    ondrop={handleDrop}
    class="relative border-2 border-dashed rounded-xl p-8 transition-all duration-200 {isDragging ? 'border-blue-500 bg-blue-50' : 'border-gray-300 hover:border-gray-400'}"
  >
    {#if sharedFiles.length === 0}
      <!-- Empty State -->
      <div class="text-center py-8">
        <div class="mb-6">
          {#if isDragging}
            <Upload class="h-16 w-16 mx-auto text-blue-500" />
          {:else}
            <FolderOpen class="h-16 w-16 mx-auto text-gray-400" />
          {/if}
        </div>

        <h3 class="text-2xl font-bold mb-3 {isDragging ? 'text-blue-600' : 'text-gray-900'}">
          {isDragging ? 'Drop files here' : 'Share Files'}
        </h3>

        <p class="text-gray-600 mb-8 text-lg">
          {#if isDragging}
            Release to upload files
          {:else}
            Drag and drop files here, or click the button below
          {/if}
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
            class="inline-flex items-center gap-2 px-6 py-3 text-sm font-medium text-white bg-blue-600 rounded-xl hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed transition-all"
          >
            <Plus class="w-5 h-5" />
            {isUploading ? 'Uploading...' : 'Add Files'}
          </button>
        {:else}
          <p class="text-sm text-gray-500">
            File upload requires the desktop application
          </p>
        {/if}

        <p class="text-xs text-gray-400 mt-4">
          Supports images, videos, audio, documents, archives, and more
        </p>
      </div>
    {:else}
      <!-- Shared Files List -->
      <div class="space-y-4">
        <div class="flex items-center justify-between mb-4">
          <div>
            <h2 class="text-lg font-semibold">Shared Files</h2>
            <p class="text-sm text-gray-600">
              {sharedFiles.length} {sharedFiles.length === 1 ? 'file' : 'files'} -
              {formatFileSize(sharedFiles.reduce((sum, f) => sum + f.size, 0))} total
            </p>
          </div>

          {#if isTauri}
            <button
              onclick={openFileDialog}
              disabled={isUploading || !$networkConnected}
              class="inline-flex items-center gap-2 px-4 py-2 text-sm font-medium text-white bg-blue-600 rounded-lg hover:bg-blue-700 disabled:opacity-50"
            >
              <Plus class="w-4 h-4" />
              Add More Files
            </button>
          {/if}
        </div>

        <div class="space-y-3">
          {#each sharedFiles as file (file.id)}
            <div class="bg-white border border-gray-200 rounded-xl p-4 hover:shadow-md transition-all">
              <div class="flex items-center gap-4">
                <!-- File Icon -->
                <div class="flex items-center justify-center w-12 h-12 bg-gray-100 rounded-lg">
                  <svelte:component this={getFileIcon(file.name)} class="w-6 h-6 {getFileColor(file.name)}" />
                </div>

                <!-- File Info -->
                <div class="flex-1 min-w-0">
                  <p class="text-sm font-semibold truncate text-gray-900">{file.name}</p>
                  <div class="flex items-center gap-3 text-xs text-gray-500 mt-1">
                    <span>{formatFileSize(file.size)}</span>
                    <span>-</span>
                    <span>{file.seeders} {file.seeders === 1 ? 'seeder' : 'seeders'}</span>
                  </div>
                  <div class="flex items-center gap-2 mt-2">
                    <code class="bg-gray-100 px-2 py-0.5 rounded text-xs font-mono text-gray-600">
                      {file.hash.slice(0, 8)}...{file.hash.slice(-6)}
                    </code>
                    <button
                      onclick={() => copyHash(file.hash)}
                      class="p-1 hover:bg-gray-100 rounded transition-colors"
                      title="Copy hash"
                    >
                      <Copy class="w-3 h-3 text-gray-400 hover:text-gray-600" />
                    </button>
                  </div>
                </div>

                <!-- Remove Button -->
                <button
                  onclick={() => removeFile(file.id)}
                  class="p-2 hover:bg-red-50 rounded-lg transition-colors"
                  title="Stop sharing"
                >
                  <X class="w-4 h-4 text-gray-400 hover:text-red-500" />
                </button>
              </div>
            </div>
          {/each}
        </div>
      </div>
    {/if}
  </div>
</div>
