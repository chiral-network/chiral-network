import { invoke } from "@tauri-apps/api/core";
import { createWebRTCSession } from "./webrtcService";
import { SignalingService } from "./signalingService";
import type { FileMetadata } from "../dht";

export interface P2PTransfer {
  id: string;
  fileHash: string;
  fileName: string;
  fileSize: number;
  seeders: string[];
  progress: number;
  status:
    | "connecting"
    | "transferring"
    | "completed"
    | "failed"
    | "cancelled"
    | "retrying";
  bytesTransferred: number;
  speed: number;
  eta?: number;
  error?: string;
  webrtcSession?: any;
  startTime: number;
  outputPath?: string;
  receivedChunks?: Map<number, Uint8Array>;
  currentSeederIndex?: number;
  retryCount?: number;
  lastError?: string;
  totalChunks?: number;
  corruptedChunks?: Set<number>;
}

export class P2PFileTransferService {
  private transfers = new Map<string, P2PTransfer>();
  private transferCallbacks = new Map<
    string,
    (transfer: P2PTransfer) => void
  >();
  private signalingService: SignalingService;

  constructor() {
    // Initialize signaling service for WebRTC coordination
    this.signalingService = new SignalingService();
  }

  private async getPeerMetrics(
    seeders: string[]
  ): Promise<Record<string, any>> {
    try {
      // Get peer metrics from DHT backend
      const metrics = (await invoke("get_peer_metrics")) as any[];

      // Convert to map for easy lookup
      const metricsMap: Record<string, any> = {};
      metrics.forEach((metric) => {
        if (seeders.includes(metric.peerId)) {
          metricsMap[metric.peerId] = metric;
        }
      });

      return metricsMap;
    } catch (error) {
      console.error("Failed to get peer metrics:", error);
      return {};
    }
  }

  private calculateSeederScore(_seederId: string, metrics: any): number {
    let score = 0;

    // Base score for being available
    score += 10;

    // Boost score for successful transfers
    if (metrics.successCount) {
      score += Math.min(metrics.successCount * 2, 20);
    }

    // Boost score for low latency (if available)
    if (metrics.averageLatency) {
      // Lower latency = higher score (inverse relationship)
      const latencyScore = Math.max(0, 10 - metrics.averageLatency / 100);
      score += latencyScore;
    }

    // Penalize for failures
    if (metrics.failureCount) {
      score -= Math.min(metrics.failureCount * 3, 15);
    }

    // Boost for recent activity
    if (metrics.lastSeenRecently) {
      score += 5;
    }

    return Math.max(0, score);
  }

  async initiateDownload(
    metadata: FileMetadata,
    seeders: string[],
    onProgress?: (transfer: P2PTransfer) => void
  ): Promise<string> {
    const transferId = `transfer-${Date.now()}-${Math.random().toString(36).substr(2, 9)}`;

    const transfer: P2PTransfer = {
      id: transferId,
      fileHash: metadata.fileHash,
      fileName: metadata.fileName,
      fileSize: metadata.fileSize,
      seeders,
      progress: 0,
      status: "connecting",
      bytesTransferred: 0,
      speed: 0,
      startTime: Date.now(),
    };

    this.transfers.set(transferId, transfer);

    if (onProgress) {
      this.transferCallbacks.set(transferId, onProgress);
    }

    // Try to establish connection with seeders
    await this.establishSeederConnection(transfer, metadata);

    return transferId;
  }

  async initiateDownloadWithSave(
    metadata: FileMetadata,
    seeders: string[],
    outputPath?: string,
    onProgress?: (transfer: P2PTransfer) => void
  ): Promise<string> {
    const transferId = await this.initiateDownload(
      metadata,
      seeders,
      onProgress
    );

    // If output path is provided, save file when transfer completes
    if (outputPath) {
      const transfer = this.transfers.get(transferId);
      if (transfer) {
        transfer.outputPath = outputPath;
      }
    }

    return transferId;
  }

  private async establishSeederConnection(
    transfer: P2PTransfer,
    metadata: FileMetadata
  ): Promise<void> {
    if (transfer.seeders.length === 0) {
      transfer.status = "failed";
      transfer.error = "No seeders available";
      this.notifyProgress(transfer);
      return;
    }

    // Initialize transfer state
    transfer.currentSeederIndex = 0;
    transfer.retryCount = 0;
    transfer.totalChunks = Math.ceil(metadata.fileSize / (16 * 1024)); // Assume 16KB chunks
    transfer.corruptedChunks = new Set();

    // Connect to signaling service if not connected
    try {
      await this.signalingService.connect();
    } catch (error) {
      transfer.status = "failed";
      transfer.error = "Failed to connect to signaling service";
      this.notifyProgress(transfer);
      return;
    }

    // Try to connect to seeders with retry logic
    await this.tryConnectToSeeder(transfer, metadata);
  }

