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
 class="flex items-center gap-2 px-4 py-2 bg-violet-500/80 border border-primary-400/30 hover:bg-violet-500/90 text-white rounded-lg transition text-sm font-medium shadow-gray-200/50 dark:shadow-black/5"
 >
 <Upload class="w-4 h-4" />
 Upload File
 </button>

 <button
 onclick={onNewFolder}
 class="flex items-center gap-2 px-4 py-2 bg-white/70 dark:bg-white/[0.05] border border-gray-200/60 dark:border-white/[0.06] hover:bg-gray-100 dark:hover:bg-white/[0.05] text-gray-500 dark:text-white/50 rounded-lg transition text-sm font-medium shadow-gray-200/50 dark:shadow-black/5"
 >
 <FolderPlus class="w-4 h-4" />
 New Folder
 </button>

 <div class="flex-1"></div>

 <div class="relative">
 <Search class="w-4 h-4 absolute left-3 top-1/2 -translate-y-1/2 text-gray-500 dark:text-white/50" />
 <input
 type="text"
 placeholder="Search files..."
 value={searchQuery}
 oninput={(e) => onSearchChange((e.target as HTMLInputElement).value)}
 class="pl-9 pr-3 py-2 bg-white/70 dark:bg-white/[0.05] border border-gray-200/60 dark:border-white/[0.06] rounded-lg text-sm text-gray-900 dark:text-white/90 placeholder:text-gray-400 dark:text-white/40 w-48 focus:outline-none"
 />
 </div>

 <div class="flex items-center bg-white/70 dark:bg-white/[0.05] border border-gray-200/60 dark:border-white/[0.06] rounded-lg p-0.5">
 <button
 onclick={() => onViewModeChange('grid')}
 class="p-1.5 rounded transition {viewMode ==='grid' ?'bg-white/70 dark:bg-white/[0.05]' :'hover:bg-gray-100 dark:hover:bg-white/[0.05]'}"
 title="Grid view"
 >
 <LayoutGrid class="w-4 h-4 text-gray-500 dark:text-white/50" />
 </button>
 <button
 onclick={() => onViewModeChange('list')}
 class="p-1.5 rounded transition {viewMode ==='list' ?'bg-white/70 dark:bg-white/[0.05]' :'hover:bg-gray-100 dark:hover:bg-white/[0.05]'}"
 title="List view"
 >
 <List class="w-4 h-4 text-gray-500 dark:text-white/50" />
 </button>
 </div>
</div>
