// DHT configuration and utilities
import { invoke } from "@tauri-apps/api/core";

// Default bootstrap nodes for network connectivity
export const DEFAULT_BOOTSTRAP_NODES = [
  "/ip4/145.40.118.135/tcp/4001/p2p/QmcZf59bWwK5XFi76CZX8cbJ4BhTzzA3gU1ZjYZcYW3dwt",
  "/ip4/139.178.91.71/tcp/4001/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
  "/ip4/147.75.87.27/tcp/4001/p2p/QmbLHAnMoJPWSCR5Zhtx6BHJX9KiKNN6tpvbUcqanj75Nb",
  "/ip4/139.178.65.157/tcp/4001/p2p/QmQCU2EcMqAqQPR2i9bChDtGNJchTbq5TbXJJ16u19uLTa",
  "/ip4/104.131.131.82/tcp/4001/p2p/QmaCpDMGvV2BGHeYERUEnRQAwe3N8SzbUtfsmvsqQLuvuJ",
  "/ip4/54.198.145.146/tcp/4001/p2p/12D3KooWNHdYWRTe98KMF1cDXXqGXvNjd1SAchDaeP5o4MsoJLu2",
];

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
}

export interface FileMetadata {
  fileHash: string;
  fileName: string;
  fileSize: number;
  seeders: string[];
  createdAt: number;
  mimeType?: string;
  isEncrypted: boolean;
  encryptionMethod?: string;
  keyFingerprint?: string;
  version?: number;
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
      bootstrapNodes = DEFAULT_BOOTSTRAP_NODES;
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

  async publishFile(metadata: FileMetadata): Promise<void> {
    if (!this.peerId) {
      throw new Error("DHT not started");
    }

    try {
      await invoke("publish_file_metadata", {
        fileHash: metadata.fileHash,
        fileName: metadata.fileName,
        fileSize: metadata.fileSize,
        mimeType: metadata.mimeType,
      });
    } catch (error) {
      console.error("Failed to publish file:", error);
      throw error;
    }
  }

  async searchFile(fileHash: string): Promise<void> {
    if (!this.peerId) {
      throw new Error("DHT not started");
    }

    try {
      await invoke("search_file_metadata", { fileHash, timeoutMs: 0 });
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

    try {
      await invoke("connect_to_peer", { peerAddress });
    } catch (error) {
      console.error("Failed to connect to peer:", error);
      throw error;
    }
  }

  async getEvents(): Promise<string[]> {
    if (!this.peerId) {
      return [];
    }

    try {
      const events = await invoke<string[]>("get_dht_events");
      return events;
    } catch (error) {
      console.error("Failed to get DHT events:", error);
      return [];
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
    timeoutMs = 10_000
  ): Promise<FileMetadata | null> {
    const trimmed = fileHash.trim();
    if (!trimmed) {
      throw new Error("File hash is required");
    }

    try {
      const result = await invoke<FileMetadata | null>("search_file_metadata", {
        fileHash: trimmed,
        timeoutMs,
      });

      if (!result) {
        return null;
      }

      return {
        ...result,
        seeders: Array.isArray(result.seeders) ? result.seeders : [],
      };
    } catch (error) {
      console.error("Failed to search file metadata:", error);
      throw error;
    }
  }
}

// Export singleton instance
export const dhtService = DhtService.getInstance();
