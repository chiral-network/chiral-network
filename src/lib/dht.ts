// DHT configuration and utilities
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { join } from "@tauri-apps/api/path";
//importing reputation store for the reputation based peer discovery
import ReputationStore from "$lib/reputationStore";
const __rep = ReputationStore.getInstance();

export type NatReachabilityState = "unknown" | "public" | "private";
export type NatConfidence = "low" | "medium" | "high";

export interface NatHistoryItem {
  state: NatReachabilityState;
  confidence: NatConfidence;
  timestamp: number;
  summary?: string | null;
}

export interface DhtConfig {
  port: number;
  bootstrapNodes: string[];
  showMultiaddr?: boolean;
  enableAutonat?: boolean;
  autonatProbeIntervalSeconds?: number;
  autonatServers?: string[];
  proxyAddress?: string;
  chunkSizeKb?: number;
  cacheSizeMb?: number;
  enableAutorelay?: boolean;
  preferredRelays?: string[];
  enableRelayServer?: boolean;
  enableUpnp?: boolean;
  relayServerAlias?: string; // Public alias for relay server (appears in logs and bootstrap)
}

export interface HttpSourceInfo {
  url: string;
  authHeader?: string;
  verifySsl: boolean;
  headers?: Array<[string, string]>;
  timeoutSecs?: number;
}

export interface FtpSourceInfo {
  url: string;
  username?: string;
  password?: string;
  supportsResume: boolean;
  fileSize?: number;
  lastChecked?: number;
  isAvailable: boolean;
}

export interface Ed2kSourceInfo {
  server_url: string;
  file_hash: string;
}

export interface FileMetadata {
  fileHash: string;
  fileName: string;
  fileSize: number;
  fileData?: Uint8Array | number[];
  seeders: string[];
  createdAt: number;
  merkleRoot?: string;
  downloadPath?: string;
  mimeType?: string;
  isEncrypted: boolean;
  encryptionMethod?: string;
  keyFingerprint?: string;
  manifest?: string;
  isRoot?: boolean;
  cids?: string[];
  price: number;
  uploaderAddress?: string;
  httpSources?: HttpSourceInfo[];
  ftpSources?: FtpSourceInfo[];
  ed2kSources?: Ed2kSourceInfo[];
  infoHash?: string;
  trackers?: string[];
}

export interface DhtHealth {
  peerCount: number;
  lastBootstrap: number | null;
  lastPeerEvent: number | null;
  lastError: string | null;
  lastErrorAt: number | null;
  bootstrapFailures: number;
  listenAddrs: string[];
  reachability: NatReachabilityState;
  reachabilityConfidence: NatConfidence;
  lastReachabilityChange: number | null;
  lastProbeAt: number | null;
  lastReachabilityError: string | null;
  observedAddrs: string[];
  reachabilityHistory: NatHistoryItem[];
  autonatEnabled: boolean;
  // AutoRelay metrics
  autorelayEnabled: boolean;
  lastAutorelayEnabledAt: number | null;
  lastAutorelayDisabledAt: number | null;
  activeRelayPeerId: string | null;
  relayReservationStatus: string | null;
  lastReservationSuccess: number | null;
  lastReservationFailure: number | null;
  reservationRenewals: number;
  reservationEvictions: number;
  // Extended relay error tracking
  relayConnectionAttempts: number;
  relayConnectionSuccesses: number;
  relayConnectionFailures: number;
  lastRelayError: string | null;
  lastRelayErrorType: string | null;
  lastRelayErrorAt: number | null;
  activeRelayCount: number;
  totalRelaysInPool: number;
  relayHealthScore: number; // Average health score of all relays
  lastReservationRenewal: number | null;
  // DCUtR hole-punching metrics
  dcutrEnabled: boolean;
  dcutrHolePunchAttempts: number;
  dcutrHolePunchSuccesses: number;
  dcutrHolePunchFailures: number;
  lastDcutrSuccess: number | null;
  lastDcutrFailure: number | null;
}

export class DhtService {
  private static instance: DhtService | null = null;
  private peerId: string | null = null;
  private port: number = 4001;

  private constructor() {}

  static getInstance(): DhtService {
    if (!DhtService.instance) {
      DhtService.instance = new DhtService();
    }
    return DhtService.instance;
  }

  setPeerId(peerId: string | null): void {
    this.peerId = peerId;
  }

