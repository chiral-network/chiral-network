import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { peers, networkStats, networkConnected } from './stores';
import type { PeerInfo } from './stores';
import { toasts } from './toastStore';

class DhtService {
  private pollInterval: number | null = null;
  private peerDiscoveryUnlisten: (() => void) | null = null;
  private pingSentUnlisten: (() => void) | null = null;
  private pingReceivedUnlisten: (() => void) | null = null;
  private pongReceivedUnlisten: (() => void) | null = null;

  async start(): Promise<void> {
    try {
      const result = await invoke<string>('start_dht');
      console.log('DHT started:', result);
      networkConnected.set(true);
      
      // Listen for peer discovery events
      this.peerDiscoveryUnlisten = await listen<PeerInfo[]>('peer-discovered', (event) => {
        console.log('Peers discovered:', event.payload);
        // The payload already has the correct structure from Rust
        peers.set(event.payload);
      });
      
      // Listen for ping events
      this.pingSentUnlisten = await listen<string>('ping-sent', (event) => {
        toasts.show('Ping sent to peer', 'success');
      });
      
      this.pingReceivedUnlisten = await listen<string>('ping-received', (event) => {
        toasts.show('Ping received from peer', 'info');
      });
      
      this.pongReceivedUnlisten = await listen<string>('pong-received', (event) => {
        toasts.show('Pong received from peer', 'success');
      });
      
      // Start polling for network stats
      this.startPolling();
      
      // Get and log our peer ID
      const peerId = await this.getPeerId();
      if (peerId) {
        console.log('Our Peer ID:', peerId);
      }
    } catch (error) {
      console.error('Failed to start DHT:', error);
      throw error;
    }
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
    } catch (error) {
      console.error('Failed to stop DHT:', error);
      throw error;
    }
  }
  
  async getPeerId(): Promise<string | null> {
    try {
      return await invoke<string | null>('get_peer_id');
    } catch (error) {
      console.error('Failed to get peer ID:', error);
      return null;
    }
  }
  
  async pingPeer(peerId: string): Promise<string> {
    try {
      const result = await invoke<string>('ping_peer', { peerId });
      console.log('Ping result:', result);
      return result;
    } catch (error) {
      console.error('Failed to ping peer:', error);
      throw error;
    }
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
        invoke<{ connected_peers: number; total_peers: number }>('get_network_stats')
      ]);
      
      // Convert snake_case to camelCase
      const formattedPeers = peerList.map(peer => ({
        ...peer,
        lastSeen: new Date(peer.lastSeen)
      }));
      
      peers.set(formattedPeers);
      networkStats.set({
        connectedPeers: stats.connected_peers,
        totalPeers: stats.total_peers
      });
    } catch (error) {
      console.error('Failed to update network info:', error);
    }
  }
}

export const dhtService = new DhtService();