  private async tryConnectToSeeder(
    transfer: P2PTransfer,
    metadata: FileMetadata,
    maxRetries: number = 3
  ): Promise<void> {
    const maxSeederIndex = transfer.seeders.length;

    while (
      transfer.currentSeederIndex! < maxSeederIndex &&
      transfer.retryCount! < maxRetries
    ) {
      const seederId = transfer.seeders[transfer.currentSeederIndex!];

      try {
        transfer.status = "connecting";
        transfer.lastError = undefined;
        this.notifyProgress(transfer);

        // Create WebRTC session for this seeder
        const webrtcSession = createWebRTCSession({
          isInitiator: true,
          peerId: seederId,
          signaling: this.signalingService,
          onConnectionStateChange: (state) => {
            if (state === "connected") {
              transfer.status = "transferring";
              transfer.retryCount = 0; // Reset retry count on successful connection
              this.notifyProgress(transfer);
              this.startFileTransfer(transfer, metadata);
            } else if (state === "failed" || state === "disconnected") {
              this.handleConnectionFailure(
                transfer,
                metadata,
                `WebRTC connection ${state}`
              );
            }
          },
          onDataChannelOpen: () => {},
          onMessage: (data) => {
            this.handleIncomingChunk(transfer, data);
          },
          onError: (error) => {
            console.error("WebRTC error:", error);
            this.handleConnectionFailure(
              transfer,
              metadata,
              "WebRTC connection error"
            );
          },
        });

        transfer.webrtcSession = webrtcSession;

        // Create offer and establish connection with timeout
        try {
          const offer = await Promise.race([
            webrtcSession.createOffer(),
            this.createTimeoutPromise(10000, "WebRTC offer creation timeout"),
          ]);

          console.log("Created WebRTC offer for seeder:", seederId);

          // Use backend to coordinate the connection
          await Promise.race([
            invoke("establish_webrtc_connection", {
              peerId: seederId,
              offer: JSON.stringify(offer),
            }),
            this.createTimeoutPromise(
              15000,
              "WebRTC connection establishment timeout"
            ),
          ]);

          // If we reach here, connection was successful
          return;
        } catch (error) {
          console.error(
            `Failed to create WebRTC offer for ${seederId}:`,
            error
          );
          webrtcSession.close();

          if (
            error === "WebRTC offer creation timeout" ||
            error === "WebRTC connection establishment timeout"
          ) {
            this.handleConnectionFailure(transfer, metadata, error as string);
          } else {
            this.handleConnectionFailure(
              transfer,
              metadata,
              `WebRTC setup failed: ${error}`
            );
          }
        }
      } catch (error) {
        console.error(`Failed to connect to seeder ${seederId}:`, error);
        this.handleConnectionFailure(
          transfer,
          metadata,
          `Connection failed: ${error}`
        );
      }
    }

    // No seeders connected successfully
    transfer.status = "failed";
    transfer.error = "Could not connect to any seeders after retries";
    this.notifyProgress(transfer);
  }

  private handleConnectionFailure(
    transfer: P2PTransfer,
    metadata: FileMetadata,
    error: string
  ): void {
    transfer.lastError = error;
    transfer.retryCount = (transfer.retryCount || 0) + 1;

    // Try next seeder
    transfer.currentSeederIndex = (transfer.currentSeederIndex || 0) + 1;

    if (transfer.currentSeederIndex! < transfer.seeders.length) {
      // Continue trying other seeders
    } else if (transfer.retryCount! < 3) {
      transfer.currentSeederIndex = 0;
      transfer.status = "retrying";
      this.notifyProgress(transfer);

      // Wait before retrying
      setTimeout(() => {
        this.tryConnectToSeeder(transfer, metadata);
      }, 2000 * transfer.retryCount!); // Exponential backoff
    } else {
      transfer.status = "failed";
      transfer.error = `Failed after ${transfer.retryCount} retries. Last error: ${error}`;
      this.notifyProgress(transfer);
    }
  }

