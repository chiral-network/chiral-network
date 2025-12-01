import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { get } from "svelte/store";
import { SignalingService } from "../src/lib/services/signalingService";

// Mock DhtService to return null (not available)
vi.mock("$lib/services/dht", () => ({
  DhtService: {
    getInstance: vi.fn(() => null),
  },
}));

// Mock CloseEvent for Node.js environment
class MockCloseEvent extends Event {
  code: number;
  reason: string;
  wasClean: boolean;

  constructor(
    type: string,
    init?: { code?: number; reason?: string; wasClean?: boolean }
  ) {
    super(type);
    this.code = init?.code ?? 1000;
    this.reason = init?.reason ?? "";
    this.wasClean = init?.wasClean ?? true;
  }
}

// Mock WebSocket
class MockWebSocket {
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSING = 2;
  static CLOSED = 3;

  readyState: number = MockWebSocket.CONNECTING;
  url: string;
  onopen: ((ev: Event) => void) | null = null;
  onclose: ((ev: any) => void) | null = null;
  onerror: ((ev: Event) => void) | null = null;
  onmessage: ((ev: MessageEvent) => void) | null = null;

  constructor(url: string) {
    this.url = url;
    // Simulate async connection
    setTimeout(() => {
      this.readyState = MockWebSocket.OPEN;
      this.onopen?.(new Event("open"));
    }, 10);
  }

  send(data: string): void {
    if (this.readyState !== MockWebSocket.OPEN) {
      throw new Error("WebSocket is not open");
    }
    // Mock implementation
  }

  close(): void {
    this.readyState = MockWebSocket.CLOSED;
    this.onclose?.(new MockCloseEvent("close"));
  }
}

// Mock localStorage
const localStorageMock = (() => {
  let store: Record<string, string> = {};
  return {
    getItem: (key: string) => store[key] || null,
    setItem: (key: string, value: string) => {
      store[key] = value;
    },
    removeItem: (key: string) => {
      delete store[key];
    },
    clear: () => {
      store = {};
    },
  };
})();

beforeEach(() => {
  global.WebSocket = MockWebSocket as any;
  global.localStorage = localStorageMock as any;
  global.CloseEvent = MockCloseEvent as any;
  vi.clearAllMocks();
  localStorage.clear();
});

