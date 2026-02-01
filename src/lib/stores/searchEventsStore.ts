/**
 * Search Events Store
 *
 * This module provides a reactive Svelte store that listens to typed search events
 * from the Rust backend and maintains the state of active file searches.
 *
 * Usage:
 * ```typescript
 * import { searchStore, subscribeToSearchEvents } from '$lib/stores/searchEventsStore';
 *
 * // Subscribe to events when component mounts
 * onMount(async () => {
 *   const unsubscribe = await subscribeToSearchEvents();
 *   return unsubscribe;
 * });
 *
 * // Access search state reactively
 * $: activeSearches = $searchStore.searches;
 * ```
 */

import { writable, derived, get, type Readable } from "svelte/store";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// ============================================================================
// Type Definitions (matching Rust types)
// ============================================================================

export type SearchStatus =
  | "idle"
  | "searching"
  | "complete"
  | "timeout";

export interface SeederInfo {
  index: number;
  peerId: string;
  walletAddress?: string;
  defaultPricePerMb?: number;
  pricePerMb?: number;
  supportedProtocols?: string[];
  protocolDetails?: Record<string, unknown>;
  hasGeneralInfo: boolean;
  hasFileInfo: boolean;
}

export interface Search {
  fileHash: string;
  status: SearchStatus;

  // Basic metadata (from DHT record)
  fileName?: string;
  fileSize?: number;
  createdAt?: number;
  mimeType?: string;

  // Provider/Seeder tracking
  providers: string[];
  seeders: Map<string, SeederInfo>;
  totalSeeders: number;

  // Timing
  startedAt?: number;
  completedAt?: number;
  durationMs?: number;

  // Timeout info
  partialSeeders?: number;
  missingCount?: number;
}

export interface SearchEventPayload {
  type: string;
  [key: string]: unknown;
}

// ============================================================================
// Store State
// ============================================================================

interface SearchStoreState {
  searches: Map<string, Search>;
  activeCount: number;
  lastEventTimestamp: number;
}

const initialState: SearchStoreState = {
  searches: new Map(),
  activeCount: 0,
  lastEventTimestamp: 0,
};

// ============================================================================
// Writable Store
// ============================================================================

function createSearchStore() {
  const { subscribe, set, update } = writable<SearchStoreState>(initialState);

  return {
    subscribe,

    /**
     * Handle a search event from the backend
     */
    handleEvent: (event: SearchEventPayload) => {
      update((state) => {
        const searches = new Map(state.searches);
        const timestamp = Date.now();

        switch (event.type) {
          case "started":
            handleStartedEvent(searches, event);
            break;
          case "metadata_found":
            handleMetadataFoundEvent(searches, event);
            break;
          case "providers_found":
            handleProvidersFoundEvent(searches, event);
            break;
          case "seeder_general_info":
            handleSeederGeneralInfoEvent(searches, event);
            break;
          case "seeder_file_info":
            handleSeederFileInfoEvent(searches, event);
            break;
          case "complete":
            handleCompleteEvent(searches, event);
            break;
          case "timeout":
            handleTimeoutEvent(searches, event);
            break;
          default:
            console.warn("Unknown search event type:", event.type);
        }

        return {
          searches,
          ...calculateStats(searches),
          lastEventTimestamp: timestamp,
        };
      });
    },

    /**
     * Get a specific search by file hash
     */
    getSearch: (fileHash: string): Search | undefined => {
      return get({ subscribe }).searches.get(fileHash);
    },

    /**
     * Remove a search from the store
     */
    removeSearch: (fileHash: string) => {
      update((state) => {
        const searches = new Map(state.searches);
        searches.delete(fileHash);
        return {
          searches,
          ...calculateStats(searches),
          lastEventTimestamp: Date.now(),
        };
      });
    },

    /**
     * Clear all completed and timed out searches
     */
    clearFinished: () => {
      update((state) => {
        const searches = new Map(state.searches);
        for (const [hash, search] of searches.entries()) {
          if (search.status === "complete" || search.status === "timeout") {
            searches.delete(hash);
          }
        }
        return {
          searches,
          ...calculateStats(searches),
          lastEventTimestamp: Date.now(),
        };
      });
    },

    /**
     * Reset the entire store
     */
    reset: () => set(initialState),
  };
}

// ============================================================================
// Event Handlers
// ============================================================================

function handleStartedEvent(searches: Map<string, Search>, event: SearchEventPayload) {
  const fileHash = event.fileHash as string;
  const search: Search = {
    fileHash,
    status: "searching",
    providers: [],
    seeders: new Map(),
    totalSeeders: 0,
    startedAt: event.timestamp as number,
  };
  searches.set(fileHash, search);
}

function handleMetadataFoundEvent(searches: Map<string, Search>, event: SearchEventPayload) {
  const fileHash = event.fileHash as string;
  let search = searches.get(fileHash);

  if (!search) {
    // Create search if it doesn't exist (in case we missed the started event)
    search = {
      fileHash,
      status: "searching",
      providers: [],
      seeders: new Map(),
      totalSeeders: 0,
    };
    searches.set(fileHash, search);
  }

  search.fileName = event.fileName as string;
  search.fileSize = event.fileSize as number;
  search.createdAt = event.createdAt as number;
  search.mimeType = event.mimeType as string | undefined;
}

