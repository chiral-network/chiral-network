<script lang="ts">
  import { Star, MoreVertical, Share2, Folder } from 'lucide-svelte';
  import { getFileIcon, getFileColor, getFolderColor } from '$lib/utils/fileIcons';
  import type { DriveItem } from '$lib/stores/driveStore';

  let {
    item,
    onOpen,
    onContextMenu,
  }: {
    item: DriveItem;
    onOpen: (item: DriveItem) => void;
    onContextMenu: (item: DriveItem, event: MouseEvent) => void;
  } = $props();

  function formatSize(bytes?: number): string {
    if (!bytes) return '';
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="group relative bg-white dark:bg-gray-800 border border-gray-200 dark:border-gray-700 rounded-xl p-4 hover:shadow-md hover:border-blue-300 dark:hover:border-blue-600 transition cursor-pointer select-none"
  ondblclick={() => onOpen(item)}
  oncontextmenu={(e) => { e.preventDefault(); onContextMenu(item, e); }}
>
  <!-- Action buttons -->
  <div class="absolute top-2 right-2 flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
    {#if item.starred}
      <Star class="w-3.5 h-3.5 text-yellow-500 fill-yellow-500" />
    {/if}
    {#if item.shared}
      <Share2 class="w-3.5 h-3.5 text-blue-500" />
    {/if}
    <button
      onclick={(e) => { e.stopPropagation(); onContextMenu(item, e); }}
      class="p-1 hover:bg-gray-100 dark:hover:bg-gray-700 rounded"
    >
      <MoreVertical class="w-4 h-4 text-gray-500" />
    </button>
  </div>

  <!-- Icon -->
  <div class="flex items-center justify-center w-12 h-12 mx-auto mb-3 rounded-lg {item.type === 'folder' ? 'bg-yellow-50 dark:bg-yellow-900/20' : 'bg-gray-50 dark:bg-gray-700'}">
    {#if item.type === 'folder'}
      <Folder class="w-7 h-7 {getFolderColor()} fill-current opacity-80" />
    {:else}
      {@const Icon = getFileIcon(item.name)}
      <svelte:component this={Icon} class="w-6 h-6 {getFileColor(item.name)}" />
    {/if}
  </div>

  <!-- Name & metadata -->
  <div class="text-center">
    <p class="text-sm font-medium text-gray-900 dark:text-white truncate" title={item.name}>
      {item.name}
    </p>
    {#if item.type === 'file' && item.size}
      <p class="text-xs text-gray-500 dark:text-gray-400 mt-0.5">{formatSize(item.size)}</p>
    {/if}
  </div>
</div>