  private createTimeoutPromise<T>(
    ms: number,
    errorMessage: string
  ): Promise<T> {
    return new Promise((_, reject) => {
      setTimeout(() => reject(errorMessage), ms);
    });
  }

  private startFileTransfer(
    transfer: P2PTransfer,
    metadata: FileMetadata
  ): void {
    if (!transfer.webrtcSession) {
      transfer.status = "failed";
      transfer.error = "No WebRTC session available";
      this.notifyProgress(transfer);
      return;
    }

    // Send file request through the WebRTC data channel
    const fileRequest = {
      type: "file_request",
      fileHash: metadata.fileHash,
      fileName: metadata.fileName,
      fileSize: metadata.fileSize,
      requesterPeerId: "local_peer", // This should be the actual local peer ID
    };

    try {
      transfer.webrtcSession.send(JSON.stringify(fileRequest));

      // Start parallel chunk downloading
      this.startParallelChunkDownload(transfer, metadata);
    } catch (error) {
      console.error("Failed to send file request:", error);
      transfer.status = "failed";
      transfer.error = "Failed to send file request";
      this.notifyProgress(transfer);
    }
  }

  private startParallelChunkDownload(
    transfer: P2PTransfer,
    metadata: FileMetadata
  ): void {
    const totalChunks = Math.ceil(metadata.fileSize / (16 * 1024)); // 16KB chunks
    const parallelRequests = Math.min(5, totalChunks); // Request up to 5 chunks in parallel

    // Request initial batch of chunks
    for (let i = 0; i < parallelRequests && i < totalChunks; i++) {
      this.requestChunk(transfer, i);
    }

    // Continue requesting chunks as they arrive
    this.continueParallelDownload(transfer, totalChunks, parallelRequests);
  }

  private continueParallelDownload(
    transfer: P2PTransfer,
    totalChunks: number,
    parallelRequests: number
  ): void {
    // This method will be called periodically to request more chunks
    // For now, we'll use a simple interval-based approach
    const checkInterval = setInterval(() => {
      // Stop if transfer is not active anymore
      if (
        transfer.status !== "transferring" &&
        transfer.status !== "connecting"
      ) {
        clearInterval(checkInterval);
        return;
      }

      const receivedCount = transfer.receivedChunks?.size || 0;
      const requestedCount = this.getRequestedChunkCount(transfer);

      // If we have capacity for more requests, request more chunks
      if (
        receivedCount + requestedCount < totalChunks &&
        requestedCount < parallelRequests
      ) {
        const nextChunkIndex = receivedCount + requestedCount;
        if (nextChunkIndex < totalChunks) {
          this.requestChunk(transfer, nextChunkIndex);
        }
      }
    }, 100); // Check every 100ms

    // Set up a one-time check for completion after a delay
    setTimeout(() => {
      if (
        transfer.status === "completed" ||
        transfer.status === "failed" ||
        transfer.status === "cancelled"
      ) {
        clearInterval(checkInterval);
      }
    }, 1000);
  }

  private requestChunk(transfer: P2PTransfer, chunkIndex: number): void {
    if (!transfer.webrtcSession) return;

    try {
      const chunkRequest = {
        type: "chunk_request",
        fileHash: transfer.fileHash,
        chunkIndex: chunkIndex,
      };

      transfer.webrtcSession.send(JSON.stringify(chunkRequest));
    } catch (error) {
      console.error(`Failed to request chunk ${chunkIndex}:`, error);
    }
  }

  private getRequestedChunkCount(transfer: P2PTransfer): number {
    // This is a simplified calculation - in a complete implementation,
    // you'd track which chunks have been requested but not yet received
    return Math.max(
      0,
      (transfer.progress / 100) * (transfer.totalChunks || 0) -
        (transfer.receivedChunks?.size || 0)
    );
  }

