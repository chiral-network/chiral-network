<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import ReputationTooltip from './ReputationTooltip.svelte';
  import type { PeerReputation } from './ReputationTooltip.svelte';
  import PeerSelectionService from '../../src/lib/services/peerSelectionService';

  // --- Data Structures ---
  interface SourcePeer {
    id: string;
    reputation: PeerReputation;
    speed: string; // e.g., "1.2 MB/s"
  }

  interface ActiveDownload {
    progress: number; // 0 to 100
    totalSpeed: string;
    sources: SourcePeer[];
  }

  let download: ActiveDownload | null = null;
  let isLoading = true;

  // Props to identify the download this component should display
  export let fileHash: string;
  export let fileName: string;

  /**
   * Fetches the real-time state of a specific download, including its sources
   * and their reputation data.
   */
  async function fetchDownloadDetailsWithReputation(hash: string): Promise<ActiveDownload | null> {
    try {
      // 1. Get the current state of the download from the backend.
      // This assumes a backend command that returns progress and source peers.
      const state = await invoke<{
        progress: number;
        total_speed_bps: number;
        sources: { peer_id: string; speed_bps: number }[];
      }>('get_download_state', { fileHash: hash });

      if (!state || !state.sources) return null;

      // 2. Get reputation metrics for only the source peers.
      const sourcePeerIds = state.sources.map((s) => s.peer_id);
      const peerMetrics = await PeerSelectionService.getMetricsForPeers(sourcePeerIds);
      const metricsMap = new Map(peerMetrics.map((m) => [m.peer_id, m]));

      // 3. Combine the data into the final structure for the UI.
      const sources: SourcePeer[] = state.sources.map((source) => {
        const metrics = metricsMap.get(source.peer_id);
        const score = metrics ? Math.round(PeerSelectionService.compositeScoreFromMetrics(metrics) * 100) : 0;
        return {
          id: source.peer_id,
          speed: PeerSelectionService.formatBytes(source.speed_bps) + '/s',
          reputation: {
            score,
            trustLevel: PeerSelectionService.getTrustLevelFromScore(score),
            successful_interactions: metrics?.successful_transfers ?? 0,
            total_interactions: metrics?.transfer_count ?? 0,
            uptime_percentage: (metrics?.uptime_score ?? 0) * 100,
            last_seen: new Date((metrics?.last_seen ?? 0) * 1000)
          }
        };
      });

      return {
        progress: state.progress,
        totalSpeed: PeerSelectionService.formatBytes(state.total_speed_bps) + '/s',
        sources
      };
    } catch (error) {
      console.error(`Failed to fetch download details for ${hash}:`, error);
      return null;
    }
  }

  // Reactive statement to fetch data when fileHash is available or changes.
  $: if (fileHash) {
    isLoading = true;
    fetchDownloadDetailsWithReputation(fileHash).then((data) => {
      download = data;
      isLoading = false;
    });
  }
</script>

<div class="p-4">
  <h2 class="text-xl font-bold mb-2">Download Details</h2>
  {#if isLoading}
    <p>Loading download details...</p>
  {:else if download}
    <div class="mb-4">
      <p class="font-semibold">{fileName}</p>
      <progress class="w-full" value={download.progress} max="100" />
      <p class="text-sm text-gray-600">{download.progress}% at {download.totalSpeed}</p>
    </div>

    <h3 class="text-lg font-bold mb-2">Sources</h3>
    <ul class="space-y-2">
      {#each download.sources as source (source.id)}
        <li class="grid grid-cols-[auto_1fr_auto] items-center gap-3 p-2 rounded-md bg-gray-100">
          <ReputationTooltip peer={source.reputation} />
          <span class="font-mono text-sm truncate">{source.id}</span>
          <span class="text-sm font-medium">{source.speed}</span>
        </li>
      {/each}
    </ul>
  {/if}
</div>