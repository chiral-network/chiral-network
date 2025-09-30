import { writable, type Writable } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";

function createClientId(): string {
  const randomUUID = globalThis?.crypto?.randomUUID;
  if (typeof randomUUID === "function") {
    return randomUUID();
  }

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
    return new Promise((resolve, reject) => {
      (async () => {
        try {
          console.log("[SignalingService] Initializing DHT-based signaling");
          console.log("[SignalingService] Client ID:", this.clientId);

          // Check if DHT is running and get our peer ID
          const peerId = await invoke("get_dht_peer_id");
          if (peerId) {
            this.dhtConnected = true;
            this.connected.set(true);
            console.log(
              "[SignalingService] DHT signaling connected with peer ID:",
              peerId
            );

            // Get connected peers from DHT
            this.refreshPeers();
            resolve();
          } else {
            throw new Error("DHT not available for signaling");
          }
        } catch (error) {
          console.error(
            "[SignalingService] DHT signaling connection failed:",
            error
          );
          reject(error);
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
    if (!this.dhtConnected) throw new Error("DHT signaling not connected");

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

      console.log(
        "[SignalingService] Sent DHT signaling message:",
        signalingMessage
      );
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
