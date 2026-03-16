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

<div class="rounded-xl border border-[var(--border)]/60 bg-[var(--surface-1)] p-5">
 <h2 class="mb-4 text-base font-semibold text-[var(--text-primary)] flex items-center gap-2">
 <div class="flex h-7 w-7 items-center justify-center rounded-lg bg-violet-500/10">
 <Plus class="h-3.5 w-3.5 text-violet-600 dark:text-violet-400" />
 </div>
 Create New Site
 </h2>

 <div class="space-y-4">
 <!-- Site Name -->
 <div>
 <label for="site-name" class="mb-1.5 block text-xs font-medium text-[var(--text-secondary)] uppercase tracking-wide">Site Name</label>
 <input
 id="site-name"
 type="text"
 value={newSiteName}
 oninput={(e) => onNameChange(e.currentTarget.value)}
 placeholder="e.g. My Portfolio"
 class="w-full rounded-xl border border-[var(--border)]/60 bg-[var(--surface-2)] px-4 py-2.5 text-sm text-[var(--text-primary)] placeholder:text-[var(--text-secondary)] transition-all
 focus:border-cyan-500/30 focus:bg-[var(--surface-2)] focus:outline-none focus:ring-cyan-500/20
"
 />
 </div>

 <!-- File source buttons -->
 <div class="flex gap-2">
 <button
 onclick={onSelectFromDrive}
 class="flex items-center gap-1.5 px-3 py-2 text-xs font-medium text-violet-600 dark:text-violet-400 border border-violet-500/20
 rounded-lg hover:bg-violet-500/10 transition-colors
 focus:outline-none focus:ring-cyan-500/20"
 >
 <HardDrive class="w-3.5 h-3.5" />
 Add from Drive
 </button>
 <button
 onclick={onSelectFiles}
 class="flex items-center gap-1.5 px-3 py-2 text-xs font-medium text-[var(--text-secondary)] border border-[var(--border)]/60
 rounded-lg hover:bg-[var(--surface-2)] transition-colors
 focus:outline-none focus:border-violet-500/50"
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
 class="flex cursor-pointer flex-col items-center justify-center gap-2 rounded-xl border-2 border-dashed py-8 px-4 transition-all
 {isDragOver
 ?'border-violet-500 bg-violet-500/10/60 scale-[1.01]'
 :'border-[var(--border)]/60 bg-[var(--surface-2)] hover:border-[var(--border)] hover:bg-[var(--surface-2)]'}"
 >
 <div class="flex h-12 w-12 items-center justify-center rounded-full bg-[var(--surface-2)]">
 <FolderOpen class="h-6 w-6 text-[var(--text-secondary)]" />
 </div>
 <p class="text-sm font-medium text-[var(--text-secondary)]">
 {isDragOver ?'Release to add files' :'Drop files here'}
 </p>
 <p class="text-xs text-[var(--text-tertiary)]">
 HTML, CSS, JS, images, photos, documents
 </p>
 </div>

 <!-- Selected Files -->
 {#if selectedFiles.length > 0}
 <div class="space-y-2">
 <div class="flex items-center justify-between">
 <p class="text-xs font-medium text-[var(--text-secondary)] uppercase tracking-wide">
 {selectedFiles.length} file{selectedFiles.length === 1 ?'' :'s'} selected
 </p>
 <p class="text-xs text-[var(--text-secondary)] tabular-nums">{formatFileSize(totalSize)}</p>
 </div>
 <div class="max-h-40 overflow-y-auto rounded-xl border border-[var(--border)]/60 divide-y divide-white/[0.06]">
 {#each selectedFiles as file, i (file.path)}
 <div class="flex items-center justify-between px-3 py-2 hover:bg-[var(--surface-2)]/80 transition-colors group">
 <div class="flex items-center gap-2.5 min-w-0">
 <FileIcon class="h-4 w-4 flex-shrink-0 text-[var(--text-secondary)]" />
 <span class="truncate text-sm text-[var(--text-secondary)]">{file.name}</span>
 <span class="text-xs text-[var(--text-secondary)] tabular-nums flex-shrink-0">{formatFileSize(file.size)}</span>
 </div>
 <button
 onclick={(e: MouseEvent) => { e.stopPropagation(); onRemoveFile(i); }}
 aria-label="Remove {file.name}"
 class="ml-2 flex-shrink-0 rounded-xl p-1 text-[var(--text-secondary)] transition-colors
 hover:bg-red-100 dark:hover:bg-red-900/30 hover:text-red-500
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
 class="flex w-full items-center justify-center gap-2 rounded-xl px-4 py-3 text-sm font-semibold text-[var(--text-primary)] transition-all
 {canCreate
 ?'bg-violet-600 hover:bg-violet-500  active:scale-[0.98]'
 :'bg-[var(--surface-2)] cursor-not-allowed'}
 focus:outline-none focus:border-violet-500/50/50"
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
