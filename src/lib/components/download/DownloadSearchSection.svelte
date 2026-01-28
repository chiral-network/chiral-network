<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import Card from "$lib/components/ui/card.svelte";
  import Input from "$lib/components/ui/input.svelte";
  import Label from "$lib/components/ui/label.svelte";
  import Button from "$lib/components/ui/button.svelte";
  import {
    Search,
    X,
    History,
    RotateCcw,
    AlertCircle,
    CheckCircle2,
  } from "lucide-svelte";
  import { createEventDispatcher, onDestroy, onMount } from "svelte";
  import { get } from "svelte/store";
  import { t } from "svelte-i18n";
  import { dhtService } from "$lib/dht";
  import type {
    CompleteFileMetadata,
    DhtFileRecord,
    FileMetadata,
    SeederCompleteMetadata,
    SeederFileInfo,
    SeederGeneralInfo,
  } from "$lib/dht";
  import SearchResultCard from "./SearchResultCard.svelte";
  import {
    dhtSearchHistory,
    type SearchHistoryEntry,
    type SearchStatus,
  } from "$lib/stores/searchHistory";
  import PeerSelectionModal from "./PeerSelectionModal.svelte";
  import { type ProtocolDetails } from "$lib/types/protocols";
  import { costFromPricePerMb } from "$lib/utils/pricing";
  import { extractInfoHashFromTorrentBytes } from "$lib/utils/torrentInfoHash";
  import { extractInfoHashFromMagnet } from "$lib/utils/magnetInfoHash";
  import { Protocol } from "$lib/services/contentProtocols/types";
  import { showToast } from "$lib/toast";
  import { PROTOCOL_BADGES, type ProgressiveSearchState } from "$lib/stores";

  type ToastType = "success" | "error" | "info" | "warning";
  type DownloadIntent = {
    fullMetadata: CompleteFileMetadata;
    selectedPeer: string;
    selectedProtocol: Protocol;
    price: number;
  };

  function deriveCompleteFileMetadata(
    state: ProgressiveSearchState,
  ): CompleteFileMetadata | null {
    if (!state.basicMetadata) {
      return null;
    }

    // !TODO: this is wrong, timestamp should be last updated seedergeneral info, but not currently passed from backend
    const now = Date.now();

    const dhtRecord: DhtFileRecord = {
      ...state.basicMetadata,
    };

    const seederInfo: Record<string, SeederCompleteMetadata> = {};

    for (const s of state.seeders) {
      // Require enough info to satisfy the "Complete" schema (general + fileSpecific)
      if (!s.hasGeneralInfo || !s.hasFileInfo) continue;
      if (!s.walletAddress) continue;

      const general: SeederGeneralInfo = {
        peerId: s.peerId,
        walletAddress: s.walletAddress,
        defaultPricePerMb: s.pricePerMb ?? 0,
        timestamp: now,
      };

      const fileSpecific: SeederFileInfo = {
        peerId: s.peerId,
        fileHash: state.basicMetadata.fileHash,
        pricePerMb: s.pricePerMb,
        supportedProtocols: s.protocols ?? [],
        protocolDetails: s.protocolDetails as ProtocolDetails,
        timestamp: now,
      };

      seederInfo[s.peerId] = { general, fileSpecific };
    }

    return { dhtRecord, seederInfo };
  }

  const dispatch = createEventDispatcher<{
    download: DownloadIntent;
  }>();
  const tr = (key: string, params?: Record<string, unknown>) =>
    (get(t) as any)(key, params);

  const DEV = import.meta.env.DEV;

  const SEARCH_TIMEOUT_MS = 10_000;

  let searchHash = $state("");
  let searchMode = $state<
    "merkle_hash" | "magnet" | "torrent" | "ed2k" | "ftp"
  >("merkle_hash");
  let torrentFileInput = $state<HTMLInputElement>();
  let torrentFileName = $state<string | null>(null);
  let hasSearched = $state(false);
  let latestStatus = $state<SearchStatus>("pending");
  let searchError = $state<string | null>(null);
  let lastSearchDuration = $state(0);
  let searchStartedAtMs = $state<number | null>(null);
  let searchCancelTimeoutId = $state<ReturnType<typeof setTimeout> | null>(
    null,
  );
  let currentSearchId = $state(0);
  let historyEntries = $state<SearchHistoryEntry[]>([]);
  let activeHistoryId = $state<string | null>(null);
  let showHistoryDropdown = $state(false);
  let warnedMissingMetadata = $state(false);

  // Peer selection modal state
  let showPeerSelectionModal = $state(false);

  // Torrent confirmation state
  let pendingTorrentIdentifier = $state<string | null>(null);
  let pendingTorrentBytes = $state<number[] | null>(null);
  let pendingTorrentType = $state<"magnet" | "file" | null>(null);

  // Progressive search state
  let progressiveSearchState = $state<ProgressiveSearchState>({
    status: "idle",
    basicMetadata: null,
    providers: [],
    seeders: [],
  });
  let isSearching = $derived(progressiveSearchState.status === "searching");

  const completeFileMetadata = $derived(
    deriveCompleteFileMetadata(progressiveSearchState),
  );

  function normalizeProtocol(value: unknown): Protocol | null {
    if (typeof value !== "string") return null;
    const upper = value.toUpperCase();
    if ((Object.values(Protocol) as string[]).includes(upper)) {
      return upper as Protocol;
    }
    return Protocol.UNKNOWN;
  }

  function normalizeProtocolDetails(details: unknown): ProtocolDetails | undefined {
    if (!details || typeof details !== "object") return undefined;

    const normalized: Record<string, unknown> = {};
    for (const [key, value] of Object.entries(details)) {
      const normalizedKey = normalizeProtocol(key);
      if (normalizedKey && normalizedKey !== Protocol.UNKNOWN) {
        normalized[normalizedKey] = value;
      } else {
        normalized[key] = value;
      }
    }

    return normalized as ProtocolDetails;
  }

  let availableProtocolIds = $derived.by(() => {
    const protocolSet = new Set<Protocol>();

    for (const seeder of progressiveSearchState.seeders) {
      seeder.protocols?.forEach((p) => protocolSet.add(p));
    }

    return [...protocolSet].sort((a, b) => (a > b ? 1 : -1));
  });

  $effect(() => {
    if (progressiveSearchState.status !== "idle" && !progressiveSearchState.basicMetadata) {
      if (!warnedMissingMetadata) {
        showToast("Search status isn't idle but no fileHash", "warning");
        warnedMissingMetadata = true;
      }
      return;
    }

    warnedMissingMetadata = false;
  });

  // Debug peer selection modal state
  $effect(() => {
    if (!DEV) return;
    console.log(
      "[DownloadSearchSection] showPeerSelectionModal:",
      showPeerSelectionModal,
    );
  });

  let availableProtocols = $derived.by(() => {
    return availableProtocolIds
      .map((protocol) => {
        const config = PROTOCOL_BADGES[protocol];

        return {
          id: config?.id ?? protocol,
          name: config?.name ?? protocol.toUpperCase(),
          icon: config?.icon,
          colorClass: config?.colorClass,
        };
      })
      .sort((a, b) => (a.id > b.id ? 1 : -1));
  });

  // need to reset progressiveState
  async function stopActiveSearch() {
    // Invalidate any in-flight async work
    currentSearchId += 1;

    if (searchCancelTimeoutId) {
      clearTimeout(searchCancelTimeoutId);
      searchCancelTimeoutId = null;
    }

    // Stop consuming progressive events; backend may still finish its search.
    await cleanupProgressiveEventListeners();
  }

  export async function cancelSearch() {
    await stopActiveSearch();

    progressiveSearchState.status = "idle";
    latestStatus = "pending";

    pushMessage("Search cancelled", "info", 2000);
  }

  export async function handleFileNotFound(fileHash: string) {
    const expectedHash =
      progressiveSearchState.basicMetadata?.fileHash ?? searchHash.trim();
    if (!expectedHash || expectedHash !== fileHash) return;

    const startedAt = searchStartedAtMs;
    await stopActiveSearch();

    if (typeof startedAt === "number") {
      lastSearchDuration = Math.round(performance.now() - startedAt);
    }

    progressiveSearchState.status = "idle";
    latestStatus = "not_found";
    hasSearched = true;
    searchError = null;

    if (searchMode === "merkle_hash" && activeHistoryId) {
      dhtSearchHistory.updateEntry(activeHistoryId, {
        status: "not_found",
        errorMessage: tr("download.search.status.notFoundDetail"),
        elapsedMs: lastSearchDuration > 0 ? lastSearchDuration : undefined,
      });
    }

    pushMessage(
      tr("download.search.status.notFoundNotification"),
      "warning",
      6000,
    );
  }

  // Event listener cleanup functions
  let eventUnlisteners = $state<Array<() => void>>([]);

  const unsubscribe = dhtSearchHistory.subscribe((entries) => {
    historyEntries = entries;
    if (entries.length > 0) {
      // 1. Always set the active ID from the most recent entry for the history dropdown.
      activeHistoryId = entries[0].id;

      // 2. Control the main UI state based on whether a search has been initiated in this session.
      if (!hasSearched) {
        // If it's a fresh load (hasSearched is false):
        // Keep the input clear, and the result panel empty.
        searchHash = "";
        latestStatus = "pending";
        searchError = null;
      } else {
        // If the user has searched in this session, ensure the current search results are displayed.
        const entry =
          entries.find((e) => e.id === activeHistoryId) || entries[0];
        if (entry) {
          latestStatus = entry.status;
          searchError = entry.errorMessage ?? null;
          searchHash = entry.hash;
        }
      }
    } else {
      activeHistoryId = null;
      // On empty history, ensure the main state is also reset.
      if (!hasSearched) {
        searchHash = "";
        latestStatus = "pending";
        searchError = null;
      }
    }
  });

  onMount(() => {
    document.addEventListener("click", handleClickOutside);
    if (DEV) {
      console.log("[DownloadSearchSection] mounted", {
        searchMode,
        isSearching,
        hasSearched,
        latestStatus,
      });
    }
  });

  onDestroy(() => {
    document.removeEventListener("click", handleClickOutside);
    unsubscribe();
    cleanupProgressiveEventListeners();
  });

  function pushMessage(
    message: string,
    type: ToastType = "info",
    duration = 4000,
  ) {
    showToast(message, type, duration);
  }

  function clearSearch() {
    searchHash = "";
    torrentFileName = null;
  }

  function handleTorrentFileSelect(event: Event) {
    const target = event.target as HTMLInputElement;
    const file = target.files?.[0];
    if (file && file.name.endsWith(".torrent")) {
      // For Tauri, we'll handle this differently in the download function
      torrentFileName = file.name;
    } else {
      torrentFileName = null;
      pushMessage("Please select a valid .torrent file", "warning");
    }
  }

  function hydrateFromEntry(entry: SearchHistoryEntry | undefined) {
    if (!entry) {
      latestStatus = "pending";
      searchError = null;
      return;
    }

    latestStatus = entry.status;
    searchError = entry.errorMessage ?? null;
    hasSearched = true;
    searchHash = entry.hash;
    lastSearchDuration = entry.elapsedMs ?? 0;
  }

  // Setup progressive search event listeners
  // Uses the new unified search:* event channels from AppEventBus
  async function setupProgressiveEventListeners() {
    // Clean up any existing listeners
    await cleanupProgressiveEventListeners();

    const unlisteners: Array<() => void> = [];

    // Listen to search:started event
    unlisteners.push(
      await listen("search:started", (event: any) => {
        console.log("🔍 Search started:", event.payload);
        progressiveSearchState.status = "searching";
        pushMessage("Searching for file...", "info", 2000);
      }),
    );

    // Listen to search:metadata_found event
    unlisteners.push(
      await listen("search:metadata_found", (event: any) => {
        const { fileHash, fileName, fileSize, createdAt, mimeType } =
          event.payload;
        progressiveSearchState.basicMetadata = {
          fileName,
          fileSize,
          createdAt,
          mimeType,
          fileHash,
        };

        latestStatus = "found";
        hasSearched = true;

        console.log("✅ Basic metadata found and displayed:", fileName);
        pushMessage(`Found file: ${fileName}`, "success", 3000);
      }),
    );

    // Listen to search:providers_found event
    unlisteners.push(
      await listen("search:providers_found", (event: any) => {
        const { providers, count } = event.payload;

        // Only update if we have more providers than before (defensive against duplicate/stale events)
        if (count > progressiveSearchState.providers.length) {
          progressiveSearchState.providers = providers;

          // Initialize/update seeder slots (preserve previously loaded info by peerId)
          const prevByPeer = new Map(
            progressiveSearchState.seeders.map((s) => [s.peerId, s] as const),
          );
          progressiveSearchState.seeders = providers.map(
            (peerId: string, index: number) => {
              const prev = prevByPeer.get(peerId);
              if (prev) return { ...prev, index };
              return {
                index,
                peerId,
                hasGeneralInfo: false,
                hasFileInfo: false,
              };
            },
          );

          console.log(`📡 Found ${count} providers:`, providers);
          console.log(
            "📡 Progressive state providers:",
            progressiveSearchState.providers,
          );
          pushMessage(`Found ${count} seeders`, "info", 2000);
        } else {
          console.log(
            `⏭️ Ignoring duplicate/stale providers_found event (current: ${progressiveSearchState.providers.length}, new: ${count})`,
          );
        }
      }),
    );

    // Listen to search:seeder_general_info event
    unlisteners.push(
      await listen("search:seeder_general_info", (event: any) => {
        const { seederIndex, walletAddress, defaultPricePerMb } = event.payload;

        const seeder = progressiveSearchState.seeders[seederIndex];
        if (seeder) {
          seeder.walletAddress = walletAddress;
          seeder.pricePerMb = defaultPricePerMb;
          seeder.hasGeneralInfo = true;
        }

        if (DEV)
          console.log(
            `[DownloadSearchSection] seeder_general_info #${seederIndex}`,
            { walletAddress },
          );
      }),
    );

    // Listen to search:seeder_file_info event
    unlisteners.push(
      await listen("search:seeder_file_info", (event: any) => {
        const { seederIndex, pricePerMb, supportedProtocols, protocolDetails } =
          event.payload;

        const seeder = progressiveSearchState.seeders[seederIndex];
        if (seeder) {
          if (pricePerMb !== null) {
            seeder.pricePerMb = pricePerMb;
          }
          const normalizedProtocols = (supportedProtocols as unknown[] | undefined)
            ?.map(normalizeProtocol)
            .filter((protocol): protocol is Protocol => protocol !== null) ?? [];
          seeder.protocols = normalizedProtocols;
          seeder.protocolDetails = normalizeProtocolDetails(protocolDetails);
          seeder.hasFileInfo = true;
          if (DEV)
          console.log(
            `[DownloadSearchSection] seeder_file_info #${seederIndex}`,
            { supportedProtocols, normalizedProtocols },
          );
        }

        
      }),
    );

    // Listen to search:complete event
    unlisteners.push(
      await listen("search:complete", (event: any) => {
        const { totalSeeders, durationMs } = event.payload;
        progressiveSearchState.status = "complete";
        if (typeof durationMs === "number" && Number.isFinite(durationMs)) {
          lastSearchDuration = durationMs;
        } else if (typeof searchStartedAtMs === "number") {
          lastSearchDuration = Math.round(
            performance.now() - searchStartedAtMs,
          );
        }

        console.log(
          `✅ Search complete: ${totalSeeders} seeders in ${durationMs}ms`,
        );
        pushMessage(
          `Search complete! Found ${totalSeeders} seeders`,
          "success",
        );
      }),
    );

    // Listen to search:timeout event
    unlisteners.push(
      await listen("search:timeout", (event: any) => {
        const { partialSeeders, missingCount } = event.payload;
        progressiveSearchState.status = "timeout";
        if (typeof searchStartedAtMs === "number") {
          lastSearchDuration = Math.round(
            performance.now() - searchStartedAtMs,
          );
        }

        console.warn(
          `⚠️ Search timeout: ${partialSeeders} complete, ${missingCount} missing`,
        );
        pushMessage(
          `Partial results: ${partialSeeders} seeders available`,
          "warning",
        );

        // Build metadata with partial results

        console.warn("⚠️ Search timeout but no metadata available");
      }),
    );

    eventUnlisteners = unlisteners;
  }

  // Clean up progressive event listeners
  async function cleanupProgressiveEventListeners() {
    for (const unlisten of eventUnlisteners) {
      unlisten();
    }
    eventUnlisteners = [];
  }

  async function searchForFile() {
    console.log(
      "🔍 searchForFile() called with searchMode:",
      searchMode,
      "searchHash:",
      searchHash,
      "isSearching:",
      isSearching,
    );
    if (isSearching) {
      console.warn("⚠️ Search already in progress, ignoring duplicate call");
      return;
    }

    currentSearchId += 1;
    const searchId = currentSearchId;

    console.log("✅ Search started, isSearching now:", isSearching);

    // Handle BitTorrent downloads - show confirmation instead of immediately downloading
    if (
      searchMode === "magnet" ||
      searchMode === "torrent" ||
      searchMode === "ed2k" ||
      searchMode === "ftp"
    ) {
      console.log("✅ Entering magnet/torrent/ed2k/ftp path");
      let identifier: string | null = null;

      if (searchMode === "magnet") {
        identifier = searchHash.trim();
        if (!identifier) {
          pushMessage("Please enter a magnet link", "warning");
          return;
        }

        // For magnet links, extract info_hash and search DHT directly
        console.log("🔍 Parsing magnet link:", identifier);
        const infoHash = extractInfoHashFromMagnet(identifier);
        console.log(
          "🔍 Extracted info_hash (normalized to lowercase):",
          infoHash,
        );
        if (infoHash) {
          try {
            console.log("🔍 Searching DHT by info_hash:", infoHash);
            // Tauri converts parameters to camelCase, so we use infoHash here
            const params = { infoHash };
            console.log(
              "🔍 Calling search_by_infohash with params:",
              JSON.stringify(params),
            );
            // Search DHT by info_hash (uses two-step lookup: info_hash -> merkle_root -> metadata)
            const metadata = (await invoke(
              "search_by_infohash",
              params,
            )) as FileMetadata | null;
            console.log("🔍 DHT search result:", metadata);
            if (metadata) {
              // Found the file! Show it instead of the placeholder
              metadata.fileHash = metadata.merkleRoot || "";
              latestStatus = "found";
              hasSearched = true;
              pushMessage(`Found file: ${metadata.fileName}`, "success");
              return;
            } else {
              console.log("⚠️ No metadata found for info_hash:", infoHash);
            }
          } catch (error) {
            console.error("❌ DHT search error:", error);
            console.log("Falling back to magnet download");
          }
        } else {
          console.log("⚠️ Could not extract info_hash from magnet link");
        }

        // If not found in DHT or no info_hash, proceed with magnet download
      } else if (searchMode === "torrent") {
        if (!torrentFileName) {
          pushMessage("Please select a .torrent file", "warning");
          return;
        }
        // Use the file input to get the actual file
        const file = torrentFileInput?.files?.[0];
        if (file) {
          // Try to parse torrent file and search for it first
          try {
            const arrayBuffer = await file.arrayBuffer();
            const bytes = new Uint8Array(arrayBuffer);
            const infoHash = await extractInfoHashFromTorrentBytes(bytes);
            console.log("🔍 Extracted info_hash from torrent file:", infoHash);

            try {
              console.log("🔍 Searching DHT by info_hash:", infoHash);
              const params = { infoHash };
              const metadata = (await invoke(
                "search_by_infohash",
                params,
              )) as FileMetadata | null;
              console.log("🔍 DHT search result:", metadata);
              if (metadata) {
                metadata.fileHash = metadata.merkleRoot || "";
                latestStatus = "found";
                hasSearched = true;
                pushMessage(`Found file: ${metadata.fileName}`, "success");
                return;
              } else {
                console.log("⚠️ No metadata found for info_hash:", infoHash);
              }
            } catch (error) {
              console.error("❌ DHT search error:", error);
            }

            // If not found in DHT, proceed with torrent download flow
            identifier = torrentFileName;
          } catch (error) {
            console.log("Failed to parse torrent file:", error);
            identifier = torrentFileName;
          }
        } else {
          pushMessage("Please select a .torrent file", "warning");
          return;
        }
      } else if (searchMode === "ed2k") {
        identifier = searchHash.trim();
        if (!identifier) {
          pushMessage("Please enter an ED2K link", "warning");
          return;
        }
        // Basic ED2K link validation
        if (!identifier.startsWith("ed2k://")) {
          pushMessage(
            "Please enter a valid ED2K link starting with ed2k://",
            "warning",
          );
          return;
        }

        // For ED2K links, extract hash and search DHT first
        const parts = identifier.split("|");
        if (parts.length >= 5) {
          const ed2kHash = parts[4];
          try {
            // Search DHT using the ED2K hash as the key (results come via events)
            await dhtService.searchFileMetadata(ed2kHash, SEARCH_TIMEOUT_MS);
            console.log("Triggered DHT search for ED2K hash:", ed2kHash);
          } catch (error) {
            console.log("DHT search failed for ED2K hash:", error);
          }
        }
      } else if (searchMode === "ftp") {
        identifier = searchHash.trim();
        if (!identifier) {
          pushMessage("Please enter an FTP URL", "warning");
          return;
        }
        // Basic FTP URL validation
        if (
          !identifier.startsWith("ftp://") &&
          !identifier.startsWith("ftps://")
        ) {
          pushMessage(
            "Please enter a valid FTP URL starting with ftp:// or ftps://",
            "warning",
          );
          return;
        }

        // Handle FTP URL - extract hash and search DHT for real metadata
        try {
          const ftpUrl = new URL(identifier);
          const pathSegments = ftpUrl.pathname
            .split("/")
            .filter((s) => s.length > 0);
          let fileName =
            pathSegments.length > 0
              ? decodeURIComponent(pathSegments[pathSegments.length - 1])
              : "unknown_file";

          // Extract hash prefix if present (format: {64-char-hash}_{original_filename})
          let extractedHash = "";
          if (fileName.length > 65 && fileName.charAt(64) === "_") {
            // Check if first 64 chars look like a hex hash
            const potentialHash = fileName.substring(0, 64);
            if (/^[a-f0-9]{64}$/i.test(potentialHash)) {
              extractedHash = potentialHash;
              fileName = fileName.substring(65); // Remove hash prefix and underscore
            }
          }

          // If we have a hash, search DHT for real metadata (results come via events)
          if (extractedHash) {
            try {
              await dhtService.searchFileMetadata(
                extractedHash,
                SEARCH_TIMEOUT_MS,
              );
              console.log("Triggered DHT search for FTP hash:", extractedHash);
            } catch (error) {
              console.log(
                "DHT search failed for FTP hash, falling back to basic FTP metadata:",
                error,
              );
            }
          }

          latestStatus = "found";
          hasSearched = true;
          const fallbackMsg = extractedHash
            ? `FTP file ready to download: ${fileName} (metadata not found)`
            : `FTP file ready to download: ${fileName}`;
          pushMessage(fallbackMsg, "success");
        } catch (error) {
          console.error("Failed to parse FTP URL:", error);
          pushMessage(`Invalid FTP URL: ${String(error)}`, "error");
        }
        return;
      }

      if (identifier) {
        try {
          // Store the pending torrent info for confirmation
          if (searchMode === "torrent") {
            const file = torrentFileInput?.files?.[0];
            if (file) {
              const arrayBuffer = await file.arrayBuffer();
              const bytes = new Uint8Array(arrayBuffer);
              pendingTorrentBytes = Array.from(bytes);
              pendingTorrentType = "file";
              pendingTorrentIdentifier = torrentFileName;
            }
          } else {
            // For magnet links
            pendingTorrentIdentifier = identifier;
            pendingTorrentType = "magnet";
            pendingTorrentBytes = null;
          }

          latestStatus = "found";
          hasSearched = true;
          pushMessage(
            `${pendingTorrentType === "magnet" ? "Magnet link" : "Torrent file"} ready to download`,
            "success",
          );
        } catch (error) {
          console.error("Failed to prepare torrent:", error);
          pushMessage(`Failed to prepare download: ${String(error)}`, "error");
        }
      }
      return;
    }

    const trimmed = searchHash.trim();
    if (!trimmed) {
      const message =
        searchMode === "merkle_hash"
          ? tr("download.notifications.enterHash")
          : searchMode === "magnet"
            ? "Please enter a magnet link"
            : searchMode === "ed2k"
              ? "Please enter an ED2K link"
              : searchMode === "ftp"
                ? "Please enter an FTP URL"
                : "Please enter a search term";
      pushMessage(message, "warning");
      return;
    }

    hasSearched = true;
    latestStatus = "pending";
    searchError = null;

    const startedAt = performance.now();
    searchStartedAtMs = startedAt;

    try {
      // Setup progressive event listeners
      await setupProgressiveEventListeners();

      // Reset progressive search state
      progressiveSearchState = {
        status: "searching",
        basicMetadata: null,
        providers: [],
        seeders: [],
      };

      // Create history entry
      const entry = dhtSearchHistory.addPending(trimmed);
      activeHistoryId = entry.id;

      const elapsed = Math.round(performance.now() - startedAt);
      // Initiate progressive search (non-blocking)
      void dhtService
        .searchFileMetadata(trimmed, SEARCH_TIMEOUT_MS)
        .catch((error) => {
          // Ignore late failures from a canceled/stale search
          if (searchId !== currentSearchId) return;

          const message =
            error instanceof Error
              ? error.message
              : tr("download.search.status.unknownError");
          lastSearchDuration = elapsed;
          latestStatus = "error";
          searchError = message;

          if (searchMode === "merkle_hash" && activeHistoryId) {
            dhtSearchHistory.updateEntry(activeHistoryId, {
              status: "error",
              errorMessage: message,
              elapsedMs: elapsed,
            });
          }

          console.error("Search failed:", error);
          pushMessage(
            `${tr("download.search.status.errorNotification")}: ${message}`,
            "error",
            6000,
          );

          void cleanupProgressiveEventListeners();
        });
      if (searchCancelTimeoutId) {
        clearTimeout(searchCancelTimeoutId);
        pushMessage(
          tr('download.search.status.foundNotification', { values: { name: metadata.fileName } }),
          'success',
        );
        isSearching = false;
      } else {
        latestStatus = 'not_found';
        
        // Get DHT health to provide better diagnostics
        const health = await dhtService.getHealth();
        const peerCount = health?.peerCount || 0;
        
        let errorMessage = 'File not found in the network.';
        if (peerCount < 3) {
          errorMessage += ` Low peer count (${peerCount} peers connected). Try waiting for more connections or restart DHT.`;
        } else {
          errorMessage += ' If you just uploaded this file, wait 60-90 seconds for DHT propagation, then search again.';
        }
        
        dhtSearchHistory.updateEntry(entry.id, {
          status: 'not_found',
          metadata: undefined,
          errorMessage,
          elapsedMs: elapsed,
        });
        
        if (peerCount < 3) {
          pushMessage(`File not found. Low peer count (${peerCount}). Check Network tab for connectivity issues.`, 'warning', 8000);
        } else {
          pushMessage('File not found. If recently uploaded, wait 60-90 seconds for DHT propagation and try again. Ensure both devices are connected to the same network.', 'warning', 10000);
        }
      }
      searchCancelTimeoutId = setTimeout(() => {
        if (searchId !== currentSearchId) return;
        if (isSearching && progressiveSearchState.status === "searching") {
          console.warn(
            "⚠️ Frontend timeout - no completion event received from backend",
          );
          cleanupProgressiveEventListeners();

          if (progressiveSearchState.basicMetadata) {
            // Build metadata with whatever we have
            pushMessage("Search completed with partial results", "warning");
          } else {
            latestStatus = "error";
            searchError = "Search timeout - no response from network";
            pushMessage("Search timeout - no response from network", "error");
          }
        }
      }, 15000);
    } catch (error) {
      const message =
        error instanceof Error
          ? error.message
          : tr("download.search.status.unknownError");
      const elapsed = Math.round(performance.now() - startedAt);
      lastSearchDuration = elapsed;
      latestStatus = "error";
      searchError = message;

      if (searchMode === "merkle_hash" && activeHistoryId) {
        dhtSearchHistory.updateEntry(activeHistoryId, {
          status: "error",
          errorMessage: message,
          elapsedMs: elapsed,
        });
      }

      console.error("Search failed:", error);
      pushMessage(
        `${tr("download.search.status.errorNotification")}: ${message}`,
        "error",
        6000,
      );

      isSearching = false;
      await cleanupProgressiveEventListeners();
    }
  }

  function clearHistory() {
    dhtSearchHistory.clear();
    historyEntries = [];
    activeHistoryId = null;
    latestStatus = "pending";
    searchError = null;
    hasSearched = false;
  }

  function statusIcon(status: string) {
    switch (status) {
      case "found":
        return CheckCircle2;
      case "error":
        return AlertCircle;
      default:
        return Search;
    }
  }

  function statusClass(status: string) {
    switch (status) {
      case "found":
        return "text-emerald-600";
      case "error":
        return "text-red-600";
      case "not_found":
        return "text-amber-600";
      default:
        return "text-muted-foreground";
    }
  }

  function toggleHistoryDropdown() {
    showHistoryDropdown = !showHistoryDropdown;
  }

  function selectHistoryEntry(entry: SearchHistoryEntry) {
    searchHash = entry.hash;
    activeHistoryId = entry.id;
    hydrateFromEntry(entry);
    showHistoryDropdown = false;
  }

  function handleClickOutside(event: MouseEvent) {
    const target = event.target as HTMLElement;
    if (!target.closest(".search-input-container")) {
      showHistoryDropdown = false;
    }
  }

  // Check if current user is seeding this file
  function checkIfSeeding(metadata: CompleteFileMetadata | null): boolean {
    if (metadata !== null) {
      try {
        const currentPeerId = dhtService.getPeerId() || ""; //can prob be saved/cached instead of call everytime
        return Object.keys(metadata.seederInfo).includes(currentPeerId);
      } catch (error) {
        console.warn("Failed to check seeding status:", error);
      }
    }
    return false;
  }

  let amISeeding = $derived.by(() => checkIfSeeding(completeFileMetadata));

  // Handle file download - show peer selection modal
  function handleFileDownload() {
    if (!completeFileMetadata) {
      pushMessage("File metadata is still loading. Please wait.", "warning");
      return;
    }

    if (availableProtocolIds.length === 0) {
      pushMessage("No download protocols available for this file.", "warning");
      return;
    }

    showPeerSelectionModal = true;
  }

  // Confirm peer selection and start download
  function confirmPeerSelection(
    selectedPeerIds: string[],
    selectedProtocol: Protocol,
  ) {
    if (!completeFileMetadata) {
      pushMessage("File metadata is still loading. Please wait.", "warning");
      return;
    }

    if (selectedPeerIds.length > 1) {
      pushMessage("Multi-source download is not yet supported.", "warning");
      return;
    }

    if (selectedPeerIds.length === 0) {
      pushMessage("Select at least one peer to download.", "warning");
      return;
    }

    const selectedPeer = selectedPeerIds[0];
    const seederInfo = completeFileMetadata.seederInfo[selectedPeer];
    if (!seederInfo) {
      pushMessage("Selected peer info is unavailable. Please try again.", "error");
      return;
    }

    const pricePerMb =
      seederInfo.fileSpecific.pricePerMb ??
      seederInfo.general.defaultPricePerMb ??
      0;
    const price = costFromPricePerMb({
      bytes: completeFileMetadata.dhtRecord.fileSize,
      pricePerMb,
    });

    dispatch("download", {
      fullMetadata: completeFileMetadata,
      selectedPeer,
      selectedProtocol,
      price,
    });

    showPeerSelectionModal = false;
    pushMessage(
      `Starting ${selectedProtocol.toUpperCase()} download with ${selectedPeerIds.length} selected peer${selectedPeerIds.length === 1 ? "" : "s"}`,
      "success",
      3000,
    );
  }

  // Cancel peer selection
  function cancelPeerSelection() {
    showPeerSelectionModal = false;
    // Clear torrent state if canceling a torrent download
    if (pendingTorrentType) {
      pendingTorrentIdentifier = null;
      pendingTorrentBytes = null;
      pendingTorrentType = null;
      latestStatus = "pending";
    }
  }
