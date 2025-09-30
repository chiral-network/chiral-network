<script lang="ts">
  import { TrustLevel, type ReputationAnalytics } from '$lib/types/reputation';
  import Card from '$lib/components/ui/card.svelte';
  import Badge from '$lib/components/ui/badge.svelte';

  export let analytics: ReputationAnalytics;

  // Chart data for trust level distribution
  const trustLevelData = Object.entries(analytics.trustLevelDistribution).map(([level, count]) => ({
    level: level as TrustLevel,
    count,
    percentage: (count / analytics.totalPeers) * 100
  }));

  // Get color for trust level
  const getTrustLevelColor = (level: TrustLevel): string => {
    switch (level) {
      case TrustLevel.Trusted: return 'bg-green-500';
      case TrustLevel.High: return 'bg-blue-500';
      case TrustLevel.Medium: return 'bg-yellow-500';
      case TrustLevel.Low: return 'bg-orange-500';
      case TrustLevel.Unknown: return 'bg-gray-500';
      default: return 'bg-gray-500';
    }
  };

  // Format event type for display
  const formatEventType = (type: string): string => {
    return type.replace(/([A-Z])/g, ' $1').trim();
  };

  // Get event icon
  const getEventIcon = (type: string): string => {
    switch (type) {
      case 'FileTransferSuccess': return '📁✅';
      case 'FileTransferFailure': return '📁❌';
      case 'PaymentSuccess': return '💰✅';
      case 'PaymentFailure': return '💰❌';
      case 'ConnectionEstablished': return '🔗✅';
      case 'ConnectionLost': return '🔗❌';
      case 'DhtQueryAnswered': return '🔍✅';
      case 'StorageOffered': return '💾';
      case 'MaliciousBehaviorReport': return '⚠️';
      case 'FileShared': return '📤';
      default: return '📋';
    }
  };

  // Format timestamp
  const formatTime = (date: Date): string => {
    return new Intl.DateTimeFormat('ko-KR', {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit'
    }).format(date);
  };
</script>

<div class="space-y-6">
  <!-- Overview Cards -->
  <div class="grid grid-cols-1 md:grid-cols-4 gap-4">
    <Card class="p-4">
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-gray-500">Total Peers</p>
          <p class="text-2xl font-bold text-gray-900">{analytics.totalPeers}</p>
        </div>
        <div class="w-8 h-8 bg-blue-100 rounded-full flex items-center justify-center">
          <span class="text-blue-600 text-sm">👥</span>
        </div>
      </div>
    </Card>

    <Card class="p-4">
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-gray-500">Trusted Peers</p>
          <p class="text-2xl font-bold text-green-600">{analytics.trustedPeers}</p>
        </div>
        <div class="w-8 h-8 bg-green-100 rounded-full flex items-center justify-center">
          <span class="text-green-600 text-sm">🛡️</span>
        </div>
      </div>
    </Card>

    <Card class="p-4">
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-gray-500">Avg Score</p>
          <p class="text-2xl font-bold text-purple-600">{(analytics.averageScore * 100).toFixed(1)}%</p>
        </div>
        <div class="w-8 h-8 bg-purple-100 rounded-full flex items-center justify-center">
          <span class="text-purple-600 text-sm">⭐</span>
        </div>
      </div>
    </Card>

    <Card class="p-4">
      <div class="flex items-center justify-between">
        <div>
          <p class="text-sm text-gray-500">Trust Rate</p>
          <p class="text-2xl font-bold text-indigo-600">
            {analytics.totalPeers > 0 ? ((analytics.trustedPeers / analytics.totalPeers) * 100).toFixed(1) : 0}%
          </p>
        </div>
        <div class="w-8 h-8 bg-indigo-100 rounded-full flex items-center justify-center">
          <span class="text-indigo-600 text-sm">📊</span>
        </div>
      </div>
    </Card>
  </div>

  <!-- Trust Level Distribution Chart -->
  <Card class="p-6">
    <h3 class="text-lg font-semibold text-gray-900 mb-4">Trust Level Distribution</h3>
    <div class="space-y-3">
      {#each trustLevelData as { level, count, percentage }}
        <div class="flex items-center gap-3 flex-wrap">
          <div class="flex items-center gap-3 min-w-0">
            <div class="w-4 h-4 rounded-full {getTrustLevelColor(level)} flex-shrink-0"></div>
            <span class="text-sm font-medium text-gray-700 truncate" title={level}>{level}</span>
          </div>
          <div class="flex items-center gap-3 min-w-0 flex-1">
            <div class="bg-gray-200 rounded-full h-2 flex-1 min-w-[6rem] overflow-hidden">
              <div 
                class="h-2 rounded-full {getTrustLevelColor(level)}"
                style="width: {percentage}%"
              ></div>
            </div>
            <span class="text-sm text-gray-600 w-12 text-right shrink-0">{count}</span>
          </div>
        </div>
      {/each}
    </div>
  </Card>

  <!-- Top Performers -->
  <Card class="p-6">
    <h3 class="text-lg font-semibold text-gray-900 mb-4">Top Performers</h3>
    <div class="space-y-3">
      {#each analytics.topPerformers.slice(0, 5) as peer, index}
        <div class="flex items-center p-3 bg-gray-50 rounded-lg gap-3 flex-wrap">
          <div class="flex items-center space-x-3 min-w-0 flex-1">
            <div class="w-6 h-6 bg-gradient-to-br from-blue-500 to-purple-600 rounded-full flex items-center justify-center text-white text-xs font-bold">
              {index + 1}
            </div>
            <div class="min-w-0">
              <p class="text-sm font-medium text-gray-900 truncate" title={peer.peerId}>
                {peer.peerId}
              </p>
              <p class="text-xs text-gray-500">{peer.totalInteractions} interactions</p>
            </div>
          </div>
          <div class="flex items-center space-x-2 shrink-0">
            <Badge class={getTrustLevelColor(peer.trustLevel).replace('bg-', 'bg-').replace('text-', 'text-')}>
              {peer.trustLevel}
            </Badge>
            <span class="text-sm font-semibold text-gray-900">
              {(peer.score * 100).toFixed(1)}%
            </span>
          </div>
        </div>
      {/each}
    </div>
  </Card>

  <!-- Recent Events -->
  <Card class="p-6">
    <h3 class="text-lg font-semibold text-gray-900 mb-4">Recent Events</h3>
    <div class="space-y-3">
      {#each analytics.recentEvents.slice(0, 10) as event}
        <div class="flex items-center justify-between p-3 hover:bg-gray-50 rounded-lg transition-colors">
          <div class="flex items-center space-x-3">
            <span class="text-lg">{getEventIcon(event.type)}</span>
            <div>
              <p class="text-sm font-medium text-gray-900">
                {formatEventType(event.type)}
              </p>
              <p class="text-xs text-gray-500">
                {event.peerId.slice(0, 8)}...{event.peerId.slice(-4)}
              </p>
            </div>
          </div>
          <div class="flex items-center space-x-2">
            <span class="text-xs text-gray-500">{formatTime(event.timestamp)}</span>
            <div class="w-2 h-2 rounded-full" class:bg-green-500={event.impact > 0} class:bg-red-500={event.impact < 0} class:bg-gray-400={event.impact === 0}></div>
          </div>
        </div>
      {/each}
    </div>
  </Card>
</div>