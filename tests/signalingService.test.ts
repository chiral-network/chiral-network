import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { get } from "svelte/store";
import { SignalingService } from "../src/lib/services/signalingService";

// Mock Tauri invoke
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";

describe("SignalingService", () => {
  let signalingService: SignalingService;
  let mockInvoke: any;

  beforeEach(() => {
    vi.clearAllMocks();
    mockInvoke = vi.mocked(invoke);
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
    it("should connect successfully when DHT is running", async () => {
      const mockPeerId = "test-peer-id-123";
      const mockPeers = ["peer1", "peer2"];

      mockInvoke
        .mockResolvedValueOnce(mockPeerId) // get_dht_peer_id
        .mockResolvedValueOnce(mockPeers); // get_dht_connected_peers

      await signalingService.connect();

      expect(mockInvoke).toHaveBeenCalledWith("get_dht_peer_id");
      expect(mockInvoke).toHaveBeenCalledWith("get_dht_connected_peers");
      expect(get(signalingService.connected)).toBe(true);
      expect(get(signalingService.peers)).toEqual(mockPeers);
    });

    it("should handle DHT not running gracefully", async () => {
      mockInvoke.mockRejectedValueOnce(new Error("DHT not running"));

      await signalingService.connect();

      expect(mockInvoke).toHaveBeenCalledWith("get_dht_peer_id");
      expect(get(signalingService.connected)).toBe(false);
      expect(get(signalingService.peers)).toEqual([]);
    });

    it("should handle null peer ID gracefully", async () => {
      mockInvoke.mockResolvedValueOnce(null);

      await signalingService.connect();

      expect(mockInvoke).toHaveBeenCalledWith("get_dht_peer_id");
      expect(get(signalingService.connected)).toBe(false);
      expect(get(signalingService.peers)).toEqual([]);
    });
  });

  describe("send", () => {
    beforeEach(async () => {
      // Set up connected state
      mockInvoke
        .mockResolvedValueOnce("test-peer-id")
        .mockResolvedValueOnce([]);
      await signalingService.connect();
    });

    it("should send signaling message successfully", async () => {
      const testMessage = {
        to: "target-peer",
        type: "offer",
        data: "test-data",
      };
      mockInvoke.mockResolvedValueOnce(undefined);

      await signalingService.send(testMessage);

      expect(mockInvoke).toHaveBeenCalledWith("send_dht_message", {
        peerId: "target-peer",
        message: {
          ...testMessage,
          from: signalingService.getClientId(),
          timestamp: expect.any(Number),
          type: "webrtc_signaling",
        },
      });
    });

    it("should throw error when DHT not connected", async () => {
      signalingService.disconnect();

      await expect(signalingService.send({ to: "peer" })).rejects.toThrow(
        "DHT signaling not connected - please ensure DHT is running"
      );
    });

    it("should handle send failure", async () => {
      const testMessage = { to: "target-peer", type: "offer" };
      mockInvoke.mockRejectedValueOnce(new Error("Send failed"));

      await expect(signalingService.send(testMessage)).rejects.toThrow(
        "Send failed"
      );
    });
  });

  describe("disconnect", () => {
    beforeEach(async () => {
      mockInvoke
        .mockResolvedValueOnce("test-peer-id")
        .mockResolvedValueOnce(["peer1"]);
      await signalingService.connect();
    });

    it("should reset connection state", () => {
      expect(get(signalingService.connected)).toBe(true);
      expect(get(signalingService.peers)).toEqual(["peer1"]);

      signalingService.disconnect();

      expect(get(signalingService.connected)).toBe(false);
      expect(get(signalingService.peers)).toEqual([]);
    });
  });

  describe("refreshPeers", () => {
    beforeEach(async () => {
      mockInvoke
        .mockResolvedValueOnce("test-peer-id")
        .mockResolvedValueOnce([]);
      await signalingService.connect();
    });

    it("should update peers list", async () => {
      const newPeers = ["peer1", "peer2", "peer3"];
      mockInvoke.mockResolvedValueOnce(newPeers);

      // Access private method through type assertion
      await (signalingService as any).refreshPeers();

      expect(get(signalingService.peers)).toEqual(newPeers);
    });

    it("should handle peer refresh failure", async () => {
      mockInvoke.mockRejectedValueOnce(new Error("Refresh failed"));

      // Access private method through type assertion
      await (signalingService as any).refreshPeers();

      // Should not crash, peers should remain unchanged
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
    it("should accept message handler (currently no-op)", () => {
      const handler = vi.fn();
      expect(() => signalingService.setOnMessage(handler)).not.toThrow();
    });
  });
});
