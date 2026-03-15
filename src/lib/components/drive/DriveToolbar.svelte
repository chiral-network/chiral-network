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
 class="flex items-center gap-2 px-4 py-2 bg-[#13111C] hover:bg-[#13111C] text-white/70 rounded-lg transition text-sm font-medium"
 >
 <FolderPlus class="w-4 h-4" />
 New Folder
 </button>

 <div class="flex-1"></div>

 <div class="relative">
 <Search class="w-4 h-4 absolute left-3 top-1/2 -translate-y-1/2 text-white/[0.06]" />
 <input
 type="text"
 placeholder="Search files..."
 value={searchQuery}
 oninput={(e) => onSearchChange((e.target as HTMLInputElement).value)}
 class="pl-9 pr-3 py-2 bg-[#13111C] border border-white/[0.06]/60 rounded-lg text-sm text-white placeholder:text-white/[0.08] w-48 focus:outline-none focus:border-blue-400/40"
 />
 </div>

 <div class="flex items-center bg-[#13111C] rounded-lg p-0.5">
 <button
 onclick={() => onViewModeChange('grid')}
 class="p-1.5 rounded transition {viewMode ==='grid' ?'bg-[#13111C]' :'hover:bg-[#13111C]'}"
 title="Grid view"
 >
 <LayoutGrid class="w-4 h-4 text-white/70" />
 </button>
 <button
 onclick={() => onViewModeChange('list')}
 class="p-1.5 rounded transition {viewMode ==='list' ?'bg-[#13111C]' :'hover:bg-[#13111C]'}"
 title="List view"
 >
 <List class="w-4 h-4 text-white/70" />
 </button>
 </div>
</div>
