import { writable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Export network status store
export const networkStatus = writable<"connected" | "disconnected">(
  "disconnected"
);

// Track real-time DHT connection status
let dhtConnectedPeerCount = 0;

// Set up event listeners for DHT peer connection changes
export function setupDhtEventListeners(): void {
  // Listen for peer connections
  listen<{ peer_id: string; address: string }>("dht_peer_connected", () => {
    dhtConnectedPeerCount++;
    updateNetworkStatusFromDht();
  }).catch((err) =>
    console.error("Failed to listen to dht_peer_connected:", err)
  );

  // Listen for peer disconnections
  listen<{ peer_id: string }>("dht_peer_disconnected", () => {
    dhtConnectedPeerCount = Math.max(0, dhtConnectedPeerCount - 1);
    updateNetworkStatusFromDht();
  }).catch((err) =>
    console.error("Failed to listen to dht_peer_disconnected:", err)
  );
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
      invoke<boolean>("is_dht_running").catch(() => false),
      invoke<number>("get_dht_peer_count").catch(() => 0),
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
  } catch (error) {
    console.error("Failed to update network status:", error);
    networkStatus.set("disconnected");
  }
}

// Start periodic monitoring
export function startNetworkMonitoring(): () => void {
  // Set up event listeners for real-time DHT connection updates
  setupDhtEventListeners();

  // Check immediately
  updateNetworkStatus();

  // Check every 3 seconds as fallback
  const interval = setInterval(updateNetworkStatus, 3000);

  // Return cleanup function
  return () => {
    clearInterval(interval);
  };
}
