<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { Shield, RefreshCw, Copy, Check, ChevronDown, ChevronUp } from 'lucide-svelte';
  import {
    type VerifiedReputation,
    type TransactionVerdict,
    trustLevelBg,
    outcomeLabel,
    scoreToStars,
  } from '$lib/reputationStore';

  // ── State ──────────────────────────────────────────────────────────────────

  let myPeerId = $state<string | null>(null);
  let myPublicKey = $state<string | null>(null);
  let peerIdCopied = $state(false);
  let loading = $state(true);
  let refreshing = $state(false);

  let myRep = $state<VerifiedReputation | null>(null);
  let myVerdicts = $state<TransactionVerdict[]>([]);
  let showVerdicts = $state(false);

  // ── Helpers ────────────────────────────────────────────────────────────────

  function confidenceLabel(c: number): string {
    if (c >= 0.7) return 'High confidence';
    if (c >= 0.4) return 'Moderate confidence';
    if (c > 0)    return 'Low confidence';
    return 'No evidence';
  }

  function relativeTime(unixSec: number): string {
    const delta = Math.floor(Date.now() / 1000) - unixSec;
    if (delta < 60)   return 'just now';
    if (delta < 3600) return `${Math.floor(delta / 60)}m ago`;
    if (delta < 86400) return `${Math.floor(delta / 3600)}h ago`;
    return `${Math.floor(delta / 86400)}d ago`;
  }

  async function loadAll() {
    try {
      myPeerId = await invoke<string | null>('get_peer_id');
      myPublicKey = await invoke<string>('get_reputation_public_key');
    } catch {
      // DHT not yet started
      return;
    }

    if (!myPeerId) return;

    // Load own reputation and verdicts in parallel
    const [repResult, verdictsResult] = await Promise.allSettled([
      invoke<VerifiedReputation>('get_reputation_score', { peerId: myPeerId }),
      invoke<TransactionVerdict[]>('get_reputation_verdicts', { peerId: myPeerId }),
    ]);

    myRep = repResult.status === 'fulfilled' ? repResult.value : null;
    myVerdicts = verdictsResult.status === 'fulfilled' ? verdictsResult.value : [];
  }

  async function refresh() {
    refreshing = true;
    await loadAll();
    refreshing = false;
  }

  function copyPeerId() {
    if (!myPeerId) return;
    navigator.clipboard.writeText(myPeerId).then(() => {
      peerIdCopied = true;
      setTimeout(() => { peerIdCopied = false; }, 2000);
    });
  }

  // ── Lifecycle ──────────────────────────────────────────────────────────────

  $effect(() => {
    loading = true;
    loadAll().finally(() => { loading = false; });
  });
</script>

<!-- ─── Page ─────────────────────────────────────────────────────────────── -->

