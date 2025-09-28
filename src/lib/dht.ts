// DHT configuration and utilities
import { invoke } from "@tauri-apps/api/core";

const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

export const DEFAULT_BOOTSTRAP_DOMAINS = ["bootstrap.chiral.network"];

type ResolveOptions = {
  forceRefresh?: boolean;
};

const bootstrapCache = new Map<string, string[]>();
const bootstrapErrors = new Map<string, string>();

function normalizeDomains(domains?: string[] | null): string[] {
  const source = domains && domains.length > 0 ? domains : DEFAULT_BOOTSTRAP_DOMAINS;
  return source
    .map((domain) => domain.trim())
    .filter((domain) => domain.length > 0);
}

function createCacheKey(domains: string[]): string {
  return [...domains]
    .map((domain) => domain.toLowerCase())
    .sort()
    .join(",");
}

export function getBootstrapDiscoveryError(domains?: string[]): string | null {
  if (!domains) {
    return bootstrapErrors.size > 0 ? Array.from(bootstrapErrors.values())[0] : null;
  }
  const normalized = normalizeDomains(domains);
  if (normalized.length === 0) {
    return null;
  }
  const key = createCacheKey(normalized);
  return bootstrapErrors.get(key) ?? null;
}

export function clearBootstrapCache(domains?: string[]): void {
  if (!domains) {
    bootstrapCache.clear();
    bootstrapErrors.clear();
    return;
  }
  const normalized = normalizeDomains(domains);
  if (normalized.length === 0) {
    return;
  }
  const key = createCacheKey(normalized);
  bootstrapCache.delete(key);
  bootstrapErrors.delete(key);
}

export async function resolveBootstrapNodes(
  domains?: string[],
  options?: ResolveOptions,
): Promise<string[]> {
  const normalized = normalizeDomains(domains);
  if (normalized.length === 0) {
    return [];
  }

  const key = createCacheKey(normalized);
  if (!options?.forceRefresh && bootstrapCache.has(key)) {
    const cached = bootstrapCache.get(key);
    return cached ? [...cached] : [];
  }

  if (!isTauri) {
    const cached = bootstrapCache.get(key);
    return cached ? [...cached] : [];
  }

  try {
    const nodes = await invoke<string[]>("resolve_bootstrap_nodes", { domains: normalized });
    const sanitized = nodes.filter((addr) => addr.length > 0);
    bootstrapCache.set(key, sanitized);
    bootstrapErrors.delete(key);
    return [...sanitized];
  } catch (error) {
    console.error("Failed to resolve bootstrap nodes from DNS:", error);
    const message = error instanceof Error ? error.message : String(error);
    bootstrapErrors.set(key, message);
    const fallback = bootstrapCache.get(key);
    return fallback ? [...fallback] : [];
  }
}

export interface DhtConfig {
  port: number;
  bootstrapNodes?: string[];
  bootstrapDomains?: string[];
  forceBootstrapRefresh?: boolean;
  showMultiaddr?: boolean;
  proxyAddress?: string; // The SOCKS5 address for routing (e.g., "127.0.0.1:9050")
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
  private bootstrapNodes: string[] = [];
  private bootstrapDomains: string[] = [...DEFAULT_BOOTSTRAP_DOMAINS];

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
    const configuredNodes = config?.bootstrapNodes?.filter((addr) => addr.length > 0) ?? [];
    const discoveryDomains = config?.bootstrapDomains;
    const forceRefresh = config?.forceBootstrapRefresh ?? false;

    let bootstrapNodes = configuredNodes;

    if (bootstrapNodes.length === 0) {
      bootstrapNodes = await resolveBootstrapNodes(discoveryDomains, { forceRefresh });
      if (bootstrapNodes.length === 0) {
        console.warn(
          "No bootstrap nodes discovered via DNS; starting without predefined peers",
        );
      } else {
        console.log(`Discovered ${bootstrapNodes.length} bootstrap node(s) via DNS`);
      }
    } else {
      console.log(
        `Using ${bootstrapNodes.length} bootstrap node(s) provided by configuration`,
      );
    }

    this.bootstrapNodes = [...bootstrapNodes];
    this.bootstrapDomains =
      discoveryDomains && discoveryDomains.length > 0
        ? [...discoveryDomains]
        : [...DEFAULT_BOOTSTRAP_DOMAINS];

    try {
      const peerId = await invoke<string>("start_dht_node", {
        port,
        bootstrapNodes,
        proxyAddress: config?.proxyAddress,
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

  getBootstrapNodes(): string[] {
    return [...this.bootstrapNodes];
  }

  getBootstrapDomains(): string[] {
    return [...this.bootstrapDomains];
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
