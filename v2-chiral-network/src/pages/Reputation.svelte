<script lang="ts">
  import { onMount } from 'svelte';
  import { Star, User, MessageSquare, Loader2 } from 'lucide-svelte';
  import { walletAccount } from '$lib/stores';
  import { ratingApi, setRatingOwner, type Rating } from '$lib/services/ratingApiService';
  import { get } from 'svelte/store';

  let ratings = $state<Rating[]>([]);
  let average = $state(0);
  let count = $state(0);
  let loading = $state(true);
  let error = $state<string | null>(null);

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

  async function loadRatings() {
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
      const resp = await ratingApi.getRatings(wallet.address);
      ratings = resp.ratings.sort((a, b) => b.createdAt - a.createdAt);
      average = resp.average;
      count = resp.count;
    } catch (err: any) {
      error = `Failed to load ratings: ${err.message}`;
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    loadRatings();
  });

  // Reload when wallet changes
  $effect(() => {
    const wallet = $walletAccount;
    if (wallet) {
      loadRatings();
    }
  });
</script>

<svelte:head>
  <title>Reputation - Chiral Network</title>
</svelte:head>

<div class="max-w-4xl mx-auto py-6 px-4 sm:px-6">
  <!-- Header -->
  <div class="mb-8">
    <h1 class="text-2xl font-bold dark:text-white">My Reputation</h1>
    <p class="text-sm text-gray-500 dark:text-gray-400 mt-1">
      Ratings received from users who downloaded your files
    </p>
  </div>

  {#if loading}
    <div class="flex items-center justify-center py-20">
      <Loader2 class="w-8 h-8 text-gray-400 animate-spin" />
    </div>
  {:else if error}
    <div class="text-center py-20">
      <Star class="w-12 h-12 mx-auto text-gray-300 dark:text-gray-600 mb-3" />
      <p class="text-gray-500 dark:text-gray-400">{error}</p>
    </div>
  {:else}
    <!-- Score overview card -->
    <div class="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 p-6 mb-6">
      <div class="flex items-center gap-6">
        <!-- Large star display -->
        <div class="flex flex-col items-center">
          <div class="text-4xl font-bold dark:text-white">
            {count > 0 ? average.toFixed(1) : '—'}
          </div>
          <div class="flex gap-0.5 mt-1">
            {#each [1, 2, 3, 4, 5] as star}
              <Star
                class="w-5 h-5 {count > 0 && average >= star ? 'text-yellow-400 fill-yellow-400' : count > 0 && average >= star - 0.5 ? 'text-yellow-400 fill-yellow-400/50' : 'text-gray-300 dark:text-gray-600'}"
              />
            {/each}
          </div>
          <p class="text-sm text-gray-500 dark:text-gray-400 mt-1">
            {count} rating{count !== 1 ? 's' : ''}
          </p>
        </div>

        <!-- Score distribution -->
        {#if count > 0}
          <div class="flex-1 space-y-1.5">
            {#each [5, 4, 3, 2, 1] as score}
              {@const scoreCount = ratings.filter(r => r.score === score).length}
              {@const pct = count > 0 ? (scoreCount / count) * 100 : 0}
              <div class="flex items-center gap-2">
                <span class="text-xs text-gray-500 dark:text-gray-400 w-3 text-right">{score}</span>
                <Star class="w-3 h-3 text-yellow-400 fill-yellow-400" />
                <div class="flex-1 h-2 bg-gray-100 dark:bg-gray-700 rounded-full overflow-hidden">
                  <div
                    class="h-full bg-yellow-400 rounded-full transition-all"
                    style="width: {pct}%"
                  ></div>
                </div>
                <span class="text-xs text-gray-400 dark:text-gray-500 w-8">{scoreCount}</span>
              </div>
            {/each}
          </div>
        {/if}
      </div>
    </div>

    <!-- Ratings list -->
    {#if ratings.length === 0}
      <div class="text-center py-16">
        <Star class="w-12 h-12 mx-auto text-gray-300 dark:text-gray-600 mb-3" />
        <p class="text-gray-500 dark:text-gray-400">No ratings yet</p>
        <p class="text-sm text-gray-400 dark:text-gray-500 mt-1">
          When others download your files and leave a rating, it will appear here
        </p>
      </div>
    {:else}
      <div class="bg-white dark:bg-gray-800 rounded-xl border border-gray-200 dark:border-gray-700 divide-y divide-gray-100 dark:divide-gray-700">
        {#each ratings as rating (rating.id)}
          <div class="p-4">
            <div class="flex items-start justify-between gap-4">
              <div class="flex items-start gap-3 min-w-0">
                <div class="p-2 bg-gray-100 dark:bg-gray-700 rounded-full flex-shrink-0">
                  <User class="w-4 h-4 text-gray-500 dark:text-gray-400" />
                </div>
                <div class="min-w-0">
                  <div class="flex items-center gap-2 flex-wrap">
                    <span class="text-sm font-medium text-gray-900 dark:text-white font-mono">
                      {formatAddr(rating.raterWallet)}
                    </span>
                    <div class="flex gap-0.5">
                      {#each [1, 2, 3, 4, 5] as star}
                        <Star
                          class="w-3.5 h-3.5 {rating.score >= star ? 'text-yellow-400 fill-yellow-400' : 'text-gray-300 dark:text-gray-600'}"
                        />
                      {/each}
                    </div>
                  </div>
                  {#if rating.comment}
                    <div class="flex items-start gap-1.5 mt-1.5">
                      <MessageSquare class="w-3.5 h-3.5 text-gray-400 mt-0.5 flex-shrink-0" />
                      <p class="text-sm text-gray-600 dark:text-gray-300">{rating.comment}</p>
                    </div>
                  {/if}
                  <p class="text-xs text-gray-400 dark:text-gray-500 mt-1.5 font-mono">
                    File: {formatAddr(rating.fileHash)}
                  </p>
                </div>
              </div>
              <span class="text-xs text-gray-400 dark:text-gray-500 whitespace-nowrap flex-shrink-0">
                {formatDate(rating.createdAt)}
              </span>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  {/if}
</div>
