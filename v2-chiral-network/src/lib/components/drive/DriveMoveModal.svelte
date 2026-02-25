<script lang="ts">
  import { Folder, ChevronRight, X } from 'lucide-svelte';
  import type { DriveItem, DriveManifest } from '$lib/stores/driveStore';
  import { driveStore } from '$lib/stores/driveStore';

  let {
    item,
    manifest,
    onMove,
    onClose,
  }: {
    item: DriveItem;
    manifest: DriveManifest;
    onMove: (itemId: string, targetFolderId: string | null) => void;
    onClose: () => void;
  } = $props();

  let selectedFolderId = $state<string | null>(null);

  // Build folder tree excluding the item being moved (and its descendants)
  function getExcludedIds(id: string): Set<string> {
    const excluded = new Set<string>();
    function collect(parentId: string) {
      excluded.add(parentId);
      manifest.items.filter(i => i.parentId === parentId).forEach(i => collect(i.id));
    }
    collect(id);
    return excluded;
  }

  const excludedIds = $derived(item.type === 'folder' ? getExcludedIds(item.id) : new Set([item.id]));

  function getFolderChildren(parentId: string | null): DriveItem[] {
    return manifest.items
      .filter(i => i.type === 'folder' && i.parentId === parentId && !excludedIds.has(i.id))
      .sort((a, b) => a.name.localeCompare(b.name));
  }

  function handleMove() {
    onMove(item.id, selectedFolderId);
    onClose();
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50" onclick={onClose}>
  <div
    class="bg-white dark:bg-gray-800 rounded-xl shadow-2xl w-full max-w-sm mx-4 p-6"
    onclick={(e) => e.stopPropagation()}
  >
    <div class="flex items-center justify-between mb-4">
      <h3 class="text-lg font-semibold text-gray-900 dark:text-white">Move "{item.name}"</h3>
      <button onclick={onClose} class="p-1 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition">
        <X class="w-5 h-5 text-gray-500" />
      </button>
    </div>

    <div class="max-h-64 overflow-y-auto border border-gray-200 dark:border-gray-700 rounded-lg">
      <!-- Root -->
      <button
        onclick={() => selectedFolderId = null}
        class="flex items-center gap-2 w-full px-3 py-2.5 text-sm transition
          {selectedFolderId === null
            ? 'bg-blue-50 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300'
            : 'text-gray-700 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-gray-700'}"
      >
        <Folder class="w-4 h-4 text-yellow-500 fill-yellow-500 opacity-80" />
        <span class="font-medium">My Drive</span>
      </button>

      {#snippet folderTree(parentId: string | null, depth: number)}
        {#each getFolderChildren(parentId) as folder}
          <button
            onclick={() => selectedFolderId = folder.id}
            class="flex items-center gap-2 w-full px-3 py-2 text-sm transition
              {selectedFolderId === folder.id
                ? 'bg-blue-50 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300'
                : 'text-gray-700 dark:text-gray-300 hover:bg-gray-50 dark:hover:bg-gray-700'}"
            style="padding-left: {12 + depth * 20}px"
          >
            <Folder class="w-4 h-4 text-yellow-500 fill-yellow-500 opacity-80 shrink-0" />
            <span class="truncate">{folder.name}</span>
          </button>
          {@render folderTree(folder.id, depth + 1)}
        {/each}
      {/snippet}

      {@render folderTree(null, 1)}
    </div>

    <div class="flex justify-end gap-2 mt-4">
      <button
        onclick={onClose}
        class="px-4 py-2 text-sm font-medium text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition"
      >
        Cancel
      </button>
      <button
        onclick={handleMove}
        class="px-4 py-2 text-sm font-medium text-white bg-blue-600 hover:bg-blue-700 rounded-lg transition"
      >
        Move Here
      </button>
    </div>
  </div>
</div>