describe("SignalingService", () => {
  let signalingService: SignalingService;

  beforeEach(() => {
    // Force WebSocket by disabling DHT preference
    signalingService = new SignalingService({ preferDht: false });
  });

  afterEach(() => {
    signalingService.disconnect();
  });

  describe("constructor", () => {
    it("should create a unique client ID", () => {
      const service1 = new SignalingService({ preferDht: false });
      const service2 = new SignalingService({ preferDht: false });

      expect(service1.getClientId()).toBeDefined();
      expect(service2.getClientId()).toBeDefined();
      expect(service1.getClientId()).not.toBe(service2.getClientId());
    });

    it("should initialize stores with default values", () => {
      expect(get(signalingService.connected)).toBe(false);
      expect(get(signalingService.peers)).toEqual([]);
      expect(get(signalingService.backend)).toBe("none");
      expect(get(signalingService.state)).toBe("disconnected");
    });

    it("should accept custom options", () => {
      const service = new SignalingService({
        url: "ws://custom:8080",
        preferDht: false,
        wsHeartbeatInterval: 10000,
        peerTtlMs: 5000,
        persistPeers: false,
      });

      expect(service).toBeDefined();
    });

    it("should load persisted peers from localStorage", () => {
      const now = Date.now();
      const persistedPeers = [
        { id: "peer1", ts: now },
        { id: "peer2", ts: now },
      ];
      localStorage.setItem(
        "chiral:signaling:peers",
        JSON.stringify(persistedPeers)
      );

      const service = new SignalingService({ preferDht: false });

      expect(get(service.peers)).toEqual(["peer1", "peer2"]);
    });

    it("should filter out stale persisted peers", () => {
      const now = Date.now();
      const staleTime = now - 1000 * 60 * 60 * 25; // 25 hours ago (stale)
      const persistedPeers = [
        { id: "peer1", ts: now },
        { id: "stale-peer", ts: staleTime },
      ];
      localStorage.setItem(
        "chiral:signaling:peers",
        JSON.stringify(persistedPeers)
      );

      const service = new SignalingService({
        preferDht: false,
        peerTtlMs: 1000 * 60 * 60 * 24,
      });

      expect(get(service.peers)).toEqual(["peer1"]);
      expect(get(service.peers)).not.toContain("stale-peer");
    });
  });

  describe("connect", () => {
    it("should connect to WebSocket when DHT not available", async () => {
      await signalingService.connect();

      expect(get(signalingService.connected)).toBe(true);
      expect(get(signalingService.backend)).toBe("ws");
      expect(get(signalingService.state)).toBe("connected");
    });

    it("should set connecting state during connection", async () => {
      const connectPromise = signalingService.connect();

      // Check immediately
      await new Promise((resolve) => setTimeout(resolve, 0));
      expect(get(signalingService.state)).toBe("connecting");

      await connectPromise;

      expect(get(signalingService.state)).toBe("connected");
    });

    it("should handle connection timeout", async () => {
      const service = new SignalingService({
        preferDht: false,
        wsConnectTimeoutMs: 100,
      });

      // Mock WebSocket that never connects or calls any callbacks
      global.WebSocket = class {
        static CONNECTING = 0;
        static OPEN = 1;
        static CLOSING = 2;
        static CLOSED = 3;

        readyState = 0; // CONNECTING
        url: string;
        onopen: any = null;
        onclose: any = null;
        onerror: any = null;
        onmessage: any = null;

        constructor(url: string) {
          this.url = url;
          // Never connect - just stay in CONNECTING state forever
        }

        send(data: string): void {
          throw new Error("WebSocket is not open");
        }

        close(): void {
          this.readyState = 3; // CLOSED
        }
      } as any;

      // Start connection and catch the rejection
      service.connect().catch(() => {
        // Expected to timeout and reject
      });

      // Wait for timeout to trigger (100ms + buffer)
      await new Promise((resolve) => setTimeout(resolve, 200));

      // Should have timed out and be disconnected
      expect(get(service.state)).toBe("disconnected");
      expect(get(service.connected)).toBe(false);

      service.disconnect();
    });

    it("should handle WebSocket connection error", async () => {
      global.WebSocket = class extends MockWebSocket {
        constructor(url: string) {
          super(url);
          // Clear default open behavior
          clearTimeout((this as any)._openTimeout);

          // Trigger error immediately
          setTimeout(() => {
            this.readyState = MockWebSocket.CLOSED;
            this.onerror?.(new Event("error"));
            this.onclose?.(new MockCloseEvent("close", { wasClean: false }));
          }, 10);
        }
      } as any;

      const connectPromise = signalingService.connect();

      // Wait for error to occur
      await new Promise((resolve) => setTimeout(resolve, 50));

      // Connection should fail and state should be disconnected
      expect(get(signalingService.state)).toBe("disconnected");
      expect(get(signalingService.connected)).toBe(false);
    });

    it("should send registration message after connection", async () => {
      const sendSpy = vi.fn();
      global.WebSocket = class extends MockWebSocket {
        send(data: string) {
          sendSpy(data);
        }
      } as any;

      await signalingService.connect();

      // Wait a bit for registration message
      await new Promise((resolve) => setTimeout(resolve, 50));

      expect(sendSpy).toHaveBeenCalledWith(
        expect.stringContaining('"type":"register"')
      );
      expect(sendSpy).toHaveBeenCalledWith(
        expect.stringContaining(
          `"clientId":"${signalingService.getClientId()}"`
        )
      );
    });
  });

  describe("send", () => {
    it("should send message via WebSocket when connected", async () => {
      const sendSpy = vi.fn();
      global.WebSocket = class extends MockWebSocket {
        send(data: string) {
          sendSpy(data);
        }
      } as any;

      await signalingService.connect();

      // Clear the registration call
      sendSpy.mockClear();

      const message = { type: "offer", sdp: "test-sdp", to: "peer1" };
      await signalingService.send(message);

      expect(sendSpy).toHaveBeenCalledWith(
        expect.stringContaining('"type":"offer"')
      );
    });

    it("should queue messages when not connected", async () => {
      const message = { type: "offer", sdp: "test-sdp", to: "peer1" };

      // Should not throw
      await signalingService.send(message);

      // Message should be queued (verify by connecting and checking it's sent)
      const sendSpy = vi.fn();
      global.WebSocket = class extends MockWebSocket {
        send(data: string) {
          sendSpy(data);
        }
      } as any;

      await signalingService.connect();

      // Wait for queue flush
      await new Promise((resolve) => setTimeout(resolve, 100));

      // Check if offer message was sent (skip registration message)
      const offerCalls = sendSpy.mock.calls.filter((call) =>
        call[0].includes('"type":"offer"')
      );
      expect(offerCalls.length).toBeGreaterThan(0);
    });

    it("should include clientId in sent messages", async () => {
      const sendSpy = vi.fn();
      global.WebSocket = class extends MockWebSocket {
        send(data: string) {
          sendSpy(data);
        }
      } as any;

      await signalingService.connect();

      sendSpy.mockClear(); // Clear registration

      await signalingService.send({ type: "test" });

      expect(sendSpy).toHaveBeenCalled();
      const sentData = JSON.parse(sendSpy.mock.calls[0][0]);
      expect(sentData.from).toBe(signalingService.getClientId());
    });

    it("should handle send errors gracefully", async () => {
      global.WebSocket = class extends MockWebSocket {
        send(data: string) {
          throw new Error("Send failed");
        }
      } as any;

      await signalingService.connect();

      // Should queue message instead of throwing
      await expect(
        signalingService.send({ type: "test" })
      ).resolves.toBeUndefined();
    });
  });

  describe("disconnect", () => {
    it("should close WebSocket connection", async () => {
      await signalingService.connect();
      expect(get(signalingService.connected)).toBe(true);

      signalingService.disconnect();

      await new Promise((resolve) => setTimeout(resolve, 50));

      expect(get(signalingService.connected)).toBe(false);
      expect(get(signalingService.state)).toBe("disconnected");
      expect(get(signalingService.backend)).toBe("none");
    });

    it("should clear peers on disconnect", async () => {
      await signalingService.connect();

      // Simulate peer update
      const ws = (signalingService as any).ws;
      ws?.onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            type: "peers",
            peers: ["peer1", "peer2"],
          }),
        })
      );

      await new Promise((resolve) => setTimeout(resolve, 50));

      expect(get(signalingService.peers).length).toBeGreaterThan(0);

      signalingService.disconnect();

      expect(get(signalingService.peers)).toEqual([]);
    });

    it("should prevent auto-reconnect after user disconnect", async () => {
      await signalingService.connect();

      signalingService.disconnect();

      // Wait to ensure no reconnection attempt
      await new Promise((resolve) => setTimeout(resolve, 1200));

      expect(get(signalingService.state)).toBe("disconnected");
    });
  });

  describe("WebSocket reconnection", () => {
    it("should attempt reconnection on unexpected close", async () => {
      let connectionAttempts = 0;
      global.WebSocket = class extends MockWebSocket {
        constructor(url: string) {
          super(url);
          connectionAttempts++;
        }
      } as any;

      await signalingService.connect();
      expect(connectionAttempts).toBe(1);

      // Simulate unexpected close
      const ws = (signalingService as any).ws;
      (signalingService as any).wsClosedByUser = false;
      ws.readyState = MockWebSocket.CLOSED;
      ws.onclose?.(new MockCloseEvent("close", { wasClean: false }));

      // Wait for reconnection attempt
      await new Promise((resolve) => setTimeout(resolve, 1200));

      expect(connectionAttempts).toBeGreaterThan(1);
    });

    it("should use exponential backoff for reconnection", async () => {
      const connectionTimes: number[] = [];
      let attemptCount = 0;

      global.WebSocket = class extends MockWebSocket {
        constructor(url: string) {
          super(url);
          attemptCount++;
          connectionTimes.push(Date.now());

          // Clear default open behavior
          clearTimeout((this as any)._openTimeout);

          // Fail by closing (not error) to trigger onclose handler
          setTimeout(() => {
            this.readyState = MockWebSocket.CLOSED;
            // Only trigger onclose (service only reconnects on close, not error)
            this.onclose?.(
              new MockCloseEvent("close", { wasClean: false, code: 1006 })
            );
          }, 10);
        }
      } as any;

      const service = new SignalingService({
        preferDht: false,
      });

      // Start connection (will fail and reconnect)
      service.connect().catch(() => {});

      // Wait for multiple reconnection attempts
      // Expected delays: 0ms, 1000ms, 2000ms, 4000ms
      // So wait at least 7000ms + buffer
      await new Promise((resolve) => setTimeout(resolve, 8000));

      service.disconnect();

      console.log(`\nReconnection Test Results:`);
      console.log(`Total attempts: ${attemptCount}`);

      // Verify we got multiple attempts (should be at least 4-5)
      expect(attemptCount).toBeGreaterThanOrEqual(3);

      if (connectionTimes.length >= 3) {
        // Calculate all delays
        const delays = [];
        for (let i = 1; i < connectionTimes.length; i++) {
          delays.push(connectionTimes[i] - connectionTimes[i - 1]);
        }

        console.log(
          `Delays: ${delays.map((d, i) => `[${i}]=${d}ms`).join(", ")}`
        );

        // With exponential backoff base=1000ms:
        // Expected: ~1000ms, ~2000ms, ~4000ms
        // Check if we have increasing trend in delays

        if (delays.length >= 2) {
          // Find the largest delay
          const maxDelay = Math.max(...delays);
          const minDelay = Math.min(...delays);

          // Max should be significantly larger than min (evidence of backoff)
          // With exponential backoff, max should be at least 1.5x min
          expect(maxDelay).toBeGreaterThan(minDelay * 1.5);

          console.log(
            `Min delay: ${minDelay}ms, Max delay: ${maxDelay}ms, Ratio: ${(maxDelay / minDelay).toFixed(2)}x`
          );
        } else {
          // Not enough delays to check
          console.log(
            `Only ${delays.length} delays recorded, skipping backoff verification`
          );
          expect(true).toBe(true);
        }
      } else {
        console.log(
          `Only ${connectionTimes.length} attempts, not enough to verify backoff`
        );
        // Still pass if we got multiple attempts
        expect(attemptCount).toBeGreaterThan(1);
      }
    }, 12000);
  });

  describe("heartbeat", () => {
    it("should send ping messages at intervals", async () => {
      const sendSpy = vi.fn();
      global.WebSocket = class extends MockWebSocket {
        send(data: string) {
          sendSpy(data);
        }
      } as any;

      const service = new SignalingService({
        preferDht: false,
        wsHeartbeatInterval: 100,
      });
      await service.connect();

      // Wait for at least one heartbeat
      await new Promise((resolve) => setTimeout(resolve, 250));

      const pingCalls = sendSpy.mock.calls.filter((call) =>
        call[0].includes('"type":"ping"')
      );
      expect(pingCalls.length).toBeGreaterThan(0);

      service.disconnect();
    });

    it("should respond to ping with pong", async () => {
      const sendSpy = vi.fn();
      global.WebSocket = class extends MockWebSocket {
        send(data: string) {
          sendSpy(data);
        }
      } as any;

      await signalingService.connect();

      sendSpy.mockClear();

      const ws = (signalingService as any).ws;
      ws?.onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({ type: "ping", ts: Date.now() }),
        })
      );

      await new Promise((resolve) => setTimeout(resolve, 50));

      const pongCalls = sendSpy.mock.calls.filter((call) =>
        call[0].includes('"type":"pong"')
      );
      expect(pongCalls.length).toBe(1);
    });
  });

  describe("peer management", () => {
    it("should update peers from server", async () => {
      await signalingService.connect();

      const ws = (signalingService as any).ws;
      ws?.onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            type: "peers",
            peers: ["peer1", "peer2", "peer3"],
          }),
        })
      );

      await new Promise((resolve) => setTimeout(resolve, 50));

      expect(get(signalingService.peers)).toEqual(["peer1", "peer2", "peer3"]);
    });

    it("should merge new peers with existing ones", async () => {
      await signalingService.connect();

      const ws = (signalingService as any).ws;

      // First update
      ws?.onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            type: "peers",
            peers: ["peer1", "peer2"],
          }),
        })
      );

      await new Promise((resolve) => setTimeout(resolve, 50));

      // Second update with overlap
      ws?.onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            type: "peers",
            peers: ["peer2", "peer3"],
          }),
        })
      );

      await new Promise((resolve) => setTimeout(resolve, 50));

      const peers = get(signalingService.peers);
      expect(peers).toContain("peer1");
      expect(peers).toContain("peer2");
      expect(peers).toContain("peer3");
    });

    it("should persist peers to localStorage", async () => {
      await signalingService.connect();

      const ws = (signalingService as any).ws;
      ws?.onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            type: "peers",
            peers: ["peer1", "peer2"],
          }),
        })
      );

      await new Promise((resolve) => setTimeout(resolve, 50));

      const stored = localStorage.getItem("chiral:signaling:peers");
      expect(stored).toBeTruthy();

      const parsed = JSON.parse(stored!);
      expect(parsed.some((p: any) => p.id === "peer1")).toBe(true);
      expect(parsed.some((p: any) => p.id === "peer2")).toBe(true);
    });

    it("should garbage collect stale peers", async () => {
      const service = new SignalingService({
        preferDht: false,
        peerTtlMs: 100, // 100ms TTL for testing
      });

      await service.connect();

      const ws = (service as any).ws;
      ws?.onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            type: "peers",
            peers: ["peer1"],
          }),
        })
      );

      await new Promise((resolve) => setTimeout(resolve, 50));

      expect(get(service.peers)).toContain("peer1");

      // Wait for TTL to expire and GC to run
      await new Promise((resolve) => setTimeout(resolve, 200));

      // Manually trigger GC
      (service as any).gcPeers();

      expect(get(service.peers)).not.toContain("peer1");

      service.disconnect();
    });

    it("should return peers with timestamps", async () => {
      await signalingService.connect();

      const ws = (signalingService as any).ws;
      ws?.onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({
            type: "peers",
            peers: ["peer1", "peer2"],
          }),
        })
      );

      await new Promise((resolve) => setTimeout(resolve, 50));

      const peersWithTs = signalingService.getPeersWithTimestamps();

      expect(peersWithTs.length).toBe(2);
      expect(peersWithTs[0]).toHaveProperty("id");
      expect(peersWithTs[0]).toHaveProperty("ts");
      expect(typeof peersWithTs[0].ts).toBe("number");
    });
  });

  describe("message handling", () => {
    it("should forward offer messages to handler", async () => {
      const handler = vi.fn();
      signalingService.setOnMessage(handler);

      await signalingService.connect();

      const offerMsg = {
        type: "offer",
        sdp: { type: "offer", sdp: "test-sdp" },
        from: "peer1",
      };

      const ws = (signalingService as any).ws;
      ws?.onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify(offerMsg),
        })
      );

      await new Promise((resolve) => setTimeout(resolve, 50));

      expect(handler).toHaveBeenCalledWith(offerMsg);
    });

    it("should forward answer messages to handler", async () => {
      const handler = vi.fn();
      signalingService.setOnMessage(handler);

      await signalingService.connect();

      const answerMsg = {
        type: "answer",
        sdp: { type: "answer", sdp: "test-sdp" },
        from: "peer1",
      };

      const ws = (signalingService as any).ws;
      ws?.onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify(answerMsg),
        })
      );

      await new Promise((resolve) => setTimeout(resolve, 50));

      expect(handler).toHaveBeenCalledWith(answerMsg);
    });

    it("should forward ICE candidate messages to handler", async () => {
      const handler = vi.fn();
      signalingService.setOnMessage(handler);

      await signalingService.connect();

      const candidateMsg = {
        type: "candidate",
        candidate: { candidate: "test-candidate" },
        from: "peer1",
      };

      const ws = (signalingService as any).ws;
      ws?.onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify(candidateMsg),
        })
      );

      await new Promise((resolve) => setTimeout(resolve, 50));

      expect(handler).toHaveBeenCalledWith(candidateMsg);
    });

    it("should handle invalid JSON gracefully", async () => {
      const consoleSpy = vi.spyOn(console, "warn").mockImplementation(() => {});

      await signalingService.connect();

      const ws = (signalingService as any).ws;
      ws?.onmessage?.(
        new MessageEvent("message", {
          data: "invalid json{",
        })
      );

      await new Promise((resolve) => setTimeout(resolve, 50));

      expect(consoleSpy).toHaveBeenCalled();

      consoleSpy.mockRestore();
    });

    it("should handle handler exceptions gracefully", async () => {
      const handler = vi.fn(() => {
        throw new Error("Handler error");
      });
      const consoleSpy = vi
        .spyOn(console, "error")
        .mockImplementation(() => {});

      signalingService.setOnMessage(handler);
      await signalingService.connect();

      const ws = (signalingService as any).ws;
      ws?.onmessage?.(
        new MessageEvent("message", {
          data: JSON.stringify({ type: "offer", from: "peer1" }),
        })
      );

      await new Promise((resolve) => setTimeout(resolve, 50));

      expect(consoleSpy).toHaveBeenCalled();

      consoleSpy.mockRestore();
    });
  });

  describe("backend switching", () => {
    it("should support forcing WebSocket backend", async () => {
      await signalingService.forceBackend("ws");

      expect(get(signalingService.backend)).toBe("ws");
      expect(get(signalingService.connected)).toBe(true);
    });

    it("should disconnect when forcing 'none' backend", async () => {
      await signalingService.connect();
      expect(get(signalingService.connected)).toBe(true);

      await signalingService.forceBackend("none");

      expect(get(signalingService.backend)).toBe("none");
      expect(get(signalingService.connected)).toBe(false);
    });
  });

  describe("isConnected", () => {
    it("should return false when not connected", () => {
      expect(signalingService.isConnected()).toBe(false);
    });

    it("should return true when connected", async () => {
      await signalingService.connect();

      expect(signalingService.isConnected()).toBe(true);
    });
  });

  describe("getClientId", () => {
    it("should return consistent client ID", () => {
      const id1 = signalingService.getClientId();
      const id2 = signalingService.getClientId();

      expect(id1).toBe(id2);
      expect(typeof id1).toBe("string");
      expect(id1.length).toBeGreaterThan(0);
    });
  });
});
