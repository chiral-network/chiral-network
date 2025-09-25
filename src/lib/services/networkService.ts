import { writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';

// Export network status store
export const networkStatus = writable<"connected" | "disconnected">("disconnected");
export const natStatus = writable<string | null>(null);

// Function to update network status
export async function updateNetworkStatus(): Promise<void> {
  try {
    // Check Geth and DHT status in parallel
    const [isGethRunning, dhtPeers, blockchainPeers, nat] = await Promise.all([
      invoke<boolean>('is_geth_running').catch(() => false),
      invoke<number>('get_dht_peer_count').catch(() => 0),
      invoke<number>('get_network_peer_count').catch(() => 0),
      invoke<string | null>('get_nat_status').catch(() => null)
    ]);
    
    // Determine network connection status
    if (isGethRunning && (dhtPeers > 0 || blockchainPeers > 0)) {
      networkStatus.set("connected");
    } else {
      networkStatus.set("disconnected");
    }
    natStatus.set(nat);
    
    console.log(`🌐 Network status updated: ${isGethRunning ? 'Node running' : 'Node stopped'}, DHT peers: ${dhtPeers}, Blockchain peers: ${blockchainPeers}, NAT: ${nat ?? 'unknown'}`);
    
  } catch (error) {
    console.error('Failed to update network status:', error);
    networkStatus.set("disconnected");
  }
}

// Start periodic monitoring
export function startNetworkMonitoring(): () => void {
  console.log('🔄 Starting network status monitoring');
  
  // Check immediately
  updateNetworkStatus();
  
  // Check every 3 seconds
  const interval = setInterval(updateNetworkStatus, 3000);
  
  // Return cleanup function
  return () => {
    console.log('⏹️ Stopping network status monitoring');
    clearInterval(interval);
  };
}