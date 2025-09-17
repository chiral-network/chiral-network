import { writable, type Writable } from "svelte/store";

export class SignalingService {
  private ws: WebSocket | null = null;
  private clientId: string;
  
  public connected: Writable<boolean> = writable(false);
  public peers: Writable<string[]> = writable([]);

  constructor(private url: string = "ws://localhost:3000") {
    this.clientId = crypto.randomUUID();
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

  disconnect(): void {
    this.ws?.close();
    this.ws = null;
    this.connected.set(false);
    this.peers.set([]);
  }
}