  async start(config?: Partial<DhtConfig>): Promise<string> {
    const port = config?.port || 4001;
    let bootstrapNodes = config?.bootstrapNodes || [];

    // Use default bootstrap nodes if none provided
    if (bootstrapNodes.length === 0) {
      bootstrapNodes = await invoke<string[]>("get_bootstrap_nodes_command");
    }

    try {
      const payload: Record<string, unknown> = {
        port,
        bootstrapNodes,
      };
      if (typeof config?.enableAutonat === "boolean") {
        payload.enableAutonat = config.enableAutonat;
      }
      if (typeof config?.autonatProbeIntervalSeconds === "number") {
        payload.autonatProbeIntervalSecs = config.autonatProbeIntervalSeconds;
      }
      if (config?.autonatServers && config.autonatServers.length > 0) {
        payload.autonatServers = config.autonatServers;
      }
      if (
        typeof config?.proxyAddress === "string" &&
        config.proxyAddress.trim().length > 0
      ) {
        payload.proxyAddress = config.proxyAddress;
      }
      if (typeof config?.chunkSizeKb === "number") {
        payload.chunkSizeKb = config.chunkSizeKb;
      }
      if (typeof config?.cacheSizeMb === "number") {
        payload.cacheSizeMb = config.cacheSizeMb;
      }
      if (typeof config?.enableAutorelay === "boolean") {
        payload.enableAutorelay = config.enableAutorelay;
      }
      if (config?.preferredRelays && config.preferredRelays.length > 0) {
        payload.preferredRelays = config.preferredRelays;
      }
      if (typeof config?.enableRelayServer === "boolean") {
        payload.enableRelayServer = config.enableRelayServer;
      }
      if (typeof config?.enableUpnp === "boolean") {
        payload.enableUpnp = config.enableUpnp;
      }
      if (
        typeof config?.relayServerAlias === "string" &&
        config.relayServerAlias.trim().length > 0
      ) {
        payload.relayServerAlias = config.relayServerAlias.trim();
      }

      const peerId = await invoke<string>("start_dht_node", payload);
      this.peerId = peerId;
      this.port = port;
      return this.peerId;
    } catch (error) {
      console.error("Failed to start DHT:", error);
      this.peerId = null; // Clear on failure
      throw error;
    }
  }

  async stop(): Promise<void> {
    try {
      await invoke("stop_dht_node");
      this.peerId = null;
    } catch (error) {
      console.error("Failed to stop DHT:", error);
      throw error;
    }
  }

  async publishFileToNetwork(
    filePath: string,
    price?: number,
    protocol?: string,
    originalFileName?: string
  ): Promise<FileMetadata> {
    try {
      // Start listening for the published_file event
      let timeoutId: NodeJS.Timeout;

      const metadataPromise = new Promise<FileMetadata>((resolve, reject) => {
        const unlistenPromise = listen<FileMetadata>(
          "published_file",
          (event) => {
            const metadata = event.payload;
            if (!metadata.merkleRoot && metadata.fileHash) {
              metadata.merkleRoot = metadata.fileHash;
            }
            if (!metadata.fileHash && metadata.merkleRoot) {
              metadata.fileHash = metadata.merkleRoot;
            }
            // Clear timeout on success
            if (timeoutId) clearTimeout(timeoutId);
            resolve(metadata);
            // Unsubscribe once we got the event
            unlistenPromise.then((unlistenFn) => unlistenFn());
          }
        );

        // Add timeout to reject the promise if publishing takes too long
        timeoutId = setTimeout(() => {
          reject(
            new Error(
              "File publishing timeout - no published_file event received"
            )
          );
          unlistenPromise.then((unlistenFn) => unlistenFn());
        }, 30000); // Increase timeout to 30 seconds for ED2K and other protocols
      });

      // Trigger the backend upload with price and protocol
      await invoke("upload_file_to_network", {
        filePath,
        price: price ?? 0, // Default to 0 instead of null
        protocol: protocol ?? "Bitswap", // Default to Bitswap if no protocol specified
        originalFileName: originalFileName || null,
      });

      // Wait until the event arrives
      return await metadataPromise;
    } catch (error) {
      console.error("Failed to publish file:", error);
      throw error;
    }
  }

