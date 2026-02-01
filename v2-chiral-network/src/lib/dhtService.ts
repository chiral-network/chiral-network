import { invoke } from '@tauri-apps/api/core';
import { peers, networkStats, networkConnected } from './stores';
import type { PeerInfo } from './stores';

class DhtService {
  private pollInterval: number | null = null;

  async start(): Promise<void> {
    try {
      const result = await invoke<string>('start_dht');
      console.log('DHT started:', result);
      networkConnected.set(true);
      
      // Start polling for network stats
      this.startPolling();
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
    } catch (error) {
      console.error('Failed to stop DHT:', error);
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
