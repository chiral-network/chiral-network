<script lang="ts">
  import { onMount } from 'svelte';
  import {
    ShieldCheck,
    User,
    MessageSquare,
    Loader2,
    ChevronLeft,
    ChevronRight,
    CheckCircle2,
    XCircle,
    Star,
  } from 'lucide-svelte';
  import { walletAccount } from '$lib/stores';
  import { ratingApi, setRatingOwner, type ReputationEvent } from '$lib/services/ratingApiService';
  import { get } from 'svelte/store';

  let events = $state<ReputationEvent[]>([]);
  let elo = $state(50);
  let baseElo = $state(50);
  let completedCount = $state(0);
  let failedCount = $state(0);
  let ratingCount = $state(0);
  let totalEarnedWei = $state('0');
  let loading = $state(true);
  let error = $state<string | null>(null);

  const EVENTS_PER_PAGE = 6;
  let currentPage = $state(0);
  let totalPages = $derived(Math.max(1, Math.ceil(events.length / EVENTS_PER_PAGE)));
  let paginatedEvents = $derived(
    events.slice(currentPage * EVENTS_PER_PAGE, (currentPage + 1) * EVENTS_PER_PAGE),
  );

  function formatAddr(addr: string): string {
    if (addr.length <= 16) return addr;
    return `${addr.slice(0, 8)}...${addr.slice(-6)}`;
  }

  function formatDate(ts: number): string {
    return new Date(ts * 1000).toLocaleDateString(undefined, {
      year: 'numeric',
      month: 'short',
      day: 'numeric',
    });
  }

  function formatWeiAsChi(wei: string): string {
    try {
      const whole = BigInt(wei || '0');
      if (whole === 0n) return '0';
      const value = Number(whole) / 1e18;
      return Number.isFinite(value) ? value.toFixed(value >= 1 ? 2 : 6) : '0';
    } catch {
      return '0';
    }
  }

  async function loadReputation() {
    const wallet = get(walletAccount);
    if (!wallet?.address) {
      loading = false;
      error = 'Connect your wallet to view your reputation';
      return;
    }

    loading = true;
    error = null;

    try {
      setRatingOwner(wallet.address);
      const resp = await ratingApi.getReputation(wallet.address);
      events = [...resp.events].sort((a, b) => b.createdAt - a.createdAt);
      elo = resp.elo;
      baseElo = resp.baseElo;
      completedCount = resp.completedCount;
      failedCount = resp.failedCount;
      ratingCount = resp.ratingCount;
      totalEarnedWei = resp.totalEarnedWei;
      currentPage = 0;
    } catch (err: unknown) {
      const message = err instanceof Error
        ? err.message
        : (typeof err === 'string' ? err : 'Unknown error');
      error = `Failed to load reputation: ${message}`;
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    loadReputation();
  });

  $effect(() => {
    const wallet = $walletAccount;
    if (wallet) {
      loadReputation();
    }
  });
</script>

