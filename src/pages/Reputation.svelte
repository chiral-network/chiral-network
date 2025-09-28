<script lang="ts">
  import { onMount } from 'svelte';
  import { TrustLevel, type PeerReputation, type ReputationAnalytics, type ReputationEvent, EventType } from '$lib/types/reputation';
  import ReputationCard from '$lib/components/ReputationCard.svelte';
  import ReputationAnalyticsComponent from '$lib/components/ReputationAnalytics.svelte';
  import Card from '$lib/components/ui/card.svelte';
  import Badge from '$lib/components/ui/badge.svelte';
  import Button from '$lib/components/ui/button.svelte';

  // State
  let peers: PeerReputation[] = [];
  let analytics: ReputationAnalytics;
  let selectedTrustLevel: TrustLevel | 'All' = 'All';
  let sortBy: 'score' | 'interactions' | 'lastSeen' = 'score';
  let searchQuery = '';
  let isLoading = true;
  let showAnalytics = true;

  // Trust level options for filter
  const trustLevelOptions: (TrustLevel | 'All')[] = ['All', TrustLevel.Trusted, TrustLevel.High, TrustLevel.Medium, TrustLevel.Low, TrustLevel.Unknown];

  // Sort options
  const sortOptions = [
    { value: 'score', label: 'Reputation Score' },
    { value: 'interactions', label: 'Total Interactions' },
    { value: 'lastSeen', label: 'Last Seen' }
  ];

  // Generate mock data
  function generateMockData() {
    const mockPeers: PeerReputation[] = [];
    const mockEvents: ReputationEvent[] = [];
    
    // Generate random peer data
    for (let i = 0; i < 25; i++) {
      const peerId = `peer_${Math.random().toString(36).substr(2, 9)}`;
      const score = Math.random();
      const totalInteractions = Math.floor(Math.random() * 100) + 10;
      const successfulInteractions = Math.floor(totalInteractions * (0.6 + Math.random() * 0.4));
      
      const trustLevel = score >= 0.8 ? TrustLevel.Trusted :
                        score >= 0.6 ? TrustLevel.High :
                        score >= 0.4 ? TrustLevel.Medium :
                        score >= 0.2 ? TrustLevel.Low : TrustLevel.Unknown;

      const peer: PeerReputation = {
        peerId,
        trustLevel,
        score,
        totalInteractions,
        successfulInteractions,
        lastSeen: new Date(Date.now() - Math.random() * 7 * 24 * 60 * 60 * 1000), // Last 7 days
        reputationHistory: [],
        metrics: {
          averageLatency: Math.floor(Math.random() * 200) + 10,
          bandwidth: Math.floor(Math.random() * 100) + 10,
          uptime: Math.floor(Math.random() * 30) + 70,
          storageOffered: Math.floor(Math.random() * 1000) + 100,
          filesShared: Math.floor(Math.random() * 50) + 5,
          encryptionSupported: Math.random() > 0.3
        }
      };

      mockPeers.push(peer);
    }

    // Generate mock events
    const eventTypes = Object.values(EventType);
    for (let i = 0; i < 50; i++) {
      const event: ReputationEvent = {
        id: `event_${i}`,
        type: eventTypes[Math.floor(Math.random() * eventTypes.length)],
        peerId: mockPeers[Math.floor(Math.random() * mockPeers.length)].peerId,
        timestamp: new Date(Date.now() - Math.random() * 24 * 60 * 60 * 1000), // Last 24 hours
        data: {},
        impact: Math.random() > 0.5 ? Math.random() * 0.5 + 0.1 : -(Math.random() * 0.5 + 0.1)
      };
      mockEvents.push(event);
    }

    // Calculate analytics
    const totalPeers = mockPeers.length;
    const trustedPeers = mockPeers.filter(p => p.trustLevel === TrustLevel.Trusted).length;
    const averageScore = mockPeers.reduce((sum, p) => sum + p.score, 0) / totalPeers;
    const topPerformers = [...mockPeers].sort((a, b) => b.score - a.score).slice(0, 10);
    const trustLevelDistribution = Object.values(TrustLevel).reduce((acc, level) => {
      acc[level] = mockPeers.filter(p => p.trustLevel === level).length;
      return acc;
    }, {} as Record<TrustLevel, number>);

    analytics = {
      totalPeers,
      trustedPeers,
      averageScore,
      topPerformers,
      recentEvents: mockEvents.sort((a, b) => b.timestamp.getTime() - a.timestamp.getTime()),
      trustLevelDistribution
    };

    peers = mockPeers;
  }

  // Filter and sort peers
  $: filteredPeers = peers
    .filter(peer => {
      const matchesTrustLevel = selectedTrustLevel === 'All' || peer.trustLevel === selectedTrustLevel;
      const matchesSearch = searchQuery === '' || peer.peerId.toLowerCase().includes(searchQuery.toLowerCase());
      return matchesTrustLevel && matchesSearch;
    })
    .sort((a, b) => {
      switch (sortBy) {
        case 'score':
          return b.score - a.score;
        case 'interactions':
          return b.totalInteractions - a.totalInteractions;
        case 'lastSeen':
          return b.lastSeen.getTime() - a.lastSeen.getTime();
        default:
          return 0;
      }
    });

  onMount(() => {
    // Simulate loading
    setTimeout(() => {
      generateMockData();
      isLoading = false;
    }, 1000);
  });

  function refreshData() {
    isLoading = true;
    setTimeout(() => {
      generateMockData();
      isLoading = false;
    }, 500);
  }
