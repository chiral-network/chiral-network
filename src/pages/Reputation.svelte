<script lang="ts">
  import { onMount } from 'svelte';
  import { t } from 'svelte-i18n';
  import { TrustLevel, type PeerReputation, type ReputationAnalytics, type ReputationEvent, EventType } from '$lib/types/reputation';
  import ReputationCard from '$lib/components/ReputationCard.svelte';
  import ReputationAnalyticsComponent from '$lib/components/ReputationAnalytics.svelte';
  import Card from '$lib/components/ui/card.svelte';
  import Button from '$lib/components/ui/button.svelte';

  // State
  let peers: PeerReputation[] = [];
  let analytics: ReputationAnalytics;
  let sortBy: 'score' | 'interactions' | 'lastSeen' = 'score';
  let searchQuery = '';
  let isLoading = true;
  let showAnalytics = true;
  let currentPage = 1;
  const peersPerPage = 8;

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

  // Sort options - will use reactive translations
  $: sortOptions = [
    { value: 'score', label: $t('reputation.sortOptions.score') },
    { value: 'interactions', label: $t('reputation.sortOptions.interactions') },
    { value: 'lastSeen', label: $t('reputation.sortOptions.lastSeen') }
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

  // Pagination
  $: totalPages = Math.ceil(filteredPeers.length / peersPerPage);
  $: paginatedPeers = filteredPeers.slice((currentPage - 1) * peersPerPage, currentPage * peersPerPage);
  $: if (currentPage > totalPages && totalPages > 0) {
    currentPage = totalPages;
  }

  // Reset to page 1 when filters or search change
  $: if (searchQuery || selectedTrustLevels || filterEncryptionSupported || minUptime || sortBy) {
    currentPage = 1;
  }

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
  <title>{$t('reputation.pageTitle')}</title>
</svelte:head>

<div class="space-y-6">
    <!-- Header -->
    <div class="mb-8">
      <div class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <h1 class="text-3xl font-bold text-gray-900">{$t('reputation.title')}</h1>
          <p class="mt-2 text-gray-600">{$t('reputation.subtitle')}</p>
        </div>
        <div class="flex flex-wrap gap-2 sm:justify-end">
          <Button on:click={refreshData} disabled={isLoading} variant="outline" class="w-full sm:w-auto">
            {isLoading ? $t('reputation.refreshing') : $t('reputation.refreshData')}
          </Button>
          <Button on:click={() => showAnalytics = !showAnalytics} variant="outline" class="w-full sm:w-auto">
            {showAnalytics ? $t('reputation.hideAnalytics') : $t('reputation.showAnalytics')}
          </Button>
        </div>
      </div>
    </div>

    {#if isLoading}
      <!-- Loading State -->
      <div class="flex items-center justify-center py-12">
        <div class="text-center">
          <div class="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-600 mx-auto"></div>
          <p class="mt-4 text-gray-600">{$t('reputation.loading')}</p>
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
        <h3 class="text-lg font-bold text-gray-900 mb-4">{$t('reputation.searchPeers')}</h3>
        <div>
          <input
            id="search"
            type="text"
            bind:value={searchQuery}
            placeholder={$t('reputation.searchPlaceholder')}
            class="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
          />
        </div>
      </Card>

      <!-- Filters and Sort Controls -->
      <div class="flex items-center justify-between mb-4 gap-2 flex-wrap">
        <!-- Filter Dropdown -->
        <div class="relative">
          <Button variant="outline" class="sm:w-auto" on:click={openFilters}>{$t('reputation.filters')}</Button>
          {#if isFilterOpen}
            <div use:clickOutside>
              <Card class="absolute top-full mt-2 p-6 w-72 z-10">
                <div class="space-y-6">
                  <!-- Trust Level Filter -->
                  <div>
                    <h4 class="font-medium text-gray-800 mb-3">{$t('reputation.trustLevel')}</h4>
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
                    <h4 class="font-medium text-gray-800 mb-3">{$t('reputation.encryption')}</h4>
                    <div class="space-y-2">
                      <label class="flex items-center gap-2 text-sm font-normal">
                        <input type="radio" bind:group={pendingFilterEncryptionSupported} value={null} />
                        {$t('reputation.any')}
                      </label>
                      <label class="flex items-center gap-2 text-sm font-normal">
                        <input type="radio" bind:group={pendingFilterEncryptionSupported} value={true} />
                        {$t('reputation.supported')}
                      </label>
                      <label class="flex items-center gap-2 text-sm font-normal">
                        <input type="radio" bind:group={pendingFilterEncryptionSupported} value={false} />
                        {$t('reputation.notSupported')}
                      </label>
                    </div>
                  </div>

                  <!-- Uptime Filter -->
                  <div>
                    <h4 class="font-medium text-gray-800 mb-3">{$t('reputation.minimumUptime')}</h4>
                    <div class="flex items-center gap-3">
                      <input type="range" min="0" max="100" bind:value={pendingMinUptime} class="w-full" />
                      <span class="text-sm font-medium w-12 text-right">{pendingMinUptime}%</span>
                    </div>
                  </div>
                </div>
                <!-- Action Buttons -->
                <div class="flex justify-between items-center mt-6 pt-4 border-t border-gray-200">
                  <Button variant="ghost" on:click={clearFilters}>{$t('reputation.clear')}</Button>
                  <Button on:click={applyFilters}>{$t('reputation.apply')}</Button>
                </div>
              </Card>
            </div>
          {/if}
        </div>

        <!-- Sort Dropdown -->
        <div class="flex items-center gap-2">
          <label for="sortBy" class="text-sm font-medium text-gray-700 sr-only">{$t('reputation.sortBy')}</label>
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

      <!-- Peer Cards Grid -->
      {#if filteredPeers.length === 0}
        <Card class="p-12 text-center">
          <div class="text-gray-400 text-6xl mb-4">🔍</div>
          <h3 class="text-lg font-medium text-gray-900 mb-2">{$t('reputation.noPeersFound')}</h3>
          <p class="text-gray-500">{$t('reputation.tryAdjusting')}</p>
        </Card>
      {:else}
        <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-6">
          {#each paginatedPeers as peer (peer.peerId)}
            <ReputationCard {peer} />
          {/each}
        </div>

        <!-- Pagination Controls -->
        {#if totalPages > 1}
          <div class="flex items-center justify-between mt-6 pt-6 border-t border-gray-200">
            <div class="text-sm text-gray-600">
              Showing {(currentPage - 1) * peersPerPage + 1}-{Math.min(currentPage * peersPerPage, filteredPeers.length)} of {filteredPeers.length} peers
            </div>
            <div class="flex items-center gap-2">
              <Button
                variant="outline"
                size="sm"
                on:click={() => currentPage = currentPage - 1}
                disabled={currentPage === 1}
              >
                Previous
              </Button>
              <div class="flex items-center gap-1">
                {#each Array(totalPages) as _, i}
                  {#if totalPages <= 7 || i === 0 || i === totalPages - 1 || (i >= currentPage - 2 && i <= currentPage)}
                    <Button
                      variant={currentPage === i + 1 ? 'default' : 'outline'}
                      size="sm"
                      class="w-10"
                      on:click={() => currentPage = i + 1}
                    >
                      {i + 1}
                    </Button>
                  {:else if i === 1 || i === totalPages - 2}
                    <span class="px-2">...</span>
                  {/if}
                {/each}
              </div>
              <Button
                variant="outline"
                size="sm"
                on:click={() => currentPage = currentPage + 1}
                disabled={currentPage === totalPages}
              >
                Next
              </Button>
            </div>
          </div>
        {/if}
      {/if}
    {/if}
</div>
