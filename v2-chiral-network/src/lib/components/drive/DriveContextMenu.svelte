<script lang="ts">
  import { FolderInput, FolderOpen, Pencil, Star, StarOff, Share2, Link, Download, Trash2, Eye, EyeOff, Globe, StopCircle, Copy, Link2 } from 'lucide-svelte';
  import type { DriveItem } from '$lib/stores/driveStore';

  let {
    item,
    x,
    y,
    onClose,
    onRename,
    onMove,
    onShare,
    onCopyLink,
    onDownload,
    onToggleStar,
    onToggleVisibility,
    onDelete,
    onSeed,
    onStopSeed,
    onCopyHash,
    onCopyMagnet,
  }: {
    item: DriveItem;
    x: number;
    y: number;
    onClose: () => void;
    onRename: (item: DriveItem) => void;
    onMove: (item: DriveItem) => void;
    onShare: (item: DriveItem) => void;
    onCopyLink: (item: DriveItem) => void;
    onDownload: (item: DriveItem) => void;
    onToggleStar: (item: DriveItem) => void;
    onToggleVisibility: (item: DriveItem) => void;
    onDelete: (item: DriveItem) => void;
    onSeed?: (item: DriveItem) => void;
    onStopSeed?: (item: DriveItem) => void;
    onCopyHash?: (item: DriveItem) => void;
    onCopyMagnet?: (item: DriveItem) => void;
    onShowInExplorer?: (item: DriveItem) => void;
  } = $props();

  function action(fn: (item: DriveItem) => void) {
    return () => { fn(item); onClose(); };
  }

  $effect(() => {
    function handleClick() { onClose(); }
    document.addEventListener('click', handleClick);
    return () => document.removeEventListener('click', handleClick);
  });

  const menuItems = $derived([
    { label: 'Rename', icon: Pencil, action: action(onRename) },
    { label: 'Move to...', icon: FolderInput, action: action(onMove) },
    ...(item.type === 'file'
      ? [{ label: 'Download', icon: Download, action: action(onDownload) }]
      : []),
    ...(onShowInExplorer
      ? [{ label: 'Show in Explorer', icon: FolderOpen, action: action(onShowInExplorer) }]
      : []),
    { label: 'Copy Link', icon: Link, action: action(onCopyLink) },
    { label: 'Share...', icon: Share2, action: action(onShare) },
    // Seeding actions
    ...(item.type === 'file' && !item.seeding && onSeed
      ? [{ label: 'Seed to Network', icon: Globe, action: action(onSeed) }]
      : []),
    ...(item.seeding && onStopSeed
      ? [{ label: 'Stop Seeding', icon: StopCircle, action: action(onStopSeed) }]
      : []),
    ...(item.merkleRoot && onCopyHash
      ? [{ label: 'Copy Merkle Hash', icon: Copy, action: action(onCopyHash) }]
      : []),
    ...(item.merkleRoot && onCopyMagnet
      ? [{ label: 'Copy Magnet Link', icon: Link2, action: action(onCopyMagnet) }]
      : []),
    ...(item.shared
      ? [{ label: item.isPublic ? 'Make Private' : 'Make Public', icon: item.isPublic ? EyeOff : Eye, action: action(onToggleVisibility) }]
      : []),
    { label: item.starred ? 'Unstar' : 'Star', icon: item.starred ? StarOff : Star, action: action(onToggleStar) },
    { label: 'Delete', icon: Trash2, action: action(onDelete), danger: true },
  ]);
</script>

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="fixed z-[100] w-48 bg-white dark:bg-gray-800 rounded-lg shadow-xl border border-gray-200 dark:border-gray-700 py-1"
  style="left: {x}px; top: {y}px;"
  onclick={(e) => e.stopPropagation()}
>
  {#each menuItems as mi}
    <button
      onclick={mi.action}
      class="flex items-center gap-2.5 w-full px-3 py-2 text-sm transition
        {mi.danger
          ? 'text-red-600 dark:text-red-400 hover:bg-red-50 dark:hover:bg-red-900/20'
          : 'text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700'}"
    >
      <svelte:component this={mi.icon} class="w-4 h-4" />
      {mi.label}
    </button>
  {/each}
</div>
