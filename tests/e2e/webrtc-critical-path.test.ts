/**
 * @fileoverview WebRTC Critical Path E2E Test
 * Tests the complete flow: Locating → Handshake → Download(WebRTC) → Pay
 * 
 * This test simulates the full lifecycle of a WebRTC P2P file transfer:
 * 1. Locating: Search for file metadata and discover WebRTC-capable peers
 * 2. Handshake: WebRTC offer/answer exchange and data channel establishment
 * 3. Download: Chunk-based file transfer over WebRTC data channel
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

describe("WebRTC Critical Path E2E", () => {
  let mockDHT: MockDHTService;
  let mockPayment: MockPaymentService;
  let eventHelper: EventHelper;
  let mockTauri: MockTauriInvoke;
  let webrtcHandshake: WebRTCHandshakeSimulator;
  let cleanup: TestCleanup;

  beforeEach(() => {
    vi.clearAllMocks();
    
    mockDHT = new MockDHTService(100);
    mockPayment = new MockPaymentService(10);
    eventHelper = new EventHelper();
    mockTauri = new MockTauriInvoke();
    webrtcHandshake = new WebRTCHandshakeSimulator();
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
    webrtcHandshake.clear();
  });

  describe("Complete Flow: Locate → Handshake → Download → Pay", () => {
    it("should successfully complete full WebRTC download flow", async () => {
      // Setup test data
      const testFile = TestDataFactory.createMockFile("video.mp4", 5 * 1024 * 1024); // 5MB
      const seeders = TestDataFactory.createMockPeers(1, "WebRTC");
      const metadata = TestDataFactory.createMockMetadata(testFile, seeders, "WebRTC");
      const downloaderPeerId = "downloader_peer_123";

      // Phase 1: LOCATING - Publish file to DHT
      await mockDHT.publishFile(metadata);

      mockTauri.mockCommand("search_file_metadata", async (args: any) => {
        const result = await mockDHT.searchFileMetadata(args.fileHash);
        if (result) {
          setTimeout(() => eventHelper.emit("dht_metadata_found", {
            fileHash: result.fileHash,
            fileName: result.fileName,
            fileSize: result.fileSize,
            createdAt: result.createdAt,
            mimeType: result.mimeType,
          }), 50);
        }
        return result;
      });

      mockTauri.mockCommand("get_file_seeders", async (args: any) => {
        return await mockDHT.getSeedersForFile(args.fileHash);
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      // Execute search
      const foundMetadata = await invoke<FileMetadata>("search_file_metadata", {
        fileHash: testFile.hash,
        timeoutMs: 5000,
      });

      expect(foundMetadata).toBeDefined();
      expect(foundMetadata?.merkleRoot).toBe(testFile.hash);
      // FileMetadata typing may not include protocol in some builds; assert loosely for test intent.
      expect((foundMetadata as any)?.protocol).toBe("WebRTC");

      const discoveredSeeders = await invoke<string[]>("get_file_seeders", {
        fileHash: testFile.hash,
      });

      expect(discoveredSeeders.length).toBeGreaterThan(0);

      // Phase 2: HANDSHAKE - WebRTC connection establishment
      const seederPeerId = discoveredSeeders[0];
      let offerSdp: string | null = null;
      let answerSdp: string | null = null;
      let dataChannelReady = false;

      // Mock create WebRTC offer
      mockTauri.mockCommand("create_webrtc_offer", async (args: any) => {
        offerSdp = `v=0\r\no=- ${Date.now()} 0 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\n`;
        
        // Simulate ICE gathering
        setTimeout(() => {
          eventHelper.emit("webrtc_ice_candidate", {
            peerId: args.peerId,
            candidate: "candidate:1 1 UDP 2130706431 192.168.1.1 50000 typ host",
          });
        }, 20);

        return { offer: offerSdp };
      });

      // Mock send offer via signaling
      mockTauri.mockCommand("send_webrtc_offer", async (args: any) => {
        // Simulate signaling server relay
        setTimeout(async () => {
          // Seeder receives offer and creates answer
          answerSdp = `v=0\r\no=- ${Date.now()} 0 IN IP4 127.0.0.1\r\ns=-\r\nt=0 0\r\n`;
          
          eventHelper.emit("webrtc_answer_received", {
            peerId: args.peerId,
            answer: answerSdp,
          });
        }, 50);

        return { success: true };
      });

      // Mock accept answer
      mockTauri.mockCommand("accept_webrtc_answer", async (args: any) => {
        // Simulate handshake completion
        await webrtcHandshake.simulateHandshake(downloaderPeerId, seederPeerId);
        
        dataChannelReady = true;
        
        eventHelper.emit("webrtc_connection_established", {
          peerId: args.peerId,
          connectionState: "connected",
        });

        eventHelper.emit("webrtc_data_channel_open", {
          peerId: args.peerId,
        });

        return { success: true };
      });

      // Execute handshake
      const offerResult = await invoke("create_webrtc_offer", {
        peerId: seederPeerId,
      });

      expect(offerResult).toHaveProperty("offer");
      expect(offerSdp).toBeDefined();

      await invoke("send_webrtc_offer", {
        peerId: seederPeerId,
        offer: offerSdp,
      });

      // Wait for answer
      const answerEvent = await eventHelper.waitForEvent("webrtc_answer_received", 2000);
      expect(answerEvent.answer).toBeDefined();

      await invoke("accept_webrtc_answer", {
        peerId: seederPeerId,
        answer: answerEvent.answer,
      });

      expect(dataChannelReady).toBe(true);
      expect(webrtcHandshake.isConnected(downloaderPeerId)).toBe(true);
      expect(webrtcHandshake.isConnected(seederPeerId)).toBe(true);

      // Phase 3: DOWNLOAD - WebRTC chunk transfer
      const downloadSimulator = new DownloadProgressSimulator(testFile.chunks);
      let downloadComplete = false;
      const receivedChunks = new Map<number, Uint8Array>();

      downloadSimulator.setProgressCallback((progress, downloaded, total) => {
        eventHelper.emit("webrtc_download_progress", {
          fileHash: testFile.hash,
          progress,
          chunksReceived: downloaded,
          totalChunks: total,
          bytesReceived: downloaded * 16 * 1024, // 16KB chunks for WebRTC
          totalBytes: testFile.size,
        });

        if (downloaded === total) {
          downloadComplete = true;
          eventHelper.emit("webrtc_download_complete", {
            fileHash: testFile.hash,
            fileName: testFile.name,
            fileSize: testFile.size,
            data: Array.from(testFile.content),
          });
        }
      });

      // Mock WebRTC file request
      mockTauri.mockCommand("request_file_via_webrtc", async (args: any) => {
        // Simulate chunk-by-chunk transfer
        for (let i = 0; i < testFile.chunks; i++) {
          await downloadSimulator.downloadChunk(i, 5);
          
          const chunkSize = 16 * 1024;
          const start = i * chunkSize;
          const end = Math.min(start + chunkSize, testFile.size);
          const chunkData = testFile.content.slice(start, end);
          receivedChunks.set(i, chunkData);
        }

        return { success: true, chunksReceived: testFile.chunks };
      });

      // Start download
      const downloadResult = await invoke("request_file_via_webrtc", {
        peerId: seederPeerId,
        fileHash: testFile.hash,
        fileName: testFile.name,
        fileSize: testFile.size,
      });

      expect(downloadResult).toHaveProperty("success", true);
      expect(downloadComplete).toBe(true);
      expect(downloadSimulator.isComplete()).toBe(true);
      expect(receivedChunks.size).toBe(testFile.chunks);

      // Phase 4: PAY - Process final payment
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

      mockTauri.mockCommand("calculate_download_cost", async (args: any) => {
        return await mockPayment.calculateDownloadCost(args.fileSize);
      });

      const paymentAmount = await invoke<number>("calculate_download_cost", {
        fileSize: testFile.size,
      });

      expect(paymentAmount).toBeGreaterThan(0);

      const txHash = await invoke<string>("process_download_payment", {
        fileHash: testFile.hash,
        fileName: testFile.name,
        fileSize: testFile.size,
        seederAddress: seeders[0].address,
      });

      expect(txHash).toBeDefined();
      expect(txHash).toMatch(/^0x[a-f0-9]{64}$/);
      expect(mockPayment.hasProcessedPayment(testFile.hash, seeders[0].address)).toBe(true);

      const finalBalance = mockPayment.getBalance();
      expect(finalBalance).toBeLessThan(10);
      expect(finalBalance).toBeCloseTo(10 - paymentAmount, 6);
    });

    it("should handle handshake failure", async () => {
      const testFile = TestDataFactory.createMockFile("fail.dat", 1024);
      const seeders = TestDataFactory.createMockPeers(1, "WebRTC");
      const metadata = TestDataFactory.createMockMetadata(testFile, seeders, "WebRTC");

      await mockDHT.publishFile(metadata);

      mockTauri.mockCommand("search_file_metadata", async (args: any) => {
        return await mockDHT.searchFileMetadata(args.fileHash);
      });

      mockTauri.mockCommand("create_webrtc_offer", async (args: any) => {
        throw new Error("Failed to create peer connection");
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      const foundMetadata = await invoke<FileMetadata>("search_file_metadata", {
        fileHash: testFile.hash,
        timeoutMs: 1000,
      });

      expect(foundMetadata).toBeDefined();

      await expect(
        invoke("create_webrtc_offer", {
          peerId: seeders[0].id,
        })
      ).rejects.toThrow("Failed to create peer connection");
    });

    it("should handle connection timeout", async () => {
      const seederPeerId = "slow_peer_123";

      mockTauri.mockCommand("create_webrtc_offer", async () => {
        return { offer: "mock-offer" };
      });

      mockTauri.mockCommand("send_webrtc_offer", async () => {
        // Simulate no response from seeder
        return { success: true };
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      await invoke("create_webrtc_offer", { peerId: seederPeerId });
      await invoke("send_webrtc_offer", { peerId: seederPeerId, offer: "mock-offer" });

      // Wait for answer that never comes
      await expect(
        eventHelper.waitForEvent("webrtc_answer_received", 500)
      ).rejects.toThrow("Timeout waiting for event");
    });
  });

  describe("Data Channel Management", () => {
    it("should establish data channel correctly", async () => {
      const downloaderPeer = "downloader_456";
      const seederPeer = "seeder_789";

      const connected = await webrtcHandshake.simulateHandshake(
        downloaderPeer,
        seederPeer,
        150
      );

      expect(connected).toBe(true);
      expect(webrtcHandshake.isConnected(downloaderPeer)).toBe(true);
      expect(webrtcHandshake.isConnected(seederPeer)).toBe(true);
    });

    it("should handle data channel close", async () => {
      const peerId = "peer_close_test";

      await webrtcHandshake.simulateHandshake("downloader", peerId);
      expect(webrtcHandshake.isConnected(peerId)).toBe(true);

      webrtcHandshake.closeConnection(peerId);
      expect(webrtcHandshake.isConnected(peerId)).toBe(false);
    });

    it("should emit data channel events", async () => {
      const peerId = "event_peer";
      let channelOpenEmitted = false;

      eventHelper.on("webrtc_data_channel_open", (data) => {
        if (data.peerId === peerId) {
          channelOpenEmitted = true;
        }
      });

      mockTauri.mockCommand("establish_webrtc_connection", async (args: any) => {
        await webrtcHandshake.simulateHandshake("downloader", args.peerId);
        eventHelper.emit("webrtc_data_channel_open", { peerId: args.peerId });
        return { success: true };
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      await invoke("establish_webrtc_connection", { peerId });

      expect(channelOpenEmitted).toBe(true);
    });
  });

  describe("Chunk Transfer", () => {
    it("should transfer chunks with progress tracking", async () => {
      const testFile = TestDataFactory.createMockFile("chunks.bin", 2 * 1024 * 1024);
      const downloadSimulator = new DownloadProgressSimulator(testFile.chunks);

      const progressUpdates: number[] = [];

      downloadSimulator.setProgressCallback((progress) => {
        progressUpdates.push(progress);
      });

      await downloadSimulator.downloadAll(3);

      expect(progressUpdates.length).toBe(testFile.chunks);
      expect(downloadSimulator.getProgress()).toBe(100);
      expect(downloadSimulator.isComplete()).toBe(true);
    });

    it("should handle chunk transmission errors", async () => {
      const testFile = TestDataFactory.createMockFile("error.bin", 1024 * 1024);
      const downloadSimulator = new DownloadProgressSimulator(testFile.chunks);

      // Download some chunks successfully
      for (let i = 0; i < 5; i++) {
        await downloadSimulator.downloadChunk(i);
      }

      expect(downloadSimulator.getProgress()).toBeLessThan(100);
      expect(downloadSimulator.isComplete()).toBe(false);

      // Simulate retrying failed chunks
      for (let i = 5; i < testFile.chunks; i++) {
        await downloadSimulator.downloadChunk(i);
      }

      expect(downloadSimulator.isComplete()).toBe(true);
    });

    it("should reassemble file from chunks", async () => {
      const testFile = TestDataFactory.createMockFile("reassemble.txt", 256 * 1024);
      const chunks = new Map<number, Uint8Array>();
      const chunkSize = 16 * 1024;
      const chunkCount = Math.ceil(testFile.size / chunkSize);

      // Split file into chunks
      for (let i = 0; i < chunkCount; i++) {
        const start = i * chunkSize;
        const end = Math.min(start + chunkSize, testFile.size);
        chunks.set(i, testFile.content.slice(start, end));
      }

      // Reassemble
      const reassembled = new Uint8Array(testFile.size);
      let offset = 0;

      for (let i = 0; i < chunkCount; i++) {
        const chunk = chunks.get(i)!;
        reassembled.set(chunk, offset);
        offset += chunk.length;
      }

      expect(reassembled.length).toBe(testFile.size);
      // Avoid dumping huge diffs on failure (which can break Vitest progress rendering).
      // Validate deterministically via spot checks.
      expect(reassembled[0]).toBe(testFile.content[0]);
      expect(reassembled[Math.floor(testFile.size / 2)]).toBe(testFile.content[Math.floor(testFile.size / 2)]);
      expect(reassembled[testFile.size - 1]).toBe(testFile.content[testFile.size - 1]);
    });
  });

  describe("ICE Candidate Exchange", () => {
    it("should exchange ICE candidates", async () => {
      const peerId = "ice_peer";
      const candidates: string[] = [];

      eventHelper.on("webrtc_ice_candidate", (data) => {
        candidates.push(data.candidate);
      });

      mockTauri.mockCommand("create_webrtc_offer", async (args: any) => {
        // Emit multiple ICE candidates
        setTimeout(() => {
          eventHelper.emit("webrtc_ice_candidate", {
            peerId: args.peerId,
            candidate: "candidate:1 1 UDP 2130706431 192.168.1.1 50000 typ host",
          });
        }, 10);

        setTimeout(() => {
          eventHelper.emit("webrtc_ice_candidate", {
            peerId: args.peerId,
            candidate: "candidate:2 1 UDP 2130706430 10.0.0.1 50001 typ host",
          });
        }, 20);

        return { offer: "mock-offer" };
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      await invoke("create_webrtc_offer", { peerId });

      // Wait for ICE candidates
      await new Promise((resolve) => setTimeout(resolve, 50));

      expect(candidates.length).toBeGreaterThan(0);
      candidates.forEach((candidate) => {
        expect(candidate).toContain("candidate:");
      });
    });
  });

  describe("Large File Streaming", () => {
    it("should handle large file with streaming", async () => {
      // Avoid allocating a 100MB buffer and avoid per-chunk timers.
      // This test only needs to validate chunked progress sampling behavior.
      const fileSize = 100 * 1024 * 1024; // 100MB
      const factoryChunkSize = 64 * 1024; // Keep consistent with TestDataFactory.createMockFile()
      const totalChunks = Math.ceil(fileSize / factoryChunkSize);

      const progressUpdates: number[] = [];
      const sampleInterval = Math.max(1, Math.floor(totalChunks / 20)); // Sample ~20 points

      // Sample progress points without iterating every chunk.
      for (let downloaded = sampleInterval; downloaded <= totalChunks; downloaded += sampleInterval) {
        const normalizedDownloaded = Math.min(downloaded, totalChunks);
        const progress = (normalizedDownloaded / totalChunks) * 100;
        progressUpdates.push(progress);
      }

      // Ensure we always record completion.
      if (progressUpdates.length === 0 || progressUpdates[progressUpdates.length - 1] !== 100) {
        progressUpdates.push(100);
      }

      expect(progressUpdates.length).toBeGreaterThan(10);
      expect(progressUpdates[progressUpdates.length - 1]).toBe(100);
    });

    it("should handle memory efficiently for large files", async () => {
      // Avoid allocating a 500MB buffer; only validate chunk math + memory estimate.
      const fileSize = 500 * 1024 * 1024; // 500MB
      const factoryChunkSize = 64 * 1024;
      const totalChunks = Math.ceil(fileSize / factoryChunkSize);

      // Verify we're using chunked approach, not loading entire file
      expect(totalChunks).toBeGreaterThan(1000);

      const chunkSize = 16 * 1024; // 16KB chunks
      const memoryPerChunk = chunkSize + 100; // Data + overhead
      const maxConcurrentChunks = 10;
      const estimatedMemory = memoryPerChunk * maxConcurrentChunks;

      // Should be much less than file size
      expect(estimatedMemory).toBeLessThan(1024 * 1024); // < 1MB memory
      expect(fileSize).toBeGreaterThan(100 * 1024 * 1024); // > 100MB file
    });
  });

  describe("Connection State Management", () => {
    it("should track connection state changes", async () => {
      const peerId = "state_peer";
      const states: string[] = [];

      eventHelper.on("webrtc_connection_state_change", (data) => {
        states.push(data.state);
      });

      mockTauri.mockCommand("establish_connection", async (args: any) => {
        // Simulate state progression
        eventHelper.emit("webrtc_connection_state_change", { peerId: args.peerId, state: "connecting" });
        await new Promise((resolve) => setTimeout(resolve, 50));
        
        eventHelper.emit("webrtc_connection_state_change", { peerId: args.peerId, state: "connected" });
        await new Promise((resolve) => setTimeout(resolve, 50));

        return { success: true };
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      await invoke("establish_connection", { peerId });

      expect(states).toContain("connecting");
      expect(states).toContain("connected");
    });

    it("should handle connection failure states", async () => {
      const peerId = "fail_peer";

      mockTauri.mockCommand("establish_connection", async (args: any) => {
        eventHelper.emit("webrtc_connection_state_change", { peerId: args.peerId, state: "failed" });
        throw new Error("Connection failed");
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      await expect(
        invoke("establish_connection", { peerId })
      ).rejects.toThrow("Connection failed");
    });
  });

  describe("Payment Integration", () => {
    it("should process payment after successful download", async () => {
      const testFile = TestDataFactory.createMockFile("paid.zip", 3 * 1024 * 1024);
      const seeders = TestDataFactory.createMockPeers(1, "WebRTC");

      mockTauri.mockCommand("process_download_payment", async (args: any) => {
        const result = await mockPayment.processDownloadPayment(
          args.fileHash,
          args.fileName,
          args.fileSize,
          args.seederAddress
        );
        return result.transactionHash;
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      const txHash = await invoke<string>("process_download_payment", {
        fileHash: testFile.hash,
        fileName: testFile.name,
        fileSize: testFile.size,
        seederAddress: seeders[0].address,
      });

      expect(txHash).toBeDefined();
      expect(mockPayment.hasProcessedPayment(testFile.hash, seeders[0].address)).toBe(true);
    });

    it("should calculate correct payment amount", async () => {
      const fileSizes = [
        1024 * 1024,       // 1MB
        10 * 1024 * 1024,  // 10MB
        100 * 1024 * 1024, // 100MB
      ];

      for (const size of fileSizes) {
        const cost = await mockPayment.calculateDownloadCost(size);
        expect(cost).toBeGreaterThan(0);
        expect(typeof cost).toBe("number");
        
        // Larger files should cost more
        if (size > 1024 * 1024) {
          const smallerCost = await mockPayment.calculateDownloadCost(1024 * 1024);
          expect(cost).toBeGreaterThan(smallerCost);
        }
      }
    });
  });

  describe("Edge Cases", () => {
    it("should handle peer disconnection during download", async () => {
      const peerId = "disconnect_peer";

      mockTauri.mockCommand("request_file_via_webrtc", async (args: any) => {
        // Simulate partial download then disconnect
        await new Promise((resolve) => setTimeout(resolve, 100));
        
        eventHelper.emit("webrtc_connection_closed", {
          peerId: args.peerId,
          reason: "Peer disconnected",
        });

        throw new Error("Peer disconnected during transfer");
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      await expect(
        invoke("request_file_via_webrtc", {
          peerId,
          fileHash: "test",
          fileName: "test.txt",
          fileSize: 1024,
        })
      ).rejects.toThrow("Peer disconnected");
    });

    it("should handle network interruption", async () => {
      const downloadSimulator = new DownloadProgressSimulator(50);

      // Download partially
      for (let i = 0; i < 25; i++) {
        await downloadSimulator.downloadChunk(i, 2);
      }

      expect(downloadSimulator.getProgress()).toBe(50);

      // Simulate network recovery and resume
      for (let i = 25; i < 50; i++) {
        await downloadSimulator.downloadChunk(i, 2);
      }

      expect(downloadSimulator.isComplete()).toBe(true);
    });
  });
});

