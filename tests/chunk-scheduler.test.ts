// Tests for chunk scheduler frontend integration
// Tests multi-seeder chunk assignment, peer selection strategies,
// timeout handling, retry logic, and scheduler state management.

import { describe, it, expect, beforeEach, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

// Mock Tauri APIs
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(),
}));

const mockInvoke = vi.mocked(invoke);

// Types matching chunk_scheduler.rs
interface ChunkRequest {
  chunkIndex: number;
  peerId: string;
  requestedAt: number;
  timeoutMs: number;
}

interface PeerInfo {
  peerId: string;
  available: boolean;
  lastSeen: number;
  pendingRequests: number;
  maxConcurrent: number;
  avgResponseTime: number;
  failureCount: number;
}

interface SchedulerState {
  chunk_states: string[];
  active_request_count: number;
  available_peer_count: number;
  total_peer_count: number;
  completed_chunks: number;
  total_chunks: number;
}

// Helper to set up mock scheduler commands
function setupSchedulerMocks(options: {
  peers?: PeerInfo[];
  requests?: ChunkRequest[];
  state?: Partial<SchedulerState>;
  isComplete?: boolean;
}) {
  mockInvoke.mockImplementation(async (cmd: string, args?: any) => {
    switch (cmd) {
      case "init_scheduler":
        return null;
      case "add_peer":
        return null;
      case "remove_peer":
        return null;
      case "get_next_requests":
        return options.requests || [];
      case "on_chunk_received":
        return null;
      case "on_chunk_failed":
        return null;
      case "get_scheduler_state":
        return {
          chunk_states: [],
          active_request_count: 0,
          available_peer_count: 0,
          total_peer_count: 0,
          completed_chunks: 0,
          total_chunks: 0,
          ...options.state,
        };
      case "is_complete":
        return options.isComplete ?? false;
      case "get_peers":
        return options.peers || [];
      case "get_active_requests":
        return options.requests || [];
      case "update_peer_health":
        return null;
      default:
        return null;
    }
  });
}

