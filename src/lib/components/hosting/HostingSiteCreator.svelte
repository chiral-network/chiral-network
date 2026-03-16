<script lang="ts">
 import { Plus, FolderOpen, HardDrive, X, File as FileIcon, Loader2 } from'lucide-svelte';
 import { formatHostedFileSize as formatFileSize } from'$lib/utils/hostingPageUtils';

 interface SelectedFile {
 name: string;
 path: string;
 size: number;
 }

 interface Props {
 newSiteName: string;
 selectedFiles: SelectedFile[];
 isDragOver: boolean;
 isCreating: boolean;
 onNameChange: (name: string) => void;
 onSelectFiles: () => void;
 onSelectFromDrive: () => void;
 onRemoveFile: (index: number) => void;
 onCreateSite: () => void;
 }

 let {
 newSiteName,
 selectedFiles,
 isDragOver,
 isCreating,
 onNameChange,
 onSelectFiles,
 onSelectFromDrive,
 onRemoveFile,
 onCreateSite,
 }: Props = $props();

 let canCreate = $derived(newSiteName.trim().length > 0 && selectedFiles.length > 0 && !isCreating);
 let totalSize = $derived(selectedFiles.reduce((s, f) => s + f.size, 0));
</script>

<div class="rounded-xl bg-white/70 dark:bg-white/[0.05] p-5 shadow-gray-200/50 dark:shadow-black/5 border border-gray-200/60 dark:border-white/[0.06]">
 <h2 class="mb-4 text-base font-semibold text-gray-900 dark:text-white/90 flex items-center gap-2">
 <div class="flex h-7 w-7 items-center justify-center rounded-lg bg-violet-500/15">
 <Plus class="h-3.5 w-3.5 text-violet-500" />
 </div>
 Create New Site
 </h2>

 <div class="space-y-4">
 <!-- Site Name -->
 <div>
 <label for="site-name" class="mb-1.5 block text-xs font-medium text-gray-500 dark:text-white/50 uppercase tracking-wide">Site Name</label>
 <input
 id="site-name"
 type="text"
 value={newSiteName}
 oninput={(e) => onNameChange(e.currentTarget.value)}
 placeholder="e.g. My Portfolio"
 class="w-full rounded-xl border border-gray-200/60 dark:border-white/[0.06] bg-white/70 dark:bg-white/[0.05] px-4 py-2.5 text-sm text-gray-900 dark:text-white/90 placeholder:text-gray-400 dark:text-white/40 transition-all
 focus:border-primary-400 focus:bg-white/70 dark:bg-white/[0.05] focus:outline-none"
 />
 </div>

 <!-- File source buttons -->
 <div class="flex gap-2">
 <button
 onclick={onSelectFromDrive}
 class="flex items-center gap-1.5 px-3 py-2 text-xs font-medium text-violet-500 border border-primary-200
 rounded-lg hover:bg-violet-100 dark:bg-violet-950/20 transition-colors
 focus:outline-none"
 >
 <HardDrive class="w-3.5 h-3.5" />
 Add from Drive
 </button>
 <button
 onclick={onSelectFiles}
 class="flex items-center gap-1.5 px-3 py-2 text-xs font-medium text-gray-500 dark:text-white/50 border border-gray-200/60 dark:border-white/[0.06]
 rounded-lg hover:bg-gray-100 dark:hover:bg-white/[0.05]/50 transition-colors
 focus:outline-none"
 >
 <FolderOpen class="w-3.5 h-3.5" />
 Browse Files
 </button>
 </div>

 <!-- Drop Zone -->
 <div
 role="button"
 tabindex="0"
 onclick={onSelectFiles}
 onkeydown={(e) => e.key ==='Enter' && onSelectFiles()}
 class="flex cursor-pointer flex-col items-center justify-center gap-2 rounded-xl border border-dashed py-8 px-4 transition-all
 {isDragOver
 ?'border-primary-400/40 bg-violet-500/15 scale-[1.01]'
 :'border-gray-200/60 dark:border-white/[0.06] bg-white/70 dark:bg-white/[0.05] hover:border-gray-200/60 dark:border-white/[0.06] hover:bg-gray-100 dark:hover:bg-white/[0.05]'}"
 >
 <div class="flex h-12 w-12 items-center justify-center rounded-full bg-white/70 dark:bg-white/[0.05]">
 <FolderOpen class="h-6 w-6 text-gray-500 dark:text-white/50" />
 </div>
 <p class="text-sm font-medium text-gray-500 dark:text-white/50">
 {isDragOver ?'Release to add files' :'Drop files here'}
 </p>
 <p class="text-xs text-gray-500 dark:text-white/50">
 HTML, CSS, JS, images, photos, documents
 </p>
 </div>

 <!-- Selected Files -->
 {#if selectedFiles.length > 0}
 <div class="space-y-2">
 <div class="flex items-center justify-between">
 <p class="text-xs font-medium text-gray-500 dark:text-white/50 uppercase tracking-wide">
 {selectedFiles.length} file{selectedFiles.length === 1 ?'' :'s'} selected
 </p>
 <p class="text-xs text-gray-500 dark:text-white/50 tabular-nums">{formatFileSize(totalSize)}</p>
 </div>
 <div class="max-h-40 overflow-y-auto rounded-xl border border-gray-200/60 dark:border-white/[0.06] divide-y divide-white/10">
 {#each selectedFiles as file, i (file.path)}
 <div class="flex items-center justify-between px-3 py-2 hover:bg-gray-100 dark:hover:bg-white/[0.05] transition-colors group">
 <div class="flex items-center gap-2.5 min-w-0">
 <FileIcon class="h-4 w-4 flex-shrink-0 text-gray-500 dark:text-white/50" />
 <span class="truncate text-sm text-gray-500 dark:text-white/50">{file.name}</span>
 <span class="text-xs text-gray-500 dark:text-white/50 tabular-nums flex-shrink-0">{formatFileSize(file.size)}</span>
 </div>
 <button
 onclick={(e: MouseEvent) => { e.stopPropagation(); onRemoveFile(i); }}
 aria-label="Remove {file.name}"
 class="ml-2 flex-shrink-0 rounded-md p-1 text-gray-500 dark:text-white/50 transition-colors
 hover:bg-red-100 dark:hover:bg-red-900/30 dark:bg-red-900/20 hover:text-red-500
 opacity-0 group-hover:opacity-100 focus:opacity-100"
 >
 <X class="h-3.5 w-3.5" />
 </button>
 </div>
 {/each}
 </div>
 </div>

 <!-- Create Button -->
 <button
 onclick={onCreateSite}
 disabled={!canCreate}
 class="flex w-full items-center justify-center gap-2 rounded-xl px-4 py-3 text-sm font-semibold text-white transition-all
 {canCreate
 ?'bg-violet-500 hover:bg-violet-500/90 shadow-primary-500/20 hover:shadow-md hover:shadow-primary-500/25 active:scale-[0.98]'
 :'bg-white/70 dark:bg-white/[0.05] cursor-not-allowed'}
 focus:outline-none"
 >
 {#if isCreating}
 <Loader2 class="h-4 w-4 animate-spin" />
 Creating...
 {:else}
 <Plus class="h-4 w-4" />
 Create Site
 {/if}
 </button>
 {/if}
 </div>
</div>
