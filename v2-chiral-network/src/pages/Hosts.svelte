<script lang="ts">
  import { onMount } from 'svelte';
  import {
    Users, Star, HardDrive, Clock, Coins, Shield,
    Check, X, Loader2, RefreshCw, FileText,
    ChevronDown, ChevronUp, Rocket, AlertCircle, Send
  } from 'lucide-svelte';
  import { walletAccount, peers, networkConnected } from '$lib/stores';
  import { get } from 'svelte/store';
  import { toasts } from '$lib/toastStore';
  import { hostingService } from '$lib/services/hostingService';
  import { invoke } from '@tauri-apps/api/core';
  import type {
    HostEntry,
    HostingAgreement,
  } from '$lib/types/hosting';

  // ── State ──
  let loading = $state(true);
  let error = $state<string | null>(null);
  let hosts = $state<HostEntry[]>([]);
  let myAgreements = $state<HostingAgreement[]>([]);
  let sortBy = $state<'reputation' | 'price' | 'storage'>('reputation');
  let showAgreements = $state(true);
  let myPeerId = $state<string | null>(null);

  // Proposal modal
  let proposalHost = $state<HostEntry | null>(null);
  let proposalFileHashes = $state('');
  let proposalDurationDays = $state(7);
  let isProposing = $state(false);

  // ── Helpers ──
  function formatPeerId(id: string): string {
    if (id.length <= 16) return id;
    return `${id.slice(0, 8)}...${id.slice(-6)}`;
  }

  function formatBytes(bytes: number): string {
    if (bytes === 0) return '0 B';
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    const i = Math.floor(Math.log(bytes) / Math.log(1024));
    return `${(bytes / Math.pow(1024, i)).toFixed(1)} ${units[i]}`;
  }

  function formatWeiAsChi(wei: string): string {
    try {
      const value = Number(BigInt(wei)) / 1e18;
      if (value === 0) return '0 CHI';
      if (value < 0.000001) return '< 0.000001 CHI';
      return `${parseFloat(value.toFixed(6))} CHI`;
    } catch {
      return '0 CHI';
    }
  }

  function formatDuration(secs: number): string {
    const days = Math.floor(secs / 86400);
    if (days >= 365) return `${(days / 365).toFixed(1)} years`;
    if (days >= 30) return `${(days / 30).toFixed(1)} months`;
    return `${days} day${days !== 1 ? 's' : ''}`;
  }

  function timeRemaining(expiresAt: number | undefined): string {
    if (!expiresAt) return 'N/A';
    const remaining = expiresAt - Math.floor(Date.now() / 1000);
    if (remaining <= 0) return 'Expired';
    return formatDuration(remaining);
  }

  function statusColor(status: string): string {
    switch (status) {
      case 'proposed': return 'bg-blue-100 text-blue-700 dark:bg-blue-900/30 dark:text-blue-400';
      case 'accepted': return 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400';
      case 'active': return 'bg-emerald-100 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-400';
      case 'rejected': return 'bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400';
      case 'expired': return 'bg-gray-100 text-gray-600 dark:bg-gray-700 dark:text-gray-400';
      case 'cancelled': return 'bg-orange-100 text-orange-700 dark:bg-orange-900/30 dark:text-orange-400';
      default: return 'bg-gray-100 text-gray-600 dark:bg-gray-700 dark:text-gray-400';
    }
  }

  function reputationStars(score: number): number {
    return Math.round(score * 5 * 10) / 10; // 0-5 scale, 1 decimal
  }

  // ── Sorting ──
  function sortedHosts(entries: HostEntry[]): HostEntry[] {
    return [...entries].sort((a, b) => {
      switch (sortBy) {
        case 'price':
          return Number(BigInt(a.advertisement.pricePerMbPerDayWei) - BigInt(b.advertisement.pricePerMbPerDayWei));
        case 'storage':
          return b.availableStorageBytes - a.availableStorageBytes;
        case 'reputation':
        default:
          return b.reputationScore - a.reputationScore;
      }
    });
  }

  // ── Data Loading ──
  async function loadData() {
    loading = true;
    error = null;

    try {
      const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

      if (isTauri) {
        const pid = await invoke<string | null>('get_peer_id');
        myPeerId = pid;
      }

      const [hostResults, agreementResults] = await Promise.all([
        hostingService.discoverHosts().catch(() => [] as HostEntry[]),
        hostingService.getMyAgreements().catch(() => [] as HostingAgreement[]),
      ]);

      hosts = hostResults;
      myAgreements = agreementResults;
    } catch (err: any) {
      error = `Failed to load hosting data: ${err.message || err}`;
    } finally {
      loading = false;
    }
  }

  async function refreshHosts() {
    try {
      hosts = await hostingService.discoverHosts();
      toasts.show('Host list refreshed', 'success');
    } catch (err: any) {
      toasts.show(`Failed to refresh: ${err.message || err}`, 'error');
    }
  }

  // ── Agreement Actions ──
  async function openProposalModal(host: HostEntry) {
    const wallet = get(walletAccount);
    if (!wallet?.address) {
      toasts.show('Connect your wallet first', 'error');
      return;
    }
    proposalHost = host;
    proposalFileHashes = '';
    proposalDurationDays = 7;
  }

  async function sendProposal() {
    if (!proposalHost || isProposing) return;
    const wallet = get(walletAccount);
    if (!wallet?.address || !myPeerId) {
      toasts.show('Wallet or peer ID not available', 'error');
      return;
    }

    const hashes = proposalFileHashes
      .split('\n')
      .map((h) => h.trim())
      .filter(Boolean);

    if (hashes.length === 0) {
      toasts.show('Enter at least one file hash', 'error');
      return;
    }

    isProposing = true;
    try {
      const agreement = await hostingService.proposeAgreement(
        myPeerId,
        wallet.address,
        proposalHost.advertisement.peerId,
        proposalHost.advertisement.walletAddress,
        hashes,
        0, // total size — will be resolved when host accepts
        proposalDurationDays * 86400,
        proposalHost.advertisement.pricePerMbPerDayWei,
        proposalHost.advertisement.minDepositWei,
      );

      myAgreements = [...myAgreements, agreement];
      proposalHost = null;
      toasts.show('Hosting proposal sent!', 'success');
    } catch (err: any) {
      toasts.show(`Failed to send proposal: ${err.message || err}`, 'error');
    } finally {
      isProposing = false;
    }
  }

  async function respondToAgreement(agreementId: string, accept: boolean) {
    try {
      await hostingService.respondToAgreement(agreementId, accept);
      myAgreements = myAgreements.map((a) =>
        a.agreementId === agreementId
          ? { ...a, status: accept ? 'accepted' : 'rejected', respondedAt: Math.floor(Date.now() / 1000) }
          : a
      );
      toasts.show(accept ? 'Agreement accepted' : 'Agreement rejected', accept ? 'success' : 'info');
    } catch (err: any) {
      toasts.show(`Failed: ${err.message || err}`, 'error');
    }
  }

  async function cancelAgreement(agreementId: string) {
    try {
      await hostingService.cancelAgreement(agreementId);
      myAgreements = myAgreements.map((a) =>
        a.agreementId === agreementId ? { ...a, status: 'cancelled' } : a
      );
      toasts.show('Agreement cancelled', 'info');
    } catch (err: any) {
      toasts.show(`Failed to cancel: ${err.message || err}`, 'error');
    }
  }

  // ── Lifecycle ──
  let pollInterval: ReturnType<typeof setInterval> | null = null;

  onMount(() => {
    loadData();
    // Poll for incoming proposals every 30 seconds
    pollInterval = setInterval(async () => {
      try {
        const updated = await hostingService.getMyAgreements();
        myAgreements = updated;
      } catch { /* ignore poll errors */ }
    }, 30_000);

    return () => {
      if (pollInterval) clearInterval(pollInterval);
    };
  });

  $effect(() => {
    // Reload when network connection changes
    if ($networkConnected) {
      loadData();
    }
  });

  // Computed
  let sortedHostList = $derived(sortedHosts(hosts));
  let proposalCostWei = $derived.by(() => {
    if (!proposalHost) return '0';
    return hostingService.calculateTotalCostWei(
      0, // size unknown until files are resolved
      proposalDurationDays * 86400,
      proposalHost.advertisement.pricePerMbPerDayWei,
    );
  });

  // Split agreements into incoming (we're host) and outgoing (we're client)
  let incomingProposals = $derived(
    myAgreements.filter((a) => a.hostPeerId === myPeerId && a.status === 'proposed')
  );
  let activeAgreements = $derived(
    myAgreements.filter((a) => a.status !== 'proposed' || a.clientPeerId === myPeerId)
  );
