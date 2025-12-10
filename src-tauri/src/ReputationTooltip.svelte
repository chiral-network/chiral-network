<script lang="ts" context="module">
  // Define the shape of the peer reputation object
  export interface PeerReputation {
    score: number;
    trustLevel: 'Trusted' | 'Medium' | 'Low';
    successful_interactions: number;
    total_interactions: number;
    uptime_percentage: number;
    last_seen: Date;
  }
</script>

<script lang="ts">
  import ReputationBadge from './ReputationBadge.svelte';
  import { timeAgo } from './time';
  
  // Props
  export let peer: PeerReputation;
</script>

<div class="group relative inline-block">
  <!-- The ReputationBadge that triggers the tooltip -->
  <ReputationBadge trustLevel={peer.trustLevel} score={peer.score} />

  <!-- The Tooltip itself -->
  <div
    class="
      absolute bottom-full left-1/2 z-20 mb-2 w-max -translate-x-1/2 
      transform rounded-lg bg-gray-800 px-3 py-2 text-sm text-white shadow-lg
      opacity-0 transition-opacity group-hover:opacity-100
      pointer-events-none group-hover:pointer-events-auto
    "
    role="tooltip"
  >
    <h4 class="mb-1 border-b border-gray-600 pb-1 font-bold">Peer Reputation</h4>
    <ul class="space-y-1 text-left">
      <li><strong>Score:</strong> {peer.score}</li>
      <li>
        <strong>Interactions:</strong>
        {peer.successful_interactions}/{peer.total_interactions} successful
      </li>
      <li><strong>Uptime:</strong> {peer.uptime_percentage.toFixed(1)}%</li>
      <li><strong>Last Seen:</strong> {timeAgo(peer.last_seen)}</li>
    </ul>
    <!-- Arrow for the tooltip -->
    <div class="absolute -bottom-1 left-1/2 h-2 w-2 -translate-x-1/2 rotate-45 bg-gray-800" />
  </div>
</div>