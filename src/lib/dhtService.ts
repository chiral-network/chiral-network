import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { peers, networkStats, networkConnected } from './stores';
import type { PeerInfo } from './stores';
import { toasts } from './toastStore';
import { logger } from './logger';

const log = logger('DHT');

export interface BootstrapNodeStatus {
  address: string;
  reachable: boolean;
}

export interface DhtHealthInfo {
  running: boolean;
  peerId: string | null;
  listeningAddresses: string[];
  connectedPeerCount: number;
  kademliaPeers: number;
  bootstrapNodes: BootstrapNodeStatus[];
  sharedFiles: number;
  protocols: string[];
}

class DhtService {
  private pollInterval: number | null = null;
  private peerDiscoveryUnlisten: (() => void) | null = null;
  private pingSentUnlisten: (() => void) | null = null;
  private pingReceivedUnlisten: (() => void) | null = null;
  private pongReceivedUnlisten: (() => void) | null = null;
  private bootstrapCompleteUnlisten: (() => void) | null = null;

  async start(): Promise<void> {
    // Wire up event listeners BEFORE starting DHT so no events are missed
    await this.ensureRuntimeWiring();

    try {
      const result = await invoke<string>('start_dht');
      log.ok('DHT started:', result);
    } catch (error) {
      const message = error instanceof Error ? error.message : String(error);
      if (!message.includes('already running')) {
        log.error('Failed to start DHT:', error);
        throw error;
      }
      log.info('DHT already running; attaching UI listeners');
    }

    networkConnected.set(true);

    // Immediately fetch current peer list (DHT may already have connections
    // from a previous session or from auto-start before UI attached)
    await this.updateNetworkInfo();

    // Ensure persisted Drive files are re-registered after restart
    try {
      await invoke('reseed_drive_files');
    } catch (error) {
      log.warn('Drive reseed refresh failed:', error);
    }

    const peerId = await this.getPeerId();
    if (peerId) {
      log.info('Our Peer ID:', peerId);
    }

    // Kademlia bootstrap runs async after start_dht returns and discovers
    // the bulk of peers. Do staggered catch-up fetches to populate ASAP.
    setTimeout(() => this.updateNetworkInfo(), 2000);
    setTimeout(() => this.updateNetworkInfo(), 5000);
    setTimeout(() => this.updateNetworkInfo(), 10000);
  }

  async stop(): Promise<void> {
    try {
      await invoke('stop_dht');
      networkConnected.set(false);
      peers.set([]);
      networkStats.set({ connectedPeers: 0, totalPeers: 0 });
      
      // Stop polling
      this.stopPolling();
      
      // Unlisten from peer discovery events
      if (this.peerDiscoveryUnlisten) {
        this.peerDiscoveryUnlisten();
        this.peerDiscoveryUnlisten = null;
      }
      
      if (this.pingSentUnlisten) {
        this.pingSentUnlisten();
        this.pingSentUnlisten = null;
      }
      
      if (this.pingReceivedUnlisten) {
        this.pingReceivedUnlisten();
        this.pingReceivedUnlisten = null;
      }
      
      if (this.pongReceivedUnlisten) {
        this.pongReceivedUnlisten();
        this.pongReceivedUnlisten = null;
      }

      if (this.bootstrapCompleteUnlisten) {
        this.bootstrapCompleteUnlisten();
        this.bootstrapCompleteUnlisten = null;
      }
    } catch (error) {
      log.error('Failed to stop DHT:', error);
      throw error;
    }
  }
  
  async getPeerId(): Promise<string | null> {
    try {
      return await invoke<string | null>('get_peer_id');
    } catch (error) {
      log.error('Failed to get peer ID:', error);
      return null;
    }
  }
  
  async pingPeer(peerId: string): Promise<string> {
    try {
      const result = await invoke<string>('ping_peer', { peerId });
      log.ok('Ping result:', result);
      return result;
    } catch (error) {
      log.error('Failed to ping peer:', error);
      throw error;
    }
  }

  async getHealth(): Promise<DhtHealthInfo> {
    try {
      return await invoke<DhtHealthInfo>('get_dht_health');
    } catch (error) {
      log.error('Failed to get DHT health:', error);
      throw error;
    }
  }

  private async ensureRuntimeWiring(): Promise<void> {
    if (!this.peerDiscoveryUnlisten) {
      this.peerDiscoveryUnlisten = await listen<PeerInfo[]>('peer-discovered', (event) => {
        log.info(`[peer-discovered event] Received ${event.payload.length} peers from backend`);
        peers.set(event.payload);
      });
    }

    if (!this.pingSentUnlisten) {
      this.pingSentUnlisten = await listen<string>('ping-sent', () => {
        // Silent — ping/pong are routine network operations
      });
    }

    if (!this.pingReceivedUnlisten) {
      this.pingReceivedUnlisten = await listen<string>('ping-received', () => {
        // Silent
      });
    }

    if (!this.pongReceivedUnlisten) {
      this.pongReceivedUnlisten = await listen<string>('pong-received', () => {
        // Silent
      });
    }

    if (!this.bootstrapCompleteUnlisten) {
      this.bootstrapCompleteUnlisten = await listen('dht-bootstrap-complete', () => {
        void invoke('reseed_drive_files').catch((error) => {
          log.warn('Drive reseed refresh after bootstrap failed:', error);
        });
      });
    }

    this.startPolling();
  }

  private startPolling(): void {
    if (this.pollInterval !== null) return;
    
    // Update immediately
    this.updateNetworkInfo();
    
    // Then poll every 5 seconds
    this.pollInterval = window.setInterval(() => {
      this.updateNetworkInfo();
    }, 5000);
  }

  private stopPolling(): void {
    if (this.pollInterval !== null) {
      window.clearInterval(this.pollInterval);
      this.pollInterval = null;
    }
  }

  private async updateNetworkInfo(): Promise<void> {
    try {
      const [peerList, stats] = await Promise.all([
        invoke<PeerInfo[]>('get_dht_peers'),
        invoke<{ connectedPeers: number; totalPeers: number }>('get_network_stats')
      ]);

      log.info(`[updateNetworkInfo] Backend returned ${peerList.length} peers, stats: connected=${stats.connectedPeers} total=${stats.totalPeers}`);

      const formattedPeers = peerList.map(peer => ({
        ...peer,
        lastSeen: new Date(peer.lastSeen)
      }));

      peers.set(formattedPeers);
      networkStats.set({
        connectedPeers: stats.connectedPeers,
        totalPeers: stats.totalPeers
      });
    } catch (error) {
      log.error('Failed to update network info:', error);
    }
  }
}

export const dhtService = new DhtService();