<div class="max-w-3xl mx-auto px-4 py-6 space-y-6">

  <!-- Header -->
  <div class="flex items-center justify-between flex-wrap gap-3">
    <div class="flex items-center gap-3">
      <Shield class="w-7 h-7 text-primary-600 dark:text-primary-400" />
      <div>
        <h1 class="text-2xl font-bold dark:text-white">My Reputation</h1>
        <p class="text-sm text-gray-500 dark:text-gray-400">
          Your reputation as seen by other peers on the network
        </p>
      </div>
    </div>
    <button
      onclick={refresh}
      disabled={refreshing}
      class="flex items-center gap-2 px-4 py-2 rounded-lg border border-gray-300 dark:border-gray-600
             text-sm font-medium text-gray-700 dark:text-gray-300
             hover:bg-gray-100 dark:hover:bg-gray-700 disabled:opacity-50 transition"
    >
      <RefreshCw class="w-4 h-4 {refreshing ? 'animate-spin' : ''}" />
      {refreshing ? 'Refreshing...' : 'Refresh'}
    </button>
  </div>

  {#if loading}
    <div class="flex items-center justify-center py-16">
      <div class="text-center space-y-3">
        <div class="animate-spin rounded-full h-10 w-10 border-b-2 border-primary-600 mx-auto"></div>
        <p class="text-sm text-gray-500 dark:text-gray-400">Loading your reputation...</p>
      </div>
    </div>
  {:else if !myPeerId}
    <div class="rounded-xl border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-12 text-center">
      <Shield class="w-12 h-12 text-gray-300 dark:text-gray-600 mx-auto mb-3" />
      <h3 class="text-lg font-medium text-gray-900 dark:text-white mb-1">Not connected</h3>
      <p class="text-sm text-gray-500 dark:text-gray-400">
        Start the DHT network to view your reputation.
      </p>
    </div>
  {:else}
    <!-- My Node Identity -->
    <div class="rounded-xl border border-blue-200 dark:border-blue-800 bg-blue-50 dark:bg-blue-950/40 p-4 space-y-2">
      <div class="flex items-center gap-2">
        <Shield class="w-4 h-4 text-blue-600 dark:text-blue-400" />
        <span class="text-sm font-semibold text-blue-800 dark:text-blue-200">My Node Identity</span>
      </div>
      <div class="flex items-center gap-2 flex-wrap">
        <code class="text-xs font-mono text-blue-900 dark:text-blue-100 break-all flex-1 bg-blue-100 dark:bg-blue-900/50 rounded px-2 py-1">
          {myPeerId}
        </code>
        <button
          onclick={copyPeerId}
          class="flex items-center gap-1 px-3 py-1 rounded-lg text-xs font-medium border border-blue-300 dark:border-blue-700
                 text-blue-700 dark:text-blue-300 hover:bg-blue-100 dark:hover:bg-blue-800 transition shrink-0"
        >
          {#if peerIdCopied}
            <Check class="w-3 h-3" /> Copied
          {:else}
            <Copy class="w-3 h-3" /> Copy
          {/if}
        </button>
      </div>
      {#if myPublicKey}
        <p class="text-xs text-blue-600 dark:text-blue-400">
          Reputation key: <code class="font-mono">{myPublicKey.slice(0, 24)}...</code>
        </p>
      {/if}
    </div>

    <!-- My Reputation Score -->
    <div class="rounded-xl border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-5 space-y-4">
      <h2 class="text-lg font-semibold dark:text-white">Reputation Score</h2>

      {#if myRep === null}
        <p class="text-sm text-gray-500 dark:text-gray-400">Unable to load reputation data.</p>
      {:else if myRep.totalVerdicts === 0}
        <div class="text-center py-6">
          <p class="text-3xl mb-2">&#11088;</p>
          <p class="text-sm font-medium text-gray-700 dark:text-gray-300">No verdicts yet</p>
          <p class="text-xs text-gray-500 dark:text-gray-400 mt-1">
            Your reputation will build as you share files and interact with other peers.
          </p>
        </div>
      {:else}
        <div class="flex items-center gap-4 flex-wrap">
          <!-- Trust badge -->
          <span class="px-3 py-1 rounded-full text-sm font-semibold {trustLevelBg(myRep.trustLevel)} capitalize">
            {myRep.trustLevel}
          </span>

          <!-- Star score -->
          <div>
            <div class="text-lg font-bold text-gray-900 dark:text-white">{scoreToStars(myRep.score)}</div>
            <div class="text-xs text-gray-400">{myRep.totalVerdicts} verdict{myRep.totalVerdicts !== 1 ? 's' : ''}</div>
          </div>
        </div>

        <!-- Verdict breakdown -->
        <div class="flex gap-4 flex-wrap text-sm">
          <span class="text-green-600 dark:text-green-400">
            {myRep.goodCount} good
          </span>
          <span class="text-yellow-600 dark:text-yellow-400">
            {myRep.disputedCount} disputed
          </span>
          <span class="text-red-600 dark:text-red-400">
            {myRep.badCount} bad
          </span>
        </div>

        <!-- Signature verification -->
        <div class="text-xs text-gray-500 dark:text-gray-400">
          {myRep.signatureVerifiedCount}/{myRep.totalVerdicts} signatures verified — {confidenceLabel(myRep.confidence)}
        </div>
      {/if}
    </div>

    <!-- Verdicts About Me -->
    <div class="rounded-xl border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 overflow-hidden">
      <button
        onclick={() => { showVerdicts = !showVerdicts; }}
        class="w-full flex items-center justify-between p-4 text-left hover:bg-gray-50 dark:hover:bg-gray-700/50 transition"
      >
        <h2 class="text-lg font-semibold dark:text-white">
          Verdicts About Me
          <span class="text-sm font-normal text-gray-400 ml-1">({myVerdicts.length})</span>
        </h2>
        {#if showVerdicts}
          <ChevronUp class="w-5 h-5 text-gray-400" />
        {:else}
          <ChevronDown class="w-5 h-5 text-gray-400" />
        {/if}
      </button>

      {#if showVerdicts}
        <div class="border-t border-gray-100 dark:border-gray-700 p-4 space-y-2">
          {#if myVerdicts.length === 0}
            <p class="text-sm text-gray-400 italic">No verdicts filed about you yet.</p>
          {:else}
            {#each myVerdicts as v}
              <div class="flex items-start gap-3 text-xs bg-gray-50 dark:bg-gray-900/50 rounded-lg p-3 border border-gray-100 dark:border-gray-700">
                <span class="shrink-0 font-medium">{outcomeLabel(v.outcome)}</span>
                <div class="flex-1 min-w-0">
                  {#if v.details}
                    <p class="text-gray-700 dark:text-gray-300 mb-1">{v.details}</p>
                  {/if}
                  <div class="flex items-center gap-2 text-gray-400 flex-wrap">
                    <span>by <code class="font-mono">{v.issuerId.slice(0, 20)}...</code></span>
                    <span>·</span>
                    <span>{relativeTime(v.issuedAt)}</span>
                    {#if v.issuerSig}
                      <span class="px-1.5 py-0.5 bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400 rounded text-[10px] font-medium">
                        Signed
                      </span>
                    {/if}
                  </div>
                </div>
              </div>
            {/each}
          {/if}
        </div>
      {/if}
    </div>

    <!-- How It Works -->
    <div class="rounded-xl border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-5 space-y-3">
      <h2 class="text-lg font-semibold dark:text-white">How Reputation Works</h2>
      <div class="text-sm text-gray-600 dark:text-gray-400 space-y-2">
        <p>
          After downloading a file, peers can rate the experience by filing a signed verdict.
          Verdicts are stored on the DHT and are cryptographically signed with Ed25519 keys,
          making them tamper-evident and publicly verifiable.
        </p>
        <p>
          When you search for a file to download, you'll see each seeder's reputation score
          so you can choose who to download from. Your own reputation builds over time
          as peers interact with you.
        </p>
      </div>
    </div>
  {/if}
</div>
