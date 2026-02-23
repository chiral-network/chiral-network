<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { Shield, RefreshCw, Flag, Copy, Check, ChevronDown, ChevronUp, AlertTriangle } from 'lucide-svelte';
  import {
    type VerifiedReputation,
    type TransactionVerdict,
    unknownReputation,
    trustLevelBg,
    outcomeLabel,
    scoreToStars,
    getCached,
    setCached,
  } from '$lib/reputationStore';

  // ── State ──────────────────────────────────────────────────────────────────

  let myPeerId = $state<string | null>(null);
  let myPublicKey = $state<string | null>(null);
  let peerIdCopied = $state(false);

  interface PeerRow {
    id: string;
    rep: VerifiedReputation | null; // null = loading
    verdicts: TransactionVerdict[] | null;
    showVerdicts: boolean;
  }

  let peers = $state<PeerRow[]>([]);
  let globalLoading = $state(true);
  let refreshing = $state(false);

  // Report modal
  let reportOpen = $state(false);
  let reportTarget = $state('');
  let reportOutcome = $state<'good' | 'disputed' | 'bad'>('bad');
  let reportDetails = $state('');
  let reportSubmitting = $state(false);
  let reportError = $state('');
  let reportSuccess = $state('');

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

  async function loadMyIdentity() {
    try {
      myPeerId = await invoke<string | null>('get_peer_id');
      myPublicKey = await invoke<string>('get_reputation_public_key');
    } catch {
      // DHT not yet started — graceful no-op
    }
  }

  async function loadPeers() {
    try {
      const raw = await invoke<{ id: string }[]>('get_dht_peers');
      // Initialise rows without scores (fill in asynchronously)
      const rows: PeerRow[] = raw.map((p) => ({
        id: p.id,
        rep: getCached(p.id),
        verdicts: null,
        showVerdicts: false,
      }));
      peers = rows;

      // Fetch all uncached scores in parallel
      const uncached = peers
        .map((p, i) => ({ idx: i, id: p.id }))
        .filter((p) => peers[p.idx].rep === null);

      if (uncached.length > 0) {
        const results = await Promise.allSettled(
          uncached.map((p) =>
            invoke<VerifiedReputation>('get_reputation_score', { peerId: p.id })
          )
        );
        for (let j = 0; j < uncached.length; j++) {
          const { idx, id } = uncached[j];
          const result = results[j];
          if (result.status === 'fulfilled') {
            setCached(id, result.value);
            peers[idx] = { ...peers[idx], rep: result.value };
          } else {
            peers[idx] = { ...peers[idx], rep: unknownReputation() };
          }
        }
        peers = [...peers];
      }
    } catch (e) {
      console.error('Failed to load peers', e);
    }
  }

  async function toggleVerdicts(idx: number) {
    const row = peers[idx];
    if (row.showVerdicts) {
      peers[idx] = { ...row, showVerdicts: false };
      peers = [...peers];
      return;
    }
    // Load verdicts if not yet fetched
    if (row.verdicts === null) {
      try {
        const v = await invoke<TransactionVerdict[]>('get_reputation_verdicts', {
          peerId: row.id,
        });
        peers[idx] = { ...row, verdicts: v, showVerdicts: true };
      } catch {
        peers[idx] = { ...row, verdicts: [], showVerdicts: true };
      }
    } else {
      peers[idx] = { ...row, showVerdicts: true };
    }
    peers = [...peers];
  }

  async function refresh() {
    refreshing = true;
    await loadMyIdentity();
    await loadPeers();
    refreshing = false;
  }

  function openReport(peerId: string) {
    reportTarget = peerId;
    reportOutcome = 'bad';
    reportDetails = '';
    reportError = '';
    reportSuccess = '';
    reportOpen = true;
  }

  async function submitReport() {
    reportSubmitting = true;
    reportError = '';
    reportSuccess = '';
    try {
      await invoke('file_reputation_verdict', {
        targetPeerId: reportTarget,
        outcome: reportOutcome,
        details: reportDetails.trim() || null,
      });
      reportSuccess = 'Verdict signed and published to DHT.';
      // Invalidate cache for this peer and refresh its score
      const idx = peers.findIndex((p) => p.id === reportTarget);
      if (idx !== -1) {
        peers[idx] = { ...peers[idx], rep: null, verdicts: null };
        peers = [...peers];
        try {
          const rep = await invoke<VerifiedReputation>('get_reputation_score', {
            peerId: reportTarget,
          });
          setCached(reportTarget, rep);
          peers[idx] = { ...peers[idx], rep };
          peers = [...peers];
        } catch { /* ignore */ }
      }
      setTimeout(() => { reportOpen = false; }, 1500);
    } catch (e: unknown) {
      reportError = e instanceof Error ? e.message : String(e);
    } finally {
      reportSubmitting = false;
    }
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
    globalLoading = true;
    loadMyIdentity().then(() => loadPeers()).finally(() => { globalLoading = false; });
  });
