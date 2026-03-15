<script lang="ts">
  import {
    Shield, Send, HardDrive, FileText, Clock, Coins,
    Check, X, Loader2, ChevronDown, ChevronUp, AlertCircle
  } from 'lucide-svelte';
  import {
    formatPeerId,
    formatWeiAsChi,
    formatDuration,
    timeRemaining,
    statusColor,
  } from '$lib/utils/hostingPageUtils';
  import type { HostingAgreement } from '$lib/types/hosting';

  interface HostedFile {
    fileHash: string;
    agreementId: string;
    clientPeerId: string;
    expiresAt?: number;
  }

  interface Props {
    myAgreements: HostingAgreement[];
    loadingAgreements: boolean;
    myPeerId: string | null;
    incomingProposals: HostingAgreement[];
    activeAgreements: HostingAgreement[];
    hostedFiles: HostedFile[];
    onRespondToAgreement: (agreementId: string, accept: boolean) => void;
    onRequestCancellation: (agreementId: string) => void;
    onRespondToCancellation: (agreementId: string, approve: boolean) => void;
  }

  let {
    myAgreements,
    loadingAgreements,
    myPeerId,
    incomingProposals,
    activeAgreements,
    hostedFiles,
    onRespondToAgreement,
    onRequestCancellation,
    onRespondToCancellation,
  }: Props = $props();

  let showAgreements = $state(true);
</script>

