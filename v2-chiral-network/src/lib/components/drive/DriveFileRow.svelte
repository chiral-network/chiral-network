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
    if (!bytes) return '—';
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
    return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  }

  function formatDate(ts: number): string {
    return new Date(ts).toLocaleDateString(undefined, { month: 'short', day: 'numeric', year: 'numeric' });
  }
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<tr
  class="group hover:bg-gray-50 dark:hover:bg-gray-700/50 transition cursor-pointer select-none border-b border-gray-100 dark:border-gray-700/50"
  ondblclick={() => onOpen(item)}
  oncontextmenu={(e) => { e.preventDefault(); onContextMenu(item, e); }}
>
  <td class="py-2.5 px-3">
    <div class="flex items-center gap-3">
      {#if item.type === 'folder'}
        <Folder class="w-5 h-5 {getFolderColor()} fill-current opacity-80 shrink-0" />
      {:else}
        {@const Icon = getFileIcon(item.name)}
        <svelte:component this={Icon} class="w-5 h-5 {getFileColor(item.name)} shrink-0" />
      {/if}
      <span class="text-sm font-medium text-gray-900 dark:text-white truncate">{item.name}</span>
      {#if item.starred}
        <Star class="w-3.5 h-3.5 text-yellow-500 fill-yellow-500 shrink-0" />
      {/if}
      {#if item.shared}
        <Share2 class="w-3.5 h-3.5 text-blue-500 shrink-0" />
      {/if}
    </div>
  </td>
  <td class="py-2.5 px-3 text-sm text-gray-500 dark:text-gray-400">
    {item.type === 'folder' ? '—' : formatSize(item.size)}
  </td>
  <td class="py-2.5 px-3 text-sm text-gray-500 dark:text-gray-400">
    {formatDate(item.modifiedAt)}
  </td>
  <td class="py-2.5 px-3 text-right">
    <button
      onclick={(e) => { e.stopPropagation(); onContextMenu(item, e); }}
      class="p-1 hover:bg-gray-200 dark:hover:bg-gray-600 rounded opacity-0 group-hover:opacity-100 transition-opacity"
    >
      <MoreVertical class="w-4 h-4 text-gray-500" />
    </button>
  </td>
</tr>