</script>

<svelte:head>
  <title>Reputation System - Chiral Network</title>
</svelte:head>

<div class="min-h-screen bg-gray-50">
  <div class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
    <!-- Header -->
    <div class="mb-8">
      <div class="flex items-center justify-between">
        <div>
          <h1 class="text-3xl font-bold text-gray-900">Reputation System</h1>
          <p class="mt-2 text-gray-600">Monitor peer trust levels and network health</p>
        </div>
        <div class="flex items-center space-x-3">
          <Button on:click={refreshData} disabled={isLoading} variant="outline">
            {isLoading ? 'Refreshing...' : 'Refresh Data'}
          </Button>
          <Button on:click={() => showAnalytics = !showAnalytics} variant="outline">
            {showAnalytics ? 'Hide Analytics' : 'Show Analytics'}
          </Button>
        </div>
      </div>
    </div>

    {#if isLoading}
      <!-- Loading State -->
      <div class="flex items-center justify-center py-12">
        <div class="text-center">
          <div class="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-600 mx-auto"></div>
          <p class="mt-4 text-gray-600">Loading reputation data...</p>
        </div>
      </div>
    {:else}
      <!-- Analytics Section -->
      {#if showAnalytics && analytics}
        <div class="mb-8">
          <ReputationAnalyticsComponent {analytics} />
        </div>
      {/if}

      <!-- Filters and Controls -->
      <Card class="p-6 mb-6">
        <div class="flex flex-col sm:flex-row gap-4">
          <!-- Search -->
          <div class="flex-1">
            <label for="search" class="block text-sm font-medium text-gray-700 mb-2">Search Peers</label>
            <input
              id="search"
              type="text"
              bind:value={searchQuery}
              placeholder="Search by peer ID..."
              class="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
            />
          </div>

          <!-- Trust Level Filter -->
          <div class="sm:w-48">
            <label for="trustLevel" class="block text-sm font-medium text-gray-700 mb-2">Trust Level</label>
            <select
              id="trustLevel"
              bind:value={selectedTrustLevel}
              class="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
            >
              {#each trustLevelOptions as option}
                <option value={option}>{option}</option>
              {/each}
            </select>
          </div>

          <!-- Sort By -->
          <div class="sm:w-48">
            <label for="sortBy" class="block text-sm font-medium text-gray-700 mb-2">Sort By</label>
            <select
              id="sortBy"
              bind:value={sortBy}
              class="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
            >
              {#each sortOptions as option}
                <option value={option.value}>{option.label}</option>
              {/each}
            </select>
          </div>
        </div>
      </Card>

      <!-- Results Summary -->
      <div class="flex items-center justify-between mb-4">
        <p class="text-sm text-gray-600">
          Showing {filteredPeers.length} of {peers.length} peers
        </p>
        <div class="flex items-center space-x-2">
          <span class="text-sm text-gray-500">View:</span>
          <Badge variant="outline">Cards</Badge>
        </div>
      </div>

      <!-- Peer Cards Grid -->
      {#if filteredPeers.length === 0}
        <Card class="p-12 text-center">
          <div class="text-gray-400 text-6xl mb-4">🔍</div>
          <h3 class="text-lg font-medium text-gray-900 mb-2">No peers found</h3>
          <p class="text-gray-500">Try adjusting your search criteria or filters.</p>
        </Card>
      {:else}
        <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-6">
          {#each filteredPeers as peer (peer.peerId)}
            <ReputationCard {peer} />
          {/each}
        </div>
      {/if}
    {/if}
  </div>
</div>