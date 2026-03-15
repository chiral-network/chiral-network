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
    class="flex items-center gap-2 px-4 py-2 bg-cyan-600/80 hover:bg-cyan-500/90 text-white rounded-lg transition text-sm font-medium"
  >
    <Upload class="w-4 h-4" />
    Upload File
  </button>

  <button
    onclick={onNewFolder}
    class="flex items-center gap-2 px-4 py-2 bg-gray-800 hover:bg-cyan-500/15 text-gray-300 rounded-lg transition text-sm font-medium"
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
      class="pl-9 pr-3 py-2 bg-gray-800 border border-cyan-500/20 rounded-lg text-sm text-gray-100 placeholder-gray-400 w-48 focus:outline-none focus:ring-2 focus:ring-cyan-500/40"
    />
  </div>

  <div class="flex items-center bg-gray-800 rounded-lg p-0.5">
    <button
      onclick={() => onViewModeChange('grid')}
      class="p-1.5 rounded transition {viewMode === 'grid' ? 'bg-gray-900 shadow-[0_0_10px_rgba(6,182,212,0.05)]' : 'hover:bg-cyan-500/15'}"
      title="Grid view"
    >
      <LayoutGrid class="w-4 h-4 text-gray-300" />
    </button>
    <button
      onclick={() => onViewModeChange('list')}
      class="p-1.5 rounded transition {viewMode === 'list' ? 'bg-gray-900 shadow-[0_0_10px_rgba(6,182,212,0.05)]' : 'hover:bg-cyan-500/15'}"
      title="List view"
    >
      <List class="w-4 h-4 text-gray-300" />
    </button>
  </div>
</div>