  async downloadFile(fileMetadata: FileMetadata): Promise<FileMetadata> {
    try {
      // Use the download path from metadata (must be provided by caller)
      let resolvedStoragePath: string;

      if (fileMetadata.downloadPath) {
        // Use the path that was already selected by the user in the file dialog
        resolvedStoragePath = fileMetadata.downloadPath;
      } else {
        // Get canonical download directory from backend (single source of truth)
        const downloadDir = await invoke<string>("get_download_directory");

        // Construct full file path
        resolvedStoragePath = await join(downloadDir, fileMetadata.fileName);
      }

      // Ensure the directory exists before starting download
      await invoke("ensure_directory_exists", { path: resolvedStoragePath });

      // IMPORTANT: Set up the event listener BEFORE invoking the backend
      // to avoid race condition where event fires before we're listening
      const metadataPromise = new Promise<FileMetadata>((resolve, reject) => {
        const unlistenPromise = listen<FileMetadata>(
          "file_content",
          async (event) => {
            resolve(event.payload);
            // Unsubscribe once we got the event
            unlistenPromise.then((unlistenFn) => unlistenFn());
          }
        );

        // Add timeout to reject the promise if download takes too long
        setTimeout(() => {
          reject(
            new Error("Download timeout - no file_content event received")
          );
          unlistenPromise.then((unlistenFn) => unlistenFn());
        }, 300000); // 5 minute timeout
      });

      // Prepare file metadata for Bitswap download
      fileMetadata.merkleRoot = fileMetadata.fileHash;
      // Preserve existing fileData if present, otherwise provide an empty placeholder
      fileMetadata.fileData = fileMetadata.fileData ?? [];
      // Ensure cids exists; Bitswap expects a root CID list. Fallback to merkleRoot when absent.
      if (!fileMetadata.cids || fileMetadata.cids.length === 0) {
        fileMetadata.cids = [fileMetadata.merkleRoot];
      }
      // Determine isRoot: true when explicitly set, or when the merkleRoot equals the first CID
      // or when there's only a single CID (fallback root).
      fileMetadata.isRoot =
        typeof fileMetadata.isRoot === "boolean"
          ? fileMetadata.isRoot
          : fileMetadata.cids[0] === fileMetadata.merkleRoot ||
            fileMetadata.cids.length === 1;

      try {
        console.log(
          "🔽 DhtService.downloadFile: Invoking download_blocks_from_network with:",
          {
            merkleRoot: fileMetadata.merkleRoot,
            fileHash: fileMetadata.fileHash,
            fileName: fileMetadata.fileName,
            cidsCount: fileMetadata.cids?.length,
          }
        );

        // Trigger the backend download AFTER setting up the listener
        await invoke("download_blocks_from_network", {
          fileMetadata,
          downloadPath: resolvedStoragePath,
        });
      } catch (error) {
        console.error(
          "🔽 Frontend: download_blocks_from_network invoke failed:",
          error
        );
        throw error;
      }

      // Wait until the event arrives
      return await metadataPromise;
    } catch (error) {
      console.error("🔽 Frontend: Failed to download file:", error);
      throw error;
    }
  }

  async searchFile(fileHash: string): Promise<void> {
    if (!this.peerId) {
      throw new Error("DHT not started");
    }

    try {
      await invoke("search_file_metadata", { fileHash, timeoutMs: 0 });
      console.log("Searching for file:", fileHash);
    } catch (error) {
      console.error("Failed to search file:", error);
      throw error;
    }
  }

  async connectPeer(peerAddress: string): Promise<void> {
    // Note: We check peerId to ensure DHT was started, but the actual error
    // might be from the backend saying networking isn't implemented
    if (!this.peerId) {
      console.error(
        "DHT service peerId not set, service may not be initialized"
      );
      throw new Error("DHT service not initialized properly");
    }

    // ADD: parse a peerId from /p2p/<id> if present; if not, use addr
    const __pid = (peerAddress?.split("/p2p/")[1] ?? peerAddress)?.trim();
    if (__pid) {
      // Mark we’ve seen this peer (freshness)
      try {
        __rep.noteSeen(__pid);
      } catch {}
    }

    try {
      await invoke("connect_to_peer", { peerAddress });

      // ADD: count a success (no RTT here, the backend doesn't expose it)
      if (__pid) {
        try {
          __rep.success(__pid);
        } catch {}
      }
    } catch (error) {
      console.error("Failed to connect to peer:", error);

      // ADD: count a failure so low-quality peers drift down
      if (__pid) {
        try {
          __rep.failure(__pid);
        } catch {}
      }
      throw error;
    }
  }

