import { writable, type Writable, get } from "svelte/store";

function createClientId(): string {
  if (globalThis?.crypto?.randomUUID) return globalThis.crypto.randomUUID();
  const timePart = Date.now().toString(36);
  const randomPart = Math.random().toString(36).slice(2, 10);
  return `client-${timePart}-${randomPart}`;
}

export type BackendKind = "dht" | "ws" | "none";
export type ConnState = "connecting" | "connected" | "disconnected";

export interface SignalingOptions {
  url?: string; // WebSocket URL
  preferDht?: boolean;
  wsHeartbeatInterval?: number; // ms
  peerTtlMs?: number; // ms to consider a peer stale
  persistPeers?: boolean; // persist peers to localStorage
}

export class SignalingService {
  private ws: WebSocket | null = null;
  private clientId: string;

  // public stores
  public connected: Writable<boolean> = writable(false);
  public peers: Writable<string[]> = writable([]);
  public backend: Writable<BackendKind> = writable("none");
  public state: Writable<ConnState> = writable("disconnected");

  // internals
  private onMessageHandler: ((msg: any) => void) | null = null;
  private dhtService: any = null;
  private dhtAvailable = false;
  private preferDht = true;

  private wsUrl: string;
  private wsReconnectAttempts = 0;
  private wsReconnectBase = 1000;
  private wsReconnectMax = 30000;
  private wsClosedByUser = false;

  // persistent peer tracking (in-memory map with timestamps)
  private peersMap: Map<string, number> = new Map();
  private readonly PERSIST_KEY = "chiral:signaling:peers";

  // outgoing message queue while connecting / switching backends
  private outQueue: Array<{
    msg: any;
    opts?: { prefer?: "dht" | "ws" | "auto" };
  }> = [];

  // heartbeat + garbage collection
  private wsHeartbeatIntervalMs: number;
  private wsHeartbeatTimer: any = null;
  private peerTtlMs: number;
  private peerGcTimer: any = null;
  private persistPeersFlag = true;

  constructor(opts: SignalingOptions = {}) {
    this.clientId = createClientId();
    this.wsUrl = opts.url ?? "ws://localhost:9000";
    this.preferDht = opts.preferDht ?? true;
    this.wsHeartbeatIntervalMs = opts.wsHeartbeatInterval ?? 20000; // 20s
    this.peerTtlMs = opts.peerTtlMs ?? 1000 * 60 * 60 * 24; // 24h
    this.persistPeersFlag = opts.persistPeers ?? true;

    // hydrate persisted peers immediately
    this.loadPersistedPeers();

    // start periodic GC for peers
    this.startPeerGc();
  }

  /* ----------------------
     Initialization & backend selection
     ---------------------- */
  async connect(): Promise<void> {
    this.state.set("connecting");
    try {
      await this.detectDht();
      // prefer DHT when available
      if (this.preferDht && this.dhtAvailable) {
        try {
          await this.connectDht();
          this.flushQueue();
          return;
        } catch (e) {
          console.warn("DHT connect failed, falling back to WS", e);
        }
      }
      await this.connectWebSocket();
      this.flushQueue();
    } catch (e) {
      this.state.set("disconnected");
      throw e;
    }
  }

  private async detectDht(): Promise<void> {
    this.dhtAvailable = false;
    // Try to dynamically detect a frontend DHT service
    try {
      const mod = await import("$lib/dht").catch(() => null);
      if (mod) {
        // prefer explicit dht export if present
        this.dhtService = (mod as any).dhtService ?? mod;
      }
    } catch (e) {
      this.dhtService = null;
    }

    // If no frontend DHT, try backend (Tauri) to see if DHT is running
    if (!this.dhtService) {
      try {
        const core = await import("@tauri-apps/api/core").catch(() => null);
        if (core && typeof (core as any).invoke === "function") {
          const id = await (core as any)
            .invoke("get_dht_peer_id")
            .catch(() => null);
          if (id) {
            this.dhtAvailable = true;
            return;
          }
        }
      } catch (e) {
        // ignore
      }
    }

    if (this.dhtService) this.dhtAvailable = true;
  }