</script>

<!-- ─── Page ─────────────────────────────────────────────────────────────── -->

<div class="max-w-5xl mx-auto px-4 py-6 space-y-6">

  <!-- Header -->
  <div class="flex items-center justify-between flex-wrap gap-3">
    <div class="flex items-center gap-3">
      <Shield class="w-7 h-7 text-primary-600 dark:text-primary-400" />
      <div>
        <h1 class="text-2xl font-bold dark:text-white">Reputation</h1>
        <p class="text-sm text-gray-500 dark:text-gray-400">
          Cryptographically signed peer ratings stored on the DHT
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
      {refreshing ? 'Refreshing…' : 'Refresh'}
    </button>
  </div>

  <!-- My Node Identity -->
  {#if myPeerId}
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
          🔑 Reputation key: <code class="font-mono">{myPublicKey.slice(0, 24)}…</code>
        </p>
      {/if}
      <p class="text-xs text-blue-500 dark:text-blue-500">
        🔒 All verdicts you issue are Ed25519-signed and stored on the DHT — they cannot be silently modified.
      </p>
    </div>
  {/if}

  <!-- Peer List -->
  {#if globalLoading}
    <div class="flex items-center justify-center py-16">
      <div class="text-center space-y-3">
        <div class="animate-spin rounded-full h-10 w-10 border-b-2 border-primary-600 mx-auto"></div>
        <p class="text-sm text-gray-500 dark:text-gray-400">Loading peer reputation data…</p>
      </div>
    </div>
  {:else if peers.length === 0}
    <div class="rounded-xl border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 p-12 text-center">
      <Shield class="w-12 h-12 text-gray-300 dark:text-gray-600 mx-auto mb-3" />
      <h3 class="text-lg font-medium text-gray-900 dark:text-white mb-1">No peers connected</h3>
      <p class="text-sm text-gray-500 dark:text-gray-400">
        Connect to the DHT network to see peer reputation data.
      </p>
    </div>
  {:else}
    <div class="space-y-3">
      {#each peers as row, idx (row.id)}
        {@const rep = row.rep}
        <div class="rounded-xl border border-gray-200 dark:border-gray-700 bg-white dark:bg-gray-800 overflow-hidden">

          <!-- Peer row header -->
          <div class="flex items-center gap-3 p-4 flex-wrap">

            <!-- Trust badge -->
            <div class="shrink-0">
              {#if rep === null}
                <div class="w-20 h-6 bg-gray-200 dark:bg-gray-700 animate-pulse rounded-full"></div>
              {:else}
                <span class="px-2.5 py-0.5 rounded-full text-xs font-semibold {trustLevelBg(rep.trustLevel)} capitalize">
                  {rep.trustLevel}
                </span>
              {/if}
            </div>

            <!-- Peer ID -->
            <code class="text-xs font-mono text-gray-600 dark:text-gray-400 flex-1 break-all min-w-0">
              {row.id}
            </code>

            <!-- Score -->
            {#if rep !== null}
              <div class="text-right shrink-0 hidden sm:block">
                <div class="text-sm font-bold text-gray-900 dark:text-white">{scoreToStars(rep.score)}</div>
                <div class="text-xs text-gray-400">{rep.totalVerdicts} verdict{rep.totalVerdicts !== 1 ? 's' : ''}</div>
              </div>
            {/if}

            <!-- Actions -->
            <div class="flex items-center gap-2 shrink-0">
              {#if row.id !== myPeerId}
                <button
                  onclick={() => openReport(row.id)}
                  class="flex items-center gap-1 px-2.5 py-1 rounded-lg text-xs font-medium border border-red-200 dark:border-red-800
                         text-red-600 dark:text-red-400 hover:bg-red-50 dark:hover:bg-red-900/30 transition"
                  title="File a verdict"
                >
                  <Flag class="w-3 h-3" /> Report
                </button>
              {/if}
              <button
                onclick={() => toggleVerdicts(idx)}
                class="flex items-center gap-1 px-2.5 py-1 rounded-lg text-xs font-medium border border-gray-200 dark:border-gray-600
                       text-gray-600 dark:text-gray-400 hover:bg-gray-50 dark:hover:bg-gray-700 transition"
              >
                {#if row.showVerdicts}
                  <ChevronUp class="w-3 h-3" /> Hide
                {:else}
                  <ChevronDown class="w-3 h-3" /> Verdicts
                {/if}
              </button>
            </div>
          </div>

          <!-- Score bar (visible on mobile where score is hidden above) -->
          {#if rep !== null && rep.totalVerdicts > 0}
            <div class="px-4 pb-3 flex items-center gap-2 sm:hidden">
              <span class="text-xs text-gray-500">Score</span>
              <span class="text-sm font-bold text-gray-900 dark:text-white">{scoreToStars(rep.score)}</span>
              <span class="text-xs text-gray-400">({rep.totalVerdicts} verdict{rep.totalVerdicts !== 1 ? 's' : ''})</span>
            </div>
          {/if}

          <!-- Score breakdown -->
          {#if rep !== null && rep.totalVerdicts > 0}
            <div class="px-4 pb-3 flex gap-3 flex-wrap text-xs text-gray-500 dark:text-gray-400">
              <span class="text-green-600 dark:text-green-400">✅ {rep.goodCount} good</span>
              <span class="text-yellow-600 dark:text-yellow-400">⚠️ {rep.disputedCount} disputed</span>
              <span class="text-red-600 dark:text-red-400">❌ {rep.badCount} bad</span>
              <span title="Verdicts whose Ed25519 signature was verified against the issuer's DHT-published key">
                🔐 {rep.signatureVerifiedCount}/{rep.totalVerdicts} verified — {confidenceLabel(rep.confidence)}
              </span>
            </div>
          {/if}

          <!-- Verdicts panel -->
          {#if row.showVerdicts}
            <div class="border-t border-gray-100 dark:border-gray-700 bg-gray-50 dark:bg-gray-900/50 p-4 space-y-2">
              <h4 class="text-xs font-semibold text-gray-500 dark:text-gray-400 uppercase tracking-wide mb-2">
                DHT Verdicts
              </h4>
              {#if row.verdicts === null}
                <p class="text-xs text-gray-400">Loading…</p>
              {:else if row.verdicts.length === 0}
                <p class="text-xs text-gray-400 italic">No verdicts stored in DHT yet.</p>
              {:else}
                {#each row.verdicts as v}
                  <div class="flex items-start gap-3 text-xs bg-white dark:bg-gray-800 rounded-lg p-3 border border-gray-100 dark:border-gray-700">
                    <span class="shrink-0 font-medium">{outcomeLabel(v.outcome)}</span>
                    <div class="flex-1 min-w-0">
                      {#if v.details}
                        <p class="text-gray-700 dark:text-gray-300 mb-1">{v.details}</p>
                      {/if}
                      <div class="flex items-center gap-2 text-gray-400 flex-wrap">
                        <span>by <code class="font-mono">{v.issuerId.slice(0, 20)}…</code></span>
                        <span>·</span>
                        <span>{relativeTime(v.issuedAt)}</span>
                        {#if v.issuerSig}
                          <span class="px-1.5 py-0.5 bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400 rounded text-[10px] font-medium">
                            🔐 Signed
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
      {/each}
    </div>
  {/if}

</div>

<!-- ─── Report Modal ──────────────────────────────────────────────────────── -->

{#if reportOpen}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    role="dialog"
    tabindex="-1"
    aria-modal="true"
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
    onkeydown={(e) => { if (e.key === 'Escape' && !reportSubmitting) reportOpen = false; }}
    onclick={(e) => { if (e.target === e.currentTarget && !reportSubmitting) reportOpen = false; }}
  >
    <div class="w-full max-w-lg bg-white dark:bg-gray-800 rounded-2xl shadow-2xl p-6 space-y-5">

      <div>
        <h2 class="text-lg font-bold dark:text-white flex items-center gap-2">
          <Flag class="w-5 h-5 text-red-500" /> File Reputation Verdict
        </h2>
        <p class="text-sm text-gray-500 dark:text-gray-400 mt-1">
          Your verdict is signed with your node's Ed25519 key and published to the DHT.
          It is tamper-evident and publicly verifiable.
        </p>
      </div>

      <!-- Target peer ID -->
      <div>
        <p class="text-xs text-gray-500 dark:text-gray-400 mb-1 font-medium">Target Peer</p>
        <code class="text-xs font-mono break-all text-gray-700 dark:text-gray-300 bg-gray-100 dark:bg-gray-700 rounded px-2 py-1 block">
          {reportTarget}
        </code>
      </div>

      <!-- Outcome -->
      <div>
        <p class="text-sm font-medium text-gray-700 dark:text-gray-300 mb-2">Outcome</p>
        <div role="radiogroup" aria-label="Outcome" class="space-y-2">
          <label class="flex items-center gap-3 cursor-pointer">
            <input type="radio" bind:group={reportOutcome} value="good" class="accent-green-600" />
            <span class="text-sm text-green-700 dark:text-green-400">✅ Positive — peer behaved well</span>
          </label>
          <label class="flex items-center gap-3 cursor-pointer">
            <input type="radio" bind:group={reportOutcome} value="disputed" />
            <span class="text-sm text-yellow-700 dark:text-yellow-400">⚠️ Disputed — uncertain outcome</span>
          </label>
          <label class="flex items-center gap-3 cursor-pointer">
            <input type="radio" bind:group={reportOutcome} value="bad" class="accent-red-600" />
            <span class="text-sm text-red-700 dark:text-red-400">❌ Negative — peer misbehaved</span>
          </label>
        </div>
      </div>

      <!-- Details -->
      <div>
        <label for="report-details" class="text-sm font-medium text-gray-700 dark:text-gray-300 block mb-1">
          Details <span class="text-gray-400 font-normal">(optional)</span>
        </label>
        <textarea
          id="report-details"
          bind:value={reportDetails}
          rows="3"
          placeholder="Describe what happened (e.g. transfer completed, file was corrupt, no response)…"
          class="w-full px-3 py-2 text-sm rounded-lg border border-gray-300 dark:border-gray-600
                 bg-white dark:bg-gray-700 text-gray-900 dark:text-gray-100
                 focus:outline-none focus:ring-2 focus:ring-primary-500 resize-none"
        ></textarea>
      </div>

      <!-- Feedback -->
      {#if reportError}
        <div class="flex items-start gap-2 text-sm text-red-700 dark:text-red-400 bg-red-50 dark:bg-red-900/30 rounded-lg px-3 py-2">
          <AlertTriangle class="w-4 h-4 shrink-0 mt-0.5" />
          {reportError}
        </div>
      {/if}
      {#if reportSuccess}
        <div class="text-sm text-green-700 dark:text-green-400 bg-green-50 dark:bg-green-900/30 rounded-lg px-3 py-2">
          ✅ {reportSuccess}
        </div>
      {/if}

      <!-- Actions -->
      <div class="flex justify-end gap-3 pt-1">
        <button
          onclick={() => { reportOpen = false; }}
          disabled={reportSubmitting}
          class="px-4 py-2 rounded-lg text-sm font-medium border border-gray-300 dark:border-gray-600
                 text-gray-700 dark:text-gray-300 hover:bg-gray-100 dark:hover:bg-gray-700 disabled:opacity-50 transition"
        >
          Cancel
        </button>
        <button
          onclick={submitReport}
          disabled={reportSubmitting}
          class="px-4 py-2 rounded-lg text-sm font-medium text-white disabled:opacity-50 transition
                 {reportOutcome === 'good'
                   ? 'bg-green-600 hover:bg-green-700'
                   : reportOutcome === 'bad'
                     ? 'bg-red-600 hover:bg-red-700'
                     : 'bg-yellow-600 hover:bg-yellow-700'}"
        >
          {reportSubmitting ? 'Submitting…' : '🔐 Sign & Submit'}
        </button>
      </div>
    </div>
  </div>
{/if}