  getPeerId(): string | null {
    return this.peerId;
  }

  getPort(): number {
    return this.port;
  }

  getMultiaddr(): string | null {
    if (!this.peerId) return null;
    return `/ip4/127.0.0.1/tcp/${this.port}/p2p/${this.peerId}`;
  }

  async getSeedersForFile(fileHash: string): Promise<string[]> {
    try {
      const seeders = await invoke<string[]>("get_file_seeders", {
        fileHash,
      });
      return Array.isArray(seeders) ? seeders : [];
    } catch (error) {
      console.error("Failed to fetch seeders:", error);
      return [];
    }
  }

  async getPeerCount(): Promise<number> {
    try {
      const count = await invoke<number>("get_dht_peer_count");
      return count;
    } catch (error) {
      console.error("Failed to get peer count:", error);
      return 0;
    }
  }

  async getHealth(): Promise<DhtHealth | null> {
    try {
      const health = await invoke<DhtHealth | null>("get_dht_health");
      return health;
    } catch (error) {
      console.error("Failed to get DHT health:", error);
      return null;
    }
  }

  async searchFileMetadata(
    fileHash: string,
    timeoutMs = 40_000
  ): Promise<FileMetadata | null> {
    const trimmed = fileHash.trim();
    if (!trimmed) {
      throw new Error("File hash is required");
    }

    const cleanup: { fn: UnlistenFn | null } = { fn: null };
    
    try {
      // Start listening for the search_result event
      const metadataPromise = new Promise<FileMetadata | null>(
        async (resolve, reject) => {
          const timeoutId = setTimeout(() => {
            reject(new Error(`Search timeout after ${timeoutMs}ms`));
          }, timeoutMs);

          // Set up the event listener and store the unlisten function
          cleanup.fn = await listen<FileMetadata | null>(
            "found_file",
            (event) => {
              clearTimeout(timeoutId);
              const result = event.payload;
              // ADDING FOR REPUTATION BASED PEER DISCOVERY: mark discovered providers as "seen" for freshness
              try {
                if (result && Array.isArray(result.seeders)) {
                  for (const addr of result.seeders) {
                    // Extract peer ID from multiaddr if present
                    const pid = (addr?.split("/p2p/")[1] ?? addr)?.trim();
                    if (pid) __rep.noteSeen(pid);
                  }
                }
              } catch (e) {
                console.warn("reputation noteSeen failed:", e);
              }
              resolve(
                result
                  ? {
                      ...result,
                      seeders: Array.isArray(result.seeders)
                        ? result.seeders
                        : [],
                    }
                  : null
              );
            }
          );

          // Trigger the backend search and wait for the direct result
          console.log("🔍 Frontend calling search_file_metadata for:", trimmed);
          const metadata = await invoke<FileMetadata | null>(
            "search_file_metadata",
            {
              fileHash: trimmed,
              timeoutMs,
            }
          );
          console.log(
            "🔍 Frontend received direct result from search_file_metadata:",
            metadata
          );

          // Resolve with the metadata from the backend
          clearTimeout(timeoutId);
          resolve(metadata);
        }
      );

      // Wait for the metadata promise to resolve
      const metadata = await metadataPromise;

      if (metadata) {
        if (!metadata.merkleRoot && metadata.fileHash) {
          metadata.merkleRoot = metadata.fileHash;
        }
        if (!metadata.fileHash && metadata.merkleRoot) {
          metadata.fileHash = metadata.merkleRoot;
        }
        const hashForSeeders =
          metadata.merkleRoot || metadata.fileHash || trimmed;
        if (hashForSeeders) {
          const seeders = await this.getSeedersForFile(hashForSeeders);
          // Always update seeders with the current live list from DHT provider query
          // This ensures we don't use stale seeders from the cached metadata
          metadata.seeders = seeders;
        }
      }
      return metadata;
    } catch (error) {
      console.error("Failed to search file metadata:", error);
      throw error;
    } finally {
      // Always clean up the event listener, whether we succeed, timeout, or error
      cleanup.fn?.();
    }
  }
}

// Export singleton instance
export const dhtService = DhtService.getInstance();
