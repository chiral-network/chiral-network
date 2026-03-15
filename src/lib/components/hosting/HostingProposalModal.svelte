<script lang="ts">
 import {
 Coins, Shield, HardDrive, FileText,
 Loader2, Send, FolderOpen, X, Calendar
 } from'lucide-svelte';
 import {
 formatHostedFileSize as formatBytes,
 formatPeerId,
 formatWeiAsChi,
 } from'$lib/utils/hostingPageUtils';
 import type { HostEntry } from'$lib/types/hosting';

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
 if (e.key ==='Escape') onClose();
 }
</script>

<svelte:window onkeydown={handleKeydown} />

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
 class="fixed inset-0 bg-gray-950/80 z-50 flex items-center justify-center p-4"
 onclick={(e) => { if (e.target === e.currentTarget) onClose(); }}
 role="dialog"
 aria-modal="true"
 aria-label="Propose hosting agreement"
>
 <div class=" w-full max-w-lg overflow-hidden">
 <!-- Header -->
 <div class="flex items-center justify-between p-5 pb-4 border-b border-gray-800/60/60">
 <div>
 <h3 class="text-lg font-semibold text-white">Propose Hosting Agreement</h3>
 <p class="text-xs text-white/[0.06] mt-0.5 font-mono">
 {formatPeerId(proposalHost.advertisement.peerId)}
 </p>
 </div>
 <button
 onclick={onClose}
 class="p-1.5 text-white/[0.06] hover:text-white/70 rounded-lg hover:bg-gray-950 transition-colors
 focus:outline-none focus:border-blue-400/40"
 aria-label="Close"
 >
 <X class="w-5 h-5" />
 </button>
 </div>

 <div class="p-5 space-y-5 max-h-[70vh] overflow-y-auto">
 <!-- Host summary -->
 <div class="grid grid-cols-3 gap-3">
 <div class="flex flex-col items-center gap-1.5 p-3 rounded-xl bg-gray-950">
 <Coins class="w-4 h-4 text-white/[0.06]" />
 <span class="text-xs font-semibold text-white tabular-nums">{formatWeiAsChi(proposalHost.advertisement.pricePerMbPerDayWei)}</span>
 <span class="text-[10px] text-white/[0.06] uppercase tracking-wide">per MB/day</span>
 </div>
 <div class="flex flex-col items-center gap-1.5 p-3 rounded-xl bg-gray-950">
 <Shield class="w-4 h-4 text-white/[0.06]" />
 <span class="text-xs font-semibold text-white tabular-nums">{formatWeiAsChi(proposalHost.advertisement.minDepositWei)}</span>
 <span class="text-[10px] text-white/[0.06] uppercase tracking-wide">deposit</span>
 </div>
 <div class="flex flex-col items-center gap-1.5 p-3 rounded-xl bg-gray-950">
 <HardDrive class="w-4 h-4 text-white/[0.06]" />
 <span class="text-xs font-semibold text-white tabular-nums">{formatBytes(proposalHost.availableStorageBytes)}</span>
 <span class="text-[10px] text-white/[0.06] uppercase tracking-wide">available</span>
 </div>
 </div>

 <!-- File hashes -->
 <div>
 <div class="flex items-center justify-between mb-2">
 <label for="proposal-file-hashes" class="text-xs font-medium text-white/[0.06] uppercase tracking-wide">
 File Hashes
 {#if hashCount > 0}
 <span class="ml-1 text-white/[0.06] normal-case tracking-normal">({hashCount})</span>
 {/if}
 </label>
 <button
 onclick={onLoadDriveFiles}
 class="flex items-center gap-1 text-xs font-medium text-blue-400 hover:text-blue-400 transition-colors
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
 class="w-full p-3 text-sm font-mono bg-gray-950 border border-gray-800/60/60 rounded-xl text-white placeholder:text-white/[0.08]
 focus:ring-blue-400/20 focus:border-blue-400/30 focus:outline-none resize-none transition-all"
 ></textarea>

 {#if showDrivePicker}
 <div class="mt-2 max-h-40 overflow-y-auto rounded-xl border border-gray-800/60/60 bg-gray-950 divide-y divide-white/[0.06]">
 {#if driveFiles.length === 0}
 <p class="text-xs text-white/[0.08] p-4 text-center">No files in Drive</p>
 {:else}
 {#each driveFiles as file (file.id)}
 <button
 onclick={() => onAddDriveFile(file.id, file.name)}
 disabled={publishingDriveFile === file.id}
 class="flex items-center justify-between w-full px-3 py-2.5 text-left text-sm hover:bg-gray-950/50 transition-colors
 disabled:opacity-50 disabled:cursor-not-allowed focus:outline-none focus:bg-gray-950"
 >
 <div class="flex items-center gap-2.5 min-w-0">
 <FileText class="w-4 h-4 text-white/[0.06] flex-shrink-0" />
 <span class="truncate text-white/70">{file.name}</span>
 </div>
 <div class="flex items-center gap-2 flex-shrink-0 ml-2">
 <span class="text-xs text-white/[0.06] tabular-nums">{formatBytes(file.size)}</span>
 {#if publishingDriveFile === file.id}
 <Loader2 class="w-3.5 h-3.5 text-blue-400 animate-spin" />
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
 <span class="text-xs font-medium text-white/[0.06] uppercase tracking-wide flex items-center gap-1.5">
 <Calendar class="w-3.5 h-3.5" />
 Duration
 </span>
 <span class="text-sm font-semibold text-white tabular-nums">
 {proposalDurationDays} day{proposalDurationDays !== 1 ?'s' :''}
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
 class="w-full accent-blue-400 h-2 rounded-full"
 />
 <div class="flex justify-between text-[10px] text-white/[0.08] mt-1 tabular-nums">
 <span>1 day</span>
 <span>1 year</span>
 </div>
 </div>

 <!-- Cost summary -->
 <div class="p-4 rounded-xl bg-blue-500/[0.06]/50 border border-gray-800/60/60">
 <div class="flex justify-between text-sm">
 <span class="text-white/[0.06]">Required Deposit</span>
 <span class="font-semibold text-white tabular-nums">{formatWeiAsChi(proposalHost.advertisement.minDepositWei)}</span>
 </div>
 <p class="text-[11px] text-white/[0.08] mt-1.5">
 Total cost depends on file sizes and will be calculated after the host accepts.
 </p>
 </div>
 </div>

 <!-- Footer actions -->
 <div class="flex justify-end gap-3 p-5 pt-4 border-t border-gray-800/60/60 bg-gray-950">
 <button
 onclick={onClose}
 class="px-4 py-2.5 text-sm font-medium text-white/70 border border-gray-800/60/60 rounded-xl
 hover:bg-gray-950 transition-colors focus:outline-none focus:border-blue-400/40"
 >
 Cancel
 </button>
 <button
 onclick={onSendProposal}
 disabled={isProposing || hashCount === 0}
 class="flex items-center gap-2 px-5 py-2.5 text-sm font-semibold bg-blue-400 hover:bg-blue-500 text-white rounded-xl transition-all
 shadow-blue-400/10 hover:shadow-sm hover:shadow-blue-400/10 active:scale-[0.98]
 focus:outline-none focus:border-blue-400/40/50 
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
