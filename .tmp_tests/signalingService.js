import { writable } from "svelte/store";
// Use a lazy tauri invoke helper so tests can stub it via `globalThis.__tauri_invoke`
async function tauriInvoke(...args) {
    // Allow tests to provide a global stub
    if (globalThis.__tauri_invoke) {
        return globalThis.__tauri_invoke(...args);
    }
    try {
        const mod = await import("@tauri-apps/api/core");
        return mod.invoke(...args);
    }
    catch (e) {
        // Not running inside Tauri or import failed
        throw e;
    }
}
function createClientId() {
    const randomUUID = globalThis?.crypto?.randomUUID;
    if (typeof randomUUID === "function") {
        return randomUUID();
    }
    const timePart = Date.now().toString(36);
    const randomPart = Math.random().toString(36).slice(2, 10);
    return `client-${timePart}-${randomPart}`;
}
export class SignalingService {
    constructor() {
        this.dhtConnected = false;
        this.connected = writable(false);
        this.peers = writable([]);
        this.messageHandlers = new Set();
        this.ws = null;
        this.wsUrl = typeof window !== "undefined" ? `${window.location.protocol === 'https:' ? 'wss' : 'ws'}://${window.location.hostname}:9000` : "ws://localhost:9000";
        this.reconnectAttempts = 0;
        this.maxReconnectDelay = 30000; // 30s
        this.isManuallyClosed = false;
        this.clientId = createClientId();
    }
    async connect() {
        // Try to connect via WebSocket signaling server first. If that fails, fall back to DHT.
        if (this.ws || this.dhtConnected)
            return Promise.resolve();
        return new Promise(async (resolve) => {
            // Attempt to check DHT first (fast) so we can optionally use DHT-only mode
            try {
                const peerId = await tauriInvoke("get_dht_peer_id");
                if (peerId) {
                    this.dhtConnected = true;
                }
            }
            catch (e) {
                // ignore - DHT not available
                this.dhtConnected = false;
            }
            // If user wants DHT-only behavior or no WebSocket server available, we will still allow DHT usage.
            // Try WebSocket connection to default local server
            this.connectWebSocket();
            // Wait a short time for ws to connect; resolve regardless — caller can observe `connected` store
            setTimeout(() => resolve(), 500);
        });
    }
    async refreshPeers() {
        try {
            // Get connected peers from DHT
            const peers = (await tauriInvoke("get_dht_connected_peers"));
            this.peers.set(peers || []);
        }
        catch (error) {
            console.error("[SignalingService] Failed to refresh peers:", error);
        }
    }
    //Set a callback for incoming signaling messages (not implemented)
    // Allow multiple handlers; return an unsubscribe function
    setOnMessage(handler) {
        this.messageHandlers.add(handler);
        return () => this.messageHandlers.delete(handler);
    }
    connectWebSocket() {
        try {
            if (typeof WebSocket === "undefined")
                return;
            if (this.ws)
                return; // already connecting/connected
            const ws = new WebSocket(this.wsUrl);
            this.ws = ws;
            this.isManuallyClosed = false;
            ws.onopen = () => {
                this.reconnectAttempts = 0;
                this.connected.set(true);
                // Register with server
                const register = { type: "register", clientId: this.clientId };
                ws.send(JSON.stringify(register));
                // Also refresh peers from DHT if available
                try {
                    this.refreshPeers();
                }
                catch (e) {
                    // ignore
                }
            };
            ws.onmessage = (ev) => {
                try {
                    const data = JSON.parse(ev.data);
                    // Peer list update
                    if (data.type === "peers" && Array.isArray(data.peers)) {
                        // Remove ourselves from visible peers
                        const peers = data.peers.filter((p) => p !== this.clientId);
                        this.peers.set(peers);
                        return;
                    }
                    // Forward other messages to registered handlers
                    for (const h of this.messageHandlers) {
                        try {
                            h(data);
                        }
                        catch (e) {
                            console.warn('[SignalingService] message handler error', e);
                        }
                    }
                }
                catch (e) {
                    console.warn("[SignalingService] Failed to parse WS message", e);
                }
            };
            ws.onclose = () => {
                this.ws = null;
                this.connected.set(false);
                this.peers.set([]);
                if (!this.isManuallyClosed) {
                    this.scheduleReconnect();
                }
            };
            ws.onerror = (ev) => {
                console.warn("[SignalingService] WebSocket error", ev);
                // Let onclose handle reconnect
            };
        }
        catch (e) {
            console.warn("[SignalingService] Failed to create WebSocket:", e);
        }
    }
    scheduleReconnect() {
        this.reconnectAttempts += 1;
        const base = Math.min(1000 * Math.pow(2, this.reconnectAttempts - 1), this.maxReconnectDelay);
        const jitter = Math.floor(Math.random() * 1000);
        const delay = Math.min(base + jitter, this.maxReconnectDelay);
        setTimeout(() => {
            if (this.isManuallyClosed)
                return;
            this.connectWebSocket();
        }, delay);
    }
    // Send a signaling message to another peer via DHT
    async send(msg) {
        const envelope = {
            ...msg,
            from: this.clientId,
            timestamp: Date.now(),
        };
        // Prefer WebSocket signaling if available
        if (this.ws && this.ws.readyState === WebSocket.OPEN) {
            try {
                this.ws.send(JSON.stringify(envelope));
                return;
            }
            catch (e) {
                console.warn("[SignalingService] WS send failed, falling back to DHT", e);
            }
        }
        // Fall back to DHT if available
        if (this.dhtConnected) {
            try {
                await tauriInvoke("send_dht_message", {
                    peerId: msg.to,
                    message: envelope,
                });
                return;
            }
            catch (e) {
                console.error("[SignalingService] Failed to send DHT signaling message:", e);
                throw e;
            }
        }
        throw new Error("No signaling transport available (WS closed and DHT not connected)");
    }
    disconnect() {
        this.dhtConnected = false;
        this.connected.set(false);
        this.peers.set([]);
        this.isManuallyClosed = true;
        try {
            if (this.ws) {
                this.ws.close();
                this.ws = null;
            }
        }
        catch (e) {
            // ignore
        }
    }
    // Expose this client’s ID
    getClientId() {
        return this.clientId;
    }
}
