import { writable } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

// Export network status store
export const networkStatus = writable<"connected" | "disconnected">("disconnected");
// Export HTTP server port store
export const httpServerPort = writable<number | null>(null);

// Listen for HTTP server port from backend
export function setupHttpPortListener(): void {
  listen<number>('http_server_port', (event) => {
    const port = event.payload;
    console.log(`🌐 HTTP server port received: ${port}`);
    httpServerPort.set(port);
    
    // Store in localStorage for persistence
    localStorage.setItem('http_server_port', port.toString());
  }).catch((err) => console.error('Failed to listen to http_server_port:', err));
  
  // Try to get port immediately from backend
  invoke<number | null>('get_http_server_port')
    .then(port => {
      if (port) {
        console.log(`📌 Retrieved HTTP server port from backend: ${port}`);
        httpServerPort.set(port);
      } else {
        // Try localStorage as fallback
        const stored = localStorage.getItem('http_server_port');
        if (stored) {
          httpServerPort.set(parseInt(stored, 10));
        }
      }
    })
    .catch(err => console.error('Failed to get HTTP server port:', err));
}

// Track real-time DHT connection status
let dhtConnectedPeerCount = 0;

// Set up event listeners for DHT peer connection changes
export function setupDhtEventListeners(): void {
  // Listen for peer connections
  listen<{ peer_id: string; address: string }>('dht_peer_connected', () => {
    dhtConnectedPeerCount++;
    console.log(`✅ DHT peer connected. Total connected peers: ${dhtConnectedPeerCount}`);
    updateNetworkStatusFromDht();
  }).catch((err) => console.error('Failed to listen to dht_peer_connected:', err));

  // Listen for peer disconnections
  listen<{ peer_id: string }>('dht_peer_disconnected', () => {
    dhtConnectedPeerCount = Math.max(0, dhtConnectedPeerCount - 1);
    console.log(`❌ DHT peer disconnected. Total connected peers: ${dhtConnectedPeerCount}`);
    updateNetworkStatusFromDht();
  }).catch((err) => console.error('Failed to listen to dht_peer_disconnected:', err));
}

// Update network status based on DHT peer count
function updateNetworkStatusFromDht(): void {
  if (dhtConnectedPeerCount > 0) {
    networkStatus.set("connected");
  } else {
    networkStatus.set("disconnected");
  }
}

// Function to update network status
export async function updateNetworkStatus(): Promise<void> {
  try {
    // Check if DHT is running and has peers
    const [isDhtRunning, dhtPeers] = await Promise.all([
      invoke<boolean>('is_dht_running').catch(() => false),
      invoke<number>('get_dht_peer_count').catch(() => 0)
    ]);
    
    // Update cached DHT peer count
    dhtConnectedPeerCount = dhtPeers;
    
    // Determine network connection status
    // Status is "connected" only if DHT is running AND has at least 1 peer
    if (isDhtRunning && dhtPeers > 0) {
      networkStatus.set("connected");
    } else {
      networkStatus.set("disconnected");
    }
    
    console.log(`🌐 Network status updated: DHT ${isDhtRunning ? 'running' : 'stopped'}, DHT peers: ${dhtPeers}`);
    
  } catch (error) {
    console.error('Failed to update network status:', error);
    networkStatus.set("disconnected");
  }
}

// Start periodic monitoring
export function startNetworkMonitoring(): () => void {
  console.log('🔄 Starting network status monitoring');
  
  // Set up event listeners for real-time DHT connection updates
  setupDhtEventListeners();
  setupHttpPortListener(); //for http port rand 
  // Check immediately
  updateNetworkStatus();
  
  // Check every 3 seconds as fallback
  const interval = setInterval(updateNetworkStatus, 3000);
  
  // Return cleanup function
  return () => {
    console.log('⏹️ Stopping network status monitoring');
    clearInterval(interval);
  };
}