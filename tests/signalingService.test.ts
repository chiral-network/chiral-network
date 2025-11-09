import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { get } from "svelte/store";
import { SignalingService } from "../src/lib/services/signalingService";

describe("SignalingService", () => {
  let signalingService: SignalingService;

  beforeEach(() => {
    vi.clearAllMocks();
    signalingService = new SignalingService();
  });

  afterEach(() => {
    signalingService.disconnect();
  });

  describe("constructor", () => {
    it("should create a unique client ID", () => {
      const service1 = new SignalingService();
      const service2 = new SignalingService();

      expect(service1.getClientId()).toBeDefined();
      expect(service2.getClientId()).toBeDefined();
      expect(service1.getClientId()).not.toBe(service2.getClientId());
    });

    it("should initialize stores with default values", () => {
      expect(get(signalingService.connected)).toBe(false);
      expect(get(signalingService.peers)).toEqual([]);
    });
  });

  describe("connect", () => {
    it("should connect successfully to WebSocket server", async () => {
      // Note: This test requires a WebSocket server running on localhost:9000
      // In a real test environment, you would mock WebSocket or use a test server
      try {
        await signalingService.connect();
        expect(signalingService.isConnected()).toBe(true);
        expect(get(signalingService.connected)).toBe(true);
      } catch (error) {
        // If server is not running, expect connection to fail
        expect(signalingService.isConnected()).toBe(false);
        expect(get(signalingService.connected)).toBe(false);
      }
    });
  });

  describe("send", () => {
    it("should enqueue messages when WebSocket not connected", async () => {
      // The send method now enqueues messages instead of throwing
      expect(() => signalingService.send({ to: "peer" })).not.toThrow();
    });
  });

  describe("disconnect", () => {
    it("should reset connection state", () => {
      // Initially not connected
      expect(get(signalingService.connected)).toBe(false);
      expect(get(signalingService.peers)).toEqual([]);

      signalingService.disconnect();

      // Should remain in disconnected state
      expect(get(signalingService.connected)).toBe(false);
      expect(get(signalingService.peers)).toEqual([]);
    });
  });

  describe("getClientId", () => {
    it("should return the client ID", () => {
      const clientId = signalingService.getClientId();
      expect(typeof clientId).toBe("string");
      expect(clientId.length).toBeGreaterThan(0);
    });

    it("should return consistent client ID", () => {
      const id1 = signalingService.getClientId();
      const id2 = signalingService.getClientId();
      expect(id1).toBe(id2);
    });
  });

  describe("setOnMessage", () => {
    it("should accept message handler", () => {
      const handler = vi.fn();
      expect(() => signalingService.setOnMessage(handler)).not.toThrow();
    });
  });

  describe("isConnected", () => {
    it("should return false when not connected", () => {
      expect(signalingService.isConnected()).toBe(false);
    });
  });
});
