<script lang="ts">
  import { Upload, FolderPlus, Search, LayoutGrid, List } from 'lucide-svelte';

  let {
    viewMode = 'grid',
    searchQuery = '',
    onUpload,
    onNewFolder,
    onViewModeChange,
    onSearchChange,
  }: {
    viewMode: 'grid' | 'list';
    searchQuery: string;
    onUpload: () => void;
    onNewFolder: () => void;
    onViewModeChange: (mode: 'grid' | 'list') => void;
    onSearchChange: (query: string) => void;
  } = $props();
</script>

<div class="flex flex-wrap items-center gap-2">
  <button
    onclick={onUpload}
    class="flex items-center gap-2 px-4 py-2 backdrop-blur-md bg-primary-500/80 dark:bg-primary-600/70 border border-primary-400/30 hover:bg-primary-500/90 dark:hover:bg-primary-600/80 text-white rounded-lg transition text-sm font-medium shadow-sm shadow-black/5"
  >
    <Upload class="w-4 h-4" />
    Upload File
  </button>

  <button
    onclick={onNewFolder}
    class="flex items-center gap-2 px-4 py-2 backdrop-blur-md bg-white/15 dark:bg-white/10 border border-white/20 dark:border-white/15 hover:bg-white/25 dark:hover:bg-white/15 text-gray-700 dark:text-gray-300 rounded-lg transition text-sm font-medium shadow-sm shadow-black/5"
  >
    <FolderPlus class="w-4 h-4" />
    New Folder
  </button>

  <div class="flex-1"></div>

  <div class="relative">
    <Search class="w-4 h-4 absolute left-3 top-1/2 -translate-y-1/2 text-gray-400" />
    <input
      type="text"
      placeholder="Search files..."
      value={searchQuery}
      oninput={(e) => onSearchChange((e.target as HTMLInputElement).value)}
      class="pl-9 pr-3 py-2 backdrop-blur-md bg-white/10 dark:bg-white/5 border border-white/20 dark:border-white/10 rounded-lg text-sm text-gray-900 dark:text-white placeholder-gray-400 w-48 focus:outline-none focus:ring-2 focus:ring-primary-500/40"
    />
  </div>

  <div class="flex items-center backdrop-blur-md bg-white/10 dark:bg-white/5 border border-white/15 dark:border-white/10 rounded-lg p-0.5">
    <button
      onclick={() => onViewModeChange('grid')}
      class="p-1.5 rounded transition {viewMode === 'grid' ? 'bg-white/20 dark:bg-white/10 shadow-sm ring-1 ring-white/20' : 'hover:bg-white/15 dark:hover:bg-white/10'}"
      title="Grid view"
    >
      <LayoutGrid class="w-4 h-4 text-gray-700 dark:text-gray-300" />
    </button>
    <button
      onclick={() => onViewModeChange('list')}
      class="p-1.5 rounded transition {viewMode === 'list' ? 'bg-white/20 dark:bg-white/10 shadow-sm ring-1 ring-white/20' : 'hover:bg-white/15 dark:hover:bg-white/10'}"
      title="List view"
    >
      <List class="w-4 h-4 text-gray-700 dark:text-gray-300" />
    </button>
  </div>
</div>
