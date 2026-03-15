<script lang="ts">
 import { HardDrive, FileText, Loader2, X, Check } from'lucide-svelte';
 import { formatHostedFileSize as formatBytes } from'$lib/utils/hostingPageUtils';

 interface DriveFile {
 id: string;
 name: string;
 size: number;
 }

 interface Props {
 files: DriveFile[];
 loading: boolean;
 onSelect: (selectedFiles: DriveFile[]) => void;
 onClose: () => void;
 }

 let { files, loading, onSelect, onClose }: Props = $props();

 let selected = $state<Set<string>>(new Set());

 function toggleFile(fileId: string) {
 const next = new Set(selected);
 if (next.has(fileId)) {
 next.delete(fileId);
 } else {
 next.add(fileId);
 }
 selected = next;
 }

 function toggleAll() {
 if (selected.size === files.length) {
 selected = new Set();
 } else {
 selected = new Set(files.map(f => f.id));
 }
 }

 function confirm() {
 const selectedFiles = files.filter((f) => selected.has(f.id));
 onSelect(selectedFiles);
 }

 let selectedSize = $derived(
 files.filter(f => selected.has(f.id)).reduce((s, f) => s + f.size, 0)
 );

 function handleKeydown(e: KeyboardEvent) {
 if (e.key ==='Escape') onClose();
 }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
 class="fixed inset-0 bg-[var(--surface-0)]/80 z-50 flex items-center justify-center p-4"
 onclick={(e) => { if (e.target === e.currentTarget) onClose(); }}
 role="dialog"
 aria-modal="true"
 aria-label="Select files from Drive"
>
 <div class=" w-full max-w-md overflow-hidden">
 <!-- Header -->
 <div class="flex items-center justify-between p-5 pb-4 border-b border-[var(--border)]/60">
 <div class="flex items-center gap-2.5">
 <div class="flex h-8 w-8 items-center justify-center rounded-lg bg-blue-400/[0.06]">
 <HardDrive class="w-4 h-4 text-blue-400" />
 </div>
 <h3 class="text-base font-semibold text-white">Select from Drive</h3>
 </div>
 <button
 onclick={onClose}
 class="p-1.5 text-[var(--text-secondary)] hover:text-white/70 rounded-lg hover:bg-[var(--surface-1)] transition-colors
 focus:outline-none focus:border-blue-400/40"
 aria-label="Close"
 >
 <X class="w-5 h-5" />
 </button>
 </div>

 {#if loading}
 <div class="flex flex-col items-center justify-center py-16">
 <Loader2 class="w-6 h-6 text-[var(--text-secondary)] animate-spin mb-3" />
 <span class="text-sm text-[var(--text-secondary)]">Loading Drive files...</span>
 </div>
 {:else if files.length === 0}
 <div class="flex flex-col items-center justify-center py-16">
 <div class="flex h-14 w-14 items-center justify-center rounded-xl bg-[var(--surface-1)] mb-3">
 <HardDrive class="w-7 h-7 text-[var(--text-secondary)] opacity-40" />
 </div>
 <p class="text-sm font-medium text-[var(--text-secondary)]">No files in Drive</p>
 <p class="text-xs text-[var(--text-tertiary)] mt-1">Upload files in the Drive page first</p>
 </div>
 {:else}
 <!-- Select all -->
 <div class="px-4 py-2.5 border-b border-[var(--border)]/60 bg-[var(--surface-1)]">
 <button
 onclick={toggleAll}
 class="flex items-center gap-2 text-xs font-medium text-[var(--text-secondary)] hover:text-white/70 transition-colors
 focus:outline-none focus:underline"
 >
 <div class="w-4.5 h-4.5 rounded border-2 flex items-center justify-center flex-shrink-0
 {selected.size === files.length && files.length > 0 ?'border-blue-400 bg-blue-500/[0.06]0' :'border-[var(--border)]/60'}">
 {#if selected.size === files.length && files.length > 0}
 <Check class="w-3 h-3 text-white" />
 {/if}
 </div>
 {selected.size === files.length ?'Deselect all' :'Select all'} ({files.length} files)
 </button>
 </div>

 <!-- File list -->
 <div class="max-h-72 overflow-y-auto divide-y divide-white/[0.06]">
 {#each files as file (file.id)}
 <button
 onclick={() => toggleFile(file.id)}
 class="flex items-center justify-between w-full px-4 py-3 text-left text-sm transition-colors
 {selected.has(file.id) ?'bg-blue-500/[0.06]/50' :'hover:bg-[var(--surface-1)]'}
 focus:outline-none focus:bg-[var(--surface-1)]"
 >
 <div class="flex items-center gap-2.5 min-w-0">
 <div class="w-5 h-5 rounded border-2 flex items-center justify-center flex-shrink-0 transition-colors
 {selected.has(file.id) ?'border-blue-400 bg-blue-500/[0.06]0' :'border-[var(--border)]/60'}">
 {#if selected.has(file.id)}
 <Check class="w-3 h-3 text-white" />
 {/if}
 </div>
 <FileText class="w-4 h-4 text-[var(--text-secondary)] flex-shrink-0" />
 <span class="truncate text-white/70">{file.name}</span>
 </div>
 <span class="text-xs text-[var(--text-secondary)] ml-2 flex-shrink-0 tabular-nums">{formatBytes(file.size)}</span>
 </button>
 {/each}
 </div>

 <!-- Footer -->
 <div class="flex items-center justify-between p-4 border-t border-[var(--border)]/60 bg-[var(--surface-1)]">
 <div class="text-xs text-[var(--text-secondary)]">
 <span class="font-medium">{selected.size}</span> file{selected.size !== 1 ?'s' :''}
 {#if selected.size > 0}
 <span class="text-[var(--text-secondary)] mx-1">|</span>
 <span class="tabular-nums">{formatBytes(selectedSize)}</span>
 {/if}
 </div>
 <div class="flex gap-2">
 <button
 onclick={onClose}
 class="px-3 py-1.5 text-sm font-medium text-white/70 border border-[var(--border)]/60 rounded-lg
 hover:bg-[var(--surface-1)] transition-colors focus:outline-none focus:border-blue-400/40"
 >
 Cancel
 </button>
 <button
 onclick={confirm}
 disabled={selected.size === 0}
 class="px-3.5 py-1.5 text-sm font-semibold bg-blue-400 hover:bg-blue-500 text-white rounded-lg transition-colors
 focus:outline-none focus:border-blue-400/40/50
 disabled:opacity-50 disabled:cursor-not-allowed"
 >
 Add {selected.size > 0 ? `${selected.size} file${selected.size !== 1 ?'s' :''}` :'Files'}
 </button>
 </div>
 </div>
 {/if}
 </div>
</div>