<!-- Incoming Proposals -->
{#if !loadingAgreements && incomingProposals.length > 0}
  <div class="rounded-2xl border-2 border-indigo-200 dark:border-indigo-800/60 bg-indigo-50/50 dark:bg-indigo-900/10 p-5 shadow-sm">
    <div class="flex items-center gap-3 mb-4">
      <div class="relative flex h-9 w-9 items-center justify-center rounded-xl bg-indigo-100 dark:bg-indigo-900/40">
        <Send class="w-4.5 h-4.5 text-indigo-600 dark:text-indigo-400" />
        <span class="absolute -top-1 -right-1 flex h-4 w-4 items-center justify-center rounded-full bg-indigo-500 text-[9px] font-bold text-white">
          {incomingProposals.length}
        </span>
      </div>
      <div>
        <h2 class="font-semibold text-base text-gray-900 dark:text-white">Incoming Proposals</h2>
        <p class="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
          {incomingProposals.length} pending request{incomingProposals.length !== 1 ? 's' : ''} to host files
        </p>
      </div>
    </div>

    <div class="space-y-2.5">
      {#each incomingProposals as proposal (proposal.agreementId)}
        <div class="flex items-center justify-between gap-3 p-4 rounded-xl bg-white/80 dark:bg-gray-950/60 border border-indigo-100 dark:border-indigo-800/40">
          <div class="min-w-0">
            <div class="flex items-center gap-2 flex-wrap">
              <span class="text-sm font-semibold text-gray-900 dark:text-white font-mono">
                {formatPeerId(proposal.clientPeerId)}
              </span>
              <span class="text-xs px-2 py-0.5 rounded-full bg-indigo-100 text-indigo-700 dark:bg-indigo-900/50 dark:text-indigo-300 font-medium">
                {proposal.fileHashes.length} file{proposal.fileHashes.length !== 1 ? 's' : ''}
              </span>
            </div>
            <div class="flex items-center gap-3 text-xs text-gray-500 dark:text-gray-400 mt-1.5">
              <span class="flex items-center gap-1">
                <Clock class="w-3 h-3" />
                {formatDuration(proposal.durationSecs)}
              </span>
              <span class="flex items-center gap-1">
                <Coins class="w-3 h-3" />
                Deposit: {formatWeiAsChi(proposal.depositWei)}
              </span>
            </div>
          </div>
          <div class="flex items-center gap-2 flex-shrink-0">
            <button
              onclick={() => onRespondToAgreement(proposal.agreementId, true)}
              class="flex items-center gap-1.5 px-3.5 py-2 text-sm font-medium bg-green-600 hover:bg-green-700 text-white rounded-lg transition-colors
                focus:outline-none focus:ring-2 focus:ring-green-500/50 focus:ring-offset-2 dark:focus:ring-offset-gray-900"
            >
              <Check class="w-3.5 h-3.5" />
              Accept
            </button>
            <button
              onclick={() => onRespondToAgreement(proposal.agreementId, false)}
              class="flex items-center gap-1.5 px-3.5 py-2 text-sm font-medium bg-red-50 hover:bg-red-100 text-red-700 dark:bg-red-900/20 dark:hover:bg-red-900/40 dark:text-red-400 rounded-lg transition-colors
                focus:outline-none focus:ring-2 focus:ring-red-400/30"
            >
              <X class="w-3.5 h-3.5" />
              Reject
            </button>
          </div>
        </div>
      {/each}
    </div>
  </div>
{/if}

<!-- My Agreements -->
<div class="rounded-2xl border border-gray-200/70 bg-white/90 shadow-sm backdrop-blur dark:border-gray-700/60 dark:bg-gray-950/85 overflow-hidden">
  <button
    onclick={() => showAgreements = !showAgreements}
    class="flex items-center justify-between w-full p-5 hover:bg-gray-50/50 dark:hover:bg-gray-700/20 transition-colors
      focus:outline-none focus:ring-2 focus:ring-inset focus:ring-primary-400/30"
    aria-expanded={showAgreements}
  >
    <div class="flex items-center gap-3">
      <div class="flex h-9 w-9 items-center justify-center rounded-xl bg-emerald-100 dark:bg-emerald-900/30">
        <Shield class="w-4.5 h-4.5 text-emerald-600 dark:text-emerald-400" />
      </div>
      <div class="text-left">
        <h2 class="font-semibold text-base text-gray-900 dark:text-white">My Agreements</h2>
        <p class="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
          {#if loadingAgreements}
            Loading...
          {:else}
            {activeAgreements.length} agreement{activeAgreements.length !== 1 ? 's' : ''}
          {/if}
        </p>
      </div>
    </div>
    <div class="flex items-center gap-2">
      {#if activeAgreements.filter(a => a.status === 'active').length > 0}
        <span class="rounded-full bg-emerald-100 px-2 py-0.5 text-[10px] font-semibold text-emerald-700 dark:bg-emerald-900/40 dark:text-emerald-300 uppercase tracking-wide">
          {activeAgreements.filter(a => a.status === 'active').length} active
        </span>
      {/if}
      {#if showAgreements}
        <ChevronUp class="w-5 h-5 text-gray-400" />
      {:else}
        <ChevronDown class="w-5 h-5 text-gray-400" />
      {/if}
    </div>
  </button>

  {#if showAgreements}
    <div class="border-t border-gray-100 dark:border-gray-700/60 p-5 pt-4">
      {#if loadingAgreements}
        <div class="flex flex-col items-center justify-center py-12">
          <Loader2 class="w-6 h-6 text-gray-400 animate-spin mb-3" />
          <span class="text-sm text-gray-400">Loading agreements...</span>
        </div>
      {:else if activeAgreements.length === 0}
        <div class="flex flex-col items-center justify-center py-12 text-gray-400 dark:text-gray-500">
          <div class="flex h-14 w-14 items-center justify-center rounded-2xl bg-gray-100 dark:bg-gray-900/40 mb-3">
            <Shield class="w-7 h-7 opacity-40" />
          </div>
          <p class="text-sm font-medium text-gray-500 dark:text-gray-400">No agreements yet</p>
          <p class="text-xs mt-1">Propose an agreement in the Marketplace tab</p>
        </div>
      {:else}
        <div class="space-y-2.5">
          {#each activeAgreements as agreement (agreement.agreementId)}
            {@const isClient = agreement.clientPeerId === myPeerId}
            {@const hasCancelRequest = !!agreement.cancelRequestedBy}
            <div class="p-4 rounded-xl border transition-all
              {hasCancelRequest && agreement.cancelRequestedBy !== myPeerId
                ? 'border-orange-200 bg-orange-50/50 dark:border-orange-800/40 dark:bg-orange-900/10'
                : 'border-gray-100 bg-gray-50/50 dark:border-gray-600/60 dark:bg-gray-900/30 hover:border-gray-200 dark:hover:border-gray-500/60'}">
              <div class="flex items-start justify-between gap-3">
                <div class="min-w-0">
                  <div class="flex items-center gap-2 flex-wrap">
                    <span class="text-xs font-medium text-gray-400 dark:text-gray-500 uppercase tracking-wide">
                      {isClient ? 'Host' : 'Client'}
                    </span>
                    <span class="text-sm font-semibold text-gray-900 dark:text-white font-mono">
                      {formatPeerId(isClient ? agreement.hostPeerId : agreement.clientPeerId)}
                    </span>
                    <span class="text-[11px] px-2 py-0.5 rounded-full font-semibold uppercase tracking-wide {statusColor(agreement.status)}">
                      {agreement.status}
                    </span>
                  </div>
                  <div class="flex items-center gap-3 text-xs text-gray-500 dark:text-gray-400 mt-2">
                    <span class="flex items-center gap-1">
                      <FileText class="w-3 h-3 text-gray-400" />
                      {agreement.fileHashes.length} file{agreement.fileHashes.length !== 1 ? 's' : ''}
                    </span>
                    <span class="flex items-center gap-1">
                      <Clock class="w-3 h-3 text-gray-400" />
                      {#if agreement.status === 'active'}
                        {timeRemaining(agreement.expiresAt)} left
                      {:else}
                        {formatDuration(agreement.durationSecs)}
                      {/if}
                    </span>
                    <span class="flex items-center gap-1">
                      <Coins class="w-3 h-3 text-gray-400" />
                      {formatWeiAsChi(agreement.totalCostWei)}
                    </span>
                  </div>
                </div>

                <div class="flex items-center gap-2 flex-shrink-0">
                  {#if agreement.cancelRequestedBy && agreement.cancelRequestedBy !== myPeerId}
                    <!-- Cancellation request from other party -->
                    <div class="flex items-center gap-1.5">
                      <span class="text-[11px] text-orange-600 dark:text-orange-400 font-medium mr-1 flex items-center gap-1">
                        <AlertCircle class="w-3 h-3" />
                        Cancel requested
                      </span>
                      <button
                        onclick={() => onRespondToCancellation(agreement.agreementId, true)}
                        class="flex items-center gap-1 text-xs font-medium px-3 py-1.5 bg-red-600 hover:bg-red-700 text-white rounded-lg transition-colors
                          focus:outline-none focus:ring-2 focus:ring-red-500/50"
                      >
                        <Check class="w-3 h-3" />
                        Approve
                      </button>
                      <button
                        onclick={() => onRespondToCancellation(agreement.agreementId, false)}
                        class="text-xs font-medium px-3 py-1.5 text-gray-600 dark:text-gray-400 border border-gray-200/60 dark:border-gray-800 rounded-lg
                          hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors focus:outline-none focus:ring-2 focus:ring-gray-400/30"
                      >
                        Deny
                      </button>
                    </div>
                  {:else if agreement.cancelRequestedBy === myPeerId}
                    <span class="text-xs text-orange-600 dark:text-orange-400 italic font-medium flex items-center gap-1">
                      <Loader2 class="w-3 h-3 animate-spin" />
                      Cancellation pending...
                    </span>
                  {:else if agreement.status === 'proposed' && isClient}
                    <button
                      onclick={() => onRequestCancellation(agreement.agreementId)}
                      class="text-xs font-medium px-3 py-1.5 text-red-600 dark:text-red-400 border border-red-200 dark:border-red-800/60 rounded-lg
                        hover:bg-red-50 dark:hover:bg-red-900/20 transition-colors focus:outline-none focus:ring-2 focus:ring-red-400/30"
                    >
                      Withdraw
                    </button>
                  {:else if agreement.status === 'accepted' || agreement.status === 'active'}
                    <button
                      onclick={() => onRequestCancellation(agreement.agreementId)}
                      class="text-xs font-medium px-3 py-1.5 text-red-600 dark:text-red-400 border border-red-200 dark:border-red-800/60 rounded-lg
                        hover:bg-red-50 dark:hover:bg-red-900/20 transition-colors focus:outline-none focus:ring-2 focus:ring-red-400/30"
                    >
                      Request Cancellation
                    </button>
                  {/if}
                </div>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>

<!-- Files I'm Hosting -->
{#if !loadingAgreements && hostedFiles.length > 0}
  <div class="rounded-2xl border border-gray-200/70 bg-white/90 p-5 shadow-sm backdrop-blur dark:border-gray-700/60 dark:bg-gray-950/85">
    <div class="flex items-center gap-3 mb-4">
      <div class="flex h-9 w-9 items-center justify-center rounded-xl bg-green-100 dark:bg-green-900/30">
        <HardDrive class="w-4.5 h-4.5 text-green-600 dark:text-green-400" />
      </div>
      <div>
        <h2 class="font-semibold text-base text-gray-900 dark:text-white">Files I'm Hosting</h2>
        <p class="text-xs text-gray-500 dark:text-gray-400 mt-0.5">
          {hostedFiles.length} file{hostedFiles.length !== 1 ? 's' : ''} being seeded for other peers
        </p>
      </div>
    </div>

    <div class="space-y-2 rounded-xl border border-gray-100 dark:border-gray-700/60 divide-y divide-gray-100 dark:divide-gray-700/40 overflow-hidden">
      {#each hostedFiles as file (file.fileHash + file.agreementId)}
        <div class="flex items-center justify-between p-3.5 hover:bg-gray-50/50 dark:hover:bg-gray-700/20 transition-colors">
          <div class="flex items-center gap-3 min-w-0">
            <div class="flex h-8 w-8 items-center justify-center rounded-lg bg-green-50 dark:bg-green-900/20 flex-shrink-0">
              <FileText class="w-4 h-4 text-green-500 dark:text-green-400" />
            </div>
            <div class="min-w-0">
              <p class="text-sm font-mono text-gray-700 dark:text-gray-300 truncate">
                {file.fileHash}
              </p>
              <p class="text-[11px] text-gray-500 dark:text-gray-400 mt-0.5">
                For {formatPeerId(file.clientPeerId)}
                {#if file.expiresAt}
                  <span class="text-gray-400 dark:text-gray-500 mx-1">|</span>
                  {timeRemaining(file.expiresAt)} remaining
                {/if}
              </p>
            </div>
          </div>
          <span class="flex items-center gap-1.5 px-2.5 py-1 text-[11px] font-semibold rounded-full bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400 flex-shrink-0 uppercase tracking-wide">
            <span class="w-1.5 h-1.5 rounded-full bg-green-500 animate-pulse"></span>
            Seeding
          </span>
        </div>
      {/each}
    </div>
  </div>
{/if}
