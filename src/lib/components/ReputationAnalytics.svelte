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
      case 'FileTransferSuccess': return '📤';
      case 'FileTransferFailure': return '⚠️';
      case 'PaymentSuccess': return '🎉';
      case 'PaymentFailure': return '💥';
      case 'ConnectionEstablished': return '🔗';
      case 'ConnectionLost': return '🔌';
      case 'DhtQueryAnswered': return '💡';
      case 'StorageOffered': return '📦';
      case 'MaliciousBehaviorReport': return '🚩';
      case 'FileShared': return '🤝';
      default: return '⚙️';
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

  // Pie chart settings
  const pieRadius = 64;
  const pieStrokeWidth = 32;
  const circumference = 2 * Math.PI * pieRadius;
  // Keeping labels always visible; threshold no longer needed

  const colorByLevel: Record<TrustLevel, string> = {
    [TrustLevel.Trusted]: '#22c55e', // green-500
    [TrustLevel.High]: '#3b82f6',    // blue-500
    [TrustLevel.Medium]: '#eab308',  // yellow-500
    [TrustLevel.Low]: '#f97316',     // orange-500
    [TrustLevel.Unknown]: '#6b7280', // gray-500
  };

  const computeSlices = () => {
    let offset = 0;
    return trustLevelData.map(({ level, count, percentage }) => {
      const length = (percentage / 100) * circumference;
      const slice = {
        level,
        count,
        percentage,
        dasharray: `${length} ${circumference - length}`,
        dashoffset: `${circumference - offset}`,
        color: colorByLevel[level],
        // angles for label placement
        startAngle: (offset / circumference) * 2 * Math.PI,
        midAngle: ((offset + length / 2) / circumference) * 2 * Math.PI,
      };
      offset += length;
      return slice;
    });
  };

  const slices = computeSlices();

  // Tooltip state for pie chart
  let hoveredSlice: { level: TrustLevel; count: number } | null = null;
  let tooltipX = 0;
  let tooltipY = 0;
  function handleSliceEnter(slice: { level: TrustLevel; count: number }, event: MouseEvent) {
    hoveredSlice = { level: slice.level, count: slice.count };
    tooltipX = event.clientX;
    tooltipY = event.clientY;
  }
  function handleSliceMove(event: MouseEvent) {
    tooltipX = event.clientX;
    tooltipY = event.clientY;
  }
  function handleSliceLeave() {
    hoveredSlice = null;
  }

  // Layout constants for pie with horizontal gutters
  const base = (pieRadius + pieStrokeWidth) * 2;
  const gutter = 90;
  const totalW = base + gutter * 2;
  const totalH = base;
  const Cx = gutter + base / 2;
  const Cy = base / 2;

  // Pagination for Recent Events
  let currentEventPage = 1;
  const eventsPerPage = 3;
  const totalEventPages = Math.ceil(analytics.recentEvents.length / eventsPerPage);

  function goToEventPage(page: number) {
    if (page >= 1 && page <= totalEventPages) {
      currentEventPage = page;
    }
  }

  function nextEventPage() {
    if (currentEventPage < totalEventPages) currentEventPage++;
  }

  function prevEventPage() {
    if (currentEventPage > 1) currentEventPage--;
  }

  $: paginatedEvents = analytics.recentEvents.slice(
    (currentEventPage - 1) * eventsPerPage,
    currentEventPage * eventsPerPage
  );
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
          <p class="text-2xl font-bold text-purple-600">{(analytics.averageScore * 5).toFixed(1)}/5.0</p>
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

  <!-- Trust + Events Row -->
  <div class="grid grid-cols-1 lg:grid-cols-2 gap-6">
    <Card class="p-6 h-96 flex flex-col">
      <h3 class="text-lg font-semibold text-gray-900 mb-4">Trust Level Distribution</h3>
      <div class="flex-grow flex flex-col items-center justify-center gap-6">
      <!-- Pie Chart -->
      <div class="relative self-center">
        <svg width={totalW} height={totalH} viewBox={`0 0 ${totalW} ${totalH}`}>
          <g transform={`translate(${Cx}, ${Cy}) rotate(-90)`}>
            <!-- background circle -->
            <circle r={pieRadius} fill="none" stroke="#e5e7eb" stroke-width={pieStrokeWidth} />
            {#each slices as slice}
              <circle
                r={pieRadius}
                fill="none"
                stroke={slice.color}
                stroke-width={pieStrokeWidth}
                stroke-dasharray={slice.dasharray}
                stroke-dashoffset={slice.dashoffset}
                class="cursor-pointer"
                role="img"
                on:mouseenter={(e) => handleSliceEnter(slice, e)}
                on:mousemove={handleSliceMove}
                on:mouseleave={handleSliceLeave}
              >
              </circle>
            {/each}
          </g>
        </svg>
        <!-- Center label for a prettier donut feel -->
        <div class="absolute inset-0 flex items-center justify-center pointer-events-none">
          <div class="text-center">
            <div class="text-xs text-gray-500">Total</div>
            <div class="text-lg font-semibold text-gray-900">{analytics.totalPeers}</div>
          </div>
        </div>
        {#if hoveredSlice}
          <div class="pointer-events-none fixed z-50 px-2 py-1 rounded text-xs bg-gray-900 text-white shadow"
            style={`left:${tooltipX + 10}px; top:${tooltipY + 10}px;`}>
            {hoveredSlice.level}: {hoveredSlice.count}
          </div>
        {/if}
      </div>
        <!-- Legend under the pie -->
        <div class="grid grid-cols-2 sm:grid-cols-3 gap-3 items-end justify-items-center">
          {#each trustLevelData as { level }}
            <div class="flex items-center gap-2 justify-center">
              <span class="w-3 h-3 rounded-full" style={`background-color: ${colorByLevel[level]}`}></span>
              <span class="text-sm text-gray-700">{level}</span>
            </div>
          {/each}
        </div>
      </div>
    </Card>

    <!-- Recent Events Card (paginated) -->
    <Card class="p-6 h-96 flex flex-col">
      <h3 class="text-lg font-semibold text-gray-900 mb-4">Recent Events</h3>
      <div class="space-y-3 mb-4 flex-grow h-64 overflow-y-auto">
        {#each paginatedEvents as event}
          <div class="flex items-center justify-between p-3 hover:bg-gray-50 rounded-lg transition-colors">
            <div class="flex items-center space-x-3 min-w-0">
              <span class="text-lg">{getEventIcon(event.type)}</span>
              <div class="min-w-0">
                <p class="text-sm font-medium text-gray-900 truncate">
                  {formatEventType(event.type)}
                </p>
                <p class="text-xs text-gray-500 truncate" title={event.peerId}>
                  {event.peerId}
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
      <!-- Pagination -->
      <div class="flex items-center justify-center gap-2 mt-auto pt-4 border-t border-gray-100">
        <button
          on:click={prevEventPage}
          disabled={currentEventPage <= 1}
          class="px-3 py-1 text-sm border rounded disabled:opacity-50 disabled:cursor-not-allowed hover:bg-gray-50"
        >←</button>
        <input
          type="number"
          bind:value={currentEventPage}
          min="1"
          max={totalEventPages}
          on:input={(e) => goToEventPage(parseInt((e.target as HTMLInputElement)?.value || '1'))}
          class="w-12 px-2 py-1 text-sm text-center border rounded [appearance:textfield] [&::-webkit-outer-spin-button]:appearance-none [&::-webkit-inner-spin-button]:appearance-none"
        />
        <span class="text-sm text-gray-500">/ {totalEventPages}</span>
        <button
          on:click={nextEventPage}
          disabled={currentEventPage >= totalEventPages}
          class="px-3 py-1 text-sm border rounded disabled:opacity-50 disabled:cursor-not-allowed hover:bg-gray-50"
        >→</button>
      </div>
    </Card>
  </div>

  <!-- Top Performers -->
  <Card class="p-6">
    <h3 class="text-lg font-semibold text-gray-900 mb-4">Top Performers</h3>
    <div class="space-y-3">
      {#each analytics.topPerformers.slice(0, 5) as peer, index}
        <div class="flex sm:flex-row flex-col p-3 bg-gray-50 rounded-lg gap-3">
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
          <div class="flex items-center space-x-2 shrink-0 sm:flex-row flex-row sm:w-auto w-full sm:justify-end justify-start">
            <Badge class={getTrustLevelColor(peer.trustLevel).replace('bg-', 'bg-').replace('text-', 'text-')}>
              {peer.trustLevel}
            </Badge>
            <span class="text-sm font-semibold text-gray-900">
              {(peer.score * 5).toFixed(1)}/5.0 ⭐
            </span>
          </div>
          <div class="sm:hidden -mt-2"></div>
        </div>
      {/each}
    </div>
  </Card>

</div>