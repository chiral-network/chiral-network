<script lang="ts">
  import { type BlockchainReputationAnalytics } from '$lib/types/reputation';
  import Card from '$lib/components/ui/card.svelte';
  import Badge from '$lib/components/ui/badge.svelte';

  export let analytics: BlockchainReputationAnalytics;

  // Format connectivity status color
  const getConnectivityStatusColor = (status: string): string => {
    switch (status) {
      case 'Connected':
        return 'bg-green-100 text-green-800 border-green-200';
      case 'Disconnected':
        return 'bg-red-100 text-red-800 border-red-200';
      case 'Error':
        return 'bg-orange-100 text-orange-800 border-orange-200';
      default:
        return 'bg-gray-100 text-gray-800 border-gray-200';
    }
  };

  // Format percentage
  const formatPercentage = (value: number): string => {
    return `${Math.round(value * 100)}%`;
  };

  // Format score
  const formatScore = (score: number): string => {
    return score.toFixed(3);
  };
</script>

<Card class="p-6">
  <div class="flex items-center justify-between mb-6">
    <h2 class="text-xl font-bold text-gray-900 flex items-center gap-2">
      <div class="w-8 h-8 bg-blue-100 rounded-full flex items-center justify-center">
        <svg class="w-5 h-5 text-blue-600" fill="currentColor" viewBox="0 0 20 20">
          <path fill-rule="evenodd" d="M3 4a1 1 0 011-1h12a1 1 0 011 1v2a1 1 0 01-1 1H4a1 1 0 01-1-1V4zM3 10a1 1 0 011-1h6a1 1 0 011 1v6a1 1 0 01-1 1H4a1 1 0 01-1-1v-6zM14 9a1 1 0 00-1 1v6a1 1 0 001 1h2a1 1 0 001-1v-6a1 1 0 00-1-1h-2z" clip-rule="evenodd"/>
        </svg>
      </div>
      Blockchain Reputation Analytics
    </h2>
    <Badge class={`${getConnectivityStatusColor(analytics.blockchainConnectivityStatus)}`}>
      {analytics.blockchainConnectivityStatus}
    </Badge>
  </div>

  <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-6">
    <!-- Total Verified Peers -->
    <div class="text-center">
      <div class="w-16 h-16 bg-green-100 rounded-full flex items-center justify-center mx-auto mb-3">
        <svg class="w-8 h-8 text-green-600" fill="currentColor" viewBox="0 0 20 20">
          <path d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z"/>
        </svg>
      </div>
      <p class="text-2xl font-bold text-gray-900">{analytics.totalVerifiedPeers}</p>
      <p class="text-sm text-gray-500">Verified Peers</p>
    </div>

    <!-- Average Blockchain Score -->
    <div class="text-center">
      <div class="w-16 h-16 bg-blue-100 rounded-full flex items-center justify-center mx-auto mb-3">
        <svg class="w-8 h-8 text-blue-600" fill="currentColor" viewBox="0 0 20 20">
          <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.707-9.293a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z" clip-rule="evenodd"/>
        </svg>
      </div>
      <p class="text-2xl font-bold text-gray-900">{formatScore(analytics.averageBlockchainScore)}</p>
      <p class="text-sm text-gray-500">Avg Blockchain Score</p>
    </div>

    <!-- Verification Success Rate -->
    <div class="text-center">
      <div class="w-16 h-16 bg-yellow-100 rounded-full flex items-center justify-center mx-auto mb-3">
        <svg class="w-8 h-8 text-yellow-600" fill="currentColor" viewBox="0 0 20 20">
          <path fill-rule="evenodd" d="M6.267 3.455a3.066 3.066 0 001.745-.723 3.066 3.066 0 013.976 0 3.066 3.066 0 001.745.723 3.066 3.066 0 012.812 2.812c.051.643.304 1.254.723 1.745a3.066 3.066 0 010 3.976 3.066 3.066 0 00-.723 1.745 3.066 3.066 0 01-2.812 2.812 3.066 3.066 0 00-1.745.723 3.066 3.066 0 01-3.976 0 3.066 3.066 0 00-1.745-.723 3.066 3.066 0 01-2.812-2.812 3.066 3.066 0 00-.723-1.745 3.066 3.066 0 010-3.976 3.066 3.066 0 00.723-1.745 3.066 3.066 0 012.812-2.812zm7.44 5.252a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z" clip-rule="evenodd"/>
        </svg>
      </div>
      <p class="text-2xl font-bold text-gray-900">{formatPercentage(analytics.verificationSuccessRate)}</p>
      <p class="text-sm text-gray-500">Success Rate</p>
    </div>

    <!-- Recent Epochs -->
    <div class="text-center">
      <div class="w-16 h-16 bg-purple-100 rounded-full flex items-center justify-center mx-auto mb-3">
        <svg class="w-8 h-8 text-purple-600" fill="currentColor" viewBox="0 0 20 20">
          <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm1-12a1 1 0 10-2 0v4a1 1 0 00.293.707l2.828 2.829a1 1 0 101.415-1.415L11 9.586V6z" clip-rule="evenodd"/>
        </svg>
      </div>
      <p class="text-2xl font-bold text-gray-900">{analytics.recentEpochs.length}</p>
      <p class="text-sm text-gray-500">Recent Epochs</p>
    </div>
  </div>

  <!-- Recent Epochs Table -->
  {#if analytics.recentEpochs.length > 0}
    <div class="mt-8">
      <h3 class="text-lg font-semibold text-gray-900 mb-4">Recent Blockchain Epochs</h3>
      <div class="overflow-x-auto">
        <table class="min-w-full divide-y divide-gray-200">
          <thead class="bg-gray-50">
            <tr>
              <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Epoch ID</th>
              <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Events</th>
              <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Block</th>
              <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Timestamp</th>
              <th class="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase tracking-wider">Status</th>
            </tr>
          </thead>
          <tbody class="bg-white divide-y divide-gray-200">
            {#each analytics.recentEpochs.slice(0, 5) as epoch}
              <tr>
                <td class="px-6 py-4 whitespace-nowrap text-sm font-medium text-gray-900">
                  {epoch.epochId}
                </td>
                <td class="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                  {epoch.eventCount}
                </td>
                <td class="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                  {epoch.blockNumber ? epoch.blockNumber : 'N/A'}
                </td>
                <td class="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                  {new Date(epoch.timestamp * 1000).toLocaleString()}
                </td>
                <td class="px-6 py-4 whitespace-nowrap">
                  <Badge class={epoch.verified ? 'bg-green-100 text-green-800 border-green-200' : 'bg-gray-100 text-gray-800 border-gray-200'}>
                    {epoch.verified ? 'Verified' : 'Pending'}
                  </Badge>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </div>
  {/if}
</Card>