function handleProvidersFoundEvent(searches: Map<string, Search>, event: SearchEventPayload) {
  const fileHash = event.fileHash as string;
  const providers = event.providers as string[];
  const count = event.count as number;

  let search = searches.get(fileHash);
  if (!search) {
    search = {
      fileHash,
      status: "searching",
      providers: [],
      seeders: new Map(),
      totalSeeders: 0,
    };
    searches.set(fileHash, search);
  }

  // Only update if we have more providers than before
  if (count > search.providers.length) {
    search.providers = providers;
    search.totalSeeders = count;

    // Initialize seeder slots
    for (let i = 0; i < providers.length; i++) {
      const peerId = providers[i];
      if (!search.seeders.has(peerId)) {
        search.seeders.set(peerId, {
          index: i,
          peerId,
          hasGeneralInfo: false,
          hasFileInfo: false,
        });
      }
    }
  }
}

function handleSeederGeneralInfoEvent(searches: Map<string, Search>, event: SearchEventPayload) {
  const fileHash = event.fileHash as string;
  const peerId = event.peerId as string;

  const search = searches.get(fileHash);
  if (!search) return;

  let seeder = search.seeders.get(peerId);
  if (!seeder) {
    seeder = {
      index: event.seederIndex as number,
      peerId,
      hasGeneralInfo: false,
      hasFileInfo: false,
    };
    search.seeders.set(peerId, seeder);
  }

  seeder.walletAddress = event.walletAddress as string;
  seeder.defaultPricePerMb = event.defaultPricePerMb as number;
  seeder.hasGeneralInfo = true;
}

function handleSeederFileInfoEvent(searches: Map<string, Search>, event: SearchEventPayload) {
  const fileHash = event.fileHash as string;
  const peerId = event.peerId as string;

  const search = searches.get(fileHash);
  if (!search) return;

  let seeder = search.seeders.get(peerId);
  if (!seeder) {
    seeder = {
      index: event.seederIndex as number,
      peerId,
      hasGeneralInfo: false,
      hasFileInfo: false,
    };
    search.seeders.set(peerId, seeder);
  }

  seeder.pricePerMb = event.pricePerMb as number | undefined;
  seeder.supportedProtocols = event.supportedProtocols as string[];
  seeder.protocolDetails = event.protocolDetails as Record<string, unknown>;
  seeder.hasFileInfo = true;
}

function handleCompleteEvent(searches: Map<string, Search>, event: SearchEventPayload) {
  const fileHash = event.fileHash as string;

  const search = searches.get(fileHash);
  if (!search) return;

  search.status = "complete";
  search.completedAt = Date.now();
  search.durationMs = event.durationMs as number;
  search.totalSeeders = event.totalSeeders as number;
}

function handleTimeoutEvent(searches: Map<string, Search>, event: SearchEventPayload) {
  const fileHash = event.fileHash as string;

  const search = searches.get(fileHash);
  if (!search) return;

  search.status = "timeout";
  search.completedAt = Date.now();
  search.partialSeeders = event.partialSeeders as number;
  search.missingCount = event.missingCount as number;
}

// ============================================================================
// Stats Calculation
// ============================================================================

function calculateStats(searches: Map<string, Search>) {
  let activeCount = 0;

  for (const search of searches.values()) {
    if (search.status === "searching") {
      activeCount++;
    }
  }

  return { activeCount };
}

// ============================================================================
// Store Instance
// ============================================================================

export const searchStore = createSearchStore();

// ============================================================================
// Derived Stores
// ============================================================================

/**
 * Get only active searches
 */
export const activeSearches: Readable<Search[]> = derived(
  searchStore,
  ($store) =>
    Array.from($store.searches.values()).filter((s) => s.status === "searching")
);

/**
 * Get completed searches
 */
export const completedSearches: Readable<Search[]> = derived(
  searchStore,
  ($store) =>
    Array.from($store.searches.values())
      .filter((s) => s.status === "complete")
      .sort((a, b) => (b.completedAt || 0) - (a.completedAt || 0))
);

/**
 * Get timed out searches
 */
export const timedOutSearches: Readable<Search[]> = derived(
  searchStore,
  ($store) =>
    Array.from($store.searches.values()).filter((s) => s.status === "timeout")
);

// ============================================================================
// Event Subscription
// ============================================================================

let unlistenFunctions: UnlistenFn[] = [];

/**
 * Subscribe to all search events from the backend
 * Call this once when your app starts, typically in App.svelte's onMount
 *
 * @returns A function to unsubscribe from all events
 */
export async function subscribeToSearchEvents(): Promise<() => void> {
  // Unsubscribe from previous listeners if any
  await unsubscribeFromSearchEvents();

  try {
    // Subscribe to the generic event channel that receives all search events
    const unlisten = await listen<SearchEventPayload>(
      "search:event",
      (event) => {
        searchStore.handleEvent(event.payload);
      }
    );

    unlistenFunctions.push(unlisten);

    return unsubscribeFromSearchEvents;
  } catch (error) {
    console.error("Failed to subscribe to search events:", error);
    throw error;
  }
}

/**
 * Unsubscribe from all search events
 */
export async function unsubscribeFromSearchEvents(): Promise<void> {
  for (const unlisten of unlistenFunctions) {
    unlisten();
  }
  unlistenFunctions = [];
}
