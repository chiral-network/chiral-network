import { describe, it, expect, vi } from "vitest";

// This module imports Tauri invoke() at the top-level; mock it so these tests
// can run in Node without a Tauri runtime.
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import PeerSelectionService, {
  type PeerMetrics,
} from "../src/lib/services/peerSelectionService";

describe("PeerSelectionService (pure helpers)", () => {
  describe("formatBytes", () => {
    it("should format bytes using shared toHumanReadableSize()", () => {
      expect(PeerSelectionService.formatBytes(0)).toBe("0 Bytes");
      expect(PeerSelectionService.formatBytes(-1)).toBe("0 Bytes");
      expect(PeerSelectionService.formatBytes(Number.NaN)).toBe("0 Bytes");

      expect(PeerSelectionService.formatBytes(1024)).toBe("1.00 KB");
      expect(PeerSelectionService.formatBytes(1536)).toBe("1.50 KB");
      expect(PeerSelectionService.formatBytes(1024 * 1024)).toBe("1.00 MB");
    });
  });

  describe("getPeerHealthScore", () => {
    it("should score good metrics higher than bad metrics", () => {
      const base: Omit<PeerMetrics, "latency_ms"> = {
        peer_id: "test_peer_1234567890",
        address: "/ip4/127.0.0.1/tcp/1234",
        reliability_score: 0.9,
        uptime_score: 0.9,
        success_rate: 0.9,
        last_seen: 1_700_000_000,
        transfer_count: 10,
        successful_transfers: 9,
        failed_transfers: 1,
        total_bytes_transferred: 1024,
        protocols: ["webrtc"],
        encryption_support: true,
      };

      const good: PeerMetrics = { ...base, latency_ms: 50 };
      const bad: PeerMetrics = {
        ...base,
        reliability_score: 0.3,
        success_rate: 0.2,
        latency_ms: 500,
      };

      const goodScore = PeerSelectionService.getPeerHealthScore(good);
      const badScore = PeerSelectionService.getPeerHealthScore(bad);

      expect(goodScore).toBeGreaterThanOrEqual(0);
      expect(goodScore).toBeLessThanOrEqual(100);
      expect(badScore).toBeGreaterThanOrEqual(0);
      expect(badScore).toBeLessThanOrEqual(100);
      expect(goodScore).toBeGreaterThan(badScore);
    });
  });

  describe("formatPeerMetrics", () => {
    it("should include truncated peer id and formatted transfer size", () => {
      const metrics: PeerMetrics = {
        peer_id: "test_peer_1234567890",
        address: "/ip4/127.0.0.1/tcp/1234",
        latency_ms: 123,
        bandwidth_kbps: 2048,
        reliability_score: 0.9,
        uptime_score: 0.9,
        success_rate: 0.9,
        last_seen: 1_700_000_000,
        transfer_count: 10,
        successful_transfers: 9,
        failed_transfers: 1,
        total_bytes_transferred: 1024,
        protocols: ["webrtc"],
        encryption_support: true,
      };

      const formatted = PeerSelectionService.formatPeerMetrics(metrics);

      expect(formatted["Peer ID"]).toBe("test_peer_12...");
      expect(formatted["Data Transferred"]).toBe("1.00 KB");
    });
  });

  describe("getTrustLevelFromScore", () => {
    it("should map numeric score to a stable trust level", () => {
      expect(PeerSelectionService.getTrustLevelFromScore(51)).toBe("Trusted");
      expect(PeerSelectionService.getTrustLevelFromScore(50)).toBe("Medium");
      expect(PeerSelectionService.getTrustLevelFromScore(0)).toBe("Medium");
      expect(PeerSelectionService.getTrustLevelFromScore(-1)).toBe("Low");
    });
  });
});