  private handleIncomingChunk(transfer: P2PTransfer, data: any): void {
    try {
      const message = typeof data === "string" ? JSON.parse(data) : data;

      if (message.type === "file_chunk") {
        // Handle incoming file chunk
        // Initialize chunks map if not exists
        if (!transfer.receivedChunks) {
          transfer.receivedChunks = new Map();
        }

        // Validate chunk data
        if (!this.validateChunk(message)) {
          console.warn("Received corrupted chunk:", message.chunk_index);
          transfer.corruptedChunks?.add(message.chunk_index);

          // Request chunk again if we have a connection
          if (transfer.webrtcSession) {
            this.requestChunkAgain(transfer, message.chunk_index);
          }
          return;
        }

        // Store the chunk data
        const chunkData = new Uint8Array(message.data);
        transfer.receivedChunks.set(message.chunk_index, chunkData);

        // Remove from corrupted chunks if it was previously corrupted
        transfer.corruptedChunks?.delete(message.chunk_index);

        // Update progress
        transfer.bytesTransferred += chunkData.length;
        const progress = (transfer.bytesTransferred / transfer.fileSize) * 100;
        transfer.progress = Math.min(100, progress);

        // Calculate speed
        const elapsed = (Date.now() - transfer.startTime) / 1000;
        transfer.speed = transfer.bytesTransferred / elapsed;

        // Check if transfer is complete
        if (this.isTransferComplete(transfer, message.total_chunks)) {
          transfer.status = "completed";

          // Save file if output path is specified
          if (transfer.outputPath) {
            this.saveCompletedFile(transfer);
          }
        }

        this.notifyProgress(transfer);
      } else if (message.type === "dht_message") {
        // Handle DHT signaling messages
        this.handleDhtMessage(message);
      }
    } catch (error) {
      console.error("Error handling incoming chunk:", error);
    }
  }

  private validateChunk(chunkMessage: any): boolean {
    // Basic validation - check if chunk data exists and chunk index is valid
    if (!chunkMessage.data || typeof chunkMessage.chunk_index !== "number") {
      return false;
    }

    // In a complete implementation, you would verify checksums here
    // For now, we'll assume chunks are valid if they have data and a valid index
    return true;
  }

  private isTransferComplete(
    transfer: P2PTransfer,
    totalChunks: number
  ): boolean {
    if (!transfer.receivedChunks) return false;

    // Check if we have all chunks
    const expectedChunks = totalChunks;
    const receivedChunks = transfer.receivedChunks.size;

    return (
      receivedChunks >= expectedChunks && transfer.corruptedChunks?.size === 0
    );
  }

  private requestChunkAgain(transfer: P2PTransfer, chunkIndex: number): void {
    if (!transfer.webrtcSession) return;

    try {
      const chunkRequest = {
        type: "chunk_request",
        fileHash: transfer.fileHash,
        chunkIndex: chunkIndex,
      };

      transfer.webrtcSession.send(JSON.stringify(chunkRequest));
    } catch (error) {
      console.error("Failed to request chunk again:", error);
    }
  }

  private async saveCompletedFile(transfer: P2PTransfer): Promise<void> {
    if (!transfer.receivedChunks || !transfer.outputPath) {
      return;
    }

    try {
      // Sort chunks by index and concatenate
      const sortedChunks = Array.from(transfer.receivedChunks.entries())
        .sort(([a], [b]) => a - b)
        .map(([, data]) => data);

      const fileData = new Uint8Array(transfer.fileSize);
      let offset = 0;

      for (const chunk of sortedChunks) {
        fileData.set(chunk, offset);
        offset += chunk.length;
      }

      // Save file using Tauri API
      await invoke("write_file", {
        path: transfer.outputPath,
        contents: Array.from(fileData),
      });
    } catch (error) {
      console.error("Error saving completed file:", error);
      transfer.status = "failed";
      transfer.error = "Failed to save file";
      this.notifyProgress(transfer);
    }
  }

  private handleDhtMessage(message: any): void {
    // Handle WebRTC signaling messages received through DHT
    if (message.message?.type === "webrtc_signaling") {
      // This would contain WebRTC signaling data (offer/answer/candidate)
      // For now, we'll log it, but in a real implementation,
      // this would be passed to the WebRTC session
    }
  }

  private notifyProgress(transfer: P2PTransfer): void {
    const callback = this.transferCallbacks.get(transfer.id);
    if (callback) {
      callback(transfer);
    }
  }

  cancelTransfer(transferId: string): void {
    const transfer = this.transfers.get(transferId);
    if (transfer) {
      transfer.status = "cancelled";

      // Close WebRTC session if it exists
      if (transfer.webrtcSession) {
        transfer.webrtcSession.close();
      }

      this.notifyProgress(transfer);
      this.transfers.delete(transferId);
      this.transferCallbacks.delete(transferId);
    }
  }

  getTransfer(transferId: string): P2PTransfer | undefined {
    return this.transfers.get(transferId);
  }

  getAllTransfers(): P2PTransfer[] {
    return Array.from(this.transfers.values());
  }
}

// Singleton instance
export const p2pFileTransferService = new P2PFileTransferService();