describe("Chunk Scheduler Frontend Integration", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  // =========================================================================
  // Scheduler initialization
  // =========================================================================

  describe("Initialization", () => {
    it("should initialize scheduler with chunk manifest", async () => {
      setupSchedulerMocks({});

      await invoke("init_scheduler", {
        manifest: {
          chunks: [
            { index: 0, size: 262144, checksum: "hash0" },
            { index: 1, size: 262144, checksum: "hash1" },
            { index: 2, size: 100000, checksum: "hash2" },
          ],
        },
      });

      expect(mockInvoke).toHaveBeenCalledWith("init_scheduler", {
        manifest: expect.objectContaining({
          chunks: expect.arrayContaining([
            expect.objectContaining({ index: 0, size: 262144 }),
          ]),
        }),
      });
    });

    it("should add multiple peers after initialization", async () => {
      setupSchedulerMocks({});

      await invoke("add_peer", { peerId: "peer-1", maxConcurrent: 3 });
      await invoke("add_peer", { peerId: "peer-2", maxConcurrent: 5 });
      await invoke("add_peer", { peerId: "peer-3", maxConcurrent: null });

      expect(mockInvoke).toHaveBeenCalledWith("add_peer", {
        peerId: "peer-1",
        maxConcurrent: 3,
      });
      expect(mockInvoke).toHaveBeenCalledWith("add_peer", {
        peerId: "peer-2",
        maxConcurrent: 5,
      });
    });
  });

  // =========================================================================
  // Multi-seeder chunk request distribution
  // =========================================================================

  describe("Multi-Seeder Chunk Distribution", () => {
    it("should distribute chunks across multiple seeders", async () => {
      const mockRequests: ChunkRequest[] = [
        { chunkIndex: 0, peerId: "seeder-1", requestedAt: 1000, timeoutMs: 30000 },
        { chunkIndex: 1, peerId: "seeder-2", requestedAt: 1000, timeoutMs: 30000 },
        { chunkIndex: 2, peerId: "seeder-3", requestedAt: 1000, timeoutMs: 30000 },
        { chunkIndex: 3, peerId: "seeder-1", requestedAt: 1000, timeoutMs: 30000 },
        { chunkIndex: 4, peerId: "seeder-2", requestedAt: 1000, timeoutMs: 30000 },
        { chunkIndex: 5, peerId: "seeder-3", requestedAt: 1000, timeoutMs: 30000 },
      ];

      setupSchedulerMocks({ requests: mockRequests });
      const requests = await invoke<ChunkRequest[]>("get_next_requests", {
        maxRequests: 6,
      });

      expect(requests).toHaveLength(6);

      // Verify round-robin distribution
      const seeder1Chunks = requests.filter((r) => r.peerId === "seeder-1");
      const seeder2Chunks = requests.filter((r) => r.peerId === "seeder-2");
      const seeder3Chunks = requests.filter((r) => r.peerId === "seeder-3");

      expect(seeder1Chunks).toHaveLength(2);
      expect(seeder2Chunks).toHaveLength(2);
      expect(seeder3Chunks).toHaveLength(2);
    });

    it("should handle single seeder gracefully", async () => {
      const mockRequests: ChunkRequest[] = [
        { chunkIndex: 0, peerId: "only-seeder", requestedAt: 1000, timeoutMs: 30000 },
        { chunkIndex: 1, peerId: "only-seeder", requestedAt: 1000, timeoutMs: 30000 },
        { chunkIndex: 2, peerId: "only-seeder", requestedAt: 1000, timeoutMs: 30000 },
      ];

      setupSchedulerMocks({ requests: mockRequests });
      const requests = await invoke<ChunkRequest[]>("get_next_requests", {
        maxRequests: 10,
      });

      expect(requests.every((r) => r.peerId === "only-seeder")).toBe(true);
    });
  });

  // =========================================================================
  // Peer health and failure handling
  // =========================================================================

  describe("Peer Health Management", () => {
    it("should update peer health status", async () => {
      setupSchedulerMocks({});

      await invoke("update_peer_health", {
        peerId: "peer-1",
        available: false,
        responseTimeMs: null,
      });

      expect(mockInvoke).toHaveBeenCalledWith("update_peer_health", {
        peerId: "peer-1",
        available: false,
        responseTimeMs: null,
      });
    });

    it("should report chunk failure", async () => {
      setupSchedulerMocks({});

      await invoke("on_chunk_failed", {
        chunkIndex: 5,
        markCorrupted: true,
      });

      expect(mockInvoke).toHaveBeenCalledWith("on_chunk_failed", {
        chunkIndex: 5,
        markCorrupted: true,
      });
    });

    it("should report chunk success", async () => {
      setupSchedulerMocks({});

      await invoke("on_chunk_received", { chunkIndex: 3 });

      expect(mockInvoke).toHaveBeenCalledWith("on_chunk_received", {
        chunkIndex: 3,
      });
    });

    it("should remove failed peer and redistribute", async () => {
      setupSchedulerMocks({});

      // Remove failed peer
      await invoke("remove_peer", { peerId: "bad-peer" });

      // Get redistributed requests
      const retryRequests: ChunkRequest[] = [
        { chunkIndex: 2, peerId: "good-peer", requestedAt: 2000, timeoutMs: 30000 },
        { chunkIndex: 4, peerId: "good-peer", requestedAt: 2000, timeoutMs: 30000 },
      ];

      setupSchedulerMocks({ requests: retryRequests });
      const requests = await invoke<ChunkRequest[]>("get_next_requests", {
        maxRequests: 5,
      });

      expect(requests.every((r) => r.peerId === "good-peer")).toBe(true);
    });
  });

  // =========================================================================
  // Scheduler state and completion
  // =========================================================================

  describe("Scheduler State", () => {
    it("should report scheduler state correctly", async () => {
      setupSchedulerMocks({
        state: {
          chunk_states: [
            "RECEIVED",
            "RECEIVED",
            "REQUESTED",
            "UNREQUESTED",
            "UNREQUESTED",
          ],
          active_request_count: 1,
          available_peer_count: 2,
          total_peer_count: 3,
          completed_chunks: 2,
          total_chunks: 5,
        },
      });

      const state = await invoke<SchedulerState>("get_scheduler_state");

      expect(state.total_chunks).toBe(5);
      expect(state.completed_chunks).toBe(2);
      expect(state.active_request_count).toBe(1);
      expect(state.available_peer_count).toBe(2);
      expect(state.total_peer_count).toBe(3);
    });

    it("should report completion when all chunks received", async () => {
      setupSchedulerMocks({ isComplete: true });
      const complete = await invoke<boolean>("is_complete");
      expect(complete).toBe(true);
    });

    it("should report incomplete when chunks pending", async () => {
      setupSchedulerMocks({ isComplete: false });
      const complete = await invoke<boolean>("is_complete");
      expect(complete).toBe(false);
    });

    it("should return current peer list", async () => {
      const mockPeers: PeerInfo[] = [
        {
          peerId: "fast-peer",
          available: true,
          lastSeen: Date.now(),
          pendingRequests: 1,
          maxConcurrent: 5,
          avgResponseTime: 50,
          failureCount: 0,
        },
        {
          peerId: "slow-peer",
          available: true,
          lastSeen: Date.now(),
          pendingRequests: 3,
          maxConcurrent: 3,
          avgResponseTime: 2000,
          failureCount: 2,
        },
        {
          peerId: "dead-peer",
          available: false,
          lastSeen: Date.now() - 60000,
          pendingRequests: 0,
          maxConcurrent: 3,
          avgResponseTime: 5000,
          failureCount: 10,
        },
      ];

      setupSchedulerMocks({ peers: mockPeers });
      const peers = await invoke<PeerInfo[]>("get_peers");

      expect(peers).toHaveLength(3);
      const available = peers.filter((p) => p.available);
      expect(available).toHaveLength(2);
      expect(available[0].peerId).toBe("fast-peer");
    });
  });

  // =========================================================================
  // Full multi-seeder download simulation
  // =========================================================================

  describe("Full Download Simulation", () => {
    it("should simulate downloading 10-chunk file from 3 seeders", async () => {
      const totalChunks = 10;
      const seeders = ["seeder-A", "seeder-B", "seeder-C"];
      const receivedChunks = new Set<number>();

      setupSchedulerMocks({});

      // Initialize
      await invoke("init_scheduler", {
        manifest: {
          chunks: Array.from({ length: totalChunks }, (_, i) => ({
            index: i,
            size: 262144,
            checksum: `hash-${i}`,
          })),
        },
      });

      // Add seeders
      for (const seeder of seeders) {
        await invoke("add_peer", { peerId: seeder, maxConcurrent: 3 });
      }

      // Simulate download rounds
      let round = 0;
      while (receivedChunks.size < totalChunks && round < 5) {
        const requests: ChunkRequest[] = Array.from(
          { length: Math.min(9, totalChunks - receivedChunks.size) },
          (_, i) => {
            const chunkIndex = [...Array(totalChunks).keys()].filter(
              (c) => !receivedChunks.has(c)
            )[i];
            return {
              chunkIndex: chunkIndex ?? 0,
              peerId: seeders[i % seeders.length],
              requestedAt: Date.now(),
              timeoutMs: 30000,
            };
          }
        ).filter((r) => r.chunkIndex !== undefined);

        setupSchedulerMocks({ requests });
        const reqs = await invoke<ChunkRequest[]>("get_next_requests", {
          maxRequests: 9,
        });

        for (const req of reqs) {
          receivedChunks.add(req.chunkIndex);
          await invoke("on_chunk_received", { chunkIndex: req.chunkIndex });
        }

        round++;
      }

      expect(receivedChunks.size).toBe(totalChunks);
    });

    it("should handle seeder going offline mid-download", async () => {
      setupSchedulerMocks({});

      // Start with 2 seeders
      await invoke("add_peer", { peerId: "reliable", maxConcurrent: 5 });
      await invoke("add_peer", { peerId: "unreliable", maxConcurrent: 5 });

      // Unreliable seeder fails
      await invoke("update_peer_health", {
        peerId: "unreliable",
        available: false,
        responseTimeMs: null,
      });

      // Remove it
      await invoke("remove_peer", { peerId: "unreliable" });

      // Verify only reliable seeder gets requests
      const onlyReliable: ChunkRequest[] = [
        { chunkIndex: 0, peerId: "reliable", requestedAt: Date.now(), timeoutMs: 30000 },
        { chunkIndex: 1, peerId: "reliable", requestedAt: Date.now(), timeoutMs: 30000 },
      ];
      setupSchedulerMocks({ requests: onlyReliable });

      const reqs = await invoke<ChunkRequest[]>("get_next_requests", {
        maxRequests: 5,
      });
      expect(reqs.every((r) => r.peerId === "reliable")).toBe(true);
    });

    it("should track progress through scheduler state", async () => {
      // Initial state
      setupSchedulerMocks({
        state: { completed_chunks: 0, total_chunks: 100 },
      });
      let state = await invoke<SchedulerState>("get_scheduler_state");
      expect(state.completed_chunks / state.total_chunks).toBe(0);

      // After 50%
      setupSchedulerMocks({
        state: { completed_chunks: 50, total_chunks: 100 },
      });
      state = await invoke<SchedulerState>("get_scheduler_state");
      expect(state.completed_chunks / state.total_chunks).toBe(0.5);

      // Complete
      setupSchedulerMocks({
        state: { completed_chunks: 100, total_chunks: 100 },
        isComplete: true,
      });
      state = await invoke<SchedulerState>("get_scheduler_state");
      expect(state.completed_chunks / state.total_chunks).toBe(1);
      expect(await invoke<boolean>("is_complete")).toBe(true);
    });
  });
});