  /* ----------------------
     Persistent peers helpers
     ---------------------- */
  private persistPeers() {
    if (!this.persistPeersFlag) return;
    try {
      const arr = Array.from(this.peersMap.entries()).map(([id, ts]) => ({
        id,
        ts,
      }));
      if (typeof localStorage !== "undefined") {
        localStorage.setItem(this.PERSIST_KEY, JSON.stringify(arr));
      }
    } catch (e) {
      // ignore persistence errors
      console.warn("persistPeers failed", e);
    }
  }

  private loadPersistedPeers() {
    try {
      if (!this.persistPeersFlag) return;
      if (typeof localStorage === "undefined") return;
      const raw = localStorage.getItem(this.PERSIST_KEY);
      if (!raw) return;
      const arr = JSON.parse(raw) as { id: string; ts: number }[];
      const now = Date.now();
      for (const p of arr) {
        // skip stale entries
        if (now - (p.ts ?? 0) <= this.peerTtlMs)
          this.peersMap.set(p.id, p.ts ?? now);
      }
      this.emitPeersFromMap();
    } catch (e) {
      console.warn("loadPersistedPeers failed", e);
    }
  }

  private mergePeers(incoming: string[]) {
    const now = Date.now();
    let changed = false;
    for (const id of incoming) {
      if (!this.peersMap.has(id)) {
        changed = true;
        this.peersMap.set(id, now);
      } else {
        // update lastSeen timestamp
        this.peersMap.set(id, now);
      }
    }
    if (changed) {
      this.emitPeersFromMap();
      this.persistPeers();
    } else {
      // still update writable with fresh ordering
      this.emitPeersFromMap();
    }
  }

  private emitPeersFromMap() {
    const ids = Array.from(this.peersMap.keys()).sort();
    this.peers.set(ids);
  }

  private startPeerGc() {
    // run once per hour by default
    const interval = Math.max(1000 * 60 * 30, Math.floor(this.peerTtlMs / 24));
    this.peerGcTimer = setInterval(() => this.gcPeers(), interval);
  }

  private gcPeers() {
    const now = Date.now();
    let removed = false;
    for (const [id, ts] of Array.from(this.peersMap.entries())) {
      if (now - ts > this.peerTtlMs) {
        this.peersMap.delete(id);
        removed = true;
      }
    }
    if (removed) {
      this.emitPeersFromMap();
      this.persistPeers();
    }
  }

  /* ----------------------
     DHT integration (shallow but usable)
     ---------------------- */
  private async connectDht(): Promise<void> {
    this.backend.set("dht");
    this.state.set("connecting");

    if (!this.dhtAvailable) throw new Error("DHT not available");

    // Hook into dhtService event API if present
    try {
      if (this.dhtService) {
        if (typeof this.dhtService.on === "function") {
          try {
            this.dhtService.on("peers", (list: any[]) => {
              const ids = (list || []).map((p: any) =>
                typeof p === "string" ? p : p.peerId || p.id || p.clientId
              );
              this.mergePeers(ids);
            });
          } catch (_) {
            /* ignore */
          }
        }

        if (typeof this.dhtService.onSignal === "function") {
          this.dhtService.onSignal((msg: any) => this.handleIncoming(msg));
        }

        // some adapters expose an event emitter name 'signal'
        if (
          typeof this.dhtService.on === "function" &&
          typeof (this.dhtService as any).on === "function"
        ) {
          try {
            (this.dhtService as any).on("signal", (msg: any) =>
              this.handleIncoming(msg)
            );
          } catch (_) {
            /* ignore */
          }
        }
      }

      // Also try to listen for backend events via Tauri if available
      try {
        const evt = await import("@tauri-apps/api/event").catch(() => null);
        if (evt && typeof (evt as any).listen === "function") {
          (evt as any)
            .listen("dht_peer_update", (e: any) => {
              const payload = e.payload;
              if (payload && Array.isArray(payload.peers))
                this.mergePeers(payload.peers);
            })
            .catch(() => {});
        }
      } catch (_) {
        /* ignore */
      }

      this.state.set("connected");
      this.connected.set(true);
      console.debug("[SignalingService] connected via DHT");

      // ensure heartbeat is not running for WS
      this.stopWsHeartbeat();
    } catch (e) {
      this.state.set("disconnected");
      this.connected.set(false);
      throw e;
    }
  }

