<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { HardDrive, FolderPlus, Upload, Loader2 } from 'lucide-svelte';
  import { driveStore, type DriveItem, type DriveManifest } from '$lib/stores/driveStore';
  import { setLocalDriveServer } from '$lib/services/driveApiService';
  import { walletAccount, networkConnected } from '$lib/stores';

  // Track whether initialization is complete (prevents wallet subscription from firing too early)
  let initialized = false;
  import { toasts } from '$lib/toastStore';
  import { open } from '@tauri-apps/plugin-shell';
  import DriveBreadcrumb from '$lib/components/drive/DriveBreadcrumb.svelte';
  import DriveToolbar from '$lib/components/drive/DriveToolbar.svelte';
  import DriveFileCard from '$lib/components/drive/DriveFileCard.svelte';
  import DriveFileRow from '$lib/components/drive/DriveFileRow.svelte';
  import DriveContextMenu from '$lib/components/drive/DriveContextMenu.svelte';
  import DriveShareModal from '$lib/components/drive/DriveShareModal.svelte';
  import DriveMoveModal from '$lib/components/drive/DriveMoveModal.svelte';
  import DriveSeedingPanel from '$lib/components/drive/DriveSeedingPanel.svelte';

  let manifest = $state<DriveManifest>({ version: 1, items: [], shares: [], lastModified: 0 });
  let currentFolderId = $state<string | null>(null);
  let viewMode = $state<'grid' | 'list'>('grid');
  let searchQuery = $state('');
  let creatingFolder = $state(false);
  let newFolderName = $state('');
  let loading = $state(false);
  let uploading = $state(false);

  // Context menu
  let contextItem = $state<DriveItem | null>(null);
  let contextX = $state(0);
  let contextY = $state(0);

  // Modals
  let shareItem = $state<DriveItem | null>(null);
  let moveItem = $state<DriveItem | null>(null);
  let renamingId = $state<string | null>(null);
  let renameValue = $state('');
  let deleteConfirmItem = $state<DriveItem | null>(null);

  // Tab state
  let activeTab = $state<'files' | 'seeding'>('files');
  let seedingUploadProtocol = $state<'WebRTC' | 'BitTorrent'>('WebRTC');
  let seedingUploadPriceChi = $state('');

  // Seeding count for tab badge
  const seedingCount = $derived(driveStore.getSeedingItems(manifest).length);

  // Drag and drop
  let isDragging = $state(false);

  // Subscribe to store
  driveStore.subscribe(m => manifest = m);

  // Reload drive when wallet changes (only after initialization)
  let prevWalletAddr = '';
  const unsubWallet = walletAccount.subscribe((account) => {
    const addr = account?.address ?? '';
    if (addr !== prevWalletAddr) {
      prevWalletAddr = addr;
      currentFolderId = null;
      if (initialized) loadCurrentFolder();
    }
  });
  onDestroy(unsubWallet);

  /** Migrate chiral_upload_history from the old Upload page into Drive items */
  async function migrateUploadHistory() {
    const UPLOAD_HISTORY_KEY = 'chiral_upload_history';
    const raw = localStorage.getItem(UPLOAD_HISTORY_KEY);
    if (!raw) return;

    let entries: any[];
    try {
      entries = JSON.parse(raw);
    } catch {
      return;
    }
    if (!Array.isArray(entries) || entries.length === 0) return;

    let migrated = 0;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const owner = $walletAccount?.address ?? '';
      if (!owner) return; // No wallet — can't migrate

      for (const entry of entries) {
        try {
          // Upload to Drive storage (copies file to local Drive directory)
          const driveItem = await driveStore.uploadFile(entry.filePath, null);
          if (!driveItem) continue;

          // If the entry had a merkle hash, seed it to the network
          if (entry.hash) {
            await driveStore.seedFile(
              driveItem.id,
              (entry.protocol as 'WebRTC' | 'BitTorrent') || 'WebRTC',
              entry.priceChi && entry.priceChi !== '0' ? entry.priceChi : undefined,
            );
          }
          migrated++;
        } catch {
          // Skip files that can't be migrated (e.g. file moved/deleted)
        }
      }

      // Remove old history after migration
      localStorage.removeItem(UPLOAD_HISTORY_KEY);
      if (migrated > 0) {
        toasts.show(`Migrated ${migrated} seeded file${migrated > 1 ? 's' : ''} to Drive`, 'success');
      }
    } catch {
      // Migration failed — keep localStorage for next attempt
    }
  }

  onMount(async () => {
    // Init local Drive server URL (Tauri only — used as fallback for downloads)
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const url = await invoke<string | null>('get_drive_server_url');
      if (url) setLocalDriveServer(url);
    } catch {
      // Non-Tauri environment — falls back to relay URL
    }

    const saved = localStorage.getItem('drive-view-mode');
    if (saved === 'list' || saved === 'grid') viewMode = saved;
    initialized = true;

    // Migrate chiral_upload_history from old Upload page into Drive
    await migrateUploadHistory();
    await loadCurrentFolder();
  });

  // Derived
  const breadcrumb = $derived(driveStore.getBreadcrumb(currentFolderId, manifest));
  const currentItems = $derived(
    searchQuery
      ? driveStore.searchByName(searchQuery, manifest)
      : driveStore.getChildren(currentFolderId, manifest)
  );
  const totalSize = $derived(
    manifest.items
      .filter(i => i.type === 'file' && i.size)
      .reduce((sum, i) => sum + (i.size || 0), 0)
  );

  async function loadCurrentFolder() {
    loading = true;
    try {
      await driveStore.loadFolder(currentFolderId);
    } finally {
      loading = false;
    }
  }

  // Navigation
  function navigateTo(folderId: string | null) {
    currentFolderId = folderId;
    searchQuery = '';
    loadCurrentFolder();
  }

  function handleOpen(item: DriveItem) {
    if (item.type === 'folder') {
      navigateTo(item.id);
    }
  }

  // View mode
  function handleViewModeChange(mode: 'grid' | 'list') {
    viewMode = mode;
    localStorage.setItem('drive-view-mode', mode);
  }

  // Upload via Tauri file dialog (or browser fallback)
  async function handleUpload() {
    // Try Tauri file dialog first — returns file paths directly (no browser security issues)
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const paths: string[] | null = await invoke('open_file_dialog', { multiple: true });
      if (!paths || paths.length === 0) return;
      uploading = true;
      let count = 0;
      try {
        for (const path of paths) {
          const result = await driveStore.uploadFile(path, currentFolderId);
          if (result) count++;
        }
        if (count > 0) {
          toasts.show(`Uploaded ${count} file${count > 1 ? 's' : ''}`, 'success');
        } else {
          toasts.show('Upload failed', 'error');
        }
      } catch (e) {
        toasts.show('Upload failed: ' + (e as Error).message, 'error');
      } finally {
        uploading = false;
      }
      return;
    } catch {
      // Not Tauri or dialog command unavailable — fall through to browser input
    }

    // Browser fallback
    const input = document.createElement('input');
    input.type = 'file';
    input.multiple = true;
    input.onchange = async () => {
      if (!input.files || input.files.length === 0) return;
      uploading = true;
      let count = 0;
      try {
        for (const file of input.files) {
          const result = await driveStore.uploadFile(file, currentFolderId);
          if (result) count++;
        }
        const total = input.files.length;
        if (count > 0) {
          toasts.show(`Uploaded ${count} file${count > 1 ? 's' : ''}`, 'success');
        } else if (total > 0) {
          toasts.show('Upload failed — could not reach the local server', 'error');
        }
      } catch (e) {
        toasts.show('Upload failed: ' + (e as Error).message, 'error');
      } finally {
        uploading = false;
      }
    };
    input.click();
  }

  // New folder
  function handleNewFolder() {
    creatingFolder = true;
    newFolderName = '';
    setTimeout(() => {
      const input = document.getElementById('new-folder-input') as HTMLInputElement;
      input?.focus();
    }, 50);
  }

  async function confirmNewFolder() {
    const name = newFolderName.trim();
    if (!name) return;
    const result = await driveStore.createFolder(name, currentFolderId);
    if (result) {
      toasts.show(`Created folder "${name}"`, 'success');
    } else {
      toasts.show('Failed to create folder — could not reach the local server', 'error');
    }
    creatingFolder = false;
    newFolderName = '';
  }

  function cancelNewFolder() {
    creatingFolder = false;
    newFolderName = '';
  }

  // Context menu
  function handleContextMenu(item: DriveItem, event: MouseEvent) {
    contextItem = item;
    contextX = event.clientX;
    contextY = event.clientY;
  }

  function closeContextMenu() {
    contextItem = null;
  }

  // Rename
  function startRename(item: DriveItem) {
    renamingId = item.id;
    renameValue = item.name;
    setTimeout(() => {
      const input = document.getElementById('rename-input') as HTMLInputElement;
      input?.focus();
      input?.select();
    }, 50);
  }

  async function confirmRename() {
    if (renamingId && renameValue.trim()) {
      await driveStore.renameItem(renamingId, renameValue.trim());
    }
    renamingId = null;
    renameValue = '';
  }

  // Copy link
  async function handleCopyLink(item: DriveItem) {
    const existingShares = driveStore.getSharesForItem(item.id, manifest);
    let url: string;
    if (existingShares.length > 0) {
      url = driveStore.getShareUrl(existingShares[0].id);
    } else {
      const share = await driveStore.createShareLink(item.id, undefined, true);
      if (!share) {
        toasts.show('Failed to create share link', 'error');
        return;
      }
      url = driveStore.getShareUrl(share.id);
    }
    try {
      await navigator.clipboard.writeText(url);
      toasts.show('Link copied to clipboard', 'success');
    } catch {
      toasts.show('Failed to copy link', 'error');
    }
  }

  // Download
  async function handleDownload(item: DriveItem) {
    if (item.type !== 'file') return;
    const url = driveStore.getDownloadUrl(item.id, item.name);
    try {
      // Open in the system's default browser which handles Content-Disposition properly
      await open(url);
    } catch {
      // Fallback for non-Tauri environments
      window.open(url, '_blank');
    }
  }

  // Share
  function handleShare(item: DriveItem) {
    shareItem = item;
  }

  // Delete
  function handleDelete(item: DriveItem) {
    deleteConfirmItem = item;
  }

  async function confirmDelete() {
    if (!deleteConfirmItem) return;
    const item = deleteConfirmItem;
    deleteConfirmItem = null;
    try {
      await driveStore.deleteItem(item.id);
      toasts.show(`Deleted "${item.name}"`, 'success');
    } catch (e) {
      toasts.show(`Failed to delete "${item.name}"`, 'error');
    }
  }

  // Move
  function handleMoveAction(item: DriveItem) {
    moveItem = item;
  }

  async function handleMoveConfirm(itemId: string, targetFolderId: string | null) {
    await driveStore.moveItem(itemId, targetFolderId);
    toasts.show('Item moved', 'success');
  }

  // Star
  async function handleToggleStar(item: DriveItem) {
    await driveStore.toggleStar(item.id);
  }

  // Toggle visibility (public/private)
  async function handleToggleVisibility(item: DriveItem) {
    await driveStore.toggleVisibility(item.id);
    const newState = !item.isPublic;
    toasts.show(
      newState ? `"${item.name}" is now public` : `"${item.name}" is now private`,
      'success'
    );
  }

  // Drag and drop
  function handleDragOver(e: DragEvent) {
    e.preventDefault();
    isDragging = true;
  }

  function handleDragLeave() {
    isDragging = false;
  }

  function normalizePriceChi(value: string | number | null | undefined): string {
    const raw = `${value ?? ''}`.trim();
    if (!raw) return '';
    const parsed = Number(raw);
    if (!Number.isFinite(parsed) || parsed <= 0) return '';
    return raw;
  }

  function handleSeedingOptionsChange(protocol: 'WebRTC' | 'BitTorrent', priceChi: string) {
    seedingUploadProtocol = protocol;
    seedingUploadPriceChi = normalizePriceChi(priceChi);
  }

  async function handleDrop(e: DragEvent) {
    e.preventDefault();
    isDragging = false;
    if (!e.dataTransfer?.files || e.dataTransfer.files.length === 0) return;

    // In Tauri mode, browser File objects can't be uploaded via HTTP (mixed-content).
    // Show a hint to use the Upload button instead.
    const isTauriEnv = !!(window as any).__TAURI_INTERNALS__;
    if (isTauriEnv) {
      toasts.show('Please use the Upload button to add files', 'info');
      return;
    }

    // Web/browser mode — use File objects
    if (activeTab === 'seeding') {
      if (!$networkConnected) {
        toasts.show('Please connect to the network first', 'error');
        return;
      }
      uploading = true;
      const priceChi = normalizePriceChi(seedingUploadPriceChi);
      let count = 0;
      try {
        for (const file of e.dataTransfer.files) {
          const driveItem = await driveStore.uploadFile(file, currentFolderId);
          if (driveItem) {
            const result = await driveStore.seedFile(
              driveItem.id,
              seedingUploadProtocol,
              priceChi || undefined,
            );
            if (result) count++;
          }
        }
        if (count > 0) {
          toasts.show(`${count} file${count > 1 ? 's' : ''} now seeding via ${seedingUploadProtocol}`, 'success');
        }
      } catch (e) {
        toasts.show('Upload failed: ' + (e as Error).message, 'error');
      } finally {
        uploading = false;
      }
      return;
    }

    uploading = true;
    let count = 0;
    try {
      for (const file of e.dataTransfer.files) {
        const result = await driveStore.uploadFile(file, currentFolderId);
        if (result) count++;
      }
      if (count > 0) {
        toasts.show(`Uploaded ${count} file${count > 1 ? 's' : ''}`, 'success');
      }
    } catch (e) {
      toasts.show('Upload failed: ' + (e as Error).message, 'error');
    } finally {
      uploading = false;
    }
  }

  // --- Seeding actions (from context menu or seeding panel) ---

  async function handleSeedToNetwork(item: DriveItem) {
    if (!$networkConnected) {
      toasts.show('Please connect to the network first', 'error');
      return;
    }
    const result = await driveStore.seedFile(item.id, 'WebRTC', item.priceChi || undefined);
    if (result) {
      toasts.show(`Now seeding "${item.name}"`, 'success');
    } else {
      toasts.show(`Failed to seed "${item.name}"`, 'error');
    }
  }

  async function handleStopSeeding(item: DriveItem) {
    await driveStore.stopSeeding(item.id);
    toasts.show(`Stopped seeding "${item.name}"`, 'info');
  }

  async function handleCopyMerkleHash(item: DriveItem) {
    if (!item.merkleRoot) return;
    try {
      await navigator.clipboard.writeText(item.merkleRoot);
      toasts.show('Hash copied to clipboard', 'success');
    } catch {
      toasts.show('Failed to copy hash', 'error');
    }
  }

  async function handleShowInExplorer(item: DriveItem) {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const addr = $walletAccount?.address;
      if (!addr) return;
      await invoke('show_drive_item_in_folder', { owner: addr, itemId: item.id });
    } catch (e) {
      toasts.show(`Failed to open file explorer: ${(e as Error).message || e}`, 'error');
    }
  }

  async function handleCopyMagnetLink(item: DriveItem) {
    if (!item.merkleRoot) return;
    const link = `magnet:?xt=urn:btih:${item.merkleRoot}&dn=${encodeURIComponent(item.name)}&xl=${item.size || 0}`;
    try {
      await navigator.clipboard.writeText(link);
      toasts.show('Magnet link copied to clipboard', 'success');
    } catch {
      toasts.show('Failed to copy magnet link', 'error');
    }
  }

  /** Called from DriveSeedingPanel "Add Files to Seed" — opens file picker, uploads to Drive, then seeds */
  async function handleAddFilesToSeed(protocol: 'WebRTC' | 'BitTorrent', priceChi: string) {
    if (!$networkConnected) {
      toasts.show('Please connect to the network first', 'error');
      return;
    }
    const normalizedPrice = normalizePriceChi(priceChi);
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const paths: string[] | null = await invoke('open_file_dialog', { multiple: true });
      if (!paths || paths.length === 0) return;
      uploading = true;
      let count = 0;
      for (const path of paths) {
        // Upload to Drive storage first
        const driveItem = await driveStore.uploadFile(path, currentFolderId);
        if (driveItem) {
          // Then publish to DHT for seeding
          const result = await driveStore.seedFile(driveItem.id, protocol, normalizedPrice || undefined);
          if (result) count++;
        }
      }
      if (count > 0) {
        toasts.show(`${count} file${count > 1 ? 's' : ''} now seeding via ${protocol}`, 'success');
      }
    } catch (e) {
      toasts.show('Failed to add files: ' + (e as Error).message, 'error');
    } finally {
      uploading = false;
    }
  }

  // Tauri native drag-drop support (only for seeding tab)
  let unlistenDragDrop: (() => void) | null = null;
  onMount(async () => {
    const isTauriEnv = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
    if (!isTauriEnv) return;
    try {
      const { getCurrentWindow } = await import('@tauri-apps/api/window');
      const appWindow = getCurrentWindow();
      const unlistenFn = await appWindow.onDragDropEvent(async (event: any) => {
        if (event.payload.type === 'drop' && event.payload.paths?.length > 0) {
          isDragging = false;
          if (activeTab === 'seeding') {
            // Seed mode: upload + publish to DHT
            if (!$networkConnected) {
              toasts.show('Please connect to the network first', 'error');
              return;
            }
            uploading = true;
            const normalizedPrice = normalizePriceChi(seedingUploadPriceChi);
            let count = 0;
            try {
              for (const path of event.payload.paths) {
                const driveItem = await driveStore.uploadFile(path as string, currentFolderId);
                if (driveItem) {
                  const result = await driveStore.seedFile(
                    driveItem.id,
                    seedingUploadProtocol,
                    normalizedPrice || undefined,
                  );
                  if (result) count++;
                }
              }
              if (count > 0) {
                toasts.show(`${count} file${count > 1 ? 's' : ''} now seeding via ${seedingUploadProtocol}`, 'success');
              }
            } catch (e) {
              toasts.show('Failed: ' + (e as Error).message, 'error');
            } finally {
              uploading = false;
            }
          } else {
            // Files mode: upload to Drive only
            uploading = true;
            let count = 0;
            try {
              for (const path of event.payload.paths) {
                const result = await driveStore.uploadFile(path as string, currentFolderId);
                if (result) count++;
              }
              if (count > 0) toasts.show(`Uploaded ${count} file${count > 1 ? 's' : ''}`, 'success');
            } catch (e) {
              toasts.show('Upload failed: ' + (e as Error).message, 'error');
            } finally {
              uploading = false;
            }
          }
        } else if (event.payload.type === 'enter') {
          isDragging = true;
        } else if (event.payload.type === 'leave') {
          isDragging = false;
        }
      });
      unlistenDragDrop = unlistenFn;
    } catch {
      // Not Tauri
    }
  });
  onDestroy(() => { unlistenDragDrop?.(); });

  function formatBytes(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }
