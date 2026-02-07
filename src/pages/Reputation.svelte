<script lang="ts">
  import { onMount } from "svelte";
  import { t } from "svelte-i18n";
  import {
    TrustLevel,
    type PeerReputation,
    type ReputationAnalytics,
  } from "$lib/types/reputation";
  import ReputationCard from "$lib/components/ReputationCard.svelte";
  import ReputationAnalyticsComponent from "$lib/components/ReputationAnalytics.svelte";
  import Card from "$lib/components/ui/card.svelte";
  import Button from "$lib/components/ui/button.svelte";
  import PeerSelectionService, {
    type PeerMetrics as BackendPeerMetrics,
  } from "$lib/services/peerSelectionService";
  import { invoke } from "@tauri-apps/api/core";
  import { debounce } from "$lib/utils/debounce";

  // LocalStorage keys for persisted UI state
  const STORAGE_KEY_SHOW_ANALYTICS = "chiral.reputation.showAnalytics";
  // Load persisted UI toggles from localStorage
  function loadPersistedToggles() {
    if (typeof window === "undefined")
      return { showAnalytics: true };

    try {
      const storedAnalytics = window.localStorage.getItem(
        STORAGE_KEY_SHOW_ANALYTICS,
      );

      return {
        showAnalytics:
          storedAnalytics !== null ? storedAnalytics === "true" : true,
      };
    } catch (e) {
      console.warn("Failed to load persisted UI toggles:", e);
      return { showAnalytics: true };
    }
  }

  // Persist UI toggle to localStorage
  function persistToggle(key: string, value: boolean) {
    if (typeof window === "undefined") return;

    try {
      window.localStorage.setItem(key, String(value));
    } catch (e) {
      console.warn("Failed to persist UI toggle:", e);
    }
  }

  const persistedToggles = loadPersistedToggles();

  // State
  let peers: PeerReputation[] = [];
  let analytics: ReputationAnalytics = {
    totalPeers: 0,
    trustedPeers: 0,
    averageScore: 0,
    topPerformers: [],
    recentEvents: [],
    trustLevelDistribution: {
      [TrustLevel.Trusted]: 0,
      [TrustLevel.High]: 0,
      [TrustLevel.Medium]: 0,
      [TrustLevel.Low]: 0,
      [TrustLevel.Unknown]: 0,
    },
  };
  let sortBy: "score" | "interactions" | "lastSeen" = "score";
  let searchQuery = "";
  let debouncedSearchQuery = ""; // Debounced version for filtering
  let isLoading = true;
  
  // Cache peer scores to prevent fluctuations during refreshes
  const peerScoreCache = new Map<string, { score: number; trustLevel: TrustLevel; timestamp: number }>();
  const SCORE_CACHE_TTL = 5000; // 5 seconds
  let showAnalytics = persistedToggles.showAnalytics;
  let currentPage = 1;
  const peersPerPage = 8;

  // Filter states
  let isFilterOpen = false;
  let selectedTrustLevels: TrustLevel[] = [];
  let filterEncryptionSupported: boolean | null = null;
  let minUptime = 0;

  // Pending filter states for the dropdown
  let pendingSelectedTrustLevels: TrustLevel[] = [];
  let pendingFilterEncryptionSupported: boolean | null = null;
  let pendingMinUptime = 0;

  // Previous filter/sort values for detecting actual changes
  let prevSelectedTrustLevels: TrustLevel[] = [];
  let prevFilterEncryptionSupported: boolean | null = null;
  let prevMinUptime = 0;
  let prevSortBy: "score" | "interactions" | "lastSeen" = "score";
  let prevDebouncedSearchQuery = "";

  // Debounced search handler
  const updateDebouncedSearch = debounce((query: string) => {
    debouncedSearchQuery = query;
  }, 300);

  // Computed variables for pagination

  // Watch search query and update debounced version
  $: updateDebouncedSearch(searchQuery);

  // Persist UI toggles when they change
  // Persist UI toggles when they change (consolidated)
  $: {
    persistToggle(STORAGE_KEY_SHOW_ANALYTICS, showAnalytics);
  }

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
    pendingMinUptime = 0;
  }

  // Close filter dropdown on escape key
  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Escape" && isFilterOpen) {
      isFilterOpen = false;
    }
  }

  // Action to detect clicks outside an element
  function clickOutside(node: HTMLElement) {
    const handleClick = (event: MouseEvent) => {
      if (node && !node.contains(event.target as Node) && isFilterOpen) {
        isFilterOpen = false;
      }
    };

    document.addEventListener("click", handleClick, true);

    return {
      destroy() {
        document.removeEventListener("click", handleClick, true);
      },
    };
  }

  // Trust level options for filter
  const trustLevelOptions: TrustLevel[] = [
    TrustLevel.Trusted,
    TrustLevel.High,
    TrustLevel.Medium,
    TrustLevel.Low,
    TrustLevel.Unknown,
  ];

  // Sort options - will use reactive translations
  $: sortOptions = [
    { value: "score", label: $t("reputation.sortOptions.score") },
    { value: "interactions", label: $t("reputation.sortOptions.interactions") },
    { value: "lastSeen", label: $t("reputation.sortOptions.lastSeen") },
  ];

  // Map backend metrics to UI PeerReputation[] and analytics
  async function loadPeersFromBackend() {
    console.log('[SYNC] loadPeersFromBackend() called');
    try {
      // Use the new method that gets metrics for ALL connected DHT peers
      // This ensures we show all peers, even those without transfer history
      const metrics: BackendPeerMetrics[] =
        await PeerSelectionService.getConnectedPeerMetrics();
      console.log(`[STATS] Loading ${metrics.length} peers from backend`);

      // Load backend metrics into ReputationStore for persistence and frontend tracking
      const repStore = (await import('$lib/reputationStore')).default.getInstance();
      repStore.loadFromBackendMetrics(metrics.map(m => ({
        peer_id: m.peer_id,
        successful_transfers: m.successful_transfers,
        failed_transfers: m.failed_transfers,
        latency_ms: m.latency_ms,
        last_seen: m.last_seen
      })));

      // Fetch reputation verdicts sequentially with delays to prevent DHT overload
      // Loading all peers at once causes DHT disconnections
      const mappedPeers: PeerReputation[] = [];

      for (let i = 0; i < metrics.length; i++) {
        const m = metrics[i];
        
        // Check cache first to prevent score fluctuations
        const now = Date.now();
        const cached = peerScoreCache.get(m.peer_id);
        let score: number;
        let trustLevel: TrustLevel;
        
        if (cached && (now - cached.timestamp) < SCORE_CACHE_TTL) {
          // Use cached score if recent (within 5 seconds)
          score = cached.score;
          trustLevel = cached.trustLevel;
        } else {
          // Recalculate score
          score = PeerSelectionService.compositeScoreFromMetrics(m);
          
          // Determine trust level based on score
          trustLevel = score >= 0.75 ? TrustLevel.Trusted :  // 2+ successful transfers
                      score >= 0.6 ? TrustLevel.High :
                      score >= 0.4 ? TrustLevel.Medium :
                      score >= 0.2 ? TrustLevel.Low : TrustLevel.Unknown;
          
          console.log(`[TARGET] Peer ${m.peer_id.substring(0,15)}... score: ${score.toFixed(3)} (${(score*5).toFixed(1)}/5.0) -> ${trustLevel}`);
          
          // Cache the score
          peerScoreCache.set(m.peer_id, { score, trustLevel, timestamp: now });
        }
        
        let totalInteractions = Math.max(1, m.transfer_count);
        let successfulInteractions = Math.min(totalInteractions, m.successful_transfers);

        console.log(
          `[STATS] Peer ${m.peer_id.substring(0, 20)}... - transfers: ${m.successful_transfers}/${m.transfer_count}, ` +
          `success_rate: ${m.success_rate.toFixed(2)}, reliability: ${m.reliability_score.toFixed(2)}, ` +
          `composite: ${score.toFixed(2)}, stars: ${(score * 5).toFixed(1)}/5.0, ` +
          `cached: ${cached && (now - cached.timestamp) < SCORE_CACHE_TTL ? 'yes' : 'no'}`
        );

        mappedPeers.push({
          peerId: m.peer_id,
          trustLevel,
          score,
          totalInteractions,
          successfulInteractions,
          lastSeen: new Date((m.last_seen || 0) * 1000),
          reputationHistory: [],
          metrics: {
            averageLatency: typeof m.latency_ms === 'number' ? m.latency_ms : 0,
            bandwidth: typeof m.bandwidth_kbps === 'number' ? Math.round(m.bandwidth_kbps / 1024) : 0,
            uptime: Math.round(m.uptime_score * 100),
            storageOffered: 0,
            filesShared: 0,
            encryptionSupported: !!m.encryption_support
          }
        });
        
        // No delay needed since we're not querying DHT anymore
      }

      // Build analytics
      const totalPeers = mappedPeers.length;
      const trustedPeers = mappedPeers.filter(
        (p) => p.trustLevel === TrustLevel.Trusted,
      ).length;
      const averageScore =
        totalPeers > 0
          ? mappedPeers.reduce((sum, p) => sum + p.score, 0) / totalPeers
          : 0;
      const topPerformers = [...mappedPeers]
        .sort((a, b) => b.score - a.score)
        .slice(0, 10);
      // Build trust level distribution deterministically from the known options
      const trustLevelDistribution = trustLevelOptions.reduce(
        (acc, level) => {
          acc[level] = mappedPeers.filter(
            (p) => p.trustLevel === level,
          ).length;
          return acc;
        },
        {
          [TrustLevel.Trusted]: 0,
          [TrustLevel.High]: 0,
          [TrustLevel.Medium]: 0,
          [TrustLevel.Low]: 0,
          [TrustLevel.Unknown]: 0,
        } as Record<TrustLevel, number>,
      );

      analytics = {
        totalPeers,
        trustedPeers,
        averageScore,
        topPerformers,
        recentEvents: [],
        trustLevelDistribution,
      };

      peers = mappedPeers;

      // If no peers from backend, try to load from ReputationStore
      if (peers.length === 0) {
        console.log('[STATS] No peers from backend, checking ReputationStore...');
        const repStore = (await import('$lib/reputationStore')).default.getInstance();
        const allStoredPeers = repStore.getAllPeers();
        
        const storedPeers: PeerReputation[] = [];
        for (const [peerId, rep] of allStoredPeers) {
          const score = repStore.composite(peerId);
          const totalInteractions = Math.max(1, rep.alpha + rep.beta);
          const successfulInteractions = rep.alpha;
          
          const trustLevel = score >= 0.75 ? TrustLevel.Trusted :
                            score >= 0.6 ? TrustLevel.High :
                            score >= 0.4 ? TrustLevel.Medium :
                            score >= 0.2 ? TrustLevel.Low : TrustLevel.Unknown;
          
          storedPeers.push({
            peerId,
            trustLevel,
            score,
            totalInteractions,
            successfulInteractions,
            lastSeen: new Date(rep.lastSeenMs),
            reputationHistory: [],
            metrics: {
              averageLatency: rep.rttMsEMA,
              bandwidth: 0,
              uptime: repStore.freshScore(peerId) * 100,
              storageOffered: 0,
              filesShared: 0,
              encryptionSupported: false
            }
          });
        }
        
        if (storedPeers.length > 0) {
          console.log(`[STATS] Loaded ${storedPeers.length} peers from ReputationStore`);
          peers = storedPeers;
          
          // Rebuild analytics
          const totalPeers = storedPeers.length;
          const trustedPeers = storedPeers.filter(p => p.trustLevel === TrustLevel.Trusted).length;
          const averageScore = totalPeers > 0
            ? storedPeers.reduce((sum, p) => sum + p.score, 0) / totalPeers
            : 0;
          const topPerformers = [...storedPeers].sort((a, b) => b.score - a.score).slice(0, 10);
          const trustLevelDistribution = trustLevelOptions.reduce(
            (acc, level) => {
              acc[level] = storedPeers.filter(p => p.trustLevel === level).length;
              return acc;
            },
            {
              [TrustLevel.Trusted]: 0,
              [TrustLevel.High]: 0,
              [TrustLevel.Medium]: 0,
              [TrustLevel.Low]: 0,
              [TrustLevel.Unknown]: 0,
            } as Record<TrustLevel, number>,
          );
          
          analytics = {
            totalPeers,
            trustedPeers,
            averageScore,
            topPerformers,
            recentEvents: [],
            trustLevelDistribution,
          };
        }
      }
    } catch (e) {
      console.error("Failed to load peer metrics", e);
      peers = [];
      analytics = {
        totalPeers: 0,
        trustedPeers: 0,
        averageScore: 0,
        topPerformers: [],
        recentEvents: [],
        trustLevelDistribution: {
          [TrustLevel.Trusted]: 0,
          [TrustLevel.High]: 0,
          [TrustLevel.Medium]: 0,
          [TrustLevel.Low]: 0,
          [TrustLevel.Unknown]: 0,
        },
      };
    }
  }

  // Attempt to trigger latency collection by connecting to known peers, when available
  async function probePeerLatencies() {
    try {
      const connectedPeers = await invoke<string[]>("get_dht_connected_peers");
      if (Array.isArray(connectedPeers) && connectedPeers.length > 0) {
        // Nudge DHT to interact, which will allow libp2p ping to update latency
        // Avoid spamming: connect to a small subset
        const sample = connectedPeers.slice(
          0,
          Math.min(5, connectedPeers.length),
        );
        await Promise.allSettled(
          sample.map((p) => invoke("connect_to_peer", { peerId: p })),
        );
      }
    } catch (e) {
      // Best-effort; ignore errors
      console.debug("probePeerLatencies ignored", e);
    }
  }

  // Filter and sort peers
  $: filteredPeers = peers
    .filter((peer) => {
      const matchesTrustLevel =
        selectedTrustLevels.length === 0 ||
        selectedTrustLevels.includes(peer.trustLevel);
      const matchesSearch =
        debouncedSearchQuery === "" ||
        peer.peerId.toLowerCase().includes(debouncedSearchQuery.toLowerCase());
      const matchesEncryption =
        filterEncryptionSupported === null ||
        peer.metrics.encryptionSupported === filterEncryptionSupported;
      const matchesUptime = peer.metrics.uptime >= minUptime;

      return (
        matchesTrustLevel && matchesSearch && matchesEncryption && matchesUptime
      );
    })
    .sort((a, b) => {
      switch (sortBy) {
        case "score":
          return b.score - a.score;
        case "interactions":
          return b.totalInteractions - a.totalInteractions;
        case "lastSeen":
          return b.lastSeen.getTime() - a.lastSeen.getTime();
        default:
          return 0;
      }
    });

  // Pagination
  $: totalPages = Math.ceil(filteredPeers.length / peersPerPage);
  $: paginatedPeers = filteredPeers.slice(
    (currentPage - 1) * peersPerPage,
    currentPage * peersPerPage,
  );
  $: if (currentPage > totalPages && totalPages > 0) {
    currentPage = totalPages;
  }

  // Reset to page 1 only when filters or sort ACTUALLY change (not on every reactive update)
  $: {
    const filtersChanged =
      JSON.stringify(selectedTrustLevels) !==
        JSON.stringify(prevSelectedTrustLevels) ||
      filterEncryptionSupported !== prevFilterEncryptionSupported ||
      minUptime !== prevMinUptime ||
      sortBy !== prevSortBy ||
      debouncedSearchQuery !== (prevDebouncedSearchQuery || "");

    if (filtersChanged) {
      currentPage = 1;
      prevSelectedTrustLevels = [...selectedTrustLevels];
      prevFilterEncryptionSupported = filterEncryptionSupported;
      prevMinUptime = minUptime;
      prevSortBy = sortBy;
      prevDebouncedSearchQuery = debouncedSearchQuery;
    }
  }

  onMount(() => {
    loadPeersFromBackend();
    // Best-effort latency probe and follow-up refresh
    probePeerLatencies();
    // Refresh after a short delay to pick up new latency
    setTimeout(() => {
      loadPeersFromBackend();
    }, 1500);
    // Periodic refresh to keep data live
    const interval = setInterval(() => {
      loadPeersFromBackend();
    }, 10000);

    // Add escape key listener
    window.addEventListener("keydown", handleKeydown);

    isLoading = false;
    return () => {
      clearInterval(interval);
      window.removeEventListener("keydown", handleKeydown);
    };
  });

  async function refreshData() {
    isLoading = true;
    await loadPeersFromBackend();
    isLoading = false;
  }