</script>

<Card class="p-6">
  <div class="space-y-4">
    <div>
      <Label for="hash-input" class="text-xl font-semibold"
        >{tr("download.addNew")}</Label
      >

      <!-- Search Mode Switcher -->
      <div class="flex gap-2 mb-3 mt-3">
        <select
          bind:value={searchMode}
          class="px-3 py-1 text-sm rounded-md border transition-colors bg-muted/50 hover:bg-muted border-border"
        >
          <option value="merkle_hash">Search by File Hash</option>
          <option value="magnet">Search by Magnet Link</option>
          <option value="torrent">Search by .torrent File</option>
          <option value="ed2k">Search by ED2K Link</option>
          <option value="ftp">Search by FTP URL</option>
        </select>
      </div>

      <div class="flex flex-col sm:flex-row gap-3">
        {#if searchMode === "torrent"}
          <!-- File input for .torrent files -->
          <div class="flex-1">
            <input
              type="file"
              bind:this={torrentFileInput}
              accept=".torrent"
              class="hidden"
              onchange={handleTorrentFileSelect}
            />
            <Button
              variant="default"
              class="w-full h-10 justify-center font-medium cursor-pointer hover:opacity-90"
              on:click={() => torrentFileInput?.click()}
            >
              <svg
                xmlns="http://www.w3.org/2000/svg"
                width="18"
                height="18"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                stroke-width="2"
                stroke-linecap="round"
                stroke-linejoin="round"
                class="mr-2"
              >
                <path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path>
                <polyline points="17 8 12 3 7 8"></polyline>
                <line x1="12" y1="3" x2="12" y2="15"></line>
              </svg>
              {torrentFileName || "Select .torrent File"}
            </Button>
          </div>
        {:else}
          <div class="relative flex-1 search-input-container">
            <Input
              id="hash-input"
              bind:value={searchHash}
              placeholder={searchMode === "merkle_hash"
                ? "Enter file hash (SHA-256)..."
                : searchMode === "magnet"
                  ? "magnet:?xt=urn:btih:..."
                  : searchMode === "ed2k"
                    ? "ed2k://|file|filename|size|hash|/"
                    : searchMode === "ftp"
                      ? "ftp://user:pass@server.com/path/file"
                      : ""}
              class="pr-20 h-10"
              on:focus={toggleHistoryDropdown}
              on:keydown={(e: CustomEvent<KeyboardEvent>) => {
                const event = e.detail;
                if (
                  event.key === "Enter" &&
                  searchHash.trim() &&
                  !isSearching
                ) {
                  event.preventDefault();
                  searchForFile();
                }
              }}
            />
            {#if searchHash}
              <button
                onclick={clearSearch}
                class="absolute right-10 top-1/2 transform -translate-y-1/2 p-1 hover:bg-muted rounded-full transition-colors"
                type="button"
                aria-label={tr("download.clearInput")}
              >
                <X
                  class="h-4 w-4 text-muted-foreground hover:text-foreground"
                />
              </button>
            {/if}
            <button
              onclick={toggleHistoryDropdown}
              class="absolute right-2 top-1/2 transform -translate-y-1/2 p-1 hover:bg-muted rounded-full transition-colors"
              type="button"
              aria-label="Toggle search history"
            >
              <History
                class="h-4 w-4 text-muted-foreground hover:text-foreground"
              />
            </button>

            {#if showHistoryDropdown}
              <div
                class="absolute top-full left-0 right-0 mt-1 bg-background border border-border rounded-md shadow-lg z-50 max-h-80 overflow-auto"
              >
                {#if historyEntries.length > 0}
                  <div class="p-2 border-b border-border">
                    <div class="flex items-center justify-between">
                      <span class="text-sm font-medium text-muted-foreground"
                        >Search History</span
                      >
                      <Button
                        variant="ghost"
                        size="sm"
                        class="h-6 px-2 text-xs"
                        on:click={clearHistory}
                      >
                        <RotateCcw class="h-3 w-3 mr-1" />
                        Clear
                      </Button>
                    </div>
                  </div>
                  <div class="py-1">
                    {#each historyEntries as entry}
                      {@const StatusIcon = statusIcon(entry.status)}
                      <button
                        type="button"
                        class="w-full px-3 py-2 text-left hover:bg-muted/60 transition-colors flex items-center justify-between"
                        onclick={() => selectHistoryEntry(entry)}
                      >
                        <div class="flex items-center gap-2 flex-1 min-w-0">
                          <span class="text-sm font-medium truncate"
                            >{entry.hash}</span
                          >
                        </div>
                        <div
                          class="flex items-center gap-2 text-xs text-muted-foreground"
                        >
                          <StatusIcon
                            class={`h-3 w-3 ${statusClass(entry.status)}`}
                          />
                          {#if entry.elapsedMs}
                            <span>{(entry.elapsedMs / 1000).toFixed(1)}s</span>
                          {/if}
                        </div>
                      </button>
                      {#if entry.metadata?.fileName}
                        <div
                          class="px-3 pb-2 text-xs text-muted-foreground truncate"
                        >
                          {entry.metadata.fileName}
                        </div>
                      {/if}
                    {/each}
                  </div>
                {:else}
                  <div class="p-4 text-center">
                    <p class="text-sm text-muted-foreground">
                      No search history yet
                    </p>
                  </div>
                {/if}
              </div>
            {/if}
          </div>
        {/if}
        <Button
          on:click={isSearching ? cancelSearch : searchForFile}
          disabled={!isSearching &&
            ((searchMode !== "torrent" && !searchHash.trim()) ||
              (searchMode === "torrent" && !torrentFileName))}
          class="h-10 px-6"
          title={isSearching
            ? "Cancel search"
            : searchMode !== "torrent" && !searchHash.trim()
              ? "Enter a search hash"
              : searchMode === "torrent" && !torrentFileName
                ? "Select a torrent file"
                : "Search"}
        >
          {#if isSearching}
            <X class="h-4 w-4 mr-2" />
            {tr("actions.cancel")}
          {:else}
            <Search class="h-4 w-4 mr-2" />
            {tr("download.search.button")}
          {/if}
        </Button>
      </div>
    </div>

    {#if hasSearched}
      <div class="pt-6 border-t">
        <div class="space-y-4">
          {#if progressiveSearchState.basicMetadata}
            <SearchResultCard
              searchState={progressiveSearchState}
              isSeeding={amISeeding}
              {availableProtocols}
              download={handleFileDownload}
            />
            {#if progressiveSearchState.status === "searching"}
              <p class="text-xs text-muted-foreground">
                Searching for more peers...
              </p>
            {:else if (progressiveSearchState.status === "complete" || progressiveSearchState.status === "timeout") && lastSearchDuration > 0}
              <p class="text-xs text-muted-foreground">
                {tr("download.search.status.completedIn", {
                  values: { seconds: (lastSearchDuration / 1000).toFixed(1) },
                })}
              </p>
            {/if}
          {:else if latestStatus === "not_found"}
            <div class="text-center py-8">
              {#if searchError}
                <p class="text-sm text-red-500">{searchError}</p>
              {:else}
                <p class="text-sm text-muted-foreground">
                  {tr("download.search.status.notFoundDetail")}
                </p>
              {/if}
            </div>
          {:else if latestStatus === "error"}
            <div class="text-center py-8">
              <p class="text-sm font-medium text-muted-foreground mb-1">
                {tr("download.search.status.errorHeadline")}
              </p>
              <p class="text-sm text-muted-foreground">{searchError}</p>
            </div>
          {:else}
            <div
              class="rounded-md border border-dashed border-muted p-5 text-sm text-muted-foreground text-center"
            >
              {tr("download.search.status.placeholder")}
            </div>
          {/if}
        </div>
      </div>
    {/if}
  </div>
</Card>

<!-- Peer Selection Modal -->
{#if completeFileMetadata !== null}
  <PeerSelectionModal
    bind:showPeerSelectionModal
    meta={completeFileMetadata}
    isSeeding={amISeeding}
    availableProtocols={availableProtocolIds}
    confirm={confirmPeerSelection}
    cancel={cancelPeerSelection}
  />
{/if}