  private async sendViaDht(msg: any): Promise<void> {
    if (!this.dhtService) throw new Error("DHT service unavailable");
    if (typeof this.dhtService.sendSignal === "function") {
      await this.dhtService.sendSignal(msg);
      return;
    }
    if (typeof this.dhtService.send === "function") {
      await this.dhtService.send(msg);
      return;
    }
    throw new Error("DHT backend has no send method");
  }

  /* ----------------------
     WebSocket connection with reconnect/backoff
     ---------------------- */
  private async connectWebSocket(): Promise<void> {
    if (this.ws)
      try {
        this.ws.close();
      } catch (e) {}

    this.backend.set("ws");
    this.state.set("connecting");
    this.wsClosedByUser = false;

    return new Promise((resolve, reject) => {
      try {
        this.ws = new WebSocket(this.wsUrl);

        // Set a connection timeout
        const connectionTimeout = setTimeout(() => {
          if (this.ws) {
            this.ws.close();
          }
          this.state.set("disconnected");
          this.connected.set(false);
          reject(new Error("WebSocket connection timeout"));
        }, 2000); // 2 second timeout

        this.ws.onopen = () => {
          clearTimeout(connectionTimeout);
          this.wsReconnectAttempts = 0;
          this.state.set("connected");
          this.connected.set(true);
          // register
          try {
            this.ws?.send(
              JSON.stringify({ type: "register", clientId: this.clientId })
            );
          } catch (_) {}
          console.debug("[SignalingService] WS connected");
          // start heartbeat
          this.startWsHeartbeat();
          resolve();
        };

        this.ws.onmessage = (ev: MessageEvent) => {
          try {
            const data = JSON.parse(ev.data);
            // respond to pings/pongs at app level
            if (data && data.type === "ping") {
              try {
                this.ws?.send(
                  JSON.stringify({
                    type: "pong",
                    ts: Date.now(),
                    from: this.clientId,
                  })
                );
              } catch (_) {}
              return;
            }
            if (data && data.type === "pong") {
              // could update latency metrics here
              return;
            }
            this.handleIncoming(data);
          } catch (e) {
            console.warn("[SignalingService] invalid ws message", e);
          }
        };

        this.ws.onclose = () => {
          clearTimeout(connectionTimeout);
          if (this.wsClosedByUser) {
            this.state.set("disconnected");
            this.connected.set(false);
            this.backend.set("none");
            this.stopWsHeartbeat();
            return;
          }
          this.state.set("disconnected");
          this.connected.set(false);
          console.warn("[SignalingService] WS closed, scheduling reconnect");
          this.stopWsHeartbeat();
          this.scheduleWsReconnect();
        };

        this.ws.onerror = (e) => {
          clearTimeout(connectionTimeout);
          console.error("[SignalingService] WS error", e);
          this.state.set("disconnected");
          this.connected.set(false);
          reject(new Error("WebSocket connection failed"));
        };
      } catch (e) {
        console.error("[SignalingService] failed to open WS", e);
        this.state.set("disconnected");
        reject(e);
      }
    });
  }

  private scheduleWsReconnect() {
    this.wsReconnectAttempts++;
    const delay = Math.min(
      this.wsReconnectBase * 2 ** (this.wsReconnectAttempts - 1),
      this.wsReconnectMax
    );
    setTimeout(() => {
      this.connectWebSocket().catch(() => {});
    }, delay);
  }

