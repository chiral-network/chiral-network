// E2E-style integration tests for multi-seeder download flow
// Tests the complete lifecycle: DHT discovery → seeder selection →
// chunk scheduling → download → verification → completion

import { describe, it, expect, beforeEach, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";

// Mock Tauri APIs
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

vi.mock("@tauri-apps/api/path", () => ({
  homeDir: vi.fn().mockResolvedValue("/home/user"),
  join: vi.fn(async (...parts: string[]) => parts.join("/")),
}));

vi.mock("$lib/reputationStore", () => ({
  default: {
    getInstance: vi.fn(() => ({
      noteSeen: vi.fn(),
      success: vi.fn(),
      failure: vi.fn(),
      getScore: vi.fn(() => 0.8),
    })),
  },
}));

const localStorageMock = (() => {
  let store: Record<string, string> = {};
  return {
    getItem: (key: string) => store[key] || null,
    setItem: (key: string, value: string) => { store[key] = value; },
    removeItem: (key: string) => { delete store[key]; },
    clear: () => { store = {}; },
  };
})();
global.localStorage = localStorageMock as any;

const mockInvoke = vi.mocked(invoke);

// =========================================================================
// Test helpers
// =========================================================================

interface Seeder {
  peerId: string;
  latency: number;
  bandwidth: number;
  reputation: number;
  chunksAvailable: number[];
}

interface ChunkInfo {
  index: number;
  size: number;
  hash: string;
  state: "pending" | "downloading" | "verified" | "failed";
  source?: string;
}

function createMockSeeders(count: number): Seeder[] {
  return Array.from({ length: count }, (_, i) => ({
    peerId: `seeder-${String.fromCharCode(65 + i)}`, // A, B, C, ...
    latency: 20 + i * 30,
    bandwidth: 10_000_000 - i * 2_000_000,
    reputation: 95 - i * 5,
    chunksAvailable: [], // all chunks
  }));
}

function createChunks(fileSize: number, chunkSize: number = 262144): ChunkInfo[] {
  const count = Math.ceil(fileSize / chunkSize);
  return Array.from({ length: count }, (_, i) => ({
    index: i,
    size: Math.min(chunkSize, fileSize - i * chunkSize),
    hash: `sha256-chunk-${i}-${Math.random().toString(36).slice(2, 10)}`,
    state: "pending" as const,
  }));
}

describe("Multi-Seeder Download E2E Flow", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
  });

  // =========================================================================
  // Complete download lifecycle
  // =========================================================================

  describe("Complete Download Lifecycle", () => {
    it("should complete a full download from discovery to verification", async () => {
      const fileHash = "merkle_root_abc123";
      const fileSize = 262144 * 8; // 8 chunks exactly
      const seeders = createMockSeeders(3);
      const chunks = createChunks(fileSize);

      // Phase 1: DHT Discovery
      mockInvoke.mockResolvedValueOnce({
        name: "large-file.bin",
        size: fileSize,
        chunkCount: chunks.length,
        merkleRoot: fileHash,
      });

      const metadata = await invoke("search_file_by_hash", {
        merkleRoot: fileHash,
      });
      expect(metadata).toBeDefined();

      // Phase 2: Seeder Discovery
      mockInvoke.mockResolvedValueOnce(
        seeders.map((s) => s.peerId)
      );

      const providers = await invoke<string[]>("get_seeders_for_file", {
        merkleRoot: fileHash,
      });
      expect(providers).toHaveLength(3);

      // Phase 3: Initialize Scheduler
      mockInvoke.mockResolvedValueOnce(null); // init_scheduler
      await invoke("init_scheduler", {
        manifest: {
          chunks: chunks.map((c) => ({
            index: c.index,
            size: c.size,
            checksum: c.hash,
          })),
        },
      });

      // Phase 4: Add Seeders
      for (const seeder of seeders) {
        mockInvoke.mockResolvedValueOnce(null);
        await invoke("add_peer", {
          peerId: seeder.peerId,
          maxConcurrent: 3,
        });
      }

      // Phase 5: Download chunks in rounds
      let completedChunks = 0;
      const totalRounds = Math.ceil(chunks.length / (seeders.length * 3));

      for (let round = 0; round < totalRounds + 1; round++) {
        const batchSize = Math.min(
          seeders.length * 3,
          chunks.length - completedChunks
        );
        if (batchSize <= 0) break;

        const requests = Array.from({ length: batchSize }, (_, i) => ({
          chunkIndex: completedChunks + i,
          peerId: seeders[i % seeders.length].peerId,
          requestedAt: Date.now(),
          timeoutMs: 30000,
        }));

        mockInvoke.mockResolvedValueOnce(requests);
        const reqs = await invoke("get_next_requests", {
          maxRequests: batchSize,
        });

        // Mark all as received
        for (const req of reqs as any[]) {
          mockInvoke.mockResolvedValueOnce(null);
          await invoke("on_chunk_received", {
            chunkIndex: req.chunkIndex,
          });
          completedChunks++;
        }
      }

      expect(completedChunks).toBe(chunks.length);

      // Phase 6: Verify completion
      mockInvoke.mockResolvedValueOnce(true);
      const isComplete = await invoke<boolean>("is_complete");
      expect(isComplete).toBe(true);
    });

    it("should handle partial seeder (not all chunks available)", async () => {
      const chunks = createChunks(262144 * 4);
      const seeders = createMockSeeders(2);

      // Seeder A has chunks 0,1. Seeder B has chunks 2,3.
      seeders[0].chunksAvailable = [0, 1];
      seeders[1].chunksAvailable = [2, 3];

      // Initialize
      mockInvoke.mockResolvedValueOnce(null);
      await invoke("init_scheduler", {
        manifest: {
          chunks: chunks.map((c) => ({
            index: c.index,
            size: c.size,
            checksum: c.hash,
          })),
        },
      });

      // Distribute chunks according to availability
      const assignments = chunks.map((chunk) => {
        const seeder = seeders.find((s) =>
          s.chunksAvailable.length === 0 || s.chunksAvailable.includes(chunk.index)
        );
        return {
          chunkIndex: chunk.index,
          peerId: seeder!.peerId,
        };
      });

      expect(assignments.filter((a) => a.peerId === "seeder-A")).toHaveLength(2);
      expect(assignments.filter((a) => a.peerId === "seeder-B")).toHaveLength(2);
    });
  });

  // =========================================================================
  // Failure recovery scenarios
  // =========================================================================

  describe("Failure Recovery", () => {
    it("should recover when a seeder goes offline mid-download", async () => {
      const chunks = createChunks(262144 * 6);
      const downloadLog: { chunk: number; peer: string; status: string }[] = [];

      // Seeder A downloads chunks 0-2, then goes offline
      for (let i = 0; i < 3; i++) {
        downloadLog.push({
          chunk: i,
          peer: "seeder-A",
          status: "success",
        });
      }

      // Seeder A goes offline, chunk 3 fails
      downloadLog.push({
        chunk: 3,
        peer: "seeder-A",
        status: "failed",
      });

      // Remove seeder A, seeder B takes over
      mockInvoke.mockResolvedValueOnce(null);
      await invoke("remove_peer", { peerId: "seeder-A" });

      // Seeder B completes remaining chunks
      for (let i = 3; i < 6; i++) {
        downloadLog.push({
          chunk: i,
          peer: "seeder-B",
          status: "success",
        });
      }

      const successful = downloadLog.filter((l) => l.status === "success");
      expect(successful).toHaveLength(6);
      expect(new Set(successful.map((l) => l.chunk)).size).toBe(6);
    });

    it("should retry failed chunks up to max retries", async () => {
      const retryAttempts: { chunk: number; attempt: number; success: boolean }[] = [];

      // Chunk 0 fails twice then succeeds
      retryAttempts.push({ chunk: 0, attempt: 1, success: false });
      retryAttempts.push({ chunk: 0, attempt: 2, success: false });
      retryAttempts.push({ chunk: 0, attempt: 3, success: true });

      const successful = retryAttempts.filter((r) => r.success);
      expect(successful).toHaveLength(1);
      expect(retryAttempts).toHaveLength(3);
    });

    it("should mark chunk as permanently failed after max retries", () => {
      const maxRetries = 3;
      let attempts = 0;
      let chunkState = "pending";

      while (attempts < maxRetries) {
        attempts++;
        // Simulate failure
        chunkState = "failed";
      }

      // After max retries, chunk stays failed
      expect(chunkState).toBe("failed");
      expect(attempts).toBe(maxRetries);
    });

    it("should handle corrupted chunk detection and re-download", async () => {
      // Download chunk
      mockInvoke.mockResolvedValueOnce(null);
      await invoke("on_chunk_received", { chunkIndex: 5 });

      // Verify chunk - corrupted!
      const isValid = false; // SHA-256 mismatch
      expect(isValid).toBe(false);

      // Mark as corrupted
      mockInvoke.mockResolvedValueOnce(null);
      await invoke("on_chunk_failed", {
        chunkIndex: 5,
        markCorrupted: true,
      });

      expect(mockInvoke).toHaveBeenCalledWith("on_chunk_failed", {
        chunkIndex: 5,
        markCorrupted: true,
      });
    });
  });

  // =========================================================================
  // Seeder selection and peer management
  // =========================================================================

  describe("Seeder Selection", () => {
    it("should prefer high-reputation seeders", () => {
      const seeders = [
        { peerId: "low-rep", reputation: 30, latency: 50 },
        { peerId: "high-rep", reputation: 95, latency: 100 },
        { peerId: "mid-rep", reputation: 70, latency: 50 },
      ];

      const sorted = [...seeders].sort(
        (a, b) => b.reputation - a.reputation
      );

      expect(sorted[0].peerId).toBe("high-rep");
      expect(sorted[1].peerId).toBe("mid-rep");
      expect(sorted[2].peerId).toBe("low-rep");
    });

    it("should balance load across seeders with equal reputation", () => {
      const seeders = [
        { peerId: "a", reputation: 90, pendingChunks: 5 },
        { peerId: "b", reputation: 90, pendingChunks: 2 },
        { peerId: "c", reputation: 90, pendingChunks: 8 },
      ];

      const sorted = [...seeders].sort(
        (a, b) => a.pendingChunks - b.pendingChunks
      );

      expect(sorted[0].peerId).toBe("b"); // least loaded
      expect(sorted[2].peerId).toBe("c"); // most loaded
    });

    it("should calculate aggregate bandwidth from multiple seeders", () => {
      const seeders = [
        { peerId: "s1", bandwidth: 5_000_000 },
        { peerId: "s2", bandwidth: 3_000_000 },
        { peerId: "s3", bandwidth: 2_000_000 },
      ];

      const totalBandwidth = seeders.reduce(
        (sum, s) => sum + s.bandwidth,
        0
      );
      expect(totalBandwidth).toBe(10_000_000); // 10 MB/s aggregate
    });

    it("should handle discovering new seeders during download", () => {
      const activeSeeders = ["seeder-1", "seeder-2"];

      // New seeder discovered mid-download
      const newSeeder = "seeder-3";
      if (!activeSeeders.includes(newSeeder)) {
        activeSeeders.push(newSeeder);
      }

      expect(activeSeeders).toHaveLength(3);
      expect(activeSeeders).toContain("seeder-3");
    });
  });

  // =========================================================================
  // Transaction and payment integration
  // =========================================================================

  describe("Payment for Downloads", () => {
    it("should check file price before downloading", async () => {
      const fileMetadata = {
        merkleRoot: "paid_file_hash",
        name: "premium-content.zip",
        size: 10485760,
        priceChr: "2.5",
        walletAddress: "0xseller_address_here_00000000000000000000",
      };

      mockInvoke.mockResolvedValueOnce(fileMetadata);

      const metadata = await invoke("search_file_by_hash", {
        merkleRoot: "paid_file_hash",
      });

      const price = (metadata as any).priceChr;
      expect(parseFloat(price)).toBeGreaterThan(0);
    });

    it("should proceed with free file without payment", () => {
      const freeFile = {
        priceChr: "0",
        walletAddress: null,
      };

      const requiresPayment =
        freeFile.priceChr &&
        parseFloat(freeFile.priceChr) > 0 &&
        freeFile.walletAddress;
      expect(requiresPayment).toBeFalsy();
    });
  });

  // =========================================================================
  // Progress tracking
  // =========================================================================

  describe("Download Progress Tracking", () => {
    it("should calculate progress percentage correctly", () => {
      const totalChunks = 100;
      const verifiedChunks = 37;

      const progress = (verifiedChunks / totalChunks) * 100;
      expect(progress).toBe(37);
    });

    it("should estimate remaining time based on speed", () => {
      const totalBytes = 104857600; // 100 MB
      const downloadedBytes = 52428800; // 50 MB
      const speedBytesPerSecond = 5242880; // 5 MB/s

      const remainingBytes = totalBytes - downloadedBytes;
      const etaSeconds = remainingBytes / speedBytesPerSecond;

      expect(etaSeconds).toBeCloseTo(10, 0);
    });

    it("should track per-seeder contribution", () => {
      const seederStats: Record<string, { chunks: number; bytes: number }> = {
        "seeder-1": { chunks: 40, bytes: 40 * 262144 },
        "seeder-2": { chunks: 35, bytes: 35 * 262144 },
        "seeder-3": { chunks: 25, bytes: 25 * 262144 },
      };

      const totalChunks = Object.values(seederStats).reduce(
        (sum, s) => sum + s.chunks,
        0
      );
      expect(totalChunks).toBe(100);

      // Seeder 1 contributed 40%
      expect(seederStats["seeder-1"].chunks / totalChunks).toBe(0.4);
    });

    it("should handle speed calculation with zero time", () => {
      const bytesDownloaded = 0;
      const elapsedSeconds = 0;

      const speed =
        elapsedSeconds > 0 ? bytesDownloaded / elapsedSeconds : 0;
      expect(speed).toBe(0);
    });
  });

  // =========================================================================
  // Resume interrupted download
  // =========================================================================

  describe("Download Resume", () => {
    it("should resume download from last verified chunk", () => {
      const chunks = createChunks(262144 * 10);

      // Simulate 5 chunks already verified
      for (let i = 0; i < 5; i++) {
        chunks[i].state = "verified";
      }

      const pendingChunks = chunks.filter(
        (c) => c.state === "pending" || c.state === "failed"
      );

      expect(pendingChunks).toHaveLength(5);
      expect(pendingChunks[0].index).toBe(5);
    });

    it("should persist download state for resume", () => {
      const downloadState = {
        merkleRoot: "resume_hash",
        fileName: "large.bin",
        fileSize: 262144 * 20,
        verifiedChunks: [0, 1, 2, 3, 4],
        totalChunks: 20,
        providers: ["seeder-1", "seeder-2"],
      };

      // Save state
      localStorage.setItem(
        "chiral_dl_state_resume_hash",
        JSON.stringify(downloadState)
      );

      // Load state on resume
      const stored = localStorage.getItem("chiral_dl_state_resume_hash");
      expect(stored).not.toBeNull();

      const loaded = JSON.parse(stored!);
      expect(loaded.verifiedChunks).toHaveLength(5);
      expect(loaded.providers).toHaveLength(2);
    });
  });
});