{#if loading}
  <div class="flex items-center justify-center py-12">
    <Loader2 class="w-8 h-8 text-gray-400 animate-spin" />
  </div>
{:else if error}
  <div class="text-center py-12">
    <ShieldCheck class="w-12 h-12 mx-auto text-gray-300 dark:text-gray-600 mb-3" />
    <p class="text-gray-500 dark:text-gray-400">{error}</p>
  </div>
{:else}
  <div class="bg-gray-50 dark:bg-gray-700/50 rounded-xl p-5 mb-5">
    <div class="flex flex-wrap items-center gap-6">
      <div class="flex flex-col items-center">
        <div class="text-4xl font-bold dark:text-white">{elo.toFixed(1)}</div>
        <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">Elo (base {baseElo})</p>
      </div>

      <div class="flex-1 grid grid-cols-2 gap-3 text-sm">
        <div class="rounded-lg bg-white dark:bg-gray-800 px-3 py-2 border border-gray-200 dark:border-gray-600">
          <p class="text-xs text-gray-500 dark:text-gray-400">Completed</p>
          <p class="font-semibold text-green-600 dark:text-green-400">{completedCount}</p>
        </div>
        <div class="rounded-lg bg-white dark:bg-gray-800 px-3 py-2 border border-gray-200 dark:border-gray-600">
          <p class="text-xs text-gray-500 dark:text-gray-400">Failed</p>
          <p class="font-semibold text-red-600 dark:text-red-400">{failedCount}</p>
        </div>
        <div class="rounded-lg bg-white dark:bg-gray-800 px-3 py-2 border border-gray-200 dark:border-gray-600">
          <p class="text-xs text-gray-500 dark:text-gray-400">Ratings</p>
          <p class="font-semibold text-gray-900 dark:text-white">{ratingCount}</p>
        </div>
        <div class="rounded-lg bg-white dark:bg-gray-800 px-3 py-2 border border-gray-200 dark:border-gray-600">
          <p class="text-xs text-gray-500 dark:text-gray-400">Earned (180d)</p>
          <p class="font-semibold text-gray-900 dark:text-white">{formatWeiAsChi(totalEarnedWei)} CHI</p>
        </div>
      </div>
    </div>
  </div>

  {#if events.length === 0}
    <div class="text-center py-12">
      <ShieldCheck class="w-12 h-12 mx-auto text-gray-300 dark:text-gray-600 mb-3" />
      <p class="text-gray-500 dark:text-gray-400">No reputation events yet</p>
      <p class="text-sm text-gray-400 dark:text-gray-500 mt-1">
        Complete downloads and ratings will contribute to your Elo
      </p>
    </div>
  {:else}
    <div class="rounded-xl border border-gray-200 dark:border-gray-600 divide-y divide-gray-100 dark:divide-gray-600">
      {#each paginatedEvents as event (event.id)}
        <div class="p-4">
          <div class="flex items-start justify-between gap-4">
            <div class="flex items-start gap-3 min-w-0">
              <div class="p-2 bg-gray-100 dark:bg-gray-700 rounded-full flex-shrink-0">
                <User class="w-4 h-4 text-gray-500 dark:text-gray-400" />
              </div>
              <div class="min-w-0">
                <div class="flex items-center gap-2 flex-wrap">
                  <span class="text-sm font-medium text-gray-900 dark:text-white font-mono">
                    {formatAddr(event.downloaderWallet)}
                  </span>
                  <span class="inline-flex items-center gap-1 text-xs px-2 py-0.5 rounded-full {event.outcome === 'completed' ? 'bg-green-100 text-green-700 dark:bg-green-900/30 dark:text-green-400' : 'bg-red-100 text-red-700 dark:bg-red-900/30 dark:text-red-400'}">
                    {#if event.outcome === 'completed'}
                      <CheckCircle2 class="w-3.5 h-3.5" />
                      Completed
                    {:else}
                      <XCircle class="w-3.5 h-3.5" />
                      Failed
                    {/if}
                  </span>
                  <span class="text-xs text-gray-500 dark:text-gray-400">
                    +{formatWeiAsChi(event.amountWei)} CHI
                  </span>
                </div>

                {#if event.ratingScore}
                  <div class="flex items-center gap-1 mt-1.5">
                    {#each [1, 2, 3, 4, 5] as star}
                      <Star
                        class="w-3.5 h-3.5 {event.ratingScore >= star ? 'text-yellow-400 fill-yellow-400' : 'text-gray-300 dark:text-gray-600'}"
                      />
                    {/each}
                  </div>
                {/if}

                {#if event.ratingComment}
                  <div class="flex items-start gap-1.5 mt-1.5">
                    <MessageSquare class="w-3.5 h-3.5 text-gray-400 mt-0.5 flex-shrink-0" />
                    <p class="text-sm text-gray-600 dark:text-gray-300">{event.ratingComment}</p>
                  </div>
                {/if}

                <p class="text-xs text-gray-400 dark:text-gray-500 mt-1.5 font-mono">
                  File: {formatAddr(event.fileHash)}
                </p>
              </div>
            </div>
            <span class="text-xs text-gray-400 dark:text-gray-500 whitespace-nowrap flex-shrink-0">
              {formatDate(event.createdAt)}
            </span>
          </div>
        </div>
      {/each}
    </div>

    {#if totalPages > 1}
      <div class="flex items-center justify-between mt-4">
        <button
          onclick={() => currentPage = Math.max(0, currentPage - 1)}
          disabled={currentPage === 0}
          class="flex items-center gap-1 px-3 py-1.5 text-sm text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
        >
          <ChevronLeft class="w-4 h-4" />
          Previous
        </button>
        <span class="text-sm text-gray-500 dark:text-gray-400">
          Page {currentPage + 1} of {totalPages}
        </span>
        <button
          onclick={() => currentPage = Math.min(totalPages - 1, currentPage + 1)}
          disabled={currentPage >= totalPages - 1}
          class="flex items-center gap-1 px-3 py-1.5 text-sm text-gray-600 dark:text-gray-400 hover:bg-gray-100 dark:hover:bg-gray-700 rounded-lg transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
        >
          Next
          <ChevronRight class="w-4 h-4" />
        </button>
      </div>
    {/if}
  {/if}
{/if}