  private startWsHeartbeat() {
    this.stopWsHeartbeat();
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN) return;
    this.wsHeartbeatTimer = setInterval(() => {
      try {
        this.ws?.send(
          JSON.stringify({ type: "ping", ts: Date.now(), from: this.clientId })
        );
      } catch (e) {
        /* ignore */
      }
    }, this.wsHeartbeatIntervalMs);
  }

  private stopWsHeartbeat() {
    try {
      if (this.wsHeartbeatTimer) clearInterval(this.wsHeartbeatTimer);
    } catch (_) {}
    this.wsHeartbeatTimer = null;
  }

  private async sendViaWs(msg: any): Promise<void> {
    if (!this.ws || this.ws.readyState !== WebSocket.OPEN)
      throw new Error("WS not open");
    this.ws.send(JSON.stringify({ ...msg, from: this.clientId }));
  }

  private enqueue(msg: any, opts?: { prefer?: "dht" | "ws" | "auto" }) {
    this.outQueue.push({ msg, opts });
  }

  private async flushQueue() {
    if (!this.outQueue.length) return;
    const queue = this.outQueue.splice(0);
    for (const item of queue) {
      try {
        await this.send(item.msg, item.opts);
      } catch (e) {
        console.warn(
          "[SignalingService] failed to flush queued message, re-enqueueing",
          e
        );
        // conservative: re-enqueue at front
        this.outQueue.unshift(item);
        // stop processing further to avoid tight loop
        break;
      }
    }
  }

  /* ----------------------
     Public API
     ---------------------- */
  setOnMessage(handler: (msg: any) => void) {
    this.onMessageHandler = handler;
  }

  async send(msg: any, opts?: { prefer?: "dht" | "ws" | "auto" }) {
    const prefer = opts?.prefer ?? "auto";

    // if a backend is not connected, queue the message and attempt connect
    const currentBackend = get(this.backend);
    const isWsReady = this.ws && this.ws.readyState === WebSocket.OPEN;

    // If preferDht explicitly and dht available, try DHT first
    if (
      prefer === "dht" ||
      (prefer === "auto" && this.dhtAvailable && this.preferDht)
    ) {
      try {
        await this.sendViaDht(msg);
        return;
      } catch (e) {
        console.warn("sendViaDht failed, falling back", e);
      }
    }

    // If WS is ready, send
    if (isWsReady) {
      try {
        await this.sendViaWs(msg);
        return;
      } catch (e) {
        console.warn("sendViaWs failed", e);
      }
    }

    // If WS not ready but we can connect, attempt to connect and queue
    if (!isWsReady && currentBackend !== "ws") {
      try {
        this.connectWebSocket().catch(() => {});
      } catch (_) {}
    }

    // If DHT is available and prefer is ws fallback, try DHT as last resort
    if (this.dhtAvailable && (prefer === "ws" || prefer === "auto")) {
      try {
        await this.sendViaDht(msg);
        return;
      } catch (e) {
        /* ignore */
      }
    }

    // As a final step, enqueue and return; caller should not treat as failure
    this.enqueue(msg, opts);
  }

  // force use of a backend; useful for debugging or UI toggles
  async forceBackend(kind: BackendKind) {
    if (kind === "dht") {
      this.preferDht = true;
      if (!this.dhtAvailable) await this.detectDht();
      if (this.dhtAvailable) await this.connectDht();
    } else if (kind === "ws") {
      this.preferDht = false;
      await this.connectWebSocket();
    } else {
      this.disconnect();
    }
  }

  disconnect(): void {
    this.wsClosedByUser = true;
    try {
      this.ws?.close();
    } catch (e) {}
    this.ws = null;
    this.connected.set(false);
    this.peers.set([]);
    this.backend.set("none");
    this.state.set("disconnected");
    this.stopWsHeartbeat();
    try {
      if (this.peerGcTimer) clearInterval(this.peerGcTimer);
    } catch (_) {}
  }

  getClientId(): string {
    return this.clientId;
  }

  isConnected(): boolean {
    return get(this.connected);
  }

  getPeersWithTimestamps(): Array<{ id: string; ts: number }> {
    return Array.from(this.peersMap.entries()).map(([id, ts]) => ({ id, ts }));
  }

  /* ----------------------
     Incoming message handling
     ---------------------- */
  private handleIncoming(message: any) {
    if (!message) return;

    if (message.type === "peers" && Array.isArray(message.peers)) {
      const ids = message.peers.map((p: any) =>
        typeof p === "string" ? p : p.clientId || p.id || p.peerId
      );
      this.mergePeers(ids);
      return;
    }

    // forward offer/answer/candidate messages to registered handler
    if (
      message.type === "offer" ||
      message.type === "answer" ||
      message.type === "candidate"
    ) {
      if (this.onMessageHandler) {
        try {
          this.onMessageHandler(message);
        } catch (e) {
          console.error("onMessageHandler error", e);
        }
        return;
      }
    }

    // otherwise, if an onMessage handler exists, forward everything
    if (this.onMessageHandler) {
      try {
        this.onMessageHandler(message);
      } catch (e) {
        console.error("onMessageHandler error", e);
      }
      return;
    }

    console.debug("[SignalingService] Unhandled message", message);
  }
}

export default SignalingService;
