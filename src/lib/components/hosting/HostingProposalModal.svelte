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
  class="fixed inset-0 bg-black/60 backdrop-blur-sm z-50 flex items-center justify-center p-4"
  onclick={(e) => { if (e.target === e.currentTarget) onClose(); }}
  role="dialog"
  aria-modal="true"
  aria-label="Propose hosting agreement"
>
  <div class="backdrop-blur-2xl bg-white/15 dark:bg-white/10 rounded-2xl shadow-2xl border border-white/20 dark:border-white/10 w-full max-w-lg overflow-hidden">
    <!-- Header -->
    <div class="flex items-center justify-between p-5 pb-4 border-b border-white/15 dark:border-white/10">
      <div>
        <h3 class="text-lg font-semibold text-gray-900 dark:text-white">Propose Hosting Agreement</h3>
        <p class="text-xs text-gray-500 dark:text-gray-400 mt-0.5 font-mono">
          {formatPeerId(proposalHost.advertisement.peerId)}
        </p>
      </div>
      <button
        onclick={onClose}
        class="p-1.5 text-gray-400 hover:text-gray-600 dark:hover:text-gray-300 rounded-lg hover:bg-white/10 dark:hover:bg-white/5 transition-colors
          focus:outline-none focus:ring-2 focus:ring-gray-400/30"
        aria-label="Close"
      >
        <X class="w-5 h-5" />
      </button>
    </div>

    <div class="p-5 space-y-5 max-h-[70vh] overflow-y-auto">
      <!-- Host summary -->
      <div class="grid grid-cols-3 gap-3">
        <div class="flex flex-col items-center gap-1.5 p-3 rounded-xl bg-white/8 dark:bg-white/5 border border-white/15 dark:border-white/10">
          <Coins class="w-4 h-4 text-gray-400" />
          <span class="text-xs font-semibold text-gray-900 dark:text-white tabular-nums">{formatWeiAsChi(proposalHost.advertisement.pricePerMbPerDayWei)}</span>
          <span class="text-[10px] text-gray-400 uppercase tracking-wide">per MB/day</span>
        </div>
        <div class="flex flex-col items-center gap-1.5 p-3 rounded-xl bg-white/8 dark:bg-white/5 border border-white/15 dark:border-white/10">
          <Shield class="w-4 h-4 text-gray-400" />
          <span class="text-xs font-semibold text-gray-900 dark:text-white tabular-nums">{formatWeiAsChi(proposalHost.advertisement.minDepositWei)}</span>
          <span class="text-[10px] text-gray-400 uppercase tracking-wide">deposit</span>
        </div>
        <div class="flex flex-col items-center gap-1.5 p-3 rounded-xl bg-white/8 dark:bg-white/5 border border-white/15 dark:border-white/10">
          <HardDrive class="w-4 h-4 text-gray-400" />
          <span class="text-xs font-semibold text-gray-900 dark:text-white tabular-nums">{formatBytes(proposalHost.availableStorageBytes)}</span>
          <span class="text-[10px] text-gray-400 uppercase tracking-wide">available</span>
        </div>
      </div>

      <!-- File hashes -->
      <div>
        <div class="flex items-center justify-between mb-2">
          <label for="proposal-file-hashes" class="text-xs font-medium text-gray-600 dark:text-gray-400 uppercase tracking-wide">
            File Hashes
            {#if hashCount > 0}
              <span class="ml-1 text-gray-400 normal-case tracking-normal">({hashCount})</span>
            {/if}
          </label>
          <button
            onclick={onLoadDriveFiles}
            class="flex items-center gap-1 text-xs font-medium text-primary-600 dark:text-primary-400 hover:text-primary-700 dark:hover:text-primary-300 transition-colors
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
          class="w-full p-3 text-sm font-mono bg-white/8 dark:bg-white/5 border border-white/20 dark:border-white/10 rounded-xl text-gray-900 dark:text-white placeholder-gray-400
            focus:ring-2 focus:ring-primary-400/20 focus:border-primary-400 focus:outline-none resize-none transition-all"
        ></textarea>

        {#if showDrivePicker}
          <div class="mt-2 max-h-40 overflow-y-auto rounded-xl border border-white/20 dark:border-white/10 bg-white/8 dark:bg-white/5 divide-y divide-white/10 dark:divide-white/5">
            {#if driveFiles.length === 0}
              <p class="text-xs text-gray-400 dark:text-gray-500 p-4 text-center">No files in Drive</p>
            {:else}
              {#each driveFiles as file (file.id)}
                <button
                  onclick={() => onAddDriveFile(file.id, file.name)}
                  disabled={publishingDriveFile === file.id}
                  class="flex items-center justify-between w-full px-3 py-2.5 text-left text-sm hover:bg-white/10 dark:hover:bg-white/5 transition-colors
                    disabled:opacity-50 disabled:cursor-not-allowed focus:outline-none focus:bg-gray-100 dark:focus:bg-gray-600/50"
                >
                  <div class="flex items-center gap-2.5 min-w-0">
                    <FileText class="w-4 h-4 text-gray-400 flex-shrink-0" />
                    <span class="truncate text-gray-700 dark:text-gray-300">{file.name}</span>
                  </div>
                  <div class="flex items-center gap-2 flex-shrink-0 ml-2">
                    <span class="text-xs text-gray-400 tabular-nums">{formatBytes(file.size)}</span>
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
          <span class="text-xs font-medium text-gray-600 dark:text-gray-400 uppercase tracking-wide flex items-center gap-1.5">
            <Calendar class="w-3.5 h-3.5" />
            Duration
          </span>
          <span class="text-sm font-semibold text-gray-900 dark:text-white tabular-nums">
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
        <div class="flex justify-between text-[10px] text-gray-400 dark:text-gray-500 mt-1 tabular-nums">
          <span>1 day</span>
          <span>1 year</span>
        </div>
      </div>

      <!-- Cost summary -->
      <div class="p-4 rounded-xl bg-primary-50/50 dark:bg-primary-900/10 border border-primary-100 dark:border-primary-900/30">
        <div class="flex justify-between text-sm">
          <span class="text-gray-600 dark:text-gray-400">Required Deposit</span>
          <span class="font-semibold text-gray-900 dark:text-white tabular-nums">{formatWeiAsChi(proposalHost.advertisement.minDepositWei)}</span>
        </div>
        <p class="text-[11px] text-gray-400 dark:text-gray-500 mt-1.5">
          Total cost depends on file sizes and will be calculated after the host accepts.
        </p>
      </div>
    </div>

    <!-- Footer actions -->
    <div class="flex justify-end gap-3 p-5 pt-4 border-t border-white/15 dark:border-white/10 bg-white/8 dark:bg-white/5">
      <button
        onclick={onClose}
        class="px-4 py-2.5 text-sm font-medium text-gray-700 dark:text-gray-300 border border-white/20 dark:border-white/10 rounded-xl
          hover:bg-white/10 dark:hover:bg-white/5 transition-colors focus:outline-none focus:ring-2 focus:ring-gray-400/30"
      >
        Cancel
      </button>
      <button
        onclick={onSendProposal}
        disabled={isProposing || hashCount === 0}
        class="flex items-center gap-2 px-5 py-2.5 text-sm font-semibold backdrop-blur-md bg-primary-500/80 dark:bg-primary-600/70 border border-primary-400/30 hover:bg-primary-500/90 dark:hover:bg-primary-600/80 text-white rounded-xl transition-all
          shadow-sm shadow-primary-500/20 hover:shadow-md hover:shadow-primary-500/25 active:scale-[0.98]
          focus:outline-none focus:ring-2 focus:ring-primary-500/50 focus:ring-offset-2 dark:focus:ring-offset-gray-900
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
