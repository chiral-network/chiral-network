import { writable, type Writable } from "svelte/store";

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
  private ws: WebSocket | null = null;
  private clientId: string;

  public connected: Writable<boolean> = writable(false);
  public peers: Writable<string[]> = writable([]);

  // handler for WebRTC signaling messages
  private onMessageHandler: ((msg: any) => void) | null = null;

  constructor(private url: string = "ws://localhost:9000") {
    this.clientId = createClientId();
  }

  async connect(): Promise<void> {
    return new Promise((resolve, reject) => {
      try {
        console.log("[SignalingService] Initializing connection to:", this.url);
        console.log("[SignalingService] Client ID:", this.clientId);

        this.ws = new WebSocket(this.url);

        this.ws.onopen = () => {
          console.log("[SignalingService] WebSocket connection established");
          this.connected.set(true);
          const msg = { type: "register", clientId: this.clientId };
          console.log("[SignalingService] Sending register message:", msg);
          this.ws?.send(JSON.stringify(msg));
          resolve();
        };

        this.ws.onmessage = (event) => {
          console.log("[SignalingService] Received message:", event.data);
          const message = JSON.parse(event.data);

          if (message.type === "peers") {
            this.peers.set(message.peers);
          } else {
            // Forward other messages (offer/answer/candidate)
            this.onMessageHandler?.(message);
          }
        };

        this.ws.onclose = () => {
          console.log("[SignalingService] WebSocket connection closed");
          this.connected.set(false);
          this.peers.set([]);
        };

        this.ws.onerror = (error) => {
          console.error("[SignalingService] WebSocket error:", error);
          reject(error);
        };
      } catch (error) {
        console.error("[SignalingService] Connection failed:", error);
        reject(error);
      }
    });
  }

  //Set a callback for incoming signaling messages
  setOnMessage(handler: (msg: any) => void) {
    this.onMessageHandler = handler;
  }

  // Send a signaling message to another peer
  send(msg: any) {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) {
      throw new Error("WebSocket not connected");
    }
    this.ws.send(JSON.stringify({ ...msg, from: this.clientId }));
  }

  disconnect(): void {
    this.ws?.close();
    this.ws = null;
    this.connected.set(false);
    this.peers.set([]);
  }

  // Expose this client’s ID
  getClientId(): string {
    return this.clientId;
  }

  // Check if connected
  isConnected(): boolean {
    return this.ws !== null && this.ws.readyState === WebSocket.OPEN;
  }
}
