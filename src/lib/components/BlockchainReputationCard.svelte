<script lang="ts">
  import { type BlockchainReputationData, VerificationStatus } from '$lib/types/reputation';
  import Badge from '$lib/components/ui/badge.svelte';
  import Card from '$lib/components/ui/card.svelte';

  export let blockchainReputation: BlockchainReputationData;

  // Verification status colors
  const getVerificationStatusColor = (status: VerificationStatus): string => {
    switch (status) {
      case VerificationStatus.Verified:
        return 'bg-green-100 text-green-800 border-green-200';
      case VerificationStatus.Pending:
        return 'bg-yellow-100 text-yellow-800 border-yellow-200';
      case VerificationStatus.Failed:
        return 'bg-red-100 text-red-800 border-red-200';
      case VerificationStatus.NotAvailable:
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

  // Format last verified time
  const formatLastVerified = (date: Date): string => {
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

  // Calculate score percentage
  const scorePercentage = Math.round(blockchainReputation.score * 100);
</script>

<Card class="p-4 border-l-4 border-blue-500">
  <div class="flex items-center justify-between mb-3">
    <h3 class="text-lg font-semibold text-gray-900 flex items-center gap-2">
      <div class="w-6 h-6 bg-blue-100 rounded-full flex items-center justify-center">
        <svg class="w-4 h-4 text-blue-600" fill="currentColor" viewBox="0 0 20 20">
          <path fill-rule="evenodd" d="M10 18a8 8 0 100-16 8 8 0 000 16zm3.707-9.293a1 1 0 00-1.414-1.414L9 10.586 7.707 9.293a1 1 0 00-1.414 1.414l2 2a1 1 0 001.414 0l4-4z" clip-rule="evenodd"/>
        </svg>
      </div>
      Blockchain Reputation
    </h3>
    <Badge class={`${getVerificationStatusColor(blockchainReputation.verificationStatus)}`}>
      {blockchainReputation.verificationStatus}
    </Badge>
  </div>

  <div class="space-y-4">
    <!-- Score Display -->
    <div class="flex items-center justify-between">
      <div>
        <p class="text-sm text-gray-500">Blockchain Score</p>
        <p class={`text-2xl font-bold ${getScoreColor(blockchainReputation.score)}`}>
          {blockchainReputation.score.toFixed(3)}
        </p>
      </div>
      <div class="text-right">
        <p class="text-sm text-gray-500">Percentage</p>
        <p class={`text-lg font-semibold ${getScoreColor(blockchainReputation.score)}`}>
          {scorePercentage}%
        </p>
      </div>
    </div>

    <!-- Progress Bar -->
    <div class="w-full bg-gray-200 rounded-full h-2">
      <div 
        class="h-2 rounded-full transition-all duration-300"
        class:bg-green-500={blockchainReputation.score >= 0.8}
        class:bg-blue-500={blockchainReputation.score >= 0.6 && blockchainReputation.score < 0.8}
        class:bg-yellow-500={blockchainReputation.score >= 0.4 && blockchainReputation.score < 0.6}
        class:bg-orange-500={blockchainReputation.score >= 0.2 && blockchainReputation.score < 0.4}
        class:bg-red-500={blockchainReputation.score < 0.2}
        style={`width: ${scorePercentage}%`}
      ></div>
    </div>

    <!-- Blockchain Metrics -->
    <div class="grid grid-cols-2 gap-4 text-sm">
      <div class="flex justify-between">
        <span class="text-gray-500">Epochs:</span>
        <span class="font-medium">{blockchainReputation.epochCount}</span>
      </div>
      <div class="flex justify-between">
        <span class="text-gray-500">Events:</span>
        <span class="font-medium">{blockchainReputation.totalEvents}</span>
      </div>
      <div class="flex justify-between">
        <span class="text-gray-500">Last Verified:</span>
        <span class="font-medium">{formatLastVerified(blockchainReputation.lastVerified)}</span>
      </div>
      <div class="flex justify-between">
        <span class="text-gray-500">Verified:</span>
        <div class="flex items-center space-x-1">
          {#if blockchainReputation.verified}
            <div class="w-2 h-2 bg-green-500 rounded-full"></div>
            <span class="text-xs text-green-600 font-medium">Yes</span>
          {:else}
            <div class="w-2 h-2 bg-red-500 rounded-full"></div>
            <span class="text-xs text-red-600 font-medium">No</span>
          {/if}
        </div>
      </div>
    </div>

    <!-- Recent Epochs -->
    {#if blockchainReputation.recentEpochs.length > 0}
      <div class="pt-3 border-t border-gray-100">
        <h4 class="text-sm font-medium text-gray-700 mb-2">Recent Epochs</h4>
        <div class="space-y-2 max-h-32 overflow-y-auto">
          {#each blockchainReputation.recentEpochs.slice(0, 3) as epoch}
            <div class="flex items-center justify-between text-xs bg-gray-50 p-2 rounded">
              <div>
                <span class="font-medium">Epoch {epoch.epochId}</span>
                <span class="text-gray-500 ml-2">{epoch.eventCount} events</span>
              </div>
              <div class="flex items-center space-x-1">
                {#if epoch.verified}
                  <div class="w-1.5 h-1.5 bg-green-500 rounded-full"></div>
                {:else}
                  <div class="w-1.5 h-1.5 bg-gray-400 rounded-full"></div>
                {/if}
              </div>
            </div>
          {/each}
        </div>
      </div>
    {/if}
  </div>
</Card>
