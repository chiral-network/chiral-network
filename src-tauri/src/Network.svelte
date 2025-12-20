<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import ReputationTooltip from './ReputationTooltip.svelte';
  import type { PeerReputation } from './ReputationTooltip.svelte';
  import PeerSelectionService from '../../src/lib/services/peerSelectionService';
  
  // Define a more complete Peer object that includes an ID and reputation
  interface Peer {
    id: string;
    reputation: PeerReputation;
  }

  let peers: Peer[] = [];
  let isLoading = true;

  /**
   * Fetches the list of currently connected peers and their reputation data
   * using an efficient batching mechanism.
   */
  async function fetchConnectedPeersWithReputation(): Promise<Peer[]> {
    console.log('Fetching peer reputations...');
    try {
      // 1. Get the list of currently connected peer IDs from the backend.
      const connectedPeerIds = await invoke<string[]>('get_connected_peer_ids');
      if (!connectedPeerIds || connectedPeerIds.length === 0) {
        return [];
      }

      // 2. Use the service to fetch metrics only for those specific peers.
      const peerMetrics = await PeerSelectionService.getMetricsForPeers(connectedPeerIds);

      // 3. Map the raw metrics to the data structure required by the UI components.
      return peerMetrics.map((metrics): Peer => {
        const score = Math.round(PeerSelectionService.compositeScoreFromMetrics(metrics) * 100);
        return {
          id: metrics.peer_id,
          reputation: {
            score,
            trustLevel: PeerSelectionService.getTrustLevelFromScore(score),
            successful_interactions: metrics.successful_transfers,
            total_interactions: metrics.transfer_count,
            uptime_percentage: metrics.uptime_score * 100,
            last_seen: new Date(metrics.last_seen * 1000)
          }
        };
      });
    } catch (error) {
      console.error('Failed to fetch connected peers with reputation:', error);
      return []; // Return an empty array on failure
    }
  }

  onMount(async () => {
    peers = await fetchConnectedPeersWithReputation();
    isLoading = false;
  });
</script>

<div class="p-4">
  <h2 class="text-xl font-bold mb-4">Connected Peers</h2>
  {#if isLoading}
    <p>Loading peers...</p>
  {:else}
    <ul class="space-y-2">
      {#each peers as peer (peer.id)}
        <li class="flex items-center gap-3 p-2 rounded-md bg-gray-100">
          <ReputationTooltip peer={peer.reputation} />
          <span class="font-mono text-sm truncate">{peer.id}</span>
        </li>
      {/each}
    </ul>
  {/if}
</div>