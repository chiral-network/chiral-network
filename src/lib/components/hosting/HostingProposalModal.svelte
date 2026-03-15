<script lang="ts">
 import {
 Coins, Shield, HardDrive, FileText,
 Loader2, Send, FolderOpen, X, Calendar
 } from 'lucide-svelte';
 import {
 formatHostedFileSize as formatBytes,
 formatPeerId,
 formatWeiAsChi,
 } from '$lib/utils/hostingPageUtils';
 import type { HostEntry } from '$lib/types/hosting';

 interface DriveFile {
 id: string;
 name: string;
 size: number;
 }

 interface Props {
 proposalHost: HostEntry;
 proposalFileHashes: string;
 proposalDurationDays: number;
 isProposing: boolean;
 driveFiles: DriveFile[];
 showDrivePicker: boolean;
 publishingDriveFile: string | null;
 onFileHashesChange: (value: string) => void;
 onDurationChange: (days: number) => void;
 onLoadDriveFiles: () => void;
 onAddDriveFile: (fileId: string, fileName: string) => void;
 onSendProposal: () => void;
 onClose: () => void;
 }

 let {
 proposalHost,
 proposalFileHashes,
 proposalDurationDays,
 isProposing,
 driveFiles,
 showDrivePicker,
 publishingDriveFile,
 onFileHashesChange,
 onDurationChange,
 onLoadDriveFiles,
 onAddDriveFile,
 onSendProposal,
 onClose,
 }: Props = $props();

 let hashCount = $derived(proposalFileHashes.split('\n').map(h => h.trim()).filter(Boolean).length);

 function handleKeydown(e: KeyboardEvent) {
 if (e.key === 'Escape') onClose();
 }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
 class="fixed inset-0 bg-[var(--surface-0)]/80 z-50 flex items-center justify-center p-4"
 onclick={(e) => { if (e.target === e.currentTarget) onClose(); }}
 role="dialog"
 aria-modal="true"
 aria-label="Propose hosting agreement"