</script>

<svelte:head>
  <title>Hosts - Chiral Network</title>
</svelte:head>

<div class="max-w-5xl mx-auto py-6 px-4 sm:px-6">
  <!-- Header -->
  <div class="mb-8">
    <h1 class="text-2xl font-bold dark:text-white">Hosts</h1>
    <p class="text-sm text-gray-500 dark:text-gray-400 mt-1">
      Find hosts to store your files in exchange for CHI tokens
    </p>
  </div>

  {#if loading}
    <div class="flex items-center justify-center py-20">
      <Loader2 class="w-8 h-8 text-gray-400 animate-spin" />
    </div>
  {:else if error}
    <div class="text-center py-20">
      <AlertCircle class="w-12 h-12 mx-auto text-gray-300 dark:text-gray-600 mb-3" />
      <p class="text-gray-500 dark:text-gray-400">{error}</p>
    </div>
  {:else}
    <!-- ──────────── Incoming Proposals ──────────── -->
    {#if incomingProposals.length > 0}
      <div class="bg-white dark:bg-gray-800 rounded-xl border border-blue-200 dark:border-blue-800 p-6 mb-6">
        <div class="flex items-center gap-3 mb-4">
          <div class="p-2 bg-blue-100 dark:bg-blue-900 rounded-lg">
            <Send class="w-5 h-5 text-blue-600 dark:text-blue-400" />
          </div>
          <div>
            <h2 class="font-semibold text-lg dark:text-white">Incoming Proposals</h2>
            <p class="text-sm text-gray-500 dark:text-gray-400">
              {incomingProposals.length} pending request{incomingProposals.length !== 1 ? 's' : ''} to host files
            </p>
          </div>
        </div>

        <div class="space-y-3">
          {#each incomingProposals as proposal (proposal.agreementId)}
            <div class="flex items-center justify-between p-4 rounded-lg bg-blue-50 dark:bg-blue-900/20 border border-blue-100 dark:border-blue-800">
              <div class="min-w-0">
                <div class="flex items-center gap-2 flex-wrap">
                  <span class="text-sm font-medium text-gray-900 dark:text-white font-mono">
                    From: {formatPeerId(proposal.clientPeerId)}
                  </span>
                  <span class="text-xs px-2 py-0.5 rounded-full bg-blue-100 text-blue-700 dark:bg-blue-900/50 dark:text-blue-300">
                    {proposal.fileHashes.length} file{proposal.fileHashes.length !== 1 ? 's' : ''}
                  </span>
                </div>
                <div class="flex items-center gap-3 text-xs text-gray-500 dark:text-gray-400 mt-1">
                  <span>{formatDuration(proposal.durationSecs)}</span>
                  <span>Deposit: {formatWeiAsChi(proposal.depositWei)}</span>
                </div>
              </div>
              <div class="flex items-center gap-2 flex-shrink-0">
                <button
                  onclick={() => respondToAgreement(proposal.agreementId, true)}
                  class="flex items-center gap-1.5 px-3 py-1.5 text-sm bg-green-600 hover:bg-green-700 text-white rounded-lg transition-colors"
                >
                  <Check class="w-3.5 h-3.5" />
                  Accept
                </button>
                <button
                  onclick={() => respondToAgreement(proposal.agreementId, false)}
                  class="flex items-center gap-1.5 px-3 py-1.5 text-sm bg-red-100 hover:bg-red-200 text-red-700 dark:bg-red-900/30 dark:hover:bg-red-900/50 dark:text-red-400 rounded-lg transition-colors"
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

    <!-- ──────────── My Agreements ──────────── -->
    <div class="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 p-6 mb-6">
      <button
        onclick={() => showAgreements = !showAgreements}
        class="flex items-center justify-between w-full"
      >
        <div class="flex items-center gap-3">
          <div class="p-2 bg-emerald-100 dark:bg-emerald-900 rounded-lg">
            <Shield class="w-5 h-5 text-emerald-600 dark:text-emerald-400" />
          </div>
          <div class="text-left">
            <h2 class="font-semibold text-lg dark:text-white">My Agreements</h2>
            <p class="text-sm text-gray-500 dark:text-gray-400">
              {activeAgreements.length} agreement{activeAgreements.length !== 1 ? 's' : ''}
            </p>
          </div>
        </div>
        {#if showAgreements}
          <ChevronUp class="w-5 h-5 text-gray-400" />
        {:else}
          <ChevronDown class="w-5 h-5 text-gray-400" />
        {/if}
      </button>

      {#if showAgreements}
        <div class="mt-4">
          {#if activeAgreements.length === 0}
            <div class="text-center py-8">
              <Shield class="w-10 h-10 mx-auto text-gray-300 dark:text-gray-600 mb-2" />
              <p class="text-sm text-gray-500 dark:text-gray-400">No agreements yet</p>
              <p class="text-xs text-gray-400 dark:text-gray-500 mt-1">
                Propose an agreement with a host below to get started
              </p>
            </div>
          {:else}
            <div class="space-y-3">
              {#each activeAgreements as agreement (agreement.agreementId)}
                {@const isClient = agreement.clientPeerId === myPeerId}
                <div class="flex items-center justify-between p-4 rounded-lg bg-gray-50 dark:bg-gray-700/50 border border-gray-100 dark:border-gray-600">
                  <div class="min-w-0">
                    <div class="flex items-center gap-2 flex-wrap">
                      <span class="text-sm font-medium text-gray-900 dark:text-white font-mono">
                        {isClient ? 'Host' : 'Client'}: {formatPeerId(isClient ? agreement.hostPeerId : agreement.clientPeerId)}
                      </span>
                      <span class="text-xs px-2 py-0.5 rounded-full {statusColor(agreement.status)}">
                        {agreement.status.charAt(0).toUpperCase() + agreement.status.slice(1)}
                      </span>
                    </div>
                    <div class="flex items-center gap-3 text-xs text-gray-500 dark:text-gray-400 mt-1">
                      <span class="flex items-center gap-1">
                        <FileText class="w-3 h-3" />
                        {agreement.fileHashes.length} file{agreement.fileHashes.length !== 1 ? 's' : ''}
                      </span>
                      <span class="flex items-center gap-1">
                        <Clock class="w-3 h-3" />
                        {#if agreement.status === 'active'}
                          {timeRemaining(agreement.expiresAt)} remaining
                        {:else}
                          {formatDuration(agreement.durationSecs)}
                        {/if}
                      </span>
                      <span class="flex items-center gap-1">
                        <Coins class="w-3 h-3" />
                        {formatWeiAsChi(agreement.totalCostWei)}
                      </span>
                    </div>
                  </div>
                  <div class="flex items-center gap-2 flex-shrink-0">
                    {#if agreement.status === 'proposed' && isClient}
                      <button
                        onclick={() => cancelAgreement(agreement.agreementId)}
                        class="text-xs px-3 py-1.5 text-red-600 dark:text-red-400 border border-red-200 dark:border-red-800 rounded-lg hover:bg-red-50 dark:hover:bg-red-900/30 transition-colors"
                      >
                        Cancel
                      </button>
                    {/if}
                  </div>
                </div>
              {/each}
            </div>
          {/if}
        </div>
      {/if}
    </div>

    <!-- ──────────── Available Hosts ──────────── -->
    <div class="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 p-6">
      <div class="flex items-center justify-between mb-4">
        <div class="flex items-center gap-3">
          <div class="p-2 bg-purple-100 dark:bg-purple-900 rounded-lg">
            <Users class="w-5 h-5 text-purple-600 dark:text-purple-400" />
          </div>
          <div>
            <h2 class="font-semibold text-lg dark:text-white">Available Hosts</h2>
            <p class="text-sm text-gray-500 dark:text-gray-400">
              {hosts.length} host{hosts.length !== 1 ? 's' : ''} on the network
            </p>
          </div>
        </div>

        <div class="flex items-center gap-2">
          <!-- Sort selector -->
          <select
            bind:value={sortBy}
            class="text-sm bg-gray-100 dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded-lg px-3 py-1.5 text-gray-700 dark:text-gray-300"
          >
            <option value="reputation">Reputation</option>
            <option value="price">Price (low)</option>
            <option value="storage">Storage (high)</option>
          </select>
          <button
            onclick={refreshHosts}
            class="p-2 text-gray-500 hover:text-gray-700 dark:text-gray-400 dark:hover:text-gray-200 rounded-lg hover:bg-gray-100 dark:hover:bg-gray-700 transition-colors"
            title="Refresh host list"
          >
            <RefreshCw class="w-4 h-4" />
          </button>
        </div>
      </div>

      {#if sortedHostList.length === 0}
        <div class="text-center py-12">
          <Users class="w-12 h-12 mx-auto text-gray-300 dark:text-gray-600 mb-3" />
          <p class="text-gray-500 dark:text-gray-400">No hosts available</p>
          <p class="text-sm text-gray-400 dark:text-gray-500 mt-1">
            When peers offer hosting services, they will appear here
          </p>
        </div>
      {:else}
        <div class="space-y-3">
          {#each sortedHostList as host (host.advertisement.peerId)}
            {@const stars = reputationStars(host.reputationScore)}
            <div class="p-4 rounded-xl border border-gray-100 dark:border-gray-600 bg-gray-50 dark:bg-gray-700/50 hover:border-gray-200 dark:hover:border-gray-500 transition-colors">
              <div class="flex items-start justify-between gap-4">
                <div class="min-w-0 flex-1">
                  <!-- Peer ID + online indicator -->
                  <div class="flex items-center gap-2">
                    <span class="inline-block w-2 h-2 rounded-full flex-shrink-0 {host.isOnline ? 'bg-green-500' : 'bg-gray-400'}"></span>
                    <span class="text-sm font-medium text-gray-900 dark:text-white font-mono">
                      {formatPeerId(host.advertisement.peerId)}
                    </span>
                    <!-- Reputation -->
                    <div class="flex items-center gap-0.5 ml-1">
                      {#each [1, 2, 3, 4, 5] as s}
                        <Star class="w-3.5 h-3.5 {stars >= s ? 'text-yellow-400 fill-yellow-400' : stars >= s - 0.5 ? 'text-yellow-400 fill-yellow-400/50' : 'text-gray-300 dark:text-gray-600'}" />
                      {/each}
                      <span class="text-xs text-gray-500 dark:text-gray-400 ml-1">{stars.toFixed(1)}</span>
                    </div>
                  </div>

                  <!-- Metrics row -->
                  <div class="flex items-center gap-4 mt-2 flex-wrap">
                    <span class="flex items-center gap-1.5 text-xs text-gray-500 dark:text-gray-400">
                      <HardDrive class="w-3.5 h-3.5" />
                      {formatBytes(host.availableStorageBytes)} available
                    </span>
                    <span class="flex items-center gap-1.5 text-xs text-gray-500 dark:text-gray-400">
                      <Coins class="w-3.5 h-3.5" />
                      {formatWeiAsChi(host.advertisement.pricePerMbPerDayWei)}/MB/day
                    </span>
                    <span class="flex items-center gap-1.5 text-xs text-gray-500 dark:text-gray-400">
                      <Shield class="w-3.5 h-3.5" />
                      Min deposit: {formatWeiAsChi(host.advertisement.minDepositWei)}
                    </span>
                    <span class="flex items-center gap-1.5 text-xs text-gray-500 dark:text-gray-400">
                      <Clock class="w-3.5 h-3.5" />
                      {host.advertisement.uptimePercent.toFixed(0)}% uptime
                    </span>
                  </div>
                </div>

                <!-- Action -->
                <button
                  onclick={() => openProposalModal(host)}
                  class="flex items-center gap-1.5 px-4 py-2 text-sm bg-primary-600 hover:bg-primary-700 text-white rounded-lg transition-colors flex-shrink-0"
                >
                  <Rocket class="w-3.5 h-3.5" />
                  Propose
                </button>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>

<!-- ──────────── Proposal Modal ──────────── -->
{#if proposalHost}
  <div
    class="fixed inset-0 bg-black/50 backdrop-blur-sm z-50 flex items-center justify-center p-4"
    onclick={(e) => { if (e.target === e.currentTarget) proposalHost = null; }}
    role="dialog"
    aria-modal="true"
  >
    <div class="bg-white dark:bg-gray-800 rounded-2xl shadow-xl border border-gray-200 dark:border-gray-700 w-full max-w-lg p-6">
      <h3 class="text-lg font-semibold dark:text-white mb-1">Propose Hosting Agreement</h3>
      <p class="text-sm text-gray-500 dark:text-gray-400 mb-5">
        Host: <span class="font-mono">{formatPeerId(proposalHost.advertisement.peerId)}</span>
      </p>

      <!-- Host summary -->
      <div class="flex items-center gap-4 p-3 rounded-lg bg-gray-50 dark:bg-gray-700/50 mb-5 text-xs text-gray-500 dark:text-gray-400">
        <span class="flex items-center gap-1">
          <Coins class="w-3.5 h-3.5" />
          {formatWeiAsChi(proposalHost.advertisement.pricePerMbPerDayWei)}/MB/day
        </span>
        <span class="flex items-center gap-1">
          <Shield class="w-3.5 h-3.5" />
          Deposit: {formatWeiAsChi(proposalHost.advertisement.minDepositWei)}
        </span>
        <span class="flex items-center gap-1">
          <HardDrive class="w-3.5 h-3.5" />
          {formatBytes(proposalHost.availableStorageBytes)}
        </span>
      </div>

      <!-- File hashes -->
      <label class="block text-sm font-medium text-gray-700 dark:text-gray-300 mb-1.5">
        File Hashes (one per line)
      </label>
      <textarea
        bind:value={proposalFileHashes}
        rows="4"
        placeholder="Enter file hashes to host, one per line..."
        class="w-full p-3 text-sm font-mono bg-gray-50 dark:bg-gray-700 border border-gray-200 dark:border-gray-600 rounded-lg text-gray-900 dark:text-white placeholder-gray-400 focus:ring-2 focus:ring-primary-500 focus:border-transparent resize-none"
      ></textarea>

      <!-- Duration -->
      <label class="block text-sm font-medium text-gray-700 dark:text-gray-300 mt-4 mb-1.5">
        Duration: {proposalDurationDays} day{proposalDurationDays !== 1 ? 's' : ''}
      </label>
      <input
        type="range"
        bind:value={proposalDurationDays}
        min="1"
        max="365"
        step="1"
        class="w-full accent-primary-600"
      />
      <div class="flex justify-between text-xs text-gray-400 dark:text-gray-500 mt-0.5">
        <span>1 day</span>
        <span>365 days</span>
      </div>

      <!-- Cost summary -->
      <div class="mt-4 p-3 rounded-lg bg-gray-50 dark:bg-gray-700/50 space-y-1.5">
        <div class="flex justify-between text-sm">
          <span class="text-gray-500 dark:text-gray-400">Deposit</span>
          <span class="font-medium dark:text-white">{formatWeiAsChi(proposalHost.advertisement.minDepositWei)}</span>
        </div>
        <p class="text-xs text-gray-400 dark:text-gray-500">
          Total cost depends on file sizes (calculated after host accepts)
        </p>
      </div>

      <!-- Actions -->
      <div class="flex justify-end gap-3 mt-5">
        <button
          onclick={() => proposalHost = null}
          class="px-4 py-2 text-sm text-gray-700 dark:text-gray-300 border border-gray-200 dark:border-gray-600 rounded-lg hover:bg-gray-50 dark:hover:bg-gray-700 transition-colors"
        >
          Cancel
        </button>
        <button
          onclick={sendProposal}
          disabled={isProposing || !proposalFileHashes.trim()}
          class="flex items-center gap-2 px-4 py-2 text-sm bg-primary-600 hover:bg-primary-700 text-white rounded-lg transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
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
{/if}
