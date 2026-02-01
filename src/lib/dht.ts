// DHT configuration and utilities
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { join } from "@tauri-apps/api/path";
import { type ProtocolDetails } from "./types/protocols";
//importing reputation store for the reputation based peer discovery
import ReputationStore from "$lib/reputationStore";
import type { Protocol } from "./services/contentProtocols";
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
  pureClientMode?: boolean; // Pure DHT client mode - cannot seed files or act as DHT server
  forceServerMode?: boolean; // Force DHT server mode - act as DHT server even behind NAT (for testing/development)
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
  leechers?: string[];
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

// ========== GossipSub Metadata Types ==========

/**
 * Minimal DHT record for file discovery (published to Kademlia DHT)
 * Contains only basic information needed to discover a file
 */
export interface DhtFileRecord {
  fileHash: string;
  fileName: string;
  fileSize: number;
  createdAt: number;
  mimeType?: string;
}

/**
 * Encryption details for a file
 */
export interface EncryptionInfo {
  algorithm: string;
  keyDerivation: string;
}
/**
 * General information about a seeder (broadcasted on topic: seeder/{peerID})
 */
export interface SeederGeneralInfo {
  peerId: string;
  walletAddress: string;
  defaultPricePerMb: number;
  timestamp: number;
}

/**
 * File-specific information from a seeder (broadcasted on topic: seeder/{peerID}/file/{fileHash})
 */
export interface SeederFileInfo {
  peerId: string;
  fileHash: string;
  pricePerMb?: number; // Overrides defaultPricePerMb if set
  supportedProtocols: Protocol[];
  protocolDetails: ProtocolDetails;
  timestamp: number;
}

/**
 * Complete metadata from a single seeder (combines general + file-specific info)
 */
export interface SeederCompleteMetadata {
  general: SeederGeneralInfo;
  fileSpecific: SeederFileInfo;
}

/**
 * Complete file metadata combining DHT discovery + GossipSub seeder metadata
 * This is what downloaders receive after querying the DHT and subscribing to GossipSub
 */
export interface CompleteFileMetadata {
  dhtRecord: DhtFileRecord;
  seederInfo: Record<string, SeederCompleteMetadata>; // peerId -> SeederCompleteMetadata
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
    const port = config?.port ?? 4001;
    let bootstrapNodes = config?.bootstrapNodes ?? [];

    // Use default bootstrap nodes if none provided
    if (bootstrapNodes.length === 0) {
      bootstrapNodes = await invoke<string[]>("get_bootstrap_nodes_command");
    }

    try {
      // start_dht_node (Tauri) expects camelCase keys (it maps to Rust snake_case).
      // Provide required values even when the caller supplies a partial config.
      const enableAutonat =
        typeof config?.enableAutonat === "boolean"
          ? config.enableAutonat
          : true;
      const autonatProbeIntervalSeconds =
        typeof config?.autonatProbeIntervalSeconds === "number"
          ? config.autonatProbeIntervalSeconds
          : 30;
      const chunkSizeKb =
        typeof config?.chunkSizeKb === "number" ? config.chunkSizeKb : 256;
      const cacheSizeMb =
        typeof config?.cacheSizeMb === "number" ? config.cacheSizeMb : 1024;
      const enableAutorelay =
        typeof config?.enableAutorelay === "boolean"
          ? config.enableAutorelay
          : true;
      const enableRelayServer =
        typeof config?.enableRelayServer === "boolean"
          ? config.enableRelayServer
          : false;
      const enableUpnp =
        typeof config?.enableUpnp === "boolean" ? config.enableUpnp : true;
      const pureClientMode =
        typeof config?.pureClientMode === "boolean"
          ? config.pureClientMode
          : false;
      const forceServerMode =
        typeof config?.forceServerMode === "boolean"
          ? config.forceServerMode
          : false;

      const payload: Record<string, unknown> = {
        port,
        bootstrapNodes,
        enableAutonat,
        autonatProbeIntervalSecs: autonatProbeIntervalSeconds,
        chunkSizeKb,
        cacheSizeMb,
        enableAutorelay,
        enableRelayServer,
        enableUpnp,
        pureClientMode,
        forceServerMode,
      };

      if (config?.autonatServers && config.autonatServers.length > 0) {
        payload.autonatServers = config.autonatServers;
      }
      if (config?.preferredRelays && config.preferredRelays.length > 0) {
        payload.preferredRelays = config.preferredRelays;
      }
      if (
        typeof config?.proxyAddress === "string" &&
        config.proxyAddress.trim().length > 0
      ) {
        payload.proxyAddress = config.proxyAddress.trim();
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
    originalFileName?: string,
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
          },
        );

        // Add timeout to reject the promise if publishing takes too long
        timeoutId = setTimeout(() => {
          reject(
            new Error(
              "File publishing timeout - no published_file event received",
            ),
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
          },
        );

        // Add timeout to reject the promise if download takes too long
        setTimeout(() => {
          reject(
            new Error("Download timeout - no file_content event received"),
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
          },
        );

        // Trigger the backend download AFTER setting up the listener
        await invoke("download_blocks_from_network", {
          fileMetadata,
          downloadPath: resolvedStoragePath,
        });
      } catch (error) {
        console.error(
          "🔽 Frontend: download_blocks_from_network invoke failed:",
          error,
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
        "DHT service peerId not set, service may not be initialized",
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
    timeoutMs = 10_000,
  ): Promise<void> {
    const trimmed = fileHash.trim();
    if (!trimmed) {
      throw new Error("File hash is required");
    }

    try {
      // Trigger the backend search - results will come via progressive events
      // Events (via search:* channels): search:started, search:metadata_found,
      // search:providers_found, search:seeder_general_info, search:seeder_file_info,
      // search:complete, search:timeout
      console.log("🔍 Frontend triggering search_file_metadata for:", trimmed);
      await invoke<void>("search_file_metadata", {
        fileHash: trimmed,
        timeoutMs,
      });
      console.log("🔍 Search triggered, awaiting progressive events...");
    } catch (error) {
      console.error("Failed to trigger search:", error);
      throw error;
    }
  }
}

// Export singleton instance
export const dhtService = DhtService.getInstance();
