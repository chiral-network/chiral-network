<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import {
    Globe,
    Plus,
    Trash2,
    Copy,
    ExternalLink,
    FolderOpen,
    Power,
    PowerOff,
    File as FileIcon,
    Server,
    X
  } from 'lucide-svelte';
  import { toasts } from '$lib/toastStore';
  import { logger } from '$lib/logger';
  const log = logger('Hosting');

  // Check if running in Tauri environment
  let isTauri = $state(false);
  function checkTauriAvailability(): boolean {
    return typeof window !== 'undefined' && ('__TAURI__' in window || '____TAURI_INTERNALS__' in window);
  }

  // ---------------------------------------------------------------------------
  // Types
  // ---------------------------------------------------------------------------

  interface SiteFile {
    path: string;
    size: number;
  }

  interface HostedSite {
    id: string;
    name: string;
    directory: string;
    createdAt: number;
    files: SiteFile[];
  }

  interface ServerStatus {
    running: boolean;
    address: string | null;
  }

  // ---------------------------------------------------------------------------
  // State
  // ---------------------------------------------------------------------------

  let serverStatus = $state<ServerStatus>({ running: false, address: null });
  let port = $state(8080);
  let sites = $state<HostedSite[]>([]);

  // New site form
  let newSiteName = $state('');
  let selectedFiles = $state<{ name: string; path: string; size: number }[]>([]);
  let isCreating = $state(false);
  let isStartingServer = $state(false);

  // Drag state
  let isDragOver = $state(false);

  // ---------------------------------------------------------------------------
  // Helpers
  // ---------------------------------------------------------------------------

  function formatFileSize(bytes: number): string {
    if (bytes === 0) return '0 B';
    const k = 1024;
    const sizes = ['B', 'KB', 'MB', 'GB'];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + ' ' + sizes[i];
  }

  function timeAgo(unixSecs: number): string {
    const diff = Math.floor(Date.now() / 1000) - unixSecs;
    if (diff < 60) return 'just now';
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
  }

  function siteUrl(siteId: string): string {
    if (serverStatus.address) {
      return `http://${serverStatus.address}/sites/${siteId}/`;
    }
    return `http://localhost:${port}/sites/${siteId}/`;
  }

  function totalSize(files: SiteFile[]): number {
    return files.reduce((sum, f) => sum + f.size, 0);
  }

  // ---------------------------------------------------------------------------
  // Server controls
  // ---------------------------------------------------------------------------

  async function loadServerStatus() {
    if (!isTauri) return;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      serverStatus = await invoke<ServerStatus>('get_hosting_server_status');
    } catch (err) {
      log.error('Failed to get server status:', err);
    }
  }

  async function startServer() {
    if (!isTauri) return;
    isStartingServer = true;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const addr = await invoke<string>('start_hosting_server', { port });
      serverStatus = { running: true, address: addr };
      toasts.show(`Hosting server started on ${addr}`, 'success');
      // Save port preference
      localStorage.setItem('chiral-hosting-port', String(port));
    } catch (err: any) {
      toasts.show(`Failed to start server: ${err}`, 'error');
      log.error('Failed to start hosting server:', err);
    } finally {
      isStartingServer = false;
    }
  }

  async function stopServer() {
    if (!isTauri) return;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('stop_hosting_server');
      serverStatus = { running: false, address: null };
      toasts.show('Hosting server stopped', 'info');
    } catch (err: any) {
      toasts.show(`Failed to stop server: ${err}`, 'error');
    }
  }

  // ---------------------------------------------------------------------------
  // Site management
  // ---------------------------------------------------------------------------

  async function loadSites() {
    if (!isTauri) return;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      sites = await invoke<HostedSite[]>('list_hosted_sites');
    } catch (err) {
      log.error('Failed to load sites:', err);
    }
  }

  async function selectFiles() {
    if (!isTauri) return;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const paths = await invoke<string[]>('open_file_dialog', { multiple: true });
      if (paths && paths.length > 0) {
        for (const p of paths) {
          const name = p.split(/[\\/]/).pop() || p;
          const size = await invoke<number>('get_file_size', { filePath: p });
          // Avoid duplicates
          if (!selectedFiles.some(f => f.path === p)) {
            selectedFiles = [...selectedFiles, { name, path: p, size }];
          }
        }
      }
    } catch (err) {
      log.error('File dialog error:', err);
    }
  }

  function removeSelectedFile(index: number) {
    selectedFiles = selectedFiles.filter((_, i) => i !== index);
  }

  async function createSite() {
    if (!isTauri || !newSiteName.trim() || selectedFiles.length === 0) return;
    isCreating = true;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const filePaths = selectedFiles.map(f => f.path);
      const site = await invoke<HostedSite>('create_hosted_site', {
        name: newSiteName.trim(),
        filePaths,
      });
      sites = [...sites, site];
      toasts.show(`Site "${site.name}" created`, 'success');

      // Reset form
      newSiteName = '';
      selectedFiles = [];
    } catch (err: any) {
      toasts.show(`Failed to create site: ${err}`, 'error');
      log.error('Create site error:', err);
    } finally {
      isCreating = false;
    }
  }

  async function deleteSite(siteId: string, siteName: string) {
    if (!isTauri) return;
    if (!confirm(`Delete "${siteName}"? This cannot be undone.`)) return;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('delete_hosted_site', { siteId });
      sites = sites.filter(s => s.id !== siteId);
      toasts.show(`Site "${siteName}" deleted`, 'info');
    } catch (err: any) {
      toasts.show(`Failed to delete site: ${err}`, 'error');
    }
  }

  function copyUrl(siteId: string) {
    const url = siteUrl(siteId);
    navigator.clipboard.writeText(url);
    toasts.show('URL copied to clipboard', 'success');
  }

  function openSite(siteId: string) {
    window.open(siteUrl(siteId), '_blank');
  }

  // ---------------------------------------------------------------------------
  // Drag and drop (Tauri window-level events)
  // ---------------------------------------------------------------------------

  let unlistenDragDrop: (() => void) | undefined;

  async function addFilesFromPaths(paths: string[]) {
    if (!isTauri) return;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      for (const p of paths) {
        const name = p.split(/[\\/]/).pop() || p;
        if (selectedFiles.some(f => f.path === p)) continue;
        let size = 0;
        try {
          size = await invoke<number>('get_file_size', { filePath: p });
        } catch (_) { /* ignore */ }
        selectedFiles = [...selectedFiles, { name, path: p, size }];
      }
    } catch (err) {
      log.error('Failed to add dropped files:', err);
    }
  }

  // ---------------------------------------------------------------------------
  // Lifecycle
  // ---------------------------------------------------------------------------

  onMount(async () => {
    isTauri = checkTauriAvailability();

    // Restore saved port
    const savedPort = localStorage.getItem('chiral-hosting-port');
    if (savedPort) {
      const parsed = parseInt(savedPort, 10);
      if (!isNaN(parsed) && parsed > 0 && parsed <= 65535) {
        port = parsed;
      }
    }

    await loadServerStatus();
    await loadSites();

    // Set up Tauri drag-drop listener
    if (isTauri) {
      try {
        const { getCurrentWindow } = await import('@tauri-apps/api/window');
        const appWindow = getCurrentWindow();

        unlistenDragDrop = await appWindow.onDragDropEvent((event) => {
          if (event.payload.type === 'drop') {
            const paths = event.payload.paths;
            if (paths && paths.length > 0) {
              addFilesFromPaths(paths);
            }
          } else if (event.payload.type === 'enter') {
            isDragOver = true;
          } else if (event.payload.type === 'leave') {
            isDragOver = false;
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

<div class="flex flex-col gap-6 p-6 max-w-5xl mx-auto">
  <!-- Header -->
  <div>
    <h1 class="text-2xl font-bold text-gray-900 dark:text-white flex items-center gap-2">
      <Globe class="h-7 w-7 text-primary-500" />
      Hosting
    </h1>
    <p class="mt-1 text-sm text-gray-500 dark:text-gray-400">
      Host static websites on the Chiral Network. Upload HTML, CSS, JS, and image files to serve them over HTTP.
    </p>
  </div>

  <!-- Server Status -->
  <div class="rounded-2xl border border-gray-200/70 bg-white/90 p-5 shadow-sm backdrop-blur dark:border-gray-700/60 dark:bg-gray-800/85">
    <div class="flex items-center justify-between">
      <div class="flex items-center gap-3">
        <div class="flex h-10 w-10 items-center justify-center rounded-xl {serverStatus.running ? 'bg-green-100 dark:bg-green-900/30' : 'bg-gray-100 dark:bg-gray-700/50'}">
          <Server class="h-5 w-5 {serverStatus.running ? 'text-green-600 dark:text-green-400' : 'text-gray-400 dark:text-gray-500'}" />
        </div>
        <div>
          <h2 class="text-sm font-semibold text-gray-900 dark:text-white">HTTP Server</h2>
          {#if serverStatus.running}
            <p class="text-xs text-green-600 dark:text-green-400">
              Running on <span class="font-mono">{serverStatus.address}</span>
            </p>
          {:else}
            <p class="text-xs text-gray-500 dark:text-gray-400">Not running</p>
          {/if}
        </div>
      </div>

      <div class="flex items-center gap-3">
        {#if !serverStatus.running}
          <div class="flex items-center gap-2">
            <label class="text-xs text-gray-500 dark:text-gray-400">Port:</label>
            <input
              type="number"
              bind:value={port}
              min="1024"
              max="65535"
              class="w-20 rounded-lg border border-gray-300 bg-white px-2 py-1.5 text-sm text-gray-900 dark:border-gray-600 dark:bg-gray-700 dark:text-white"
            />
          </div>
        {/if}

        {#if serverStatus.running}
          <button
            onclick={stopServer}
            class="flex items-center gap-2 rounded-xl bg-red-500 px-4 py-2 text-sm font-medium text-white transition hover:bg-red-600"
          >
            <PowerOff class="h-4 w-4" />
            Stop
          </button>
        {:else}
          <button
            onclick={startServer}
            disabled={isStartingServer}
            class="flex items-center gap-2 rounded-xl bg-primary-500 px-4 py-2 text-sm font-medium text-white transition hover:bg-primary-600 disabled:opacity-50"
          >
            <Power class="h-4 w-4" />
            {isStartingServer ? 'Starting...' : 'Start Server'}
          </button>
        {/if}
      </div>
    </div>
  </div>

  <!-- Create New Site -->
  <div class="rounded-2xl border border-gray-200/70 bg-white/90 p-5 shadow-sm backdrop-blur dark:border-gray-700/60 dark:bg-gray-800/85">
    <h2 class="mb-4 text-sm font-semibold text-gray-900 dark:text-white flex items-center gap-2">
      <Plus class="h-4 w-4" />
      Create New Site
    </h2>

    <!-- Site Name -->
    <div class="mb-4">
      <label class="mb-1 block text-xs font-medium text-gray-600 dark:text-gray-400">Site Name</label>
      <input
        type="text"
        bind:value={newSiteName}
        placeholder="My Website"
        class="w-full rounded-xl border border-gray-300 bg-white px-4 py-2.5 text-sm text-gray-900 placeholder-gray-400 transition focus:border-primary-400 focus:outline-none focus:ring-2 focus:ring-primary-400/30 dark:border-gray-600 dark:bg-gray-700/50 dark:text-white dark:placeholder-gray-500"
      />
    </div>

    <!-- Drop Zone -->
    <div
      role="button"
      tabindex="0"
      onclick={selectFiles}
      onkeydown={(e) => e.key === 'Enter' && selectFiles()}
      class="flex cursor-pointer flex-col items-center justify-center gap-2 rounded-xl border-2 border-dashed p-8 transition
        {isDragOver
          ? 'border-primary-400 bg-primary-50/50 dark:border-primary-500 dark:bg-primary-900/20'
          : 'border-gray-300 bg-gray-50/50 hover:border-gray-400 dark:border-gray-600 dark:bg-gray-700/30 dark:hover:border-gray-500'}"
    >
      <FolderOpen class="h-8 w-8 text-gray-400 dark:text-gray-500" />
      <p class="text-sm text-gray-500 dark:text-gray-400">
        {isDragOver ? 'Release to add files' : 'Drop files here or click to browse'}
      </p>
      <p class="text-xs text-gray-400 dark:text-gray-500">
        HTML, CSS, JS, images, fonts
      </p>
    </div>

    <!-- Selected Files -->
    {#if selectedFiles.length > 0}
      <div class="mt-4 space-y-1.5">
        <p class="text-xs font-medium text-gray-500 dark:text-gray-400">
          {selectedFiles.length} file{selectedFiles.length === 1 ? '' : 's'} selected
          ({formatFileSize(selectedFiles.reduce((s, f) => s + f.size, 0))})
        </p>
        <div class="max-h-40 overflow-y-auto space-y-1 pr-1">
          {#each selectedFiles as file, i (file.path)}
            <div class="flex items-center justify-between rounded-lg bg-gray-50 px-3 py-1.5 dark:bg-gray-700/40">
              <div class="flex items-center gap-2 min-w-0">
                <FileIcon class="h-3.5 w-3.5 flex-shrink-0 text-gray-400" />
                <span class="truncate text-xs text-gray-700 dark:text-gray-300">{file.name}</span>
                <span class="text-xs text-gray-400">{formatFileSize(file.size)}</span>
              </div>
              <button
                onclick={(e: MouseEvent) => { e.stopPropagation(); removeSelectedFile(i); }}
                class="ml-2 flex-shrink-0 rounded p-0.5 text-gray-400 transition hover:bg-red-100 hover:text-red-500 dark:hover:bg-red-900/30"
              >
                <X class="h-3.5 w-3.5" />
              </button>
            </div>
          {/each}
        </div>
      </div>

      <!-- Create Button -->
      <button
        onclick={createSite}
        disabled={isCreating || !newSiteName.trim()}
        class="mt-4 flex w-full items-center justify-center gap-2 rounded-xl bg-primary-500 px-4 py-2.5 text-sm font-medium text-white transition hover:bg-primary-600 disabled:opacity-50"
      >
        <Plus class="h-4 w-4" />
        {isCreating ? 'Creating...' : 'Create Site'}
      </button>
    {/if}
  </div>

  <!-- Hosted Sites -->
  <div class="rounded-2xl border border-gray-200/70 bg-white/90 p-5 shadow-sm backdrop-blur dark:border-gray-700/60 dark:bg-gray-800/85">
    <h2 class="mb-4 text-sm font-semibold text-gray-900 dark:text-white flex items-center gap-2">
      <Globe class="h-4 w-4" />
      Hosted Sites
      {#if sites.length > 0}
        <span class="rounded-full bg-primary-100 px-2 py-0.5 text-xs font-medium text-primary-700 dark:bg-primary-900/40 dark:text-primary-300">
          {sites.length}
        </span>
      {/if}
    </h2>

    {#if sites.length === 0}
      <div class="flex flex-col items-center justify-center py-12 text-gray-400 dark:text-gray-500">
        <Globe class="h-12 w-12 mb-3 opacity-30" />
        <p class="text-sm">No hosted sites yet</p>
        <p class="text-xs mt-1">Create a site above to start hosting</p>
      </div>
    {:else}
      <div class="space-y-3">
        {#each sites as site (site.id)}
          <div class="rounded-xl border border-gray-100 bg-gray-50/50 p-4 transition hover:border-gray-200 dark:border-gray-700/50 dark:bg-gray-700/30 dark:hover:border-gray-600">
            <div class="flex items-start justify-between gap-3">
              <div class="min-w-0 flex-1">
                <h3 class="font-medium text-gray-900 dark:text-white">{site.name}</h3>
                <p class="mt-0.5 font-mono text-xs text-primary-600 dark:text-primary-400 truncate">
                  {siteUrl(site.id)}
                </p>
                <p class="mt-1 text-xs text-gray-400 dark:text-gray-500">
                  {site.files.length} file{site.files.length === 1 ? '' : 's'}
                  · {formatFileSize(totalSize(site.files))}
                  · Created {timeAgo(site.createdAt)}
                </p>

                <!-- File list preview -->
                {#if site.files.length > 0}
                  <div class="mt-2 flex flex-wrap gap-1.5">
                    {#each site.files.slice(0, 6) as file}
                      <span class="rounded bg-gray-100 px-1.5 py-0.5 text-[10px] text-gray-500 dark:bg-gray-600/50 dark:text-gray-400">
                        {file.path}
                      </span>
                    {/each}
                    {#if site.files.length > 6}
                      <span class="rounded bg-gray-100 px-1.5 py-0.5 text-[10px] text-gray-400 dark:bg-gray-600/50">
                        +{site.files.length - 6} more
                      </span>
                    {/if}
                  </div>
                {/if}
              </div>

              <!-- Actions -->
              <div class="flex items-center gap-1.5 flex-shrink-0">
                <button
                  onclick={() => copyUrl(site.id)}
                  title="Copy URL"
                  class="rounded-lg p-2 text-gray-400 transition hover:bg-gray-100 hover:text-gray-600 dark:hover:bg-gray-600/50 dark:hover:text-gray-300"
                >
                  <Copy class="h-4 w-4" />
                </button>
                <button
                  onclick={() => openSite(site.id)}
                  title="Open in browser"
                  class="rounded-lg p-2 text-gray-400 transition hover:bg-blue-50 hover:text-blue-500 dark:hover:bg-blue-900/30 dark:hover:text-blue-400"
                >
                  <ExternalLink class="h-4 w-4" />
                </button>
                <button
                  onclick={() => deleteSite(site.id, site.name)}
                  title="Delete site"
                  class="rounded-lg p-2 text-gray-400 transition hover:bg-red-50 hover:text-red-500 dark:hover:bg-red-900/30 dark:hover:text-red-400"
                >
                  <Trash2 class="h-4 w-4" />
                </button>
              </div>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  </div>
</div>
