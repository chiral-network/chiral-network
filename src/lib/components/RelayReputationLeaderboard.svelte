<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from 'svelte-i18n';
  import { invoke } from '@tauri-apps/api/core';
  import Card from '$lib/components/ui/card.svelte';
  import Button from '$lib/components/ui/button.svelte';

  interface RelayNodeStats {
    peer_id: string;
    reputation_score: number;
    reservations_accepted: number;
    circuits_established: number;
    circuits_successful: number;
    total_events: number;
    last_seen: number;
  }

  interface RelayReputationStats {
    total_relays: number;
    top_relays: RelayNodeStats[];
  }

  let stats: RelayReputationStats | null = null;
  let isLoading = true;
  let limit = 100;

  function getBadge(score: number): { name: string; color: string; emoji: string } {
    if (score >= 1000) return { name: 'Diamond Relay', color: 'text-blue-400', emoji: '💎' };
    if (score >= 500) return { name: 'Platinum Relay', color: 'text-purple-400', emoji: '🏆' };
    if (score >= 100) return { name: 'Gold Relay', color: 'text-yellow-400', emoji: '🥇' };
    return { name: 'Standard Relay', color: 'text-gray-400', emoji: '⚡' };
  }

  function formatUptime(lastSeen: number): string {
    const now = Math.floor(Date.now() / 1000);
    const diff = now - lastSeen;
    if (diff < 60) return 'Online';
    if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
    if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
    return `${Math.floor(diff / 86400)}d ago`;
  }

  function formatPeerId(peerId: string): string {
    if (peerId.length <= 12) return peerId;
    return `${peerId.slice(0, 6)}...${peerId.slice(-6)}`;
  }

  async function loadStats() {
    isLoading = true;
    try {
      const result = await invoke<RelayReputationStats>('get_relay_reputation_stats', { limit });
      stats = result;
    } catch (error) {
      console.error('Failed to load relay reputation stats:', error);
      stats = { total_relays: 0, top_relays: [] };
    } finally {
      isLoading = false;
    }
  }

  onMount(() => {
    loadStats();
    const interval = setInterval(loadStats, 30000); // Refresh every 30s
    return () => clearInterval(interval);
  });
</script>

<Card class="p-6">
  <div class="mb-6">
    <div class="flex items-center justify-between">
      <div>
        <h2 class="text-2xl font-bold text-gray-900">{$t('relay.leaderboard.title')}</h2>
        <p class="text-gray-600 mt-1">{$t('relay.leaderboard.subtitle')}</p>
      </div>
      <Button on:click={loadStats} disabled={isLoading} variant="outline">
        {isLoading ? $t('relay.leaderboard.refreshing') : $t('relay.leaderboard.refresh')}
      </Button>
    </div>
  </div>

  {#if isLoading}
    <div class="flex items-center justify-center py-12">
      <div class="text-center">
        <div class="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-600 mx-auto"></div>
        <p class="mt-4 text-gray-600">{$t('relay.leaderboard.loading')}</p>
      </div>
    </div>
  {:else if stats && stats.top_relays.length > 0}
    <div class="space-y-2">
      <!-- Header -->
      <div class="grid grid-cols-7 gap-4 px-4 py-2 text-xs font-semibold text-gray-600 uppercase tracking-wide border-b">
        <div class="col-span-1">{$t('relay.leaderboard.rank')}</div>
        <div class="col-span-2">{$t('relay.leaderboard.peerId')}</div>
        <div class="col-span-1 text-right">{$t('relay.leaderboard.score')}</div>
        <div class="col-span-1 text-right">{$t('relay.leaderboard.circuits')}</div>
        <div class="col-span-1 text-right">{$t('relay.leaderboard.bandwidth')}</div>
        <div class="col-span-1 text-right">{$t('relay.leaderboard.uptime')}</div>
      </div>

      <!-- Leaderboard Rows -->
      {#each stats.top_relays as relay, index}
        {@const badge = getBadge(relay.reputation_score)}
        <div
          class="grid grid-cols-7 gap-4 px-4 py-3 rounded-lg hover:bg-gray-50 transition-colors items-center"
          class:bg-yellow-50={index < 3}
        >
          <!-- Rank -->
          <div class="col-span-1 flex items-center gap-2">
            <span class="font-bold text-lg text-gray-700">#{index + 1}</span>
            {#if index === 0}
              <span class="text-xl">🥇</span>
            {:else if index === 1}
              <span class="text-xl">🥈</span>
            {:else if index === 2}
              <span class="text-xl">🥉</span>
            {/if}
          </div>

          <!-- Peer ID with Badge -->
          <div class="col-span-2">
            <div class="flex items-center gap-2">
              <span class="font-mono text-sm text-gray-900">{formatPeerId(relay.peer_id)}</span>
              <span class="text-lg" title={badge.name}>{badge.emoji}</span>
            </div>
            <div class="text-xs {badge.color} font-medium">{badge.name}</div>
          </div>

          <!-- Score -->
          <div class="col-span-1 text-right">
            <div class="font-bold text-gray-900">{relay.reputation_score.toFixed(0)}</div>
            <div class="text-xs text-gray-500">{$t('relay.leaderboard.points')}</div>
          </div>

          <!-- Circuits -->
          <div class="col-span-1 text-right">
            <div class="font-semibold text-gray-900">{relay.circuits_successful}</div>
            <div class="text-xs text-gray-500">/ {relay.circuits_established}</div>
          </div>

          <!-- Bandwidth (placeholder - using total events as proxy) -->
          <div class="col-span-1 text-right">
            <div class="font-semibold text-gray-900">{(relay.total_events * 2.5).toFixed(1)}</div>
            <div class="text-xs text-gray-500">GB</div>
          </div>

          <!-- Uptime -->
          <div class="col-span-1 text-right">
            <div class="font-semibold text-gray-900">{formatUptime(relay.last_seen)}</div>
            <div class="text-xs text-gray-500">{$t('relay.leaderboard.lastSeen')}</div>
          </div>
        </div>
      {/each}
    </div>

    <!-- Summary Stats -->
    <div class="mt-6 pt-6 border-t border-gray-200">
      <div class="grid grid-cols-3 gap-4 text-center">
        <div>
          <div class="text-2xl font-bold text-gray-900">{stats.total_relays}</div>
          <div class="text-sm text-gray-600">{$t('relay.leaderboard.totalRelays')}</div>
        </div>
        <div>
          <div class="text-2xl font-bold text-gray-900">
            {stats.top_relays.filter(r => getBadge(r.reputation_score).name !== 'Standard Relay').length}
          </div>
          <div class="text-sm text-gray-600">{$t('relay.leaderboard.badgedRelays')}</div>
        </div>
        <div>
          <div class="text-2xl font-bold text-gray-900">
            {stats.top_relays.reduce((sum, r) => sum + r.circuits_successful, 0)}
          </div>
          <div class="text-sm text-gray-600">{$t('relay.leaderboard.totalCircuits')}</div>
        </div>
      </div>
    </div>
  {:else}
    <div class="text-center py-12">
      <div class="text-gray-400 text-6xl mb-4">🏆</div>
      <h3 class="text-lg font-medium text-gray-900 mb-2">{$t('relay.leaderboard.noRelays')}</h3>
      <p class="text-gray-500">{$t('relay.leaderboard.noRelaysDesc')}</p>
    </div>
  {/if}
</Card>
