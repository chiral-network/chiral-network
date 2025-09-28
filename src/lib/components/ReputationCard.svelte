<script lang="ts">
  import { TrustLevel, type PeerReputation } from '$lib/types/reputation';
  import Badge from '$lib/components/ui/badge.svelte';
  import Card from '$lib/components/ui/card.svelte';

  export let peer: PeerReputation;

  // Trust level colors
  const getTrustLevelColor = (level: TrustLevel): string => {
    switch (level) {
      case TrustLevel.Trusted:
        return 'bg-green-100 text-green-800 border-green-200';
      case TrustLevel.High:
        return 'bg-blue-100 text-blue-800 border-blue-200';
      case TrustLevel.Medium:
        return 'bg-yellow-100 text-yellow-800 border-yellow-200';
      case TrustLevel.Low:
        return 'bg-orange-100 text-orange-800 border-orange-200';
      case TrustLevel.Unknown:
      default:
        return 'bg-gray-100 text-gray-800 border-gray-200';
    }
  };

  // Score color based on value
  const getScoreColor = (score: number): string => {
    if (score >= 0.8) return 'text-green-600';
    if (score >= 0.6) return 'text-blue-600';
    if (score >= 0.4) return 'text-yellow-600';
    if (score >= 0.2) return 'text-orange-600';
    return 'text-red-600';
  };

  // Format last seen time
  const formatLastSeen = (date: Date): string => {
    const now = new Date();
    const diff = now.getTime() - date.getTime();
    const minutes = Math.floor(diff / 60000);
    const hours = Math.floor(minutes / 60);
    const days = Math.floor(hours / 24);

    if (days > 0) return `${days}d ago`;
    if (hours > 0) return `${hours}h ago`;
    if (minutes > 0) return `${minutes}m ago`;
    return 'Just now';
  };

  // Calculate success rate
  const successRate = peer.totalInteractions > 0 
    ? (peer.successfulInteractions / peer.totalInteractions) * 100 
    : 0;
</script>

<Card class="p-4 hover:shadow-md transition-shadow">
  <div class="flex items-start justify-between mb-3">
    <div class="flex items-center space-x-2">
      <div class="w-8 h-8 bg-gradient-to-br from-blue-500 to-purple-600 rounded-full flex items-center justify-center text-white text-sm font-bold">
        {peer.peerId.slice(0, 2).toUpperCase()}
      </div>
      <div>
        <h3 class="font-semibold text-gray-900 text-sm">
          {peer.peerId.slice(0, 8)}...{peer.peerId.slice(-4)}
        </h3>
        <p class="text-xs text-gray-500">{formatLastSeen(peer.lastSeen)}</p>
      </div>
    </div>
    <Badge class={getTrustLevelColor(peer.trustLevel)}>
      {peer.trustLevel}
    </Badge>
  </div>

  <div class="space-y-3">
    <!-- Score and Success Rate -->
    <div class="flex justify-between items-center">
      <div>
        <p class="text-xs text-gray-500">Reputation Score</p>
        <p class={`text-lg font-bold ${getScoreColor(peer.score)}`}>
          {(peer.score * 100).toFixed(1)}%
        </p>
      </div>
      <div class="text-right">
        <p class="text-xs text-gray-500">Success Rate</p>
        <p class="text-sm font-semibold text-gray-900">
          {successRate.toFixed(1)}%
        </p>
      </div>
    </div>

    <!-- Progress Bar -->
    <div class="w-full bg-gray-200 rounded-full h-2">
      <div 
        class="h-2 rounded-full transition-all duration-300"
        class:bg-green-500={peer.score >= 0.8}
        class:bg-blue-500={peer.score >= 0.6 && peer.score < 0.8}
        class:bg-yellow-500={peer.score >= 0.4 && peer.score < 0.6}
        class:bg-orange-500={peer.score >= 0.2 && peer.score < 0.4}
        class:bg-red-500={peer.score < 0.2}
        style="width: {peer.score * 100}%"
      ></div>
    </div>

    <!-- Metrics Grid -->
    <div class="grid grid-cols-2 gap-3 text-xs">
      <div class="flex justify-between">
        <span class="text-gray-500">Interactions:</span>
        <span class="font-medium">{peer.totalInteractions}</span>
      </div>
      <div class="flex justify-between">
        <span class="text-gray-500">Latency:</span>
        <span class="font-medium">{peer.metrics.averageLatency}ms</span>
      </div>
      <div class="flex justify-between">
        <span class="text-gray-500">Bandwidth:</span>
        <span class="font-medium">{peer.metrics.bandwidth} Mbps</span>
      </div>
      <div class="flex justify-between">
        <span class="text-gray-500">Uptime:</span>
        <span class="font-medium">{peer.metrics.uptime}%</span>
      </div>
    </div>

    <!-- Encryption Support -->
    <div class="flex items-center justify-between pt-2 border-t border-gray-100">
      <span class="text-xs text-gray-500">Encryption</span>
      <div class="flex items-center space-x-1">
        {#if peer.metrics.encryptionSupported}
          <div class="w-2 h-2 bg-green-500 rounded-full"></div>
          <span class="text-xs text-green-600 font-medium">Supported</span>
        {:else}
          <div class="w-2 h-2 bg-red-500 rounded-full"></div>
          <span class="text-xs text-red-600 font-medium">Not Supported</span>
        {/if}
      </div>
    </div>
  </div>
</Card>