</script>

<div
  class="p-6 space-y-6"
  ondragover={handleDragOver}
  ondragleave={handleDragLeave}
  ondrop={handleDrop}
  role="main"
>
  <!-- Header -->
  <div>
    <h1 class="text-3xl font-bold text-gray-900 dark:text-white">My Drive</h1>
    <p class="text-muted-foreground mt-2">
      Cloud storage with shareable links
      {#if manifest.items.length > 0}
        <span class="ml-2 text-xs">
          — {manifest.items.filter(i => i.type === 'file').length} files, {formatBytes(totalSize)}
        </span>
      {/if}
    </p>
  </div>

  <!-- Toolbar -->
  <DriveToolbar
    {viewMode}
    {searchQuery}
    onUpload={handleUpload}
    onNewFolder={handleNewFolder}
    onViewModeChange={handleViewModeChange}
    onSearchChange={(q) => searchQuery = q}
  />

  <!-- Tab bar -->
  <div class="flex border-b border-gray-200 dark:border-gray-700">
    <button
      onclick={() => activeTab = 'files'}
      class="px-4 py-2.5 text-sm font-medium transition border-b-2 -mb-px
        {activeTab === 'files'
          ? 'text-blue-600 dark:text-blue-400 border-blue-600 dark:border-blue-400'
          : 'text-gray-500 dark:text-gray-400 border-transparent hover:text-gray-700 dark:hover:text-gray-300'}"
    >
      All Files
    </button>
    <button
      onclick={() => activeTab = 'seeding'}
      class="px-4 py-2.5 text-sm font-medium transition border-b-2 -mb-px flex items-center gap-1.5
        {activeTab === 'seeding'
          ? 'text-blue-600 dark:text-blue-400 border-blue-600 dark:border-blue-400'
          : 'text-gray-500 dark:text-gray-400 border-transparent hover:text-gray-700 dark:hover:text-gray-300'}"
    >
      Seeding
      {#if seedingCount > 0}
        <span class="px-1.5 py-0.5 text-xs rounded-full bg-green-100 text-green-800 dark:bg-green-900/30 dark:text-green-400">
          {seedingCount}
        </span>
      {/if}
    </button>
  </div>

  {#if activeTab === 'seeding'}
    <!-- Seeding tab -->
    <DriveSeedingPanel
      {manifest}
      onAddFiles={handleAddFilesToSeed}
      onOptionsChange={handleSeedingOptionsChange}
    />
  {:else}
  <!-- Breadcrumb -->
  {#if !searchQuery}
    <DriveBreadcrumb {breadcrumb} onNavigate={navigateTo} />
  {:else}
    <p class="text-sm text-gray-500 dark:text-gray-400">
      Search results for "<span class="font-medium">{searchQuery}</span>" — {currentItems.length} result{currentItems.length !== 1 ? 's' : ''}
    </p>
  {/if}

  <!-- New folder input -->
  {#if creatingFolder}
    <div class="flex items-center gap-2">
      <FolderPlus class="w-5 h-5 text-yellow-500" />
      <input
        id="new-folder-input"
        type="text"
        placeholder="Folder name"
        bind:value={newFolderName}
        onkeydown={(e) => { if (e.key === 'Enter') confirmNewFolder(); if (e.key === 'Escape') cancelNewFolder(); }}
        class="px-3 py-1.5 bg-gray-100 dark:bg-gray-700 border border-gray-300 dark:border-gray-600 rounded-lg text-sm text-gray-900 dark:text-white focus:outline-none focus:ring-2 focus:ring-blue-500 w-64"
      />
      <button onclick={confirmNewFolder} class="px-3 py-1.5 bg-blue-600 text-white text-sm rounded-lg hover:bg-blue-700 transition">Create</button>
      <button onclick={cancelNewFolder} class="px-3 py-1.5 text-gray-600 dark:text-gray-400 text-sm hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition">Cancel</button>
    </div>
  {/if}

  <!-- Upload progress -->
  {#if uploading}
    <div class="flex items-center gap-2 text-sm text-blue-600 dark:text-blue-400">
      <Loader2 class="w-4 h-4 animate-spin" />
      Uploading files to server...
    </div>
  {/if}

  <!-- Drag overlay -->
  {#if isDragging}
    <div class="border-2 border-dashed border-blue-400 bg-blue-50 dark:bg-blue-900/20 rounded-xl p-12 text-center">
      <Upload class="w-10 h-10 mx-auto text-blue-500 mb-2" />
      <p class="text-blue-600 dark:text-blue-400 font-medium">Drop files here to upload</p>
    </div>
  {/if}

  <!-- Loading -->
  {#if loading}
    <div class="flex items-center justify-center py-16">
      <Loader2 class="w-8 h-8 animate-spin text-blue-500" />
    </div>
  {:else if currentItems.length === 0 && !creatingFolder && !isDragging}
    <!-- Empty state -->
    <div class="flex flex-col items-center justify-center py-16 text-center">
      <div class="w-16 h-16 bg-gray-100 dark:bg-gray-700 rounded-full flex items-center justify-center mb-4">
        <HardDrive class="w-8 h-8 text-gray-400" />
      </div>
      <h3 class="text-lg font-medium text-gray-900 dark:text-white mb-1">
        {searchQuery ? 'No files found' : 'This folder is empty'}
      </h3>
      <p class="text-sm text-gray-500 dark:text-gray-400 mb-6">
        {searchQuery ? 'Try a different search term' : 'Upload files or create a folder to get started'}
      </p>
      {#if !searchQuery}
        <div class="flex gap-3">
          <button
            onclick={handleUpload}
            class="flex items-center gap-2 px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded-lg transition text-sm font-medium"
          >
            <Upload class="w-4 h-4" />
            Upload File
          </button>
          <button
            onclick={handleNewFolder}
            class="flex items-center gap-2 px-4 py-2 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 text-gray-700 dark:text-gray-300 rounded-lg transition text-sm font-medium"
          >
            <FolderPlus class="w-4 h-4" />
            New Folder
          </button>
        </div>
      {/if}
    </div>
  {:else if viewMode === 'grid'}
    <div class="grid grid-cols-2 sm:grid-cols-3 md:grid-cols-4 lg:grid-cols-5 xl:grid-cols-6 gap-3">
      {#each currentItems as item (item.id)}
        {#if renamingId === item.id}
          <div class="bg-white dark:bg-gray-800 border border-blue-400 rounded-xl p-4">
            <input
              id="rename-input"
              type="text"
              bind:value={renameValue}
              onkeydown={(e) => { if (e.key === 'Enter') confirmRename(); if (e.key === 'Escape') { renamingId = null; } }}
              onblur={confirmRename}
              class="w-full px-2 py-1 text-sm bg-gray-100 dark:bg-gray-700 border border-gray-300 dark:border-gray-600 rounded text-gray-900 dark:text-white focus:outline-none focus:ring-2 focus:ring-blue-500"
            />
          </div>
        {:else}
          <DriveFileCard
            {item}
            onOpen={handleOpen}
            onContextMenu={handleContextMenu}
          />
        {/if}
      {/each}
    </div>
  {:else}
    <div class="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 overflow-hidden">
      <table class="w-full">
        <thead>
          <tr class="border-b border-gray-200 dark:border-gray-700 text-left">
            <th class="py-2.5 px-3 text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wide">Name</th>
            <th class="py-2.5 px-3 text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wide w-24">Size</th>
            <th class="py-2.5 px-3 text-xs font-medium text-gray-500 dark:text-gray-400 uppercase tracking-wide w-32">Modified</th>
            <th class="py-2.5 px-3 w-12"></th>
          </tr>
        </thead>
        <tbody>
          {#each currentItems as item (item.id)}
            {#if renamingId === item.id}
              <tr class="border-b border-gray-100 dark:border-gray-700/50">
                <td colspan="4" class="py-2 px-3">
                  <input
                    id="rename-input"
                    type="text"
                    bind:value={renameValue}
                    onkeydown={(e) => { if (e.key === 'Enter') confirmRename(); if (e.key === 'Escape') { renamingId = null; } }}
                    onblur={confirmRename}
                    class="w-full px-2 py-1 text-sm bg-gray-100 dark:bg-gray-700 border border-gray-300 dark:border-gray-600 rounded text-gray-900 dark:text-white focus:outline-none focus:ring-2 focus:ring-blue-500"
                  />
                </td>
              </tr>
            {:else}
              <DriveFileRow
                {item}
                onOpen={handleOpen}
                onContextMenu={handleContextMenu}
              />
            {/if}
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
  {/if}
</div>

<!-- Context menu -->
{#if contextItem}
  <DriveContextMenu
    item={contextItem}
    x={contextX}
    y={contextY}
    onClose={closeContextMenu}
    onRename={startRename}
    onMove={handleMoveAction}
    onShare={handleShare}
    onCopyLink={handleCopyLink}
    onDownload={handleDownload}
    onToggleStar={handleToggleStar}
    onToggleVisibility={handleToggleVisibility}
    onDelete={handleDelete}
    onSeed={handleSeedToNetwork}
    onStopSeed={handleStopSeeding}
    onCopyHash={handleCopyMerkleHash}
    onCopyMagnet={handleCopyMagnetLink}
    onShowInExplorer={handleShowInExplorer}
  />
{/if}

<!-- Share modal -->
{#if shareItem}
  <DriveShareModal item={shareItem} {manifest} onClose={() => shareItem = null} />
{/if}

<!-- Move modal -->
{#if moveItem}
  <DriveMoveModal
    item={moveItem}
    {manifest}
    onMove={handleMoveConfirm}
    onClose={() => moveItem = null}
  />
{/if}

<!-- Delete confirmation modal -->
{#if deleteConfirmItem}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/50"
    onclick={() => deleteConfirmItem = null}
    onkeydown={(e) => { if (e.key === 'Escape') deleteConfirmItem = null; }}
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="bg-white dark:bg-gray-800 rounded-xl shadow-2xl p-6 max-w-sm w-full mx-4"
      onclick={(e) => e.stopPropagation()}
    >
      <h3 class="text-lg font-semibold text-gray-900 dark:text-white mb-2">Delete {deleteConfirmItem.type === 'folder' ? 'Folder' : 'File'}</h3>
      <p class="text-sm text-gray-600 dark:text-gray-400 mb-1">
        Are you sure you want to delete <strong class="text-gray-900 dark:text-white">"{deleteConfirmItem.name}"</strong>?
      </p>
      {#if deleteConfirmItem.type === 'folder'}
        <p class="text-sm text-amber-600 dark:text-amber-400 mb-4">This will delete all contents inside the folder.</p>
      {:else}
        <p class="text-sm text-gray-500 dark:text-gray-500 mb-4">This will remove it from your Drive.</p>
      {/if}
      <div class="flex justify-end gap-3">
        <button
          onclick={() => deleteConfirmItem = null}
          class="px-4 py-2 text-sm font-medium rounded-lg text-gray-700 dark:text-gray-300 bg-gray-100 dark:bg-gray-700 hover:bg-gray-200 dark:hover:bg-gray-600 transition"
        >Cancel</button>
        <button
          onclick={confirmDelete}
          class="px-4 py-2 text-sm font-medium rounded-lg text-white bg-red-600 hover:bg-red-700 transition"
        >Delete</button>
      </div>
    </div>
  </div>
{/if}
