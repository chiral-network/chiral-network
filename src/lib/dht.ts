// DHT configuration and utilities
import { invoke } from '@tauri-apps/api/core';

// Default bootstrap node
export const DEFAULT_BOOTSTRAP_NODE = '/ip4/130.245.173.105/tcp/4001';

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

  async start(config?: Partial<DhtConfig>): Promise<string> {
    const port = config?.port || 4001;
    let bootstrapNodes = config?.bootstrapNodes || [];
    
    // Add default bootstrap node if not running on port 4001 (to avoid connecting to self)
    if (bootstrapNodes.length === 0 && port !== 4001) {
      bootstrapNodes = [DEFAULT_BOOTSTRAP_NODE];
      console.log('Using default bootstrap node:', DEFAULT_BOOTSTRAP_NODE);
    }

    try {
      this.peerId = await invoke('start_dht_node', {
        port,
        bootstrapNodes
      });
      this.port = port;
      console.log('DHT started with peer ID:', this.peerId);
      return this.peerId;
    } catch (error) {
      console.error('Failed to start DHT:', error);
      throw error;
    }
  }

  async stop(): Promise<void> {
    try {
      await invoke('stop_dht_node');
      this.peerId = null;
      console.log('DHT stopped');
    } catch (error) {
      console.error('Failed to stop DHT:', error);
      throw error;
    }
  }

  async publishFile(metadata: FileMetadata): Promise<void> {
    if (!this.peerId) {
      throw new Error('DHT not started');
    }

    try {
      await invoke('publish_file_metadata', {
        fileHash: metadata.fileHash,
        fileName: metadata.fileName,
        fileSize: metadata.fileSize,
        mimeType: metadata.mimeType
      });
      console.log('Published file metadata:', metadata.fileHash);
    } catch (error) {
      console.error('Failed to publish file:', error);
      throw error;
    }
  }

  async searchFile(fileHash: string): Promise<void> {
    if (!this.peerId) {
      throw new Error('DHT not started');
    }

    try {
      await invoke('search_file_metadata', { fileHash });
      console.log('Searching for file:', fileHash);
    } catch (error) {
      console.error('Failed to search file:', error);
      throw error;
    }
  }

  async connectPeer(peerAddress: string): Promise<void> {
    if (!this.peerId) {
      throw new Error('DHT not started');
    }

    try {
      await invoke('connect_to_peer', { peerAddress });
      console.log('Connecting to peer:', peerAddress);
    } catch (error) {
      console.error('Failed to connect to peer:', error);
      throw error;
    }
  }

  async getEvents(): Promise<string[]> {
    if (!this.peerId) {
      return [];
    }

    try {
      const events = await invoke<string[]>('get_dht_events');
      return events;
    } catch (error) {
      console.error('Failed to get DHT events:', error);
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
}

// Export singleton instance
export const dhtService = DhtService.getInstance();