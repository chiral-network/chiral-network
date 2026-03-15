<script lang="ts">
 import { Upload, FolderPlus, Search, LayoutGrid, List } from'lucide-svelte';

 let {
 viewMode ='grid',
 searchQuery ='',
 onUpload,
 onNewFolder,
 onViewModeChange,
 onSearchChange,
 }: {
 viewMode:'grid' |'list';
 searchQuery: string;
 onUpload: () => void;
 onNewFolder: () => void;
 onViewModeChange: (mode:'grid' |'list') => void;
 onSearchChange: (query: string) => void;
 } = $props();
</script>

<div class="flex flex-wrap items-center gap-2">
 <button
 onclick={onUpload}
 class="flex items-center gap-2 px-4 py-2 bg-indigo-600 hover:bg-indigo-700 text-white rounded-lg transition text-sm font-medium"
 >
 <Upload class="w-4 h-4" />
 Upload File
 </button>

 <button
 onclick={onNewFolder}
 class="flex items-center gap-2 px-4 py-2 bg-[var(--surface-1)] hover:bg-[var(--surface-1)] text-white/70 rounded-lg transition text-sm font-medium"
 >
 <FolderPlus class="w-4 h-4" />
 New Folder
 </button>

 <div class="flex-1"></div>

 <div class="relative">
 <Search class="w-4 h-4 absolute left-3 top-1/2 -translate-y-1/2 text-[var(--text-secondary)]" />
 <input
 type="text"
 placeholder="Search files..."
 value={searchQuery}
 oninput={(e) => onSearchChange((e.target as HTMLInputElement).value)}
 class="pl-9 pr-3 py-2 bg-[var(--surface-1)] border border-[var(--border)]/60 rounded-lg text-sm text-white placeholder:text-[var(--text-tertiary)] w-48 focus:outline-none focus:border-blue-400/40"
 />
 </div>

 <div class="flex items-center bg-[var(--surface-1)] rounded-lg p-0.5">
 <button
 onclick={() => onViewModeChange('grid')}
 class="p-1.5 rounded transition {viewMode ==='grid' ?'bg-[var(--surface-0)] ' :'hover:bg-[var(--surface-1)]'}"
 title="Grid view"
 >
 <LayoutGrid class="w-4 h-4 text-white/70" />
 </button>
 <button
 onclick={() => onViewModeChange('list')}
 class="p-1.5 rounded transition {viewMode ==='list' ?'bg-[var(--surface-0)] ' :'hover:bg-[var(--surface-1)]'}"
 title="List view"
 >
 <List class="w-4 h-4 text-white/70" />
 </button>
 </div>
</div>