</script>

<svelte:head>
  <title>{$t("reputation.pageTitle")}</title>
</svelte:head>

<div class="space-y-6">
  <!-- Header -->
  <div class="mb-8">
    <div
      class="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between"
    >
      <div>
        <h1 class="text-3xl font-bold">{$t("reputation.title")}</h1>
        <p class="text-muted-foreground mt-2">{$t("reputation.subtitle")}</p>
      </div>
      <div class="flex flex-wrap gap-2 sm:justify-end">
        <Button
          on:click={refreshData}
          disabled={isLoading}
          variant="outline"
          class="w-full sm:w-auto"
        >
          {isLoading
            ? $t("reputation.refreshing")
            : $t("reputation.refreshData")}
        </Button>
        <Button
          on:click={() => (showAnalytics = !showAnalytics)}
          variant="outline"
          class="w-full sm:w-auto"
        >
          {showAnalytics
            ? $t("reputation.hideAnalytics")
            : $t("reputation.showAnalytics")}
        </Button>
      </div>
    </div>
  </div>

  {#if isLoading}
    <!-- Loading State -->
    <div class="flex items-center justify-center py-12">
      <div class="text-center">
        <div
          class="animate-spin rounded-full h-12 w-12 border-b-2 border-blue-600 mx-auto"
        ></div>
        <p class="mt-4 text-gray-600">{$t("reputation.loading")}</p>
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
      <h3 class="text-lg font-bold text-gray-900 mb-4">
        {$t("reputation.searchPeers")}
      </h3>
      <div>
        <input
          id="search"
          type="text"
          bind:value={searchQuery}
          placeholder={$t("reputation.searchPlaceholder")}
          class="w-full px-3 py-2 border border-gray-300 rounded-md focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent"
        />
      </div>
    </Card>

    <!-- Filters and Sort Controls -->
    <div class="flex items-center justify-between mb-4 gap-2 flex-wrap">
      <!-- Filter Dropdown -->
      <div class="relative" use:clickOutside>
        <Button
          variant="outline"
          class="sm:w-auto"
          on:click={() =>
            isFilterOpen ? (isFilterOpen = false) : openFilters()}
          aria-haspopup="true"
          aria-expanded={isFilterOpen}>{$t("reputation.filters")}</Button
        >
        {#if isFilterOpen}
          <Card class="absolute top-full mt-2 p-6 w-72 z-10">
            <div class="space-y-6">
              <!-- Trust Level Filter -->
              <div>
                <h4 class="font-medium text-gray-800 mb-3">
                  {$t("reputation.trustLevel")}
                </h4>
                <div class="space-y-2">
                  {#each trustLevelOptions as level}
                    <label class="flex items-center gap-2 text-sm font-normal">
                      <input
                        type="checkbox"
                        bind:group={pendingSelectedTrustLevels}
                        value={level}
                      />
                      {$t(`reputation.trustLevels.${level}`)}
                    </label>
                  {/each}
                </div>
              </div>

              <!-- Encryption Filter -->
              <div>
                <h4 class="font-medium text-gray-800 mb-3">
                  {$t("reputation.encryption")}
                </h4>
                <div class="space-y-2">
                  <label class="flex items-center gap-2 text-sm font-normal">
                    <input
                      type="radio"
                      bind:group={pendingFilterEncryptionSupported}
                      value={null}
                    />
                    {$t("reputation.any")}
                  </label>
                  <label class="flex items-center gap-2 text-sm font-normal">
                    <input
                      type="radio"
                      bind:group={pendingFilterEncryptionSupported}
                      value={true}
                    />
                    {$t("reputation.supported")}
                  </label>
                  <label class="flex items-center gap-2 text-sm font-normal">
                    <input
                      type="radio"
                      bind:group={pendingFilterEncryptionSupported}
                      value={false}
                    />
                    {$t("reputation.notSupported")}
                  </label>
                </div>
              </div>

              <!-- Uptime Filter -->
              <div>
                <h4 class="font-medium text-gray-800 mb-3">
                  {$t("reputation.minimumUptime")}
                </h4>
                <div class="flex items-center gap-3">
                  <input
                    type="range"
                    min="0"
                    max="100"
                    bind:value={pendingMinUptime}
                    class="w-full"
                  />
                  <span class="text-sm font-medium w-12 text-right"
                    >{pendingMinUptime}%</span
                  >
                </div>
              </div>
            </div>
            <!-- Action Buttons -->
            <div
              class="flex justify-between items-center mt-6 pt-4 border-t border-gray-200"
            >
              <Button variant="ghost" on:click={clearFilters}
                >{$t("reputation.clear")}</Button
              >
              <Button on:click={applyFilters}>{$t("reputation.apply")}</Button>
            </div>
          </Card>
        {/if}
      </div>

      <!-- Sort Dropdown -->
      <div class="flex items-center gap-2">
        <label for="sortBy" class="text-sm font-medium text-gray-700 sr-only"
          >{$t("reputation.sortBy")}</label
        >
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
          <div
            class="pointer-events-none absolute inset-y-0 right-0 flex items-center px-2 text-gray-700"
          >
            <svg
              class="fill-current h-4 w-4"
              xmlns="http://www.w3.org/2000/svg"
              viewBox="0 0 20 20"
              ><path
                d="M9.293 12.95l.707.707L15.657 8l-1.414-1.414L10 10.828 5.757 6.586 4.343 8z"
              /></svg
            >
          </div>
        </div>
      </div>
    </div>

    <!-- Peer Cards Grid -->
    {#if filteredPeers.length === 0}
      <Card class="p-12 text-center">
        <div class="text-gray-400 text-6xl mb-4">[SEARCH]</div>
        <h3 class="text-lg font-medium text-gray-900 mb-2">
          {$t("reputation.noPeersFound")}
        </h3>
        <p class="text-gray-500">{$t("reputation.tryAdjusting")}</p>
      </Card>
    {:else}
      <div
        class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-6"
      >
        {#each paginatedPeers as peer (peer.peerId)}
          <ReputationCard {peer} />
        {/each}
      </div>

      <!-- Pagination Controls -->
      {#if totalPages > 1}
        <div
          class="flex items-center justify-between mt-6 pt-6 border-t border-gray-200"
        >
          <div class="text-sm text-gray-600">
            {$t("reputation.pagination.showing", {
              start: (currentPage - 1) * peersPerPage + 1,
              end: Math.min(currentPage * peersPerPage, filteredPeers.length),
              total: filteredPeers.length,
            })}
          </div>
          <div class="flex items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              on:click={() => (currentPage = currentPage - 1)}
              disabled={currentPage === 1}
            >
              {$t("reputation.pagination.previous")}
            </Button>
            <div class="flex items-center gap-1">
              {#each Array(totalPages) as _, i}
                {#if totalPages <= 7 || i === 0 || i === totalPages - 1 || (i >= currentPage - 2 && i <= currentPage)}
                  <Button
                    variant={currentPage === i + 1 ? "default" : "outline"}
                    size="sm"
                    class="w-10"
                    on:click={() => (currentPage = i + 1)}
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
              on:click={() => (currentPage = currentPage + 1)}
              disabled={currentPage === totalPages}
            >
              {$t("reputation.pagination.next")}
            </Button>
          </div>
        </div>
      {/if}
    {/if}
  {/if}
</div>