>
 <div class=" bg-[var(--surface-1)] rounded-xl border border-[var(--border)] w-full max-w-lg overflow-hidden">
 <!-- Header -->
 <div class="flex items-center justify-between p-5 pb-4 border-b border-[var(--border)]">
 <div>
 <h3 class="text-lg font-semibold text-gray-900">Propose Hosting Agreement</h3>
 <p class="text-xs text-[var(--text-tertiary)] mt-0.5 font-mono">
 {formatPeerId(proposalHost.advertisement.peerId)}
 </p>
 </div>
 <button
 onclick={onClose}
 class="p-1.5 text-[var(--text-secondary)] hover:text-[var(--text-secondary)] dark:hover:text-[var(--text-secondary)] rounded-lg hover:bg-[var(--surface-1)] dark:hover:bg-[var(--surface-1)] transition-colors
 focus:outline-none focus:ring-2 focus:ring-gray-400/30"
 aria-label="Close"
 >
 <X class="w-5 h-5" />
 </button>
 </div>

 <div class="p-5 space-y-5 max-h-[70vh] overflow-y-auto">
 <!-- Host summary -->
 <div class="grid grid-cols-3 gap-3">
 <div class="flex flex-col items-center gap-1.5 p-3 rounded-xl bg-[var(--surface-1)] border border-[var(--border)]">
 <Coins class="w-4 h-4 text-[var(--text-secondary)]" />
 <span class="text-xs font-semibold text-gray-900 tabular-nums">{formatWeiAsChi(proposalHost.advertisement.pricePerMbPerDayWei)}</span>
 <span class="text-[10px] text-[var(--text-secondary)] uppercase tracking-wide">per MB/day</span>
 </div>
 <div class="flex flex-col items-center gap-1.5 p-3 rounded-xl bg-[var(--surface-1)] border border-[var(--border)]">
 <Shield class="w-4 h-4 text-[var(--text-secondary)]" />
 <span class="text-xs font-semibold text-gray-900 tabular-nums">{formatWeiAsChi(proposalHost.advertisement.minDepositWei)}</span>
 <span class="text-[10px] text-[var(--text-secondary)] uppercase tracking-wide">deposit</span>
 </div>
 <div class="flex flex-col items-center gap-1.5 p-3 rounded-xl bg-[var(--surface-1)] border border-[var(--border)]">
 <HardDrive class="w-4 h-4 text-[var(--text-secondary)]" />
 <span class="text-xs font-semibold text-gray-900 tabular-nums">{formatBytes(proposalHost.availableStorageBytes)}</span>
 <span class="text-[10px] text-[var(--text-secondary)] uppercase tracking-wide">available</span>
 </div>
 </div>

 <!-- File hashes -->
 <div>
 <div class="flex items-center justify-between mb-2">
 <label for="proposal-file-hashes" class="text-xs font-medium text-[var(--text-secondary)] uppercase tracking-wide">
 File Hashes
 {#if hashCount > 0}
 <span class="ml-1 text-[var(--text-secondary)] normal-case tracking-normal">({hashCount})</span>
 {/if}
 </label>
 <button
 onclick={onLoadDriveFiles}
 class="flex items-center gap-1 text-xs font-medium text-violet-500 hover:text-primary-700 dark:hover:text-violet-300 transition-colors
 focus:outline-none focus:underline"
 >
 <FolderOpen class="w-3.5 h-3.5" />
 Select from Drive
 </button>
 </div>
 <textarea
 id="proposal-file-hashes"
 value={proposalFileHashes}
 oninput={(e) => onFileHashesChange(e.currentTarget.value)}
 rows="3"
 placeholder="Paste file hashes, one per line..."
 class="w-full p-3 text-sm font-mono bg-[var(--surface-1)] border border-[var(--border)] rounded-xl text-gray-900 placeholder-gray-400
 focus:ring-2 focus:ring-primary-400/20 focus:border-primary-400 focus:outline-none resize-none transition-all"
 ></textarea>

 {#if showDrivePicker}
 <div class="mt-2 max-h-40 overflow-y-auto rounded-xl border border-[var(--border)] bg-[var(--surface-1)] divide-y divide-white/10">
 {#if driveFiles.length === 0}
 <p class="text-xs text-[var(--text-secondary)] p-4 text-center">No files in Drive</p>
 {:else}
 {#each driveFiles as file (file.id)}
 <button
 onclick={() => onAddDriveFile(file.id, file.name)}
 disabled={publishingDriveFile === file.id}
 class="flex items-center justify-between w-full px-3 py-2.5 text-left text-sm hover:bg-[var(--surface-1)] dark:hover:bg-[var(--surface-1)] transition-colors
 disabled:opacity-50 disabled:cursor-not-allowed focus:outline-none focus:bg-gray-100 dark:focus:bg-gray-600/50"
 >
 <div class="flex items-center gap-2.5 min-w-0">
 <FileText class="w-4 h-4 text-[var(--text-secondary)] flex-shrink-0" />
 <span class="truncate text-[var(--text-secondary)]">{file.name}</span>
 </div>
 <div class="flex items-center gap-2 flex-shrink-0 ml-2">
 <span class="text-xs text-[var(--text-secondary)] tabular-nums">{formatBytes(file.size)}</span>
 {#if publishingDriveFile === file.id}
 <Loader2 class="w-3.5 h-3.5 text-primary-500 animate-spin" />
 {/if}
 </div>
 </button>
 {/each}
 {/if}
 </div>
 {/if}
 </div>

 <!-- Duration -->
 <div>
 <label for="proposal-duration" class="flex items-center justify-between mb-2">
 <span class="text-xs font-medium text-[var(--text-secondary)] uppercase tracking-wide flex items-center gap-1.5">
 <Calendar class="w-3.5 h-3.5" />
 Duration
 </span>
 <span class="text-sm font-semibold text-gray-900 tabular-nums">
 {proposalDurationDays} day{proposalDurationDays !== 1 ? 's' : ''}
 </span>
 </label>
 <input
 id="proposal-duration"
 type="range"
 value={proposalDurationDays}
 oninput={(e) => onDurationChange(Number(e.currentTarget.value))}
 min="1"
 max="365"
 step="1"
 class="w-full accent-primary-600 h-2 rounded-full"
 />
 <div class="flex justify-between text-[10px] text-[var(--text-secondary)] mt-1 tabular-nums">
 <span>1 day</span>
 <span>1 year</span>
 </div>
 </div>

 <!-- Cost summary -->
 <div class="p-4 rounded-xl bg-violet-950/20/50 border border-primary-100">
 <div class="flex justify-between text-sm">
 <span class="text-[var(--text-secondary)]">Required Deposit</span>
 <span class="font-semibold text-gray-900 tabular-nums">{formatWeiAsChi(proposalHost.advertisement.minDepositWei)}</span>
 </div>
 <p class="text-[11px] text-[var(--text-secondary)] mt-1.5">
 Total cost depends on file sizes and will be calculated after the host accepts.
 </p>
 </div>
 </div>

 <!-- Footer actions -->
 <div class="flex justify-end gap-3 p-5 pt-4 border-t border-[var(--border)] bg-[var(--surface-1)]">
 <button
 onclick={onClose}
 class="px-4 py-2.5 text-sm font-medium text-[var(--text-secondary)] border border-[var(--border)] rounded-xl
 hover:bg-[var(--surface-1)] dark:hover:bg-[var(--surface-1)] transition-colors focus:outline-none focus:ring-2 focus:ring-gray-400/30"
 >
 Cancel
 </button>
 <button
 onclick={onSendProposal}
 disabled={isProposing || hashCount === 0}
 class="flex items-center gap-2 px-5 py-2.5 text-sm font-semibold bg-violet-500/80 border border-primary-400/30 hover:bg-violet-500/90 dark:hover:bg-violet-600/80 text-white rounded-xl transition-all
 shadow-primary-500/20 hover:shadow-md hover:shadow-primary-500/25 active:scale-[0.98]
 focus:outline-none focus:ring-2 focus:ring-violet-500/50 focus:ring-offset-2 
 disabled:opacity-50 disabled:cursor-not-allowed disabled:shadow-none"
 >
 {#if isProposing}
 <Loader2 class="w-4 h-4 animate-spin" />
 Sending...
 {:else}
 <Send class="w-4 h-4" />
 Send Proposal
 {/if}
 </button>
 </div>
 </div>
</div>
