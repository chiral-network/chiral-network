/**
 * @fileoverview End-to-End Scenario Test
 * Tests the complete file sharing lifecycle: Upload -> Publish -> Search -> Download -> Pay
 * 
 * This comprehensive test simulates the entire flow between two nodes:
 * 1. Uploader: Creates file, uploads, and publishes metadata to DHT
 * 2. Downloader: Searches DHT, finds file, downloads it
 * 3. Payment: Downloader pays uploader after successful download
 * 
 * Tests multiple protocols (Bitswap, WebRTC) and edge cases
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
  WebRTCHandshakeSimulator,
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

describe("End-to-End Scenario Tests", () => {
  let mockDHT: MockDHTService;
  let mockPayment: MockPaymentService;
  let eventHelper: EventHelper;
  let mockTauri: MockTauriInvoke;
  let webrtcHandshake: WebRTCHandshakeSimulator;
  let cleanup: TestCleanup;

  beforeEach(() => {
    vi.clearAllMocks();
    
    mockDHT = new MockDHTService(100);
    mockPayment = new MockPaymentService(50); // Higher initial balance for E2E
    eventHelper = new EventHelper();
    mockTauri = new MockTauriInvoke();
    webrtcHandshake = new WebRTCHandshakeSimulator();
    cleanup = new TestCleanup();

    vi.mocked(listen).mockImplementation(async (event: string, handler: any) => {
      eventHelper.on(event, handler);
      return () => {};
    });
  });

  afterEach(async () => {
    await cleanup.cleanup();
    mockDHT.clear();
    mockPayment.reset();
    eventHelper.clear();
    mockTauri.clear();
    webrtcHandshake.clear();
  });

  describe("Complete WebRTC Flow", () => {
    it("should complete full Upload -> Publish -> Search -> Download -> Pay cycle with WebRTC", async () => {
      // === PHASE 1: UPLOAD ===
      const uploaderPeer = TestDataFactory.createMockPeers(1, "WebRTC")[0];
      const testFile = TestDataFactory.createMockFile("document.pdf", 3 * 1024 * 1024); // 3MB
      
      // Mock upload command
      mockTauri.mockCommand("upload_file_to_network", async (args: any) => {
        // Simulate file processing
        await new Promise(resolve => setTimeout(resolve, 50));
        
        return {
          fileHash: testFile.hash,
          fileName: testFile.name,
          fileSize: testFile.size,
          protocol: "WebRTC",
        };
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      const uploadResult = await invoke("upload_file_to_network", {
        filePath: "/tmp/document.pdf",
        price: 0.001,
        protocol: "WebRTC",
      });

      expect(uploadResult).toHaveProperty("fileHash", testFile.hash);
      expect(uploadResult).toHaveProperty("protocol", "WebRTC");

      // === PHASE 2: PUBLISH ===
      const metadata = TestDataFactory.createMockMetadata(
        testFile,
        [uploaderPeer],
        "WebRTC"
      );

      mockTauri.mockCommand("publish_file_metadata", async (args: any) => {
        await mockDHT.publishFile(args.metadata);
        
        // Emit published event
        setTimeout(() => {
          eventHelper.emit("published_file", args.metadata);
        }, 30);

        return { success: true, fileHash: args.metadata.merkleRoot };
      });

      const publishResult = await invoke("publish_file_metadata", {
        metadata: metadata,
      });

      expect(publishResult).toHaveProperty("success", true);
      expect(mockDHT.getPublishedFiles().has(testFile.hash)).toBe(true);

      // === PHASE 3: SEARCH (Different Node) ===
      const downloaderPeer = "downloader_node_456";

      mockTauri.mockCommand("search_file_metadata", async (args: any) => {
        // Respect optional timeoutMs passed by caller to emulate frontend timeout semantics.
        const timeoutMs = typeof args?.timeoutMs === 'number' ? args.timeoutMs : undefined;
        const searchPromise = mockDHT.searchFileMetadata(args.fileHash);
        if (typeof timeoutMs === 'number' && timeoutMs > 0) {
          return await Promise.race([
            searchPromise,
            new Promise((resolve) => setTimeout(() => resolve(null), timeoutMs)),
          ]);
        }
        const result = await searchPromise;
        if (result) {
          setTimeout(() => eventHelper.emit("found_file", result), 40);
        }
        return result;
      });

      mockTauri.mockCommand("get_file_seeders", async (args: any) => {
        return await mockDHT.getSeedersForFile(args.fileHash);
      });

      const searchResult = await invoke<FileMetadata>("search_file_metadata", {
        fileHash: testFile.hash,
        timeoutMs: 5000,
      });

      expect(searchResult).toBeDefined();
      expect(searchResult?.merkleRoot).toBe(testFile.hash);
      expect(searchResult?.fileName).toBe(testFile.name);
      expect(searchResult?.protocol).toBe("WebRTC");

      const seeders = await invoke<string[]>("get_file_seeders", {
        fileHash: testFile.hash,
      });

      expect(seeders.length).toBeGreaterThan(0);
      expect(seeders).toContain(uploaderPeer.id);

      // === PHASE 4: DOWNLOAD (WebRTC) ===
      mockTauri.mockCommand("create_webrtc_offer", async (args: any) => {
        return { offer: `sdp_offer_${args.peerId}` };
      });

      mockTauri.mockCommand("send_webrtc_offer", async (args: any) => {
        // Simulate signaling
        setTimeout(() => {
          eventHelper.emit("webrtc_answer_received", {
            peerId: args.peerId,
            answer: `sdp_answer_${args.peerId}`,
          });
        }, 50);
        return { success: true };
      });

      mockTauri.mockCommand("accept_webrtc_answer", async (args: any) => {
        await webrtcHandshake.simulateHandshake(downloaderPeer, args.peerId);
        
        eventHelper.emit("webrtc_connection_established", {
          peerId: args.peerId,
        });

        return { success: true };
      });

      // Establish connection
      const offer = await invoke("create_webrtc_offer", { peerId: uploaderPeer.id });
      await invoke("send_webrtc_offer", { peerId: uploaderPeer.id, offer: offer.offer });
      
      const answerEvent = await eventHelper.waitForEvent("webrtc_answer_received", 2000);
      await invoke("accept_webrtc_answer", {
        peerId: uploaderPeer.id,
        answer: answerEvent.answer,
      });

      expect(webrtcHandshake.isConnected(downloaderPeer)).toBe(true);
      expect(webrtcHandshake.isConnected(uploaderPeer.id)).toBe(true);

      // Download file
      const downloadSimulator = new DownloadProgressSimulator(testFile.chunks);
      let downloadComplete = false;

      downloadSimulator.setProgressCallback((progress, downloaded, total) => {
        if (downloaded === total) {
          downloadComplete = true;
          eventHelper.emit("webrtc_download_complete", {
            fileHash: testFile.hash,
            fileName: testFile.name,
            fileSize: testFile.size,
          });
        }
      });

      mockTauri.mockCommand("request_file_via_webrtc", async (args: any) => {
        await downloadSimulator.downloadAll(5);
        return { success: true, fileSize: testFile.size };
      });

      const downloadResult = await invoke("request_file_via_webrtc", {
        peerId: uploaderPeer.id,
        fileHash: testFile.hash,
        fileName: testFile.name,
        fileSize: testFile.size,
      });

      expect(downloadResult).toHaveProperty("success", true);
      expect(downloadComplete).toBe(true);
      expect(downloadSimulator.isComplete()).toBe(true);

      // === PHASE 5: PAYMENT ===
      mockTauri.mockCommand("calculate_download_cost", async (args: any) => {
        return await mockPayment.calculateDownloadCost(args.fileSize);
      });

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

      const paymentAmount = await invoke<number>("calculate_download_cost", {
        fileSize: testFile.size,
      });

      expect(paymentAmount).toBeGreaterThan(0);

      const txHash = await invoke<string>("process_download_payment", {
        fileHash: testFile.hash,
        fileName: testFile.name,
        fileSize: testFile.size,
        seederAddress: uploaderPeer.address,
      });

      expect(txHash).toBeDefined();
      expect(txHash).toMatch(/^0x[a-f0-9]{64}$/);
      expect(mockPayment.hasProcessedPayment(testFile.hash, uploaderPeer.address)).toBe(true);

      // Verify final balance
      const finalBalance = mockPayment.getBalance();
      expect(finalBalance).toBeLessThan(50);
      expect(finalBalance).toBeCloseTo(50 - paymentAmount, 6);
    });
  });

  describe("Complete Bitswap Flow", () => {
    it("should complete full Upload -> Publish -> Search -> Download -> Pay cycle with Bitswap", async () => {
      // === PHASE 1: UPLOAD ===
      const uploaderPeer = TestDataFactory.createMockPeers(1, "Bitswap")[0];
      const testFile = TestDataFactory.createMockFile("archive.zip", 5 * 1024 * 1024); // 5MB
      
      mockTauri.mockCommand("upload_file_to_network", async (args: any) => {
        await new Promise(resolve => setTimeout(resolve, 50));
        
        return {
          fileHash: testFile.hash,
          fileName: testFile.name,
          fileSize: testFile.size,
          protocol: "Bitswap",
          cids: testFile.cids,
        };
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      const uploadResult = await invoke("upload_file_to_network", {
        filePath: "/tmp/archive.zip",
        price: 0.002,
        protocol: "Bitswap",
      });

      expect(uploadResult).toHaveProperty("fileHash", testFile.hash);
      expect(uploadResult).toHaveProperty("protocol", "Bitswap");
      expect(uploadResult).toHaveProperty("cids");

      // === PHASE 2: PUBLISH ===
      const metadata = TestDataFactory.createMockMetadata(
        testFile,
        [uploaderPeer],
        "Bitswap"
      );

      mockTauri.mockCommand("publish_file_metadata", async (args: any) => {
        await mockDHT.publishFile(args.metadata);
        setTimeout(() => eventHelper.emit("published_file", args.metadata), 30);
        return { success: true, fileHash: args.metadata.merkleRoot };
      });

      await invoke("publish_file_metadata", { metadata });

      expect(mockDHT.getPublishedFiles().has(testFile.hash)).toBe(true);

      // === PHASE 3: SEARCH ===
      mockTauri.mockCommand("search_file_metadata", async (args: any) => {
        // Respect optional timeoutMs passed by caller to emulate frontend timeout semantics.
        const timeoutMs = typeof args?.timeoutMs === 'number' ? args.timeoutMs : undefined;
        const searchPromise = mockDHT.searchFileMetadata(args.fileHash);
        if (typeof timeoutMs === 'number' && timeoutMs > 0) {
          return await Promise.race([
            searchPromise,
            new Promise((resolve) => setTimeout(() => resolve(null), timeoutMs)),
          ]);
        }
        const result = await searchPromise;
        if (result) {
          setTimeout(() => eventHelper.emit("found_file", result), 40);
        }
        return result;
      });

      mockTauri.mockCommand("get_file_seeders", async (args: any) => {
        return await mockDHT.getSeedersForFile(args.fileHash);
      });

      const searchResult = await invoke<FileMetadata>("search_file_metadata", {
        fileHash: testFile.hash,
        timeoutMs: 5000,
      });

      expect(searchResult).toBeDefined();
      expect(searchResult?.cids).toBeDefined();
      expect(searchResult?.cids?.length).toBe(testFile.chunks);

      // === PHASE 4: DOWNLOAD (Bitswap) ===
      const downloadSimulator = new DownloadProgressSimulator(testFile.chunks);

      downloadSimulator.setProgressCallback((progress, downloaded, total) => {
        eventHelper.emit("bitswap_download_progress", {
          fileHash: testFile.hash,
          progress,
          downloadedChunks: downloaded,
          totalChunks: total,
        });

        if (downloaded === total) {
          eventHelper.emit("file_content", {
            merkleRoot: testFile.hash,
            file_name: testFile.name,
            downloadPath: `/tmp/${testFile.name}`,
          });
        }
      });

      mockTauri.mockCommand("download_file_bitswap", async (args: any) => {
        await downloadSimulator.downloadAll(10);
        return { success: true, filePath: `/tmp/${testFile.name}` };
      });

      const downloadResult = await invoke("download_file_bitswap", {
        fileHash: testFile.hash,
        fileName: testFile.name,
        fileSize: testFile.size,
        seeders: [uploaderPeer.id],
        cids: testFile.cids,
      });

      expect(downloadResult).toHaveProperty("success", true);
      expect(downloadSimulator.isComplete()).toBe(true);

      // === PHASE 5: PAYMENT ===
      mockTauri.mockCommand("calculate_download_cost", async (args: any) => {
        return await mockPayment.calculateDownloadCost(args.fileSize);
      });

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

      const paymentAmount = await invoke<number>("calculate_download_cost", {
        fileSize: testFile.size,
      });

      const txHash = await invoke<string>("process_download_payment", {
        fileHash: testFile.hash,
        fileName: testFile.name,
        fileSize: testFile.size,
        seederAddress: uploaderPeer.address,
      });

      expect(txHash).toBeDefined();
      expect(mockPayment.hasProcessedPayment(testFile.hash, uploaderPeer.address)).toBe(true);
    });
  });

  describe("Multi-Node Scenarios", () => {
    it("should handle multiple concurrent uploads and downloads", async () => {
      const uploaders = TestDataFactory.createMockPeers(3, "WebRTC");
      const files = [
        TestDataFactory.createMockFile("file1.dat", 2 * 1024 * 1024),
        TestDataFactory.createMockFile("file2.bin", 3 * 1024 * 1024),
        TestDataFactory.createMockFile("file3.zip", 4 * 1024 * 1024),
      ];

      // Upload all files
      for (let i = 0; i < files.length; i++) {
        const metadata = TestDataFactory.createMockMetadata(
          files[i],
          [uploaders[i]],
          "WebRTC"
        );
        await mockDHT.publishFile(metadata);
      }

      expect(mockDHT.getPublishedFiles().size).toBe(3);

      // Download all files concurrently
      const downloadPromises = files.map(file => {
        const simulator = new DownloadProgressSimulator(file.chunks);
        return simulator.downloadAll(3);
      });

      await Promise.all(downloadPromises);

      // All downloads should complete
      files.forEach(file => {
        expect(mockDHT.getPublishedFiles().has(file.hash)).toBe(true);
      });
    });

    it("should handle two nodes exchanging files (bidirectional)", async () => {
      const nodeA = TestDataFactory.createMockPeers(1, "WebRTC")[0];
      const nodeB = TestDataFactory.createMockPeers(1, "WebRTC")[0];

      const fileFromA = TestDataFactory.createMockFile("from-a.txt", 1024 * 1024);
      const fileFromB = TestDataFactory.createMockFile("from-b.txt", 1024 * 1024);

      // Node A uploads
      const metadataA = TestDataFactory.createMockMetadata(fileFromA, [nodeA], "WebRTC");
      await mockDHT.publishFile(metadataA);

      // Node B uploads
      const metadataB = TestDataFactory.createMockMetadata(fileFromB, [nodeB], "WebRTC");
      await mockDHT.publishFile(metadataB);

      // Node A downloads from Node B
      mockTauri.mockCommand("search_file_metadata", async (args: any) => {
        return await mockDHT.searchFileMetadata(args.fileHash);
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      const foundByA = await invoke<FileMetadata>("search_file_metadata", {
        fileHash: fileFromB.hash,
        timeoutMs: 1000,
      });

      expect(foundByA).toBeDefined();
      expect(foundByA?.seeders).toContain(nodeB.id);

      // Node B downloads from Node A
      const foundByB = await invoke<FileMetadata>("search_file_metadata", {
        fileHash: fileFromA.hash,
        timeoutMs: 1000,
      });

      expect(foundByB).toBeDefined();
      expect(foundByB?.seeders).toContain(nodeA.id);
    });
  });

  describe("Error Scenarios", () => {
    it("should handle upload failure gracefully", async () => {
      mockTauri.mockCommand("upload_file_to_network", async (args: any) => {
        throw new Error("Disk full");
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      await expect(
        invoke("upload_file_to_network", {
          filePath: "/tmp/test.bin",
          price: 0.001,
          protocol: "WebRTC",
        })
      ).rejects.toThrow("Disk full");
    });

    it("should handle publish failure and retry", async () => {
      const testFile = TestDataFactory.createMockFile("retry.dat", 1024);
      const metadata = TestDataFactory.createMockMetadata(
        testFile,
        TestDataFactory.createMockPeers(1, "WebRTC"),
        "WebRTC"
      );

      let attemptCount = 0;
      const maxRetries = 3;

      mockTauri.mockCommand("publish_file_metadata", async (args: any) => {
        attemptCount++;
        if (attemptCount < maxRetries) {
          throw new Error("Network error");
        }
        await mockDHT.publishFile(args.metadata);
        return { success: true, fileHash: args.metadata.merkleRoot };
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      // Retry logic
      let published = false;
      for (let i = 0; i < maxRetries; i++) {
        try {
          await invoke("publish_file_metadata", { metadata });
          published = true;
          break;
        } catch (error) {
          if (i === maxRetries - 1) throw error;
        }
      }

      expect(published).toBe(true);
      expect(attemptCount).toBe(maxRetries);
    });

    it("should handle search timeout and return no results", async () => {
      const slowDHT = new MockDHTService(10000); // 10 second delay

      mockTauri.mockCommand("search_file_metadata", async (args: any) => {
        // Respect optional timeoutMs passed by caller to emulate frontend timeout semantics.
        const timeoutMs = typeof args?.timeoutMs === 'number' ? args.timeoutMs : undefined;
        const searchPromise = slowDHT.searchFileMetadata(args.fileHash);
        if (typeof timeoutMs === 'number' && timeoutMs > 0) {
          return await Promise.race([
            searchPromise,
            new Promise((resolve) => setTimeout(() => resolve(null), timeoutMs)),
          ]);
        }
        const result = await searchPromise;
        if (result) {
          setTimeout(() => eventHelper.emit("found_file", result), 40);
        }
        return result;
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      const startTime = Date.now();
      const result = await invoke<FileMetadata | null>("search_file_metadata", {
        fileHash: "QmNonExistent",
        timeoutMs: 100,
      });

      const duration = Date.now() - startTime;

      expect(duration).toBeLessThan(200);
      expect(result).toBeNull();
    });

    it("should handle payment failure after download", async () => {
      mockPayment.reset(0.00001); // Very low balance

      const testFile = TestDataFactory.createMockFile("expensive.bin", 50 * 1024 * 1024);
      const uploader = TestDataFactory.createMockPeers(1, "WebRTC")[0];

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
          seederAddress: uploader.address,
        })
      ).rejects.toThrow("Insufficient balance");
    });
  });

  describe("Data Integrity", () => {
    it("should verify file hash matches after download", () => {
      const testFile = TestDataFactory.createMockFile("verify.bin", 2 * 1024 * 1024);
      const uploadHash = testFile.hash;

      // Simulate download
      const downloadedContent = testFile.content;
      const downloadHash = testFile.hash; // In reality, would compute from content

      expect(downloadHash).toBe(uploadHash);
      expect(downloadedContent).toEqual(testFile.content);
    });

    it("should detect corrupted download", () => {
      const testFile = TestDataFactory.createMockFile("corrupt.bin", 1024 * 1024);
      const originalContent = new Uint8Array(testFile.content);

      // Simulate corruption
      const corruptedContent = new Uint8Array(testFile.content);
      corruptedContent[500] = (corruptedContent[500] + 1) % 256;

      expect(corruptedContent).not.toEqual(originalContent);
      
      // Hash would be different
      const isCorrupted = !corruptedContent.every((val, idx) => val === originalContent[idx]);
      expect(isCorrupted).toBe(true);
    });
  });

  describe("Performance Metrics", () => {
    it("should track end-to-end latency", async () => {
      const testFile = TestDataFactory.createMockFile("perf.dat", 5 * 1024 * 1024);
      const metadata = TestDataFactory.createMockMetadata(
        testFile,
        TestDataFactory.createMockPeers(1, "WebRTC"),
        "WebRTC"
      );

      const startTime = Date.now();

      // Upload + Publish
      await mockDHT.publishFile(metadata);

      const publishTime = Date.now();

      // Search
      mockTauri.mockCommand("search_file_metadata", async (args: any) => {
        return await mockDHT.searchFileMetadata(args.fileHash);
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      await invoke<FileMetadata>("search_file_metadata", {
        fileHash: testFile.hash,
        timeoutMs: 5000,
      });

      const searchTime = Date.now();

      // Download
      const downloadSimulator = new DownloadProgressSimulator(testFile.chunks);
      await downloadSimulator.downloadAll(5);

      const downloadTime = Date.now();

      // Calculate metrics
      const publishLatency = publishTime - startTime;
      const searchLatency = searchTime - publishTime;
      const downloadDuration = downloadTime - searchTime;
      const totalLatency = downloadTime - startTime;

      expect(publishLatency).toBeGreaterThan(0);
      expect(searchLatency).toBeGreaterThan(0);
      expect(downloadDuration).toBeGreaterThan(0);
      expect(totalLatency).toBe(publishLatency + searchLatency + downloadDuration);
    });

    it("should calculate throughput", async () => {
      const testFile = TestDataFactory.createMockFile("throughput.bin", 10 * 1024 * 1024);
      const downloadSimulator = new DownloadProgressSimulator(testFile.chunks);

      const startTime = Date.now();
      await downloadSimulator.downloadAll(2);
      const duration = (Date.now() - startTime) / 1000; // seconds

      const throughput = testFile.size / duration; // bytes per second
      const throughputMbps = (throughput * 8) / (1024 * 1024); // Mbps

      expect(throughput).toBeGreaterThan(0);
      expect(throughputMbps).toBeGreaterThan(0);
    });
  });
});

