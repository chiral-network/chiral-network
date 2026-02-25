<script lang="ts">
  import { onMount } from 'svelte';
  import { HardDrive, FolderPlus, Upload } from 'lucide-svelte';
  import { driveStore, type DriveItem, type DriveManifest } from '$lib/stores/driveStore';
  import { toasts } from '$lib/toastStore';
  import DriveBreadcrumb from '$lib/components/drive/DriveBreadcrumb.svelte';
  import DriveToolbar from '$lib/components/drive/DriveToolbar.svelte';
  import DriveFileCard from '$lib/components/drive/DriveFileCard.svelte';
  import DriveFileRow from '$lib/components/drive/DriveFileRow.svelte';
  import DriveContextMenu from '$lib/components/drive/DriveContextMenu.svelte';
  import DriveShareModal from '$lib/components/drive/DriveShareModal.svelte';
  import DriveMoveModal from '$lib/components/drive/DriveMoveModal.svelte';

  let manifest = $state<DriveManifest>({ version: 1, items: [], lastModified: 0 });
  let currentFolderId = $state<string | null>(null);
  let viewMode = $state<'grid' | 'list'>('grid');
  let searchQuery = $state('');
  let creatingFolder = $state(false);
  let newFolderName = $state('');

  // Context menu
  let contextItem = $state<DriveItem | null>(null);
  let contextX = $state(0);
  let contextY = $state(0);

  // Modals
  let shareItem = $state<DriveItem | null>(null);
  let moveItem = $state<DriveItem | null>(null);
  let renamingId = $state<string | null>(null);
  let renameValue = $state('');

  // Drag and drop
  let isDragging = $state(false);

  // Subscribe to store
  driveStore.subscribe(m => manifest = m);

  onMount(async () => {
    await driveStore.load();
    const saved = localStorage.getItem('drive-view-mode');
    if (saved === 'list' || saved === 'grid') viewMode = saved;
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

  function isTauri(): boolean {
    return typeof window !== 'undefined' && ('__TAURI__' in window || '__TAURI_INTERNALS__' in window);
  }

  // Navigation
  function navigateTo(folderId: string | null) {
    currentFolderId = folderId;
    searchQuery = '';
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

  // Upload
  async function handleUpload() {
    if (isTauri()) {
      try {
        const { open } = await import('@tauri-apps/plugin-dialog');
        const selected = await open({ multiple: true });
        if (!selected) return;
        const paths = Array.isArray(selected) ? selected : [selected];
        for (const filePath of paths) {
          const name = filePath.split(/[/\\]/).pop() || 'unnamed';
          let size: number | undefined;
          try {
            const invoke = (globalThis as any).__tauri_invoke ?? (globalThis as any).invoke;
            if (invoke) size = await invoke('get_file_size', { filePath });
          } catch { /* ignore */ }

          // Publish to network
          let hash: string | undefined;
          try {
            const invoke = (globalThis as any).__tauri_invoke ?? (globalThis as any).invoke;
            if (invoke) {
              const result = await invoke('publish_file', { filePath, price: 0 });
              hash = result?.merkle_root || result?.hash;
            }
          } catch (e) {
            console.warn('Failed to publish file to network:', e);
          }

          driveStore.addFile({
            name,
            parentId: currentFolderId,
            hash,
            size,
            localPath: filePath,
          });
        }
        toasts.show(`Uploaded ${paths.length} file${paths.length > 1 ? 's' : ''}`, 'success');
      } catch (e) {
        toasts.show('Upload failed: ' + (e as Error).message, 'error');
      }
    } else {
      // Web fallback: file input
      const input = document.createElement('input');
      input.type = 'file';
      input.multiple = true;
      input.onchange = () => {
        if (!input.files) return;
        for (const file of input.files) {
          driveStore.addFile({
            name: file.name,
            parentId: currentFolderId,
            size: file.size,
          });
        }
        toasts.show(`Added ${input.files.length} file${input.files.length > 1 ? 's' : ''}`, 'success');
      };
      input.click();
    }
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

  function confirmNewFolder() {
    const name = newFolderName.trim();
    if (!name) return;
    driveStore.createFolder(name, currentFolderId);
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

  function confirmRename() {
    if (renamingId && renameValue.trim()) {
      driveStore.renameItem(renamingId, renameValue.trim());
    }
    renamingId = null;
    renameValue = '';
  }

  // Copy hash
  async function handleCopyHash(item: DriveItem) {
    if (!item.hash) {
      toasts.show('No hash available for this file', 'error');
      return;
    }
    try {
      await navigator.clipboard.writeText(item.hash);
      toasts.show('Hash copied to clipboard', 'success');
    } catch {
      toasts.show('Failed to copy hash', 'error');
    }
    driveStore.markShared(item.id);
  }

  // Share
  function handleShare(item: DriveItem) {
    shareItem = item;
    driveStore.markShared(item.id);
  }

  // Delete
  function handleDelete(item: DriveItem) {
    if (confirm(`Delete "${item.name}"? ${item.type === 'folder' ? 'This will delete all contents.' : 'This will remove it from your Drive.'}`)) {
      driveStore.deleteItem(item.id);
      toasts.show(`Deleted "${item.name}"`, 'success');
    }
  }

  // Move
  function handleMoveAction(item: DriveItem) {
    moveItem = item;
  }

  function handleMoveConfirm(itemId: string, targetFolderId: string | null) {
    driveStore.moveItem(itemId, targetFolderId);
    toasts.show('Item moved', 'success');
  }

  // Star
  function handleToggleStar(item: DriveItem) {
    driveStore.toggleStar(item.id);
  }

  // Drag and drop
  function handleDragOver(e: DragEvent) {
    e.preventDefault();
    isDragging = true;
  }

  function handleDragLeave() {
    isDragging = false;
  }

  function handleDrop(e: DragEvent) {
    e.preventDefault();
    isDragging = false;
    if (e.dataTransfer?.files) {
      for (const file of e.dataTransfer.files) {
        driveStore.addFile({
          name: file.name,
          parentId: currentFolderId,
          size: file.size,
        });
      }
      if (e.dataTransfer.files.length > 0) {
        toasts.show(`Added ${e.dataTransfer.files.length} file${e.dataTransfer.files.length > 1 ? 's' : ''}`, 'success');
      }
    }
  }

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
      Personal cloud storage with P2P sharing
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

  <!-- Drag overlay -->
  {#if isDragging}
    <div class="border-2 border-dashed border-blue-400 bg-blue-50 dark:bg-blue-900/20 rounded-xl p-12 text-center">
      <Upload class="w-10 h-10 mx-auto text-blue-500 mb-2" />
      <p class="text-blue-600 dark:text-blue-400 font-medium">Drop files here to upload</p>
    </div>
  {/if}

  <!-- Content -->
  {#if currentItems.length === 0 && !creatingFolder && !isDragging}
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
    onCopyHash={handleCopyHash}
    onToggleStar={handleToggleStar}
    onDelete={handleDelete}
  />
{/if}

<!-- Share modal -->
{#if shareItem}
  <DriveShareModal item={shareItem} onClose={() => shareItem = null} />
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
