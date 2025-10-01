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
  let sortBy: 'score' | 'interactions' | 'lastSeen' = 'score';
  let searchQuery = '';
  let isLoading = true;
  let showAnalytics = true;

  // Filter states
  let isFilterOpen = false;
  let selectedTrustLevels: TrustLevel[] = [];
  let filterEncryptionSupported: boolean | null = null;
  let minUptime = 70;

  // Pending filter states for the dropdown
  let pendingSelectedTrustLevels: TrustLevel[] = [];
  let pendingFilterEncryptionSupported: boolean | null = null;
  let pendingMinUptime = 70;

  function openFilters() {
    // Sync pending state with applied state when opening
    pendingSelectedTrustLevels = [...selectedTrustLevels];
    pendingFilterEncryptionSupported = filterEncryptionSupported;
    pendingMinUptime = minUptime;
    isFilterOpen = true;
  }

  function applyFilters() {
    selectedTrustLevels = [...pendingSelectedTrustLevels];
    filterEncryptionSupported = pendingFilterEncryptionSupported;
    minUptime = pendingMinUptime;
    isFilterOpen = false;
  }

  function clearFilters() {
    pendingSelectedTrustLevels = [];
    pendingFilterEncryptionSupported = null;
    pendingMinUptime = 70;
  }

  // Action to detect clicks outside an element
  function clickOutside(node: HTMLElement) {
    const handleClick = (event: MouseEvent) => {
      if (node && !node.contains(event.target as Node) && isFilterOpen) {
        isFilterOpen = false;
      }
    };

    document.addEventListener('click', handleClick, true);

    return {
      destroy() {
        document.removeEventListener('click', handleClick, true);
      }
    };
  }

  // Trust level options for filter
  const trustLevelOptions: TrustLevel[] = [TrustLevel.Trusted, TrustLevel.High, TrustLevel.Medium, TrustLevel.Low, TrustLevel.Unknown];

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
      const matchesTrustLevel = selectedTrustLevels.length === 0 || selectedTrustLevels.includes(peer.trustLevel);
      const matchesSearch = searchQuery === '' || peer.peerId.toLowerCase().includes(searchQuery.toLowerCase());
      const matchesEncryption = filterEncryptionSupported === null || peer.metrics.encryptionSupported === filterEncryptionSupported;
      const matchesUptime = peer.metrics.uptime >= minUptime;

      return matchesTrustLevel && matchesSearch && matchesEncryption && matchesUptime;
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

<div class="space-y-6">
    <!-- Header -->
    <div class="mb-8">
      <div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h1 class="text-3xl font-bold text-gray-900">Reputation System</h1>
          <p class="mt-2 text-gray-600">Monitor peer trust levels and network health</p>
        </div>
        <div class="flex flex-wrap gap-2 sm:justify-end">
          <Button on:click={refreshData} disabled={isLoading} variant="outline" class="w-full sm:w-auto">
            {isLoading ? 'Refreshing...' : 'Refresh Data'}
          </Button>
          <Button on:click={() => showAnalytics = !showAnalytics} variant="outline" class="w-full sm:w-auto">
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

      <!-- Search Box -->
      <Card class="p-6 mb-4">
        <h3 class="text-lg font-bold text-gray-900 mb-4">Search Peers</h3>
        <div>
          <input
            id="search"
            type="text"
            bind:value={searchQuery}
            placeholder="Search by peer ID..."
            class="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
          />
        </div>
      </Card>

      <!-- Filters and Sort Controls -->
      <div class="flex items-center justify-between mb-4 gap-2 flex-wrap">
        <!-- Filter Dropdown -->
        <div class="relative">
          <Button variant="outline" class="sm:w-auto" on:click={openFilters}>Filters</Button>
          {#if isFilterOpen}
            <div use:clickOutside>
              <Card class="absolute top-full mt-2 p-6 w-72 z-10">
                <div class="space-y-6">
                  <!-- Trust Level Filter -->
                  <div>
                    <h4 class="font-medium text-gray-800 mb-3">Trust Level</h4>
                    <div class="space-y-2">
                      {#each trustLevelOptions as level}
                        <label class="flex items-center gap-2 text-sm font-normal">
                          <input type="checkbox" bind:group={pendingSelectedTrustLevels} value={level} />
                          {level}
                        </label>
                      {/each}
                    </div>
                  </div>

                  <!-- Encryption Filter -->
                  <div>
                    <h4 class="font-medium text-gray-800 mb-3">Encryption</h4>
                    <div class="space-y-2">
                      <label class="flex items-center gap-2 text-sm font-normal">
                        <input type="radio" bind:group={pendingFilterEncryptionSupported} value={null} />
                        Any
                      </label>
                      <label class="flex items-center gap-2 text-sm font-normal">
                        <input type="radio" bind:group={pendingFilterEncryptionSupported} value={true} />
                        Supported
                      </label>
                      <label class="flex items-center gap-2 text-sm font-normal">
                        <input type="radio" bind:group={pendingFilterEncryptionSupported} value={false} />
                        Not Supported
                      </label>
                    </div>
                  </div>

                  <!-- Uptime Filter -->
                  <div>
                    <h4 class="font-medium text-gray-800 mb-3">Minimum Uptime</h4>
                    <div class="flex items-center gap-3">
                      <input type="range" min="0" max="100" bind:value={pendingMinUptime} class="w-full" />
                      <span class="text-sm font-medium w-12 text-right">{pendingMinUptime}%</span>
                    </div>
                  </div>
                </div>
                <!-- Action Buttons -->
                <div class="flex justify-between items-center mt-6 pt-4 border-t border-gray-200">
                  <Button variant="ghost" on:click={clearFilters}>Clear</Button>
                  <Button on:click={applyFilters}>Apply</Button>
                </div>
              </Card>
            </div>
          {/if}
        </div>

        <!-- Sort Dropdown -->
        <div class="flex items-center gap-2">
          <label for="sortBy" class="text-sm font-medium text-gray-700 sr-only">Sort By</label>
          <div class="relative">
            <select
              id="sortBy"
              bind:value={sortBy}
              class="appearance-none bg-white border border-gray-300 rounded-md py-2 pl-3 pr-8 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
            >
              {#each sortOptions as option}
                <option value={option.value}>{option.label}</option>
              {/each}
            </select>
            <div class="pointer-events-none absolute inset-y-0 right-0 flex items-center px-2 text-gray-700">
              <svg class="fill-current h-4 w-4" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 20 20"><path d="M9.293 12.95l.707.707L15.657 8l-1.414-1.414L10 10.828 5.757 6.586 4.343 8z"/></svg>
            </div>
          </div>
        </div>
      </div>

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
