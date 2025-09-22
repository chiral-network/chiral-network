// DHT configuration and utilities
import { invoke } from "@tauri-apps/api/core";

// Default bootstrap nodes for network connectivity
export const DEFAULT_BOOTSTRAP_NODES = [
  "/ip4/145.40.118.135/tcp/4001/p2p/QmcZf59bWwK5XFi76CZX8cbJ4BhTzzA3gU1ZjYZcYW3dwt",
  "/ip4/139.178.91.71/tcp/4001/p2p/QmNnooDu7bfjPFoTZYxMNLWUQJyrVwtbZg5gBMjTezGAJN",
  "/ip4/147.75.87.27/tcp/4001/p2p/QmbLHAnMoJPWSCR5Zhtx6BHJX9KiKNN6tpvbUcqanj75Nb",
  "/ip4/139.178.65.157/tcp/4001/p2p/QmQCU2EcMqAqQPR2i9bChDtGNJchTbq5TbXJJ16u19uLTa",
  "/ip4/104.131.131.82/tcp/4001/p2p/QmaCpDMGvV2BGHeYERUEnRQAwe3N8SzbUtfsmvsqQLuvuJ",
];

export interface DhtConfig {
  port: number;
  bootstrapNodes: string[];
  showMultiaddr?: boolean;
}

export interface FileMetadata {
  fileHash: string;
  fileName: string;
  fileSize: number;
  seeders: string[];
  createdAt: number;
  mimeType?: string;
}

export interface DhtHealth {
  peerCount: number;
  lastBootstrap: number | null;
  lastPeerEvent: number | null;
  lastError: string | null;
  lastErrorAt: number | null;
  bootstrapFailures: number;
  listenAddrs: string[];
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
      console.log("Using default bootstrap nodes for network connectivity");
    } else {
      console.log(`Using ${bootstrapNodes.length} custom bootstrap nodes`);
    }

    try {
      const peerId = await invoke<string>("start_dht_node", {
        port,
        bootstrapNodes,
      });
      this.peerId = peerId;
      this.port = port;
      console.log("DHT started with peer ID:", this.peerId);
      console.log("Your multiaddr for others to connect:", this.getMultiaddr());
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
      console.log("DHT stopped");
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
      console.log("Published file metadata:", metadata.fileHash);
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
      await invoke("search_file_metadata", { fileHash });
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

    try {
      await invoke("connect_to_peer", { peerAddress });
      console.log("Connecting to peer:", peerAddress);
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
}

// Export singleton instance
export const dhtService = DhtService.getInstance();
