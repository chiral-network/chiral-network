import { writable, type Writable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";

function createClientId(): string {
  // Check if crypto.randomUUID exists and call it directly to preserve 'this' context
  if (globalThis?.crypto?.randomUUID) {
    return globalThis.crypto.randomUUID();
  }

  // Fallback for environments without crypto.randomUUID
  const timePart = Date.now().toString(36);
  const randomPart = Math.random().toString(36).slice(2, 10);
  return `client-${timePart}-${randomPart}`;
}

export class SignalingService {
  private clientId: string;
  private dhtConnected: boolean = false;

  public connected: Writable<boolean> = writable(false);
  public peers: Writable<string[]> = writable([]);

  constructor() {
    this.clientId = createClientId();
  }

  async connect(): Promise<void> {
    return new Promise((resolve) => {
      (async () => {
        try {
          // Check if DHT is running and get our peer ID
          const peerId = await invoke("get_dht_peer_id");
          if (peerId) {
            this.dhtConnected = true;
            this.connected.set(true);

            // Get connected peers from DHT
            this.refreshPeers();
            resolve();
          } else {
            this.dhtConnected = false;
            this.connected.set(false);
            // Don't reject - just resolve since this is expected when DHT isn't running
            resolve();
          }
        } catch (error) {
          this.dhtConnected = false;
          this.connected.set(false);
          // Don't reject - resolve since this is expected when DHT isn't running
          resolve();
        }
      })();
    });
  }

  private async refreshPeers(): Promise<void> {
    try {
      // Get connected peers from DHT
      const peers = (await invoke("get_dht_connected_peers")) as string[];
      this.peers.set(peers || []);
    } catch (error) {
      console.error("[SignalingService] Failed to refresh peers:", error);
    }
  }

  //Set a callback for incoming signaling messages (not implemented)
  setOnMessage(_handler: (msg: any) => void) {
    // TODO: Implement message handling if needed
  }

  // Send a signaling message to another peer via DHT
  async send(msg: any): Promise<void> {
    if (!this.dhtConnected) {
      console.warn(
        "[SignalingService] Cannot send message - DHT signaling not connected"
      );
      throw new Error(
        "DHT signaling not connected - please ensure DHT is running"
      );
    }

    try {
      const signalingMessage = {
        ...msg,
        from: this.clientId,
        timestamp: Date.now(),
        type: "webrtc_signaling",
      };

      // Send the signaling message through DHT to the target peer
      await invoke("send_dht_message", {
        peerId: msg.to,
        message: signalingMessage,
      });
    } catch (error) {
      console.error(
        "[SignalingService] Failed to send DHT signaling message:",
        error
      );
      throw error;
    }
  }

  disconnect(): void {
    this.dhtConnected = false;
    this.connected.set(false);
    this.peers.set([]);
  }

  // Expose this client’s ID
  getClientId(): string {
    return this.clientId;
  }
}
