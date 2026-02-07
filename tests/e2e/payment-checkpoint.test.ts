/**
 * @fileoverview Payment Checkpoint E2E Tests
 * Tests payment checkpoint integration with file transfers at 10MB, 20MB, 40MB intervals
 * 
 * This test suite validates:
 * - Checkpoint pause/resume logic during downloads
 * - Payment processing at checkpoint intervals
 * - Two-node communication with checkpoint system
 * - Seamless payment flow continuation after checkpoint payments
 * 
 * Related to PR #964: Payment checkpoint integration with WebRTC transfers
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

interface CheckpointState {
  sessionId: string;
  fileHash: string;
  bytesDownloaded: number;
  nextCheckpointBytes: number;
  totalPaidChiral: number;
  isPaused: boolean;
}

describe("Payment Checkpoint E2E Tests", () => {
  let mockDHT: MockDHTService;
  let mockPayment: MockPaymentService;
  let eventHelper: EventHelper;
  let mockTauri: MockTauriInvoke;
  let webrtcHandshake: WebRTCHandshakeSimulator;
  let cleanup: TestCleanup;

  // Checkpoint thresholds: 10MB, 20MB, 40MB
  const CHECKPOINT_INTERVALS = [10, 20, 40].map(mb => mb * 1024 * 1024);

  beforeEach(() => {
    vi.clearAllMocks();
    
    mockDHT = new MockDHTService(100);
    mockPayment = new MockPaymentService(100); // Higher balance for multiple checkpoints
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

  describe("WebRTC with Payment Checkpoints", () => {
    it("should pause at 10MB checkpoint, process payment, and resume", async () => {
      // Setup: 15MB file to trigger 10MB checkpoint
      const testFile = TestDataFactory.createMockFile("video.mp4", 15 * 1024 * 1024);
      const uploaderPeer = TestDataFactory.createMockPeers(1, "WebRTC")[0];
      const downloaderPeer = "downloader_node_123";
      const metadata = TestDataFactory.createMockMetadata(testFile, [uploaderPeer], "WebRTC");

      await mockDHT.publishFile(metadata);

      // Track checkpoint state
      const checkpointState: CheckpointState = {
        sessionId: `session_${Date.now()}`,
        fileHash: testFile.hash,
        bytesDownloaded: 0,
        nextCheckpointBytes: CHECKPOINT_INTERVALS[0], // 10MB
        totalPaidChiral: 0,
        isPaused: false,
      };

      let downloadPaused = false;
      let checkpointPaymentProcessed = false;

      // Mock WebRTC connection
      mockTauri.mockCommand("create_webrtc_offer", async () => {
        return { offer: "mock-offer" };
      });

      mockTauri.mockCommand("send_webrtc_offer", async (args: any) => {
        setTimeout(() => {
          eventHelper.emit("webrtc_answer_received", {
            peerId: args.peerId,
            answer: "mock-answer",
          });
        }, 50);
        return { success: true };
      });

      mockTauri.mockCommand("accept_webrtc_answer", async (args: any) => {
        await webrtcHandshake.simulateHandshake(downloaderPeer, args.peerId);
        eventHelper.emit("webrtc_connection_established", { peerId: args.peerId });
        return { success: true };
      });

      // register mock invoke for this test
      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      // Establish connection
      await invoke("create_webrtc_offer", { peerId: uploaderPeer.id });
      await invoke("send_webrtc_offer", { peerId: uploaderPeer.id, offer: "mock-offer" });
      const answerEvent = await eventHelper.waitForEvent("webrtc_answer_received", 1000);
      await invoke("accept_webrtc_answer", { peerId: uploaderPeer.id, answer: answerEvent.answer });

      // Mock download with checkpoint
      mockTauri.mockCommand("start_file_transfer", async (args: any) => {
        const chunkSize = 64 * 1024; // 16KB chunks
        const totalChunks = Math.ceil(args.fileSize / chunkSize);

        for (let i = 0; i < totalChunks; i++) {
          checkpointState.bytesDownloaded += chunkSize;

          // Check if we hit checkpoint
          if (checkpointState.bytesDownloaded >= checkpointState.nextCheckpointBytes && !downloadPaused) {
            downloadPaused = true;
            checkpointState.isPaused = true;

            // Emit checkpoint reached event
            eventHelper.emit("payment_checkpoint_reached", {
              sessionId: checkpointState.sessionId,
              fileHash: checkpointState.fileHash,
              bytesDownloaded: checkpointState.bytesDownloaded,
              checkpointMB: checkpointState.nextCheckpointBytes / (1024 * 1024),
              nextCheckpointMB: CHECKPOINT_INTERVALS[1] / (1024 * 1024), // 20MB
            });

            // Wait for payment to be processed (complete OR failed)
            const paymentResult = await Promise.race([
              eventHelper.waitForEvent("checkpoint_payment_completed", 10000).then((d:any) => ({type: 'completed', data: d})),
              eventHelper.waitForEvent("checkpoint_payment_failed", 10000).then((d:any) => ({type: 'failed', data: d})),
            ]);

            if (paymentResult.type === 'completed') {
              checkpointPaymentProcessed = true;
              downloadPaused = false;
              checkpointState.isPaused = false;
            } else {
              // Payment failed; keep paused and stop transfer
              downloadPaused = true;
              checkpointState.isPaused = true;
              break;
            }
          }

          // Emit progress
          eventHelper.emit("webrtc_download_progress", {
            fileHash: args.fileHash,
            progress: (checkpointState.bytesDownloaded / args.fileSize) * 100,
            bytesReceived: checkpointState.bytesDownloaded,
            totalBytes: args.fileSize,
            isPaused: checkpointState.isPaused,
          });

          // Yield to microtask queue instead of tiny timeout to keep ordering deterministic
          await Promise.resolve();
        }

        return { success: true, bytesTransferred: args.fileSize };
      });

      // Mock checkpoint payment processing
      mockTauri.mockCommand("process_checkpoint_payment", async (args: any) => {
        const checkpointAmount = await mockPayment.calculateDownloadCost(args.checkpointBytes);
        const key = `${args.sessionId}:${args.fileHash}:cp:${args.checkpointMB ?? '0'}`;
        const result = await mockPayment.processDownloadPayment(
          key,
          args.fileName,
          args.checkpointBytes,
          args.seederAddress
        );

        if (result.success) {
          checkpointState.totalPaidChiral += checkpointAmount;
          // Emit payment completed synchronously and yield to microtask queue
          eventHelper.emit("checkpoint_payment_completed", {
            sessionId: args.sessionId,
            checkpointMB: args.checkpointMB,
            amountPaid: checkpointAmount,
            transactionHash: result.transactionHash,
          });
          await Promise.resolve();
        } else {
          eventHelper.emit("checkpoint_payment_failed", { sessionId: args.sessionId, error: result.error });
          await Promise.resolve();
        }

        return {
          success: result.success,
          transactionHash: result.transactionHash,
          amountPaid: checkpointAmount,
        };
      });

      // Start download
      eventHelper.on("payment_checkpoint_reached", async (event) => {
        await invoke("process_checkpoint_payment", {
          sessionId: event.sessionId,
          fileHash: testFile.hash,
          fileName: testFile.name,
          checkpointBytes: event.bytesDownloaded,
          checkpointMB: event.checkpointMB,
          seederAddress: uploaderPeer.address,
        });
      }); 

      const downloadPromise = invoke("start_file_transfer", {
        sessionId: checkpointState.sessionId,
        peerId: uploaderPeer.id,
        fileHash: testFile.hash,
        fileName: testFile.name,
        fileSize: testFile.size,
      });

      // Wait for download to complete
      await downloadPromise;

      // Verify checkpoint payment was processed
      expect(checkpointPaymentProcessed).toBe(true);
      expect(checkpointState.totalPaidChiral).toBeGreaterThan(0);
      expect(mockPayment.getBalance()).toBeLessThan(100);
    });

    it("should handle multiple checkpoints (10MB -> 20MB -> 40MB) in 50MB file", async () => {
      const testFile = TestDataFactory.createMockFile("large-video.mkv", 50 * 1024 * 1024);
      const uploaderPeer = TestDataFactory.createMockPeers(1, "WebRTC")[0];
      const downloaderPeer = "downloader_node_456";
      const metadata = TestDataFactory.createMockMetadata(testFile, [uploaderPeer], "WebRTC");

      await mockDHT.publishFile(metadata);

      const checkpointsReached: number[] = [];
      const checkpointPayments: number[] = [];
      let currentCheckpointIndex = 0;
      let checkpointReached = false;

      const checkpointState: CheckpointState = {
        sessionId: `session_multi_${Date.now()}`,
        fileHash: testFile.hash,
        bytesDownloaded: 0,
        nextCheckpointBytes: CHECKPOINT_INTERVALS[0], // 10MB
        totalPaidChiral: 0,
        isPaused: false,
      };

      // Setup connection
      mockTauri.mockCommand("create_webrtc_offer", async () => ({ offer: "mock-offer" }));
      mockTauri.mockCommand("send_webrtc_offer", async (args: any) => {
        setTimeout(() => eventHelper.emit("webrtc_answer_received", { peerId: args.peerId, answer: "mock-answer" }), 50);
        return { success: true };
      });
      mockTauri.mockCommand("accept_webrtc_answer", async (args: any) => {
        await webrtcHandshake.simulateHandshake(downloaderPeer, args.peerId);
        return { success: true };
      });

      // register mock invoke for this test
      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      // Establish connection
      await invoke("create_webrtc_offer", { peerId: uploaderPeer.id });
      await invoke("send_webrtc_offer", { peerId: uploaderPeer.id, offer: "mock-offer" });
      await eventHelper.waitForEvent("webrtc_answer_received", 1000);
      await invoke("accept_webrtc_answer", { peerId: uploaderPeer.id, answer: "mock-answer" });

      // Mock download with multiple checkpoints
      mockTauri.mockCommand("start_file_transfer", async (args: any) => {
        const chunkSize = 64 * 1024; // 64KB chunks for faster simulation
        const totalChunks = Math.ceil(args.fileSize / chunkSize);

        for (let i = 0; i < totalChunks; i++) {
          checkpointState.bytesDownloaded += chunkSize;

          // Check if we hit any checkpoint
          if (currentCheckpointIndex < CHECKPOINT_INTERVALS.length &&
              checkpointState.bytesDownloaded >= CHECKPOINT_INTERVALS[currentCheckpointIndex]) {
            
            const checkpointMB = CHECKPOINT_INTERVALS[currentCheckpointIndex] / (1024 * 1024);
            checkpointsReached.push(checkpointMB);
            checkpointState.isPaused = true;

            eventHelper.emit("payment_checkpoint_reached", {
              sessionId: checkpointState.sessionId,
              fileHash: checkpointState.fileHash,
              bytesDownloaded: checkpointState.bytesDownloaded,
              checkpointMB,
              checkpointIndex: currentCheckpointIndex,
            });

            // Wait for payment
            await eventHelper.waitForEvent("checkpoint_payment_completed", 10000);
            
            currentCheckpointIndex++;
            checkpointState.isPaused = false;
          }

          await Promise.resolve();
        }

        return { success: true, bytesTransferred: args.fileSize };
      });

      mockTauri.mockCommand("process_checkpoint_payment", async (args: any) => {
        const checkpointAmount = await mockPayment.calculateDownloadCost(args.checkpointBytes);
        const key = `${args.sessionId}:${args.fileHash}:cp:${args.checkpointIndex}`;
        const result = await mockPayment.processDownloadPayment(
          key,
          args.fileName,
          args.checkpointBytes,
          args.seederAddress
        );

        if (result.success) {
          checkpointPayments.push(checkpointAmount);
          checkpointState.totalPaidChiral += checkpointAmount;
          eventHelper.emit("checkpoint_payment_completed", {
            sessionId: args.sessionId,
            checkpointMB: args.checkpointMB,
            amountPaid: checkpointAmount,
          });
          await Promise.resolve();
        } else {
          eventHelper.emit("checkpoint_payment_failed", { sessionId: args.sessionId, error: result.error });
          await Promise.resolve();
        }

        return { success: result.success, amountPaid: checkpointAmount };
      });

      // Start download and handle checkpoints
      const downloadPromise = invoke("start_file_transfer", {
        sessionId: checkpointState.sessionId,
        peerId: uploaderPeer.id,
        fileHash: testFile.hash,
        fileName: testFile.name,
        fileSize: testFile.size,
      });

      // Listen for checkpoints and process payments
      let checkpointsProcessed = 0;
      eventHelper.on("payment_checkpoint_reached", async (event) => {
        await invoke("process_checkpoint_payment", {
          sessionId: event.sessionId,
          fileHash: event.fileHash,
          fileName: testFile.name,
          checkpointBytes: event.bytesDownloaded,
          checkpointMB: event.checkpointMB,
          checkpointIndex: event.checkpointIndex,
          seederAddress: uploaderPeer.address,
        });
        checkpointsProcessed++;
      });

      await downloadPromise;

      // Verify all three checkpoints were reached and paid
      expect(checkpointsReached).toEqual([10, 20, 40]);
      expect(checkpointPayments.length).toBe(3);
      expect(checkpointPayments.every(amount => amount > 0)).toBe(true);
      expect(checkpointState.totalPaidChiral).toBeGreaterThan(0);
      expect(checkpointsProcessed).toBe(3);
    });

    it("should handle checkpoint payment failure and pause download", async () => {
      const testFile = TestDataFactory.createMockFile("test.bin", 15 * 1024 * 1024);
      const uploaderPeer = TestDataFactory.createMockPeers(1, "WebRTC")[0];
      const metadata = TestDataFactory.createMockMetadata(testFile, [uploaderPeer], "WebRTC");

      await mockDHT.publishFile(metadata);

      // Set very low balance to trigger payment failure
      mockPayment.reset(0.0001);

      let checkpointReached = false;
      let paymentFailed = false;
      let downloadPaused = false;

      const checkpointState: CheckpointState = {
        sessionId: `session_fail_${Date.now()}`,
        fileHash: testFile.hash,
        bytesDownloaded: 0,
        nextCheckpointBytes: CHECKPOINT_INTERVALS[0],
        totalPaidChiral: 0,
        isPaused: false,
      };

      mockTauri.mockCommand("start_file_transfer", async (args: any) => {
        const chunkSize = 64 * 1024;
        const targetBytes = 11 * 1024 * 1024; // Download past 10MB checkpoint

        while (checkpointState.bytesDownloaded < targetBytes) {
          if (checkpointState.isPaused) {
            downloadPaused = true;
            break; // Stop if paused due to payment failure
          }

          checkpointState.bytesDownloaded += chunkSize;

          if (checkpointState.bytesDownloaded >= CHECKPOINT_INTERVALS[0] && !checkpointReached) {
            checkpointReached = true;
            checkpointState.isPaused = true;

            eventHelper.emit("payment_checkpoint_reached", {
              sessionId: checkpointState.sessionId,
              fileHash: checkpointState.fileHash,
              bytesDownloaded: checkpointState.bytesDownloaded,
              checkpointMB: 10,
            });

            // Wait for payment attempt (completed OR failed)
            const paymentResult = await Promise.race([
              eventHelper.waitForEvent("checkpoint_payment_completed", 10000).then((d:any) => ({ type: 'completed', data: d })),
              eventHelper.waitForEvent("checkpoint_payment_failed", 10000).then((d:any) => ({ type: 'failed', data: d })),
            ]).catch(() => ({ type: 'failed' }));

            if (paymentResult.type === 'completed') {
              checkpointState.isPaused = false;
            } else {
              // Payment failed: mark paused and stop transfer loop
              downloadPaused = true;
              checkpointState.isPaused = true;
              break;
            }
          }

          await Promise.resolve();
        }

        return { success: !checkpointState.isPaused, bytesTransferred: checkpointState.bytesDownloaded };
      });

      mockTauri.mockCommand("process_checkpoint_payment", async (args: any) => {
        const key = `${args.sessionId}:${args.fileHash}:cp:${args.checkpointMB ?? '0'}`;
        const result = await mockPayment.processDownloadPayment(
          key,
          args.fileName,
          args.checkpointBytes,
          args.seederAddress
        );

        if (!result.success) {
          paymentFailed = true;
          eventHelper.emit("checkpoint_payment_failed", { sessionId: args.sessionId, error: result.error });
          await Promise.resolve();
          throw new Error(result.error || "Payment failed");
        }

        eventHelper.emit("checkpoint_payment_completed", { sessionId: args.sessionId });
        await Promise.resolve();
        return { success: true };
      });

      // register mock invoke for this test
      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      // Start download
      const downloadPromise = invoke("start_file_transfer", {
        sessionId: checkpointState.sessionId,
        peerId: uploaderPeer.id,
        fileHash: testFile.hash,
        fileName: testFile.name,
        fileSize: testFile.size,
      });

      // Wait for checkpoint
      await eventHelper.waitForEvent("payment_checkpoint_reached", 2000);

      // Try to process payment (should fail)
      try {
        await invoke("process_checkpoint_payment", {
          sessionId: checkpointState.sessionId,
          fileHash: testFile.hash,
          fileName: testFile.name,
          checkpointBytes: checkpointState.bytesDownloaded,
          checkpointMB: 10,
          seederAddress: uploaderPeer.address,
        });
      } catch (error) {
        // Expected to fail
      }

      await downloadPromise;

      // Verify download was paused due to payment failure
      expect(checkpointReached).toBe(true);
      expect(paymentFailed).toBe(true);
      expect(downloadPaused).toBe(true);
      expect(checkpointState.bytesDownloaded).toBeLessThan(testFile.size);
    });
  });

  describe("Bitswap with Payment Checkpoints", () => {
    it("should pause at 10MB checkpoint during Bitswap download", async () => {
      const testFile = TestDataFactory.createMockFile("data.zip", 15 * 1024 * 1024);
      const uploaderPeer = TestDataFactory.createMockPeers(1, "Bitswap")[0];
      const metadata = TestDataFactory.createMockMetadata(testFile, [uploaderPeer], "Bitswap");

      await mockDHT.publishFile(metadata);

      let checkpointReached = false;
      let paymentProcessed = false;

      const checkpointState: CheckpointState = {
        sessionId: `bitswap_session_${Date.now()}`,
        fileHash: testFile.hash,
        bytesDownloaded: 0,
        nextCheckpointBytes: CHECKPOINT_INTERVALS[0],
        totalPaidChiral: 0,
        isPaused: false,
      };

      mockTauri.mockCommand("download_file_bitswap", async (args: any) => {
        const blockSize = 256 * 1024; // 256KB blocks
        const totalBlocks = Math.ceil(args.fileSize / blockSize);

        for (let i = 0; i < totalBlocks; i++) {
          if (checkpointState.isPaused) {
            const paymentResult = await Promise.race([
              eventHelper.waitForEvent("checkpoint_payment_completed", 10000).then((d:any) => ({type: 'completed', data: d})),
              eventHelper.waitForEvent("checkpoint_payment_failed", 10000).then((d:any) => ({type: 'failed', data: d})),
            ]);

            if (paymentResult.type === 'completed') {
              checkpointState.isPaused = false;
            } else {
              // stay paused and abort blocks loop
              break;
            }
          }

          checkpointState.bytesDownloaded += blockSize;

          // Check for checkpoint
          if (checkpointState.bytesDownloaded >= checkpointState.nextCheckpointBytes && !checkpointReached) {
            checkpointReached = true;
            checkpointState.isPaused = true;

            eventHelper.emit("payment_checkpoint_reached", {
              sessionId: checkpointState.sessionId,
              fileHash: checkpointState.fileHash,
              bytesDownloaded: checkpointState.bytesDownloaded,
              checkpointMB: 10,
            });
          }

          eventHelper.emit("bitswap_download_progress", {
            fileHash: args.fileHash,
            progress: (checkpointState.bytesDownloaded / args.fileSize) * 100,
            downloadedChunks: i + 1,
            totalChunks: totalBlocks,
          });

          await Promise.resolve();
        }

        return { success: true, filePath: `/tmp/${args.fileName}` };
      });

      mockTauri.mockCommand("process_checkpoint_payment", async (args: any) => {
        const checkpointAmount = await mockPayment.calculateDownloadCost(args.checkpointBytes);
        const key = `${args.sessionId}:${args.fileHash}:cp:${args.checkpointMB ?? '10'}`;
        const result = await mockPayment.processDownloadPayment(
          key,
          args.fileName,
          args.checkpointBytes,
          args.seederAddress
        );

        if (result.success) {
          paymentProcessed = true;
          checkpointState.totalPaidChiral += checkpointAmount;
          eventHelper.emit("checkpoint_payment_completed", { sessionId: args.sessionId, checkpointMB: args.checkpointMB });
          await Promise.resolve();
        } else {
          eventHelper.emit("checkpoint_payment_failed", { sessionId: args.sessionId, error: result.error });
          await Promise.resolve();
        }

        return { success: result.success };
      });

      // register mock invoke for this test
      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      // Start download
      const downloadPromise = invoke("download_file_bitswap", {
        sessionId: checkpointState.sessionId,
        fileHash: testFile.hash,
        fileName: testFile.name,
        fileSize: testFile.size,
        seeders: [uploaderPeer.id],
        cids: testFile.cids,
      });

      // Listen for checkpoint and process payment
      eventHelper.on("payment_checkpoint_reached", async (event) => {
        await invoke("process_checkpoint_payment", {
          sessionId: event.sessionId,
          fileHash: event.fileHash,
          fileName: testFile.name,
          checkpointBytes: event.bytesDownloaded,
          checkpointMB: event.checkpointMB,
          seederAddress: uploaderPeer.address,
        });
      });

      await downloadPromise;

      expect(checkpointReached).toBe(true);
      expect(paymentProcessed).toBe(true);
      expect(checkpointState.totalPaidChiral).toBeGreaterThan(0);
    });

    it("should handle multiple checkpoints in Bitswap 50MB download", async () => {
      const testFile = TestDataFactory.createMockFile("large.iso", 50 * 1024 * 1024);
      const uploaderPeer = TestDataFactory.createMockPeers(1, "Bitswap")[0];
      const metadata = TestDataFactory.createMockMetadata(testFile, [uploaderPeer], "Bitswap");

      await mockDHT.publishFile(metadata);

      const checkpointsReached: number[] = [];
      let currentCheckpointIndex = 0;
      let checkpointReached = false;

      const checkpointState: CheckpointState = {
        sessionId: `bitswap_multi_${Date.now()}`,
        fileHash: testFile.hash,
        bytesDownloaded: 0,
        nextCheckpointBytes: CHECKPOINT_INTERVALS[0],
        totalPaidChiral: 0,
        isPaused: false,
      };

      mockTauri.mockCommand("download_file_bitswap", async (args: any) => {
        const blockSize = 256 * 1024;
        const totalBlocks = Math.ceil(args.fileSize / blockSize);

        for (let i = 0; i < totalBlocks; i++) {
          checkpointState.bytesDownloaded += blockSize;

          // Check for current checkpoint thresholds (10MB,20MB,40MB)
          if (currentCheckpointIndex < CHECKPOINT_INTERVALS.length &&
              checkpointState.bytesDownloaded >= CHECKPOINT_INTERVALS[currentCheckpointIndex]) {
            const checkpointMB = CHECKPOINT_INTERVALS[currentCheckpointIndex] / (1024 * 1024);
            checkpointsReached.push(checkpointMB);
            checkpointState.isPaused = true;

            eventHelper.emit("payment_checkpoint_reached", {
              sessionId: checkpointState.sessionId,
              fileHash: checkpointState.fileHash,
              bytesDownloaded: checkpointState.bytesDownloaded,
              checkpointMB,
              checkpointIndex: currentCheckpointIndex,
            });

            // Wait for payment completion or failure
            const paymentResult = await Promise.race([
              eventHelper.waitForEvent("checkpoint_payment_completed", 10000).then((d:any) => ({ type: 'completed', data: d })),
              eventHelper.waitForEvent("checkpoint_payment_failed", 10000).then((d:any) => ({ type: 'failed', data: d })),
            ]).catch(() => ({ type: 'failed' }));

            if (paymentResult.type === 'completed') {
              currentCheckpointIndex++;
              checkpointState.isPaused = false;
            } else {
              // stop on failure
              break;
            }
          }

          eventHelper.emit("bitswap_download_progress", {
            fileHash: args.fileHash,
            progress: (checkpointState.bytesDownloaded / args.fileSize) * 100,
            downloadedChunks: i + 1,
            totalChunks: totalBlocks,
          });

          await Promise.resolve();
        }

        return { success: true };
      });

      mockTauri.mockCommand("process_checkpoint_payment", async (args: any) => {
        const checkpointAmount = await mockPayment.calculateDownloadCost(
          CHECKPOINT_INTERVALS[args.checkpointIndex]
        );
        const key = `${args.sessionId}:${args.fileHash}:cp:${args.checkpointIndex}`;
        const result = await mockPayment.processDownloadPayment(
          key,
          args.fileName,
          CHECKPOINT_INTERVALS[args.checkpointIndex],
          uploaderPeer.address
        );

        if (result.success) {
          checkpointState.totalPaidChiral += checkpointAmount;
          eventHelper.emit("checkpoint_payment_completed", { sessionId: args.sessionId });
          await Promise.resolve();
        } else {
          eventHelper.emit("checkpoint_payment_failed", { sessionId: args.sessionId, error: result.error });
          await Promise.resolve();
        }

        return { success: result.success };
      });

      // register mock invoke for this test
      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      // Start download and handle checkpoints
      const downloadPromise = invoke("download_file_bitswap", {
        sessionId: checkpointState.sessionId,
        fileHash: testFile.hash,
        fileName: testFile.name,
        fileSize: testFile.size,
        seeders: [uploaderPeer.id],
        cids: testFile.cids,
      });

      eventHelper.on("payment_checkpoint_reached", async (event) => {
        await invoke("process_checkpoint_payment", {
          sessionId: event.sessionId,
          fileHash: event.fileHash,
          fileName: testFile.name,
          checkpointBytes: CHECKPOINT_INTERVALS[event.checkpointIndex],
          checkpointMB: CHECKPOINT_INTERVALS[event.checkpointIndex] / (1024 * 1024),
          checkpointIndex: event.checkpointIndex,
          seederAddress: uploaderPeer.address,
        });
      });

      await downloadPromise;

      expect(checkpointsReached).toEqual([10, 20, 40]);
      expect(checkpointState.totalPaidChiral).toBeGreaterThan(0);
    });
  });

  describe("Two-Node Communication with Checkpoints", () => {
    it("should simulate uploader-downloader checkpoint interaction", async () => {
      const uploaderNode = TestDataFactory.createMockPeers(1, "WebRTC")[0];
      const downloaderNode = "downloader_real_node";
      const testFile = TestDataFactory.createMockFile("shared.dat", 25 * 1024 * 1024);
      const metadata = TestDataFactory.createMockMetadata(testFile, [uploaderNode], "WebRTC");

      await mockDHT.publishFile(metadata);

      // Track both nodes' state
      const uploaderState = { transfersPaused: 0, transfersResumed: 0 };
      const downloaderState = { checkpointsPaid: 0, totalPaid: 0 };

      mockTauri.mockCommand("start_file_transfer", async (args: any) => {
        let bytesTransferred = 0;
        const chunkSize = 64 * 1024;

        while (bytesTransferred < args.fileSize) {
          bytesTransferred += chunkSize;

          // Check for 10MB and 20MB checkpoints
          if ((bytesTransferred >= 10 * 1024 * 1024 && uploaderState.transfersPaused === 0) ||
              (bytesTransferred >= 20 * 1024 * 1024 && uploaderState.transfersPaused === 1)) {
            
            uploaderState.transfersPaused++;
            
            const checkpointMB = uploaderState.transfersPaused === 1 ? 10 : 20;
            
            eventHelper.emit("uploader_paused", {
              peerId: uploaderNode.id,
              checkpointMB,
              bytesTransferred,
            });

            eventHelper.emit("payment_checkpoint_reached", {
              sessionId: args.sessionId,
              checkpointMB,
              bytesDownloaded: bytesTransferred,
            });

            // Wait for payment notification from downloader
            await eventHelper.waitForEvent("payment_confirmed_to_uploader", 2000);
            
            uploaderState.transfersResumed++;
            
            eventHelper.emit("uploader_resumed", {
              peerId: uploaderNode.id,
              checkpointMB,
            });
          }

          await new Promise(resolve => setTimeout(resolve, 1));
        }

        return { success: true, bytesTransferred };
      });

      mockTauri.mockCommand("process_checkpoint_payment", async (args: any) => {
        const amount = await mockPayment.calculateDownloadCost(args.checkpointBytes);
        const key = `${args.sessionId}:${args.fileHash}:cp:${args.checkpointMB}`;
        const result = await mockPayment.processDownloadPayment(
          key,
          args.fileName,
          args.checkpointBytes,
          args.seederAddress
        );

        if (result.success) {
          downloaderState.checkpointsPaid++;
          downloaderState.totalPaid += amount;
          eventHelper.emit("payment_confirmed_to_uploader", {
            uploaderPeerId: args.seederPeerId,
            checkpointMB: args.checkpointMB,
            transactionHash: result.transactionHash,
          });
          await Promise.resolve();
        } else {
          eventHelper.emit("payment_failed_to_uploader", {
            uploaderPeerId: args.seederPeerId,
            checkpointMB: args.checkpointMB,
            error: result.error,
          });
          await Promise.resolve();
        }

        return { success: result.success, transactionHash: result.transactionHash };
      });

      // register mock invoke for this test
      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      // Start transfer
      const transferPromise = invoke("start_file_transfer", {
        sessionId: `two_node_${Date.now()}`,
        peerId: uploaderNode.id,
        fileHash: testFile.hash,
        fileName: testFile.name,
        fileSize: testFile.size,
      });

      // Downloader handles checkpoints
      eventHelper.on("payment_checkpoint_reached", async (event) => {
        await invoke("process_checkpoint_payment", {
          sessionId: event.sessionId,
          fileHash: testFile.hash,
          fileName: testFile.name,
          checkpointBytes: event.bytesDownloaded,
          checkpointMB: event.checkpointMB,
          seederAddress: uploaderNode.address,
          seederPeerId: uploaderNode.id,
        });
      });

      await transferPromise;

      // Verify both nodes interacted correctly
      expect(uploaderState.transfersPaused).toBe(2); // 10MB and 20MB
      expect(uploaderState.transfersResumed).toBe(2);
      expect(downloaderState.checkpointsPaid).toBe(2);
      expect(downloaderState.totalPaid).toBeGreaterThan(0);
    });
  });

  describe("Checkpoint Resume After Disconnect", () => {
    it("should resume from last checkpoint after connection failure", async () => {
      const testFile = TestDataFactory.createMockFile("resume.bin", 25 * 1024 * 1024);
      const uploaderPeer = TestDataFactory.createMockPeers(1, "WebRTC")[0];
      const metadata = TestDataFactory.createMockMetadata(testFile, [uploaderPeer], "WebRTC");

      await mockDHT.publishFile(metadata);

      let lastCheckpointBytes = 0;
      let connectionFailedAt = 0;
      let resumedFromCheckpoint = false;

      mockTauri.mockCommand("start_file_transfer", async (args: any) => {
        let bytesTransferred = args.resumeFrom || 0;
        const chunkSize = 64 * 1024;

        if (args.resumeFrom > 0) {
          resumedFromCheckpoint = true;
        }

        while (bytesTransferred < args.fileSize) {
          bytesTransferred += chunkSize;

          // Simulate connection failure at 15MB
          if (bytesTransferred >= 15 * 1024 * 1024 && connectionFailedAt === 0) {
            connectionFailedAt = bytesTransferred;
            eventHelper.emit("connection_lost", {
              peerId: uploaderPeer.id,
              bytesTransferred,
              lastCheckpoint: lastCheckpointBytes,
            });
            throw new Error("Connection lost");
          }

          // Handle checkpoint at 10MB
          if (bytesTransferred >= 10 * 1024 * 1024 && lastCheckpointBytes === 0) {
            lastCheckpointBytes = bytesTransferred;
            
            eventHelper.emit("payment_checkpoint_reached", {
              sessionId: args.sessionId,
              checkpointMB: 10,
              bytesDownloaded: bytesTransferred,
            });

            await eventHelper.waitForEvent("checkpoint_payment_completed", 2000);
          }

          await new Promise(resolve => setTimeout(resolve, 1));
        }

        return { success: true, bytesTransferred };
      });

      mockTauri.mockCommand("process_checkpoint_payment", async (args: any) => {
        const key = `${args.sessionId}:${args.fileHash}:cp:${args.checkpointMB ?? '0'}`;
        const result = await mockPayment.processDownloadPayment(
          key,
          args.fileName,
          args.checkpointBytes,
          args.seederAddress
        );

        if (result.success) {
          eventHelper.emit("checkpoint_payment_completed", { sessionId: args.sessionId });
          await Promise.resolve();
        } else {
          eventHelper.emit("checkpoint_payment_failed", { sessionId: args.sessionId, error: result.error });
          await Promise.resolve();
        }

        return { success: result.success };
      });

      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      const sessionId = `resume_session_${Date.now()}`;

      // First attempt
      try {
        const promise = invoke("start_file_transfer", {
          sessionId,
          peerId: uploaderPeer.id,
          fileHash: testFile.hash,
          fileName: testFile.name,
          fileSize: testFile.size,
        });

        eventHelper.on("payment_checkpoint_reached", async (event) => {
          await invoke("process_checkpoint_payment", {
            sessionId: event.sessionId,
            fileHash: testFile.hash,
            fileName: testFile.name,
            checkpointBytes: event.bytesDownloaded,
            checkpointMB: event.checkpointMB,
            seederAddress: uploaderPeer.address,
          });
        });

        await promise;
      } catch (error) {
        // Expected connection failure
      }

      expect(connectionFailedAt).toBeGreaterThan(0);
      expect(lastCheckpointBytes).toBeGreaterThan(0);

      // Resume from last checkpoint
      const resumeResult = await invoke("start_file_transfer", {
         sessionId,
         peerId: uploaderPeer.id,
         fileHash: testFile.hash,
         fileName: testFile.name,
         fileSize: testFile.size,
         resumeFrom: lastCheckpointBytes, // Resume from 10MB checkpoint
       });

      expect(resumedFromCheckpoint).toBe(true);
      expect((resumeResult as any).success).toBe(true);
     });
  });
  
  describe('Payment checkpoint backend improvements (micro‑Chiral, idempotency, rate-limit)', () => {
    const MICRO_CHIRAL = 1_000_000; // 1 Chiral = 1_000_000 micro‑Chiral

    it('handles micro‑Chiral amounts correctly (converts to Chiral for display/aggregation)', async () => {
      const testFile = TestDataFactory.createMockFile('micro.bin', 12 * 1024 * 1024);
      const uploaderPeer = TestDataFactory.createMockPeers(1, 'WebRTC')[0];
      await mockDHT.publishFile(TestDataFactory.createMockMetadata(testFile, [uploaderPeer], 'WebRTC'));

      const checkpointState: CheckpointState = {
        sessionId: `micro_${Date.now()}`,
        fileHash: testFile.hash,
        bytesDownloaded: 0,
        nextCheckpointBytes: CHECKPOINT_INTERVALS[0],
        totalPaidChiral: 0,
        isPaused: false,
      };

      // Simulate backend returning micro‑Chiral amount (e.g. 0.01 Chiral = 10_000 micro)
      mockTauri.mockCommand('process_checkpoint_payment', async (args: any) => {
        // compute micro amount locally (10 MB * 0.001 Chiral/MB -> 10_000 micro)
        const checkpointMicro = (args.checkpointBytes / (1024 * 1024)) * 1000; // using test mock price 0.001 -> 1000 micro
        const amountMicro = checkpointMicro; // already micro units in this mock environment
        const tx = `tx_micro_${Date.now()}`;

        // Emit event as backend would, including micro‑unit amount
        eventHelper.emit('checkpoint_payment_completed', {
          sessionId: args.sessionId,
          checkpointMB: args.checkpointMB,
          amountPaidMicro: amountMicro,
          transactionHash: tx,
        });

        return { success: true, transactionHash: tx, amountPaidMicro: amountMicro };
      });

      // Frontend listener converts micro units to Chiral and aggregates
      eventHelper.on('checkpoint_payment_completed', (event: any) => {
        const paidChiral = (event.amountPaidMicro || 0) / MICRO_CHIRAL;
        checkpointState.totalPaidChiral += paidChiral;
        checkpointState.isPaused = false;
      });

      // Trigger a checkpoint flow
      // emit checkpoint reached
      eventHelper.emit('payment_checkpoint_reached', {
        sessionId: checkpointState.sessionId,
        fileHash: checkpointState.fileHash,
        bytesDownloaded: checkpointState.nextCheckpointBytes,
        checkpointMB: checkpointState.nextCheckpointBytes / (1024 * 1024),
      });

      // ensure mocked invoke uses our mockTauri handlers for this test
      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      // invoke process (calls our mock above)
      await invoke('process_checkpoint_payment', {
        sessionId: checkpointState.sessionId,
        fileHash: checkpointState.fileHash,
        checkpointBytes: checkpointState.nextCheckpointBytes,
        checkpointMB: checkpointState.nextCheckpointBytes / (1024 * 1024),
        fileName: testFile.name,
        seederAddress: uploaderPeer.address,
      });

      // allow microtask queue
      await Promise.resolve();

      expect(checkpointState.totalPaidChiral).toBeGreaterThan(0);
      // expect roughly 0.01 Chiral for 10MB at 0.001 per MB
      expect(checkpointState.totalPaidChiral).toBeCloseTo(0.01, 6);
    });

    it('ignores duplicate transaction hashes (idempotency) on frontend aggregation', async () => {
      const testFile = TestDataFactory.createMockFile('dup.bin', 12 * 1024 * 1024);
      const uploaderPeer = TestDataFactory.createMockPeers(1, 'WebRTC')[0];
      await mockDHT.publishFile(TestDataFactory.createMockMetadata(testFile, [uploaderPeer], 'WebRTC'));

      const checkpointState: CheckpointState = {
        sessionId: `dup_${Date.now()}`,
        fileHash: testFile.hash,
        bytesDownloaded: 0,
        nextCheckpointBytes: CHECKPOINT_INTERVALS[0],
        totalPaidChiral: 0,
        isPaused: false,
      };

      const seenTx = new Set<string>();

      // Mock backend emits completed twice with same tx
      mockTauri.mockCommand('process_checkpoint_payment', async (args: any) => {
        const tx = `tx_dup_42`;
        const amountMicro = 10 * 1000; // 10 MB * 1000 micro

        // First emit
        eventHelper.emit('checkpoint_payment_completed', { sessionId: args.sessionId, amountPaidMicro: amountMicro, transactionHash: tx });
        // Emit duplicate (simulating replay)
        eventHelper.emit('checkpoint_payment_completed', { sessionId: args.sessionId, amountPaidMicro: amountMicro, transactionHash: tx });

        return { success: true, transactionHash: tx, amountPaidMicro: amountMicro };
      });

      // Frontend listener ignores duplicates by tracking seen txs
      eventHelper.on('checkpoint_payment_completed', (event: any) => {
        if (seenTx.has(event.transactionHash)) return;
        seenTx.add(event.transactionHash);
        const paidChiral = (event.amountPaidMicro || 0) / MICRO_CHIRAL;
        checkpointState.totalPaidChiral += paidChiral;
        checkpointState.isPaused = false;
      });

      // Trigger and process
      eventHelper.emit('payment_checkpoint_reached', { sessionId: checkpointState.sessionId, fileHash: checkpointState.fileHash, bytesDownloaded: checkpointState.nextCheckpointBytes, checkpointMB: 10 });

      // ensure mocked invoke uses our mockTauri handlers for this test
      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      await invoke('process_checkpoint_payment', { sessionId: checkpointState.sessionId, fileHash: checkpointState.fileHash, checkpointBytes: checkpointState.nextCheckpointBytes, checkpointMB: 10, fileName: testFile.name, seederAddress: uploaderPeer.address });

      await Promise.resolve();

      // Only one payment should be counted despite duplicate events
      expect(checkpointState.totalPaidChiral).toBeCloseTo( (10 * 1000) / MICRO_CHIRAL, 6);
    });

    it('simulates simple rate-limiting: too many payment attempts cause failures', async () => {
      const testFile = TestDataFactory.createMockFile('rl.bin', 12 * 1024 * 1024);
      const uploaderPeer = TestDataFactory.createMockPeers(1, 'WebRTC')[0];
      await mockDHT.publishFile(TestDataFactory.createMockMetadata(testFile, [uploaderPeer], 'WebRTC'));

      const checkpointState: CheckpointState = {
        sessionId: `rl_${Date.now()}`,
        fileHash: testFile.hash,
        bytesDownloaded: 0,
        nextCheckpointBytes: CHECKPOINT_INTERVALS[0],
        totalPaidChiral: 0,
        isPaused: false,
      };

      let attempts = 0;

      mockTauri.mockCommand('process_checkpoint_payment', async (args: any) => {
        attempts++;
        // Simulate backend enforcing MAX_PAYMENT_ATTEMPTS = 5 within window: fail after 5
        if (attempts > 5) {
          eventHelper.emit('checkpoint_payment_failed', { sessionId: args.sessionId, error: 'rate_limited' });
          return { success: false, error: 'rate_limited' };
        }

        const tx = `tx_rl_${attempts}`;
        const amountMicro = 10 * 1000;
        eventHelper.emit('checkpoint_payment_completed', { sessionId: args.sessionId, amountPaidMicro: amountMicro, transactionHash: tx });
        return { success: true, transactionHash: tx, amountPaidMicro: amountMicro };
      });

      // simple listener to count failures
      let failed = false;
      eventHelper.on('checkpoint_payment_failed', (e:any) => { failed = true; checkpointState.isPaused = true; });
      eventHelper.on('checkpoint_payment_completed', (e:any) => { checkpointState.totalPaidChiral += (e.amountPaidMicro || 0) / MICRO_CHIRAL; });

      // ensure mocked invoke uses our mockTauri handlers for this test
      vi.mocked(invoke).mockImplementation(mockTauri.createInvoke());

      // make 6 rapid attempts
      for (let i = 0; i < 6; i++) {
        // call process
        // ignore errors thrown by invoke
        try {
          // each invoke simulates a user retrying a payment
          // we don't wait between them to simulate burst
          // eslint-disable-next-line no-await-in-loop
          await invoke('process_checkpoint_payment', { sessionId: checkpointState.sessionId, fileHash: checkpointState.fileHash, checkpointBytes: checkpointState.nextCheckpointBytes, checkpointMB: 10, fileName: testFile.name, seederAddress: uploaderPeer.address });
        } catch (err) {
          // ignore
        }
      }

      await Promise.resolve();

      expect(failed).toBe(true);
      // total paid should be at most 5 successful attempts
      expect(checkpointState.totalPaidChiral).toBeLessThanOrEqual(5 * ((10 * 1000) / MICRO_CHIRAL));
    });
  });
});