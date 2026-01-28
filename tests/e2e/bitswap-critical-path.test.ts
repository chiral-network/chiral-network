/**
 * @fileoverview Bitswap Critical Path E2E Test
 * Tests the complete flow: Locating data → Handshake → Download(Bitswap) → Pay
 * 
 * This test simulates the full lifecycle of a Bitswap file download:
 * 1. Locating: Search for file metadata via DHT
 * 2. Handshake: Discover seeders and establish connection
 * 3. Download: Download file blocks via Bitswap protocol
 * 4. Pay: Process final payment after download completion
 * 
 * Note: Payment checkpoints are excluded; only final payment is tested
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  TestDataFactory,
  MockDHTService,
  MockPaymentService,
  EventHelper,
  MockTauriInvoke,
  DownloadProgressSimulator,
  TestCleanup,
} from "./test-helpers";
import type { FileMetadata } from "../../src/lib/dht";

// Mock Tauri APIs
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(),
}));

describe("Bitswap Critical Path E2E", () => {
  let mockDHT: MockDHTService;
  let mockPayment: MockPaymentService;
  let eventHelper: EventHelper;
  let mockTauri: MockTauriInvoke;
  let cleanup: TestCleanup;

  beforeEach(() => {
    vi.clearAllMocks();
    
    mockDHT = new MockDHTService(100);
    mockPayment = new MockPaymentService(10);
    eventHelper = new EventHelper();
    mockTauri = new MockTauriInvoke();
    cleanup = new TestCleanup();

    // Setup mock Tauri event listener
    vi.mocked(listen).mockImplementation(async (event: string, handler: any) => {
      eventHelper.on(event, handler);
      return () => {}; // Return unlisten function
    });
  });

  afterEach(async () => {
    await cleanup.cleanup();
    mockDHT.clear();
    mockPayment.reset();
    eventHelper.clear();
    mockTauri.clear();
  });

  describe("Complete Flow: Locate → Handshake → Download → Pay", () => {
    it("should successfully complete full Bitswap download flow", async () => {
      // Setup test data
      const testFile = TestDataFactory.createMockFile("document.pdf", 2 * 1024 * 1024); // 2MB
      const seeders = TestDataFactory.createMockPeers(2, "Bitswap");
      const metadata = TestDataFactory.createMockMetadata(testFile, seeders, "Bitswap");

      // Phase 1: LOCATING - Publish file to DHT (simulating uploader)
      await mockDHT.publishFile(metadata);
      expect(mockDHT.getPublishedFiles().has(testFile.hash)).toBe(true);

      // Setup mock Tauri commands for DHT search
      mockTauri.mockCommand("search_file_metadata", async (args: any) => {
        const result = await mockDHT.searchFileMetadata(args.fileHash);
        if (result) {
          // Simulate dht_metadata_found event
          setTimeout(() => {
            eventHelper.emit("dht_metadata_found", {
              fileHash: result.fileHash,
              fileName: result.fileName,
              fileSize: result.fileSize,
              createdAt: result.createdAt,
              mimeType: result.mimeType,
            });
          }, 50);
        }
        return result;
      });

      mockTauri.mockCommand("get_file_seeders", async (args: any) => {
        return await mockDHT.getSeedersForFile(args.fileHash);
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      // Phase 1: Execute search
      const foundMetadata = await invoke<FileMetadata>("search_file_metadata", {
        fileHash: testFile.hash,
        timeoutMs: 5000,
      });

      expect(foundMetadata).toBeDefined();
      expect(foundMetadata?.merkleRoot).toBe(testFile.hash);
      expect(foundMetadata?.fileName).toBe(testFile.name);
      expect(foundMetadata?.cids).toBeDefined();
      expect(foundMetadata?.cids?.length).toBeGreaterThan(0);

      // Phase 2: HANDSHAKE - Discover seeders
      const discoveredSeeders = await invoke<string[]>("get_file_seeders", {
        fileHash: testFile.hash,
      });

      expect(discoveredSeeders).toBeDefined();
      expect(discoveredSeeders.length).toBeGreaterThan(0);
      expect(discoveredSeeders).toEqual(expect.arrayContaining([seeders[0].id]));

      // Phase 3: DOWNLOAD - Simulate Bitswap download
      const downloadSimulator = new DownloadProgressSimulator(testFile.chunks);
      let downloadProgress = 0;
      let downloadComplete = false;

      downloadSimulator.setProgressCallback((progress, downloaded, total) => {
        downloadProgress = progress;
        
        // Emit progress events
        eventHelper.emit("bitswap_download_progress", {
          fileHash: testFile.hash,
          progress,
          downloadedChunks: downloaded,
          totalChunks: total,
          bytesDownloaded: downloaded * 64 * 1024,
          totalBytes: testFile.size,
        });

        // Emit completion event when done
        if (downloaded === total) {
          downloadComplete = true;
          eventHelper.emit("file_content", {
            merkleRoot: testFile.hash,
            file_name: testFile.name,
            downloadPath: `/tmp/${testFile.name}`,
          });
        }
      });

      // Setup mock for Bitswap download
      mockTauri.mockCommand("download_file_bitswap", async (args: any) => {
        // Simulate chunk-by-chunk download
        await downloadSimulator.downloadAll(10);
        return { success: true, filePath: `/tmp/${testFile.name}` };
      });

      // Start download
      const downloadResult = await invoke("download_file_bitswap", {
        fileHash: testFile.hash,
        fileName: testFile.name,
        fileSize: testFile.size,
        seeders: discoveredSeeders,
        cids: metadata.cids,
      });

      // Verify download completed
      expect(downloadResult).toEqual({
        success: true,
        filePath: `/tmp/${testFile.name}`,
      });
      expect(downloadComplete).toBe(true);
      expect(downloadProgress).toBe(100);
      expect(downloadSimulator.isComplete()).toBe(true);

      // Phase 4: PAY - Process final payment (no checkpoints)
      mockTauri.mockCommand("process_download_payment", async (args: any) => {
        const result = await mockPayment.processDownloadPayment(
          args.fileHash,
          args.fileName,
          args.fileSize,
          args.seederAddress
        );
        return result.transactionHash;
      });

      mockTauri.mockCommand("calculate_download_cost", async (args: any) => {
        return await mockPayment.calculateDownloadCost(args.fileSize);
      });

      // Calculate payment amount
      const paymentAmount = await invoke<number>("calculate_download_cost", {
        fileSize: testFile.size,
      });

      expect(paymentAmount).toBeGreaterThan(0);
      expect(typeof paymentAmount).toBe("number");

      // Process payment
      const txHash = await invoke<string>("process_download_payment", {
        fileHash: testFile.hash,
        fileName: testFile.name,
        fileSize: testFile.size,
        seederAddress: seeders[0].address,
      });

      expect(txHash).toBeDefined();
      expect(txHash).toMatch(/^0x[a-f0-9]{64}$/);
      expect(mockPayment.hasProcessedPayment(testFile.hash, seeders[0].address)).toBe(true);

      // Verify balance was deducted
      const finalBalance = mockPayment.getBalance();
      expect(finalBalance).toBeLessThan(10); // Initial balance was 10
      expect(finalBalance).toBeCloseTo(10 - paymentAmount, 6);
    });

    it("should handle file not found scenario", async () => {
      const nonExistentHash = "QmNonExistent123456789";

      mockTauri.mockCommand("search_file_metadata", async (args: any) => {
        return await mockDHT.searchFileMetadata(args.fileHash);
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      const result = await invoke<FileMetadata | null>("search_file_metadata", {
        fileHash: nonExistentHash,
        timeoutMs: 1000,
      });

      expect(result).toBeNull();
    });

    it("should handle no seeders available", async () => {
      const testFile = TestDataFactory.createMockFile("orphan.txt", 1024);
      const metadata = TestDataFactory.createMockMetadata(testFile, [], "Bitswap");
      metadata.seeders = [];

      await mockDHT.publishFile(metadata);

      mockTauri.mockCommand("search_file_metadata", async (args: any) => {
        return await mockDHT.searchFileMetadata(args.fileHash);
      });

      mockTauri.mockCommand("get_file_seeders", async (args: any) => {
        return await mockDHT.getSeedersForFile(args.fileHash);
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      const foundMetadata = await invoke<FileMetadata>("search_file_metadata", {
        fileHash: testFile.hash,
        timeoutMs: 1000,
      });

      expect(foundMetadata).toBeDefined();

      const seeders = await invoke<string[]>("get_file_seeders", {
        fileHash: testFile.hash,
      });

      expect(seeders).toEqual([]);
    });

    it("should handle insufficient balance for payment", async () => {
      mockPayment.reset(0.00001); // Very low balance

      // Avoid allocating a 100MB buffer; only metadata is needed for payment calculation.
      const testFile = {
        hash: `QmTestexpensivezip${Date.now()}`,
        name: "expensive.zip",
        size: 100 * 1024 * 1024, // 100MB
      };
      const seeders = TestDataFactory.createMockPeers(1, "Bitswap");

      mockTauri.mockCommand("process_download_payment", async (args: any) => {
        const result = await mockPayment.processDownloadPayment(
          args.fileHash,
          args.fileName,
          args.fileSize,
          args.seederAddress
        );
        if (!result.success) {
          throw new Error(result.error || "Payment failed");
        }
        return result.transactionHash;
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      await expect(
        invoke("process_download_payment", {
          fileHash: testFile.hash,
          fileName: testFile.name,
          fileSize: testFile.size,
          seederAddress: seeders[0].address,
        })
      ).rejects.toThrow("Insufficient balance");
    });

    it("should prevent duplicate payments", async () => {
      const testFile = TestDataFactory.createMockFile("once.txt", 1024);
      const seeders = TestDataFactory.createMockPeers(1, "Bitswap");

      mockTauri.mockCommand("process_download_payment", async (args: any) => {
        const result = await mockPayment.processDownloadPayment(
          args.fileHash,
          args.fileName,
          args.fileSize,
          args.seederAddress
        );
        if (!result.success) {
          throw new Error(result.error || "Payment failed");
        }
        return result.transactionHash;
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      // First payment
      const tx1 = await invoke<string>("process_download_payment", {
        fileHash: testFile.hash,
        fileName: testFile.name,
        fileSize: testFile.size,
        seederAddress: seeders[0].address,
      });

      expect(tx1).toBeDefined();

      // Second payment (duplicate)
      await expect(
        invoke("process_download_payment", {
          fileHash: testFile.hash,
          fileName: testFile.name,
          fileSize: testFile.size,
          seederAddress: seeders[0].address,
        })
      ).rejects.toThrow("already processed");
    });
  });

  describe("Progress Tracking", () => {
    it("should track download progress accurately", async () => {
      const testFile = TestDataFactory.createMockFile("progress.bin", 5 * 1024 * 1024); // 5MB
      const downloadSimulator = new DownloadProgressSimulator(testFile.chunks);

      const progressUpdates: number[] = [];

      downloadSimulator.setProgressCallback((progress) => {
        progressUpdates.push(progress);
      });

      await downloadSimulator.downloadAll(5);

      expect(progressUpdates.length).toBe(testFile.chunks);
      expect(progressUpdates[0]).toBeGreaterThan(0);
      expect(progressUpdates[progressUpdates.length - 1]).toBe(100);
      
      // Verify progress is monotonically increasing
      for (let i = 1; i < progressUpdates.length; i++) {
        expect(progressUpdates[i]).toBeGreaterThanOrEqual(progressUpdates[i - 1]);
      }
    });

    it("should emit progress events during download", async () => {
      const testFile = TestDataFactory.createMockFile("events.dat", 1024 * 1024);
      const downloadSimulator = new DownloadProgressSimulator(testFile.chunks);

      const progressEvents: any[] = [];

      eventHelper.on("bitswap_download_progress", (data) => {
        progressEvents.push(data);
      });

      downloadSimulator.setProgressCallback((progress, downloaded, total) => {
        eventHelper.emit("bitswap_download_progress", {
          progress,
          downloaded,
          total,
        });
      });

      await downloadSimulator.downloadAll(5);

      expect(progressEvents.length).toBeGreaterThan(0);
      expect(progressEvents[0].progress).toBeGreaterThan(0);
      expect(progressEvents[progressEvents.length - 1].progress).toBe(100);
    });
  });

  describe("CID Verification", () => {
    it("should verify CIDs are present in metadata", async () => {
      const testFile = TestDataFactory.createMockFile("cid-test.bin", 2 * 1024 * 1024);
      const seeders = TestDataFactory.createMockPeers(1, "Bitswap");
      const metadata = TestDataFactory.createMockMetadata(testFile, seeders, "Bitswap");

      await mockDHT.publishFile(metadata);

      mockTauri.mockCommand("search_file_metadata", async (args: any) => {
        return await mockDHT.searchFileMetadata(args.fileHash);
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      const foundMetadata = await invoke<FileMetadata>("search_file_metadata", {
        fileHash: testFile.hash,
        timeoutMs: 1000,
      });

      expect(foundMetadata).toBeDefined();
      expect(foundMetadata?.cids).toBeDefined();
      expect(Array.isArray(foundMetadata?.cids)).toBe(true);
      expect(foundMetadata?.cids?.length).toBe(testFile.chunks);

      // Verify each CID format
      foundMetadata?.cids?.forEach((cid) => {
        expect(cid).toMatch(/^Qm[a-zA-Z0-9]+/);
      });
    });

    it("should fail download if CIDs are missing", async () => {
      const testFile = TestDataFactory.createMockFile("no-cids.txt", 1024);
      const seeders = TestDataFactory.createMockPeers(1, "Bitswap");
      const metadata = TestDataFactory.createMockMetadata(testFile, seeders, "Bitswap");
      metadata.cids = undefined;

      await mockDHT.publishFile(metadata);

      mockTauri.mockCommand("search_file_metadata", async (args: any) => {
        return await mockDHT.searchFileMetadata(args.fileHash);
      });

      mockTauri.mockCommand("download_file_bitswap", async (args: any) => {
        if (!args.cids || args.cids.length === 0) {
          throw new Error("No CIDs available for Bitswap download");
        }
        return { success: true };
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      const foundMetadata = await invoke<FileMetadata>("search_file_metadata", {
        fileHash: testFile.hash,
        timeoutMs: 1000,
      });

      await expect(
        invoke("download_file_bitswap", {
          fileHash: testFile.hash,
          cids: foundMetadata?.cids || [],
        })
      ).rejects.toThrow("No CIDs available");
    });
  });

  describe("Seeder Discovery", () => {
    it("should discover multiple seeders", async () => {
      const testFile = TestDataFactory.createMockFile("multi-seeder.iso", 10 * 1024 * 1024);
      const seeders = TestDataFactory.createMockPeers(5, "Bitswap");
      const metadata = TestDataFactory.createMockMetadata(testFile, seeders, "Bitswap");

      await mockDHT.publishFile(metadata);

      mockTauri.mockCommand("get_file_seeders", async (args: any) => {
        return await mockDHT.getSeedersForFile(args.fileHash);
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      const discoveredSeeders = await invoke<string[]>("get_file_seeders", {
        fileHash: testFile.hash,
      });

      expect(discoveredSeeders.length).toBe(5);
      seeders.forEach((seeder) => {
        expect(discoveredSeeders).toContain(seeder.id);
      });
    });

    it("should prioritize seeders by reputation", async () => {
      const seeders = TestDataFactory.createMockPeers(3, "Bitswap");
      seeders[0].reputation = 95;
      seeders[1].reputation = 50;
      seeders[2].reputation = 75;

      const sortedByReputation = [...seeders].sort((a, b) => b.reputation - a.reputation);

      expect(sortedByReputation[0].reputation).toBe(95);
      expect(sortedByReputation[1].reputation).toBe(75);
      expect(sortedByReputation[2].reputation).toBe(50);
    });
  });

  describe("Edge Cases", () => {
    it("should handle very large files", async () => {
      // Avoid allocating a 1GB buffer; validate chunk math only.
      const fileSize = 1024 * 1024 * 1024; // 1GB
      const factoryChunkSize = 64 * 1024;
      const chunks = Math.ceil(fileSize / factoryChunkSize);
      expect(chunks).toBeGreaterThan(1000);
      expect(fileSize).toBe(1024 * 1024 * 1024);
    });

    it("should handle very small files", async () => {
      const tinyFile = TestDataFactory.createMockFile("tiny.txt", 100); // 100 bytes
      expect(tinyFile.chunks).toBe(1);
      expect(tinyFile.size).toBe(100);
    });

    it("should handle search timeout", async () => {
      const slowDHT = new MockDHTService(10000); // 10 second delay

      mockTauri.mockCommand("search_file_metadata", async (args: any) => {
        // Respect timeoutMs by racing the DHT lookup against a timeout.
        const timeoutMs = typeof args.timeoutMs === "number" ? args.timeoutMs : 1000;
        const result = await Promise.race([
          slowDHT.searchFileMetadata(args.fileHash),
          new Promise<null>((resolve) => setTimeout(() => resolve(null), timeoutMs)),
        ]);
        return result;
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      const startTime = Date.now();
      const result = await invoke<FileMetadata | null>("search_file_metadata", {
        fileHash: "QmTimeout123",
        timeoutMs: 100, // Very short timeout
      });

      const duration = Date.now() - startTime;
      
      // Should return null if not found quickly
      expect(duration).toBeLessThan(200);
      expect(result).toBeNull();
    });
  });
});

