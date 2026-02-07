import { invoke } from "@tauri-apps/api/core";
import { createWebRTCSession } from "./webrtcService";
import { SignalingService } from "./signalingService";
import type { FileMetadata } from "../dht";
import type { FileManifestForJs } from "./encryption";
import PeerSelectionService from "./peerSelectionService";

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
  receivedChunks?: Map<number, Uint8Array>; // Legacy: only used for small files
  requestedChunks?: Set<number>;
  currentSeederIndex?: number;
  retryCount?: number;
  lastError?: string;
  totalChunks?: number;
  corruptedChunks?: Set<number>;
  // Streaming download session (for large files - writes directly to disk)
  streamingSessionId?: string;
  receivedChunkIndices?: Set<number>; // Track which chunks received (not the data)
  chunkSize: number;
}

export class P2PFileTransferService {
  private transfers = new Map<string, P2PTransfer>();
  private transferCallbacks = new Map<
    string,
    (transfer: P2PTransfer) => void
  >();
  private webrtcSessions = new Map<string, any>(); // peerId → WebRTCSession
  private signalingService: SignalingService;

  constructor() {
    // Initialize signaling service for WebRTC coordination
    this.signalingService = new SignalingService({
      preferDht: false,  // Force WebSocket for WebRTC file transfers
      persistPeers: false  // Don't persist peers to avoid stale peer IDs
    });
  }

  async getFileMetadata(fileHash: string): Promise<any> {
    // Use the file hash to retrieve metadata from DHT
    return await invoke("get_file_metadata", { fileHash });
  }

  // Constants for streaming vs in-memory threshold
  private static readonly CHUNK_SIZE = 16 * 1024; // 16KB chunks
  private static readonly STREAMING_THRESHOLD = 1024 * 1024; // 1MB - use streaming for files larger than this

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
      chunkSize: P2PFileTransferService.CHUNK_SIZE,
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
    transfer.totalChunks = Math.ceil(metadata.fileSize / P2PFileTransferService.CHUNK_SIZE);
    transfer.corruptedChunks = new Set();
    transfer.requestedChunks = new Set();
    transfer.receivedChunkIndices = new Set();
    // Note: Streaming session is initialized after manifest response for accurate chunk count

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
    maxRetries: number = 3,
    preferredSeeders?: string[]
  ): Promise<void> {
    // For WebRTC, we need to find the correct seeders from the file metadata
    // metadata.seeders contains both libp2p peer IDs and WebSocket client IDs
    // We need to find which ones are WebSocket client IDs by checking if they're in the connected peers list
    let availableSeeders = preferredSeeders;

    if (!availableSeeders) {
      const availablePeers = this.signalingService.peers;
      let peersList: string[] = [];

      // Subscribe to peers list
      const unsubscribe = availablePeers.subscribe((peers) => {
        peersList = peers;
      });

      // Filter seeders that are currently connected via WebSocket
      availableSeeders =
        metadata.seeders?.filter((seeder) => peersList.includes(seeder)) || [];

      unsubscribe();
    }

    // If no matching seeder found in connected peers, fail immediately
    if (!availableSeeders || availableSeeders.length === 0) {
      transfer.status = "failed";
      const connectedPeersMessage = preferredSeeders?.length
        ? preferredSeeders.join(", ")
        : "none";
      transfer.error = `No WebRTC seeder online for this file. File seeders: ${metadata.seeders?.join(', ') || 'none'}. Connected peers: ${connectedPeersMessage}`;
      this.notifyProgress(transfer);
      return;
    }

    transfer.currentSeederIndex = transfer.currentSeederIndex ?? 0;
    transfer.retryCount = transfer.retryCount ?? 0;

    const maxSeeders = availableSeeders.length;
    const seederId = availableSeeders[transfer.currentSeederIndex % maxSeeders];

    console.log(
      `Attempting WebRTC connection to seeder ${seederId} (index ${transfer.currentSeederIndex! + 1}/${maxSeeders})`
    );

    try {
      transfer.status = "connecting";
      transfer.lastError = undefined;
      this.notifyProgress(transfer);

      // Create WebRTC session for this seeder
      const webrtcSession = createWebRTCSession({
        isInitiator: true,
        peerId: seederId,
        onLocalIceCandidate: (_candidate) => {
          // ICE candidates are handled by the backend WebRTC coordination
          console.log("ICE candidate generated for peer:", seederId);
        },
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
              `WebRTC connection ${state}`,
              availableSeeders,
              maxRetries
            );
          }
        },
        onDataChannelOpen: () => {},
        onMessage: async (data) => {
          await this.handleIncomingChunk(transfer, data);
        },
        onError: (error) => {
          console.error("WebRTC error:", error);
          this.handleConnectionFailure(
            transfer,
            metadata,
            "WebRTC connection error",
            availableSeeders,
            maxRetries
          );
        },
      });

      transfer.webrtcSession = webrtcSession;
      this.webrtcSessions.set(seederId, webrtcSession);

      // Create offer - signaling service will automatically send it to the peer
      try {
        // createOffer() both creates and sends the offer via signaling
        await Promise.race([
          webrtcSession.createOffer(),
          this.createTimeoutPromise(10000, "WebRTC offer creation timeout"),
        ]);

        console.log("Created and sent WebRTC offer to seeder:", seederId);
        console.log("Waiting for answer via signaling...");

        // Wait for connection to establish via signaling (answer will come back automatically)
        await Promise.race([
          this.waitForConnection(webrtcSession, 15000),
          this.createTimeoutPromise(
            15000,
            "WebRTC connection establishment timeout"
          ),
        ]);

        console.log("WebRTC connection established with peer:", seederId);
      } catch (error) {
        console.error(
          `Failed to establish WebRTC connection with ${seederId}:`,
          error
        );
        webrtcSession.close();
        this.webrtcSessions.delete(seederId);

        if (
          error === "WebRTC offer creation timeout" ||
          error === "WebRTC connection establishment timeout"
        ) {
          this.handleConnectionFailure(
            transfer,
            metadata,
            error as string,
            availableSeeders,
            maxRetries
          );
        } else {
          this.handleConnectionFailure(
            transfer,
            metadata,
            `WebRTC setup failed: ${error}`,
            availableSeeders,
            maxRetries
          );
        }
      }
    } catch (error) {
      console.error(`Failed to connect to seeder ${seederId}:`, error);
      this.handleConnectionFailure(
        transfer,
        metadata,
        `Connection failed: ${error}`,
        availableSeeders,
        maxRetries
      );
    }
  }

  private async handleConnectionFailure(
    transfer: P2PTransfer,
    metadata: FileMetadata,
    error: string,
    availableSeeders?: string[],
    maxRetries: number = 5
  ): Promise<void> {
    transfer.lastError = error;
    transfer.retryCount = (transfer.retryCount || 0) + 1;

    // Update reputation for failed connection/transfer
    const failedSeederPeerId = transfer.seeders[transfer.currentSeederIndex || 0];
    if (failedSeederPeerId) {
      try {
        await invoke('record_transfer_failure', {
          peerId: failedSeederPeerId,
          error: error,
        });
        PeerSelectionService.notePeerFailure(failedSeederPeerId);
        console.log(`❌ Updated reputation for peer ${failedSeederPeerId.substring(0, 20)}... after connection failure`);
      } catch (repError) {
        console.error('Failed to update peer reputation:', repError);
      }
    }

    // Save checkpoint before retry so progress isn't lost
    if (transfer.streamingSessionId) {
      this.saveCheckpoint(transfer);
    }

    // Clean up current session
    if (transfer.webrtcSession) {
      try {
        transfer.webrtcSession.close();
      } catch (e) { /* ignore */ }
      if (transfer.webrtcSession.peerId) {
        this.webrtcSessions.delete(transfer.webrtcSession.peerId);
      }
      transfer.webrtcSession = undefined;
    }

    // Try next seeder
    transfer.currentSeederIndex = (transfer.currentSeederIndex || 0) + 1;

    const maxSeeders = availableSeeders?.length || transfer.seeders.length;

    if (transfer.currentSeederIndex! < maxSeeders) {
      // Continue trying other seeders
      console.log(
        `Trying next seeder (${transfer.currentSeederIndex}/${maxSeeders})`
      );
      transfer.status = "retrying";
      this.notifyProgress(transfer);
      this.tryConnectToSeeder(
        transfer,
        metadata,
        maxRetries,
        availableSeeders
      );
    } else if (transfer.retryCount! < maxRetries) {
      // Reset to first seeder and retry
      transfer.currentSeederIndex = 0;
      transfer.status = "retrying";
      this.notifyProgress(transfer);

      const backoffMs = Math.min(2000 * Math.pow(2, transfer.retryCount! - 1), 30000);
      console.log(`Retry ${transfer.retryCount}/${maxRetries} in ${backoffMs}ms`);

      setTimeout(() => {
        this.tryConnectToSeeder(transfer, metadata);
      }, backoffMs);
    } else {
      transfer.status = "failed";
      transfer.error = `Failed after ${transfer.retryCount} retries. Last error: ${error}`;
      // Save final checkpoint so user can manually resume later
      if (transfer.streamingSessionId) {
        this.saveCheckpoint(transfer);
      }
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

  /**
   * Wait for WebRTC connection to establish (via signaling)
   */
  private waitForConnection(session: any, timeoutMs: number): Promise<void> {
    return new Promise((resolve, reject) => {
      const startTime = Date.now();

      const checkConnection = () => {
        if (session.pc.connectionState === "connected") {
          resolve();
          return;
        }
        if (session.pc.connectionState === "failed" || session.pc.connectionState === "closed") {
          reject(new Error(`Connection ${session.pc.connectionState}`));
          return;
        }
        if (Date.now() - startTime > timeoutMs) {
          reject(new Error("Connection timeout"));
          return;
        }
        // Check again in 100ms
        setTimeout(checkConnection, 100);
      };

      checkConnection();
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

    // First request manifest to get accurate file info
    const manifestRequest = {
      type: "ManifestRequest",
      file_hash: metadata.fileHash,
    };

    try {
      console.log("Requesting manifest for file:", metadata.fileHash);
      transfer.webrtcSession.send(JSON.stringify(manifestRequest));

      // Store metadata for when manifest response arrives
      (transfer as any).pendingMetadata = metadata;

      // Manifest response will trigger chunk downloading via handleIncomingChunk
    } catch (error) {
      console.error("Failed to send manifest request:", error);
      // Fallback to direct file request
      this.sendFileRequestAndStartDownload(transfer, metadata);
    }
  }

  private sendFileRequestAndStartDownload(
    transfer: P2PTransfer,
    metadata: FileMetadata
  ): void {
    if (!transfer.webrtcSession) return;

    // Send file request through the WebRTC data channel
    const fileRequest = {
      type: "file_request",
      fileHash: metadata.fileHash,
      fileName: metadata.fileName,
      fileSize: metadata.fileSize,
      requesterPeerId: "local_peer",
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

  /**
   * Handle manifest response from seeder - use to initialize streaming download
   */
  private async handleManifestResponse(
    transfer: P2PTransfer,
    message: any
  ): Promise<void> {
    try {
      const manifest = JSON.parse(message.manifest_json);
      const totalChunks = manifest.chunks?.length || 0;
      const pendingMetadata = (transfer as any).pendingMetadata;

      if (!pendingMetadata) {
        console.warn("No pending metadata for manifest response");
        return;
      }

      // Update transfer with accurate info from manifest
      transfer.totalChunks = totalChunks;

      // Calculate actual file size from chunks
      let actualFileSize = 0;
      for (const chunk of manifest.chunks || []) {
        actualFileSize += chunk.size || P2PFileTransferService.CHUNK_SIZE;
      }
      if (actualFileSize > 0) {
        transfer.fileSize = actualFileSize;
      }

      console.log(`Manifest received: ${totalChunks} chunks, ${transfer.fileSize} bytes`);

      // Initialize streaming session if file is large enough and output path is set
      if (transfer.outputPath && transfer.fileSize > P2PFileTransferService.STREAMING_THRESHOLD) {
        try {
          const sessionId = await invoke<string>("init_streaming_download", {
            fileHash: transfer.fileHash,
            fileName: transfer.fileName,
            fileSize: transfer.fileSize,
            outputPath: transfer.outputPath,
            totalChunks: totalChunks,
            chunkSize: P2PFileTransferService.CHUNK_SIZE,
          });
          transfer.streamingSessionId = sessionId;
          console.log(`Initialized streaming download session: ${sessionId}`);
        } catch (error) {
          console.error("Failed to init streaming download:", error);
          // Fall back to in-memory mode
          transfer.receivedChunks = new Map();
        }
      } else {
        // Use in-memory for small files
        transfer.receivedChunks = new Map();
      }

      // Clear pending metadata
      delete (transfer as any).pendingMetadata;

      // Now send file request to start receiving chunks
      this.sendFileRequestAndStartDownload(transfer, pendingMetadata);
    } catch (error) {
      console.error("Failed to parse manifest response:", error);
      // Fallback to direct file request
      const pendingMetadata = (transfer as any).pendingMetadata;
      if (pendingMetadata) {
        delete (transfer as any).pendingMetadata;
        this.sendFileRequestAndStartDownload(transfer, pendingMetadata);
      }
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
    const requestMoreChunks = () => {
      // Stop if transfer is not active anymore
      if (
        transfer.status !== "transferring" &&
        transfer.status !== "connecting"
      ) {
        return;
      }

      const receivedCount = transfer.receivedChunks?.size || 0;
      const requestedCount = this.getRequestedChunkCount(transfer);

      // Calculate how many more chunks we can request
      const availableSlots = parallelRequests - requestedCount;

      if (availableSlots > 0 && receivedCount + requestedCount < totalChunks) {
        // Request multiple chunks in parallel if slots are available
        const startIndex = receivedCount + requestedCount;
        const endIndex = Math.min(startIndex + availableSlots, totalChunks);

        for (let i = startIndex; i < endIndex; i++) {
          this.requestChunk(transfer, i);
        }
      }

      // Schedule next check if transfer is still active and not complete
      if (
        transfer.status === "transferring" ||
        transfer.status === "connecting"
      ) {
        // Use a longer interval since we now track requests properly
        setTimeout(requestMoreChunks, 200);
      }
    };

    // Start the chunk requesting process
    setTimeout(requestMoreChunks, 100);
  }

  private requestChunk(transfer: P2PTransfer, chunkIndex: number): void {
    if (!transfer.webrtcSession) return;

    // Don't request chunks that are already requested or received
    if (
      transfer.requestedChunks?.has(chunkIndex) ||
      transfer.receivedChunks?.has(chunkIndex)
    ) {
      return;
    }

    try {
      const chunkRequest = {
        type: "chunk_request",
        fileHash: transfer.fileHash,
        chunkIndex: chunkIndex,
      };

      // Track that we've requested this chunk
      transfer.requestedChunks?.add(chunkIndex);

      transfer.webrtcSession.send(JSON.stringify(chunkRequest));
    } catch (error) {
      console.error(`Failed to request chunk ${chunkIndex}:`, error);
      // Remove from requested if send failed
      transfer.requestedChunks?.delete(chunkIndex);
    }
  }

  private getRequestedChunkCount(transfer: P2PTransfer): number {
    // Return the actual count of chunks that have been requested but not yet received
    if (!transfer.requestedChunks || !transfer.receivedChunks) {
      return 0;
    }

    // Count requested chunks that haven't been received yet
    let count = 0;
    for (const chunkIndex of transfer.requestedChunks) {
      if (!transfer.receivedChunks.has(chunkIndex)) {
        count++;
      }
    }

    return count;
  }

  private async handleIncomingChunk(
    transfer: P2PTransfer,
    data: any
  ): Promise<void> {
    try {
      const message = typeof data === "string" ? JSON.parse(data) : data;

      // Handle manifest response
      if (message.type === "ManifestResponse") {
        console.log("Received manifest response for file:", message.file_hash);
        await this.handleManifestResponse(transfer, message);
        return;
      }

      if (message.type === "file_chunk") {
        // Validate chunk data
        if (!(await this.validateChunk(message))) {
          console.warn("Received corrupted chunk:", message.chunk_index);
          transfer.corruptedChunks?.add(message.chunk_index);

          // Request chunk again if we have a connection
          if (transfer.webrtcSession) {
            this.requestChunkAgain(transfer, message.chunk_index);
          }
          return;
        }

        const chunkData = new Uint8Array(message.data);
        const chunkIndex = message.chunk_index;

        // Check if using streaming mode (large files)
        if (transfer.streamingSessionId) {
          // Write chunk directly to disk via Tauri
          try {
            const isComplete = await invoke<boolean>("write_download_chunk", {
              sessionId: transfer.streamingSessionId,
              chunkIndex: chunkIndex,
              chunkData: Array.from(chunkData), // Convert to array for Tauri
            });

            transfer.receivedChunkIndices?.add(chunkIndex);

            // Update progress
            transfer.bytesTransferred += chunkData.length;
            const progress = (transfer.bytesTransferred / transfer.fileSize) * 100;
            transfer.progress = Math.min(100, progress);

            // Calculate speed
            const elapsed = (Date.now() - transfer.startTime) / 1000;
            transfer.speed = transfer.bytesTransferred / elapsed;

            // Save checkpoint periodically (every 50 chunks)
            const receivedCount = transfer.receivedChunkIndices?.size || 0;
            if (receivedCount > 0 && receivedCount % 50 === 0) {
              this.saveCheckpoint(transfer);
            }

            // Check if transfer is complete
            if (isComplete) {
              await this.finalizeStreamingDownload(transfer);
            }
          } catch (error) {
            console.error("Failed to write chunk to disk:", error);
            transfer.corruptedChunks?.add(chunkIndex);
            // Re-request the chunk
            if (transfer.webrtcSession) {
              this.requestChunkAgain(transfer, chunkIndex);
            }
            return;
          }
        } else {
          // In-memory mode for small files
          if (!transfer.receivedChunks) {
            transfer.receivedChunks = new Map();
          }

          transfer.receivedChunks.set(chunkIndex, chunkData);
          transfer.receivedChunkIndices?.add(chunkIndex);

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

            // Update reputation for successful transfer
            const duration = Date.now() - transfer.startTime;
            const seederPeerId = transfer.seeders[transfer.currentSeederIndex || 0];
            if (seederPeerId) {
              try {
                await invoke('record_transfer_success', {
                  peerId: seederPeerId,
                  bytes: transfer.fileSize,
                  durationMs: duration,
                });
                PeerSelectionService.notePeerSuccess(seederPeerId, duration);
                console.log(`✅ Updated reputation for peer ${seederPeerId.substring(0, 20)}... after P2P transfer (+${transfer.fileSize} bytes)`);
              } catch (repError) {
                console.error('Failed to update peer reputation:', repError);
              }
            }

            // Save file if output path is specified
            if (transfer.outputPath) {
              this.saveCompletedFile(transfer);
            }
          }
        }

        // Remove from requested chunks since it's now received
        transfer.requestedChunks?.delete(chunkIndex);

        // Remove from corrupted chunks if it was previously corrupted
        transfer.corruptedChunks?.delete(chunkIndex);

        // Send ACK to seeder for flow control
        this.sendChunkAck(transfer, chunkIndex);

        this.notifyProgress(transfer);
      } else if (message.type === "dht_message") {
        // Handle DHT signaling messages
        this.handleDhtMessage(message);
      }
    } catch (error) {
      console.error("Error handling incoming chunk:", error);
    }
  }

  private async finalizeStreamingDownload(transfer: P2PTransfer): Promise<void> {
    if (!transfer.streamingSessionId) return;

    try {
      const outputPath = await invoke<string>("finalize_streaming_download", {
        sessionId: transfer.streamingSessionId,
      });

      transfer.status = "completed";
      transfer.outputPath = outputPath;
      console.log(`Streaming download completed: ${outputPath}`);

      // Update reputation for successful streaming transfer
      const duration = Date.now() - transfer.startTime;
      const seederPeerId = transfer.seeders[transfer.currentSeederIndex || 0];
      if (seederPeerId) {
        try {
          await invoke('record_transfer_success', {
            peerId: seederPeerId,
            bytes: transfer.fileSize,
            durationMs: duration,
          });
          PeerSelectionService.notePeerSuccess(seederPeerId, duration);
          console.log(`✅ Updated reputation for peer ${seederPeerId.substring(0, 20)}... after streaming transfer (+${transfer.fileSize} bytes)`);
        } catch (repError) {
          console.error('Failed to update peer reputation:', repError);
        }
      }
    } catch (error) {
      console.error("Failed to finalize streaming download:", error);
      transfer.status = "failed";
      transfer.error = `Failed to finalize download: ${error}`;

      // Update reputation for failed transfer
      const seederPeerId = transfer.seeders[transfer.currentSeederIndex || 0];
      if (seederPeerId) {
        try {
          await invoke('record_transfer_failure', {
            peerId: seederPeerId,
            error: `Streaming finalization failed: ${error}`,
          });
          PeerSelectionService.notePeerFailure(seederPeerId);
          console.log(`❌ Updated reputation for peer ${seederPeerId.substring(0, 20)}... after streaming failure`);
        } catch (repError) {
          console.error('Failed to update peer reputation:', repError);
        }
      }
    }

    transfer.streamingSessionId = undefined;
    this.notifyProgress(transfer);
  }

  /**
   * Save checkpoint for resume support
   */
  private saveCheckpoint(transfer: P2PTransfer): void {
    if (!transfer.streamingSessionId) return;

    invoke("save_download_checkpoint", {
      sessionId: transfer.streamingSessionId,
    }).catch((err) => console.warn("Failed to save checkpoint:", err));
  }

  /**
   * Send ACK message to seeder for flow control
   */
  private sendChunkAck(transfer: P2PTransfer, chunkIndex: number): void {
    if (!transfer.webrtcSession) return;

    try {
      const ackMessage = {
        type: "ChunkAck",
        file_hash: transfer.fileHash,
        chunk_index: chunkIndex,
        ready_for_more: true,
      };

      transfer.webrtcSession.send(JSON.stringify(ackMessage));
    } catch (error) {
      // ACK sending failure is non-critical, just log it
      console.warn(`Failed to send ACK for chunk ${chunkIndex}:`, error);
    }
  }

  private async validateChunk(chunkMessage: any): Promise<boolean> {
    // Basic validation - check if chunk data exists and chunk index is valid
    if (!chunkMessage.data || typeof chunkMessage.chunk_index !== "number") {
      return false;
    }

    // Verify checksum if provided
    if (chunkMessage.checksum) {
      try {
        const chunkData = new Uint8Array(chunkMessage.data);
        const calculatedChecksum = await this.calculateSHA256(chunkData);

        if (calculatedChecksum !== chunkMessage.checksum) {
          console.warn(
            `Chunk checksum mismatch for chunk ${chunkMessage.chunk_index}. Expected: ${chunkMessage.checksum}, Got: ${calculatedChecksum}`
          );
          return false;
        }
      } catch (error) {
        console.error("Failed to verify chunk checksum:", error);
        return false;
      }
    }

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

      // Add back to requested chunks since we're re-requesting it
      transfer.requestedChunks?.add(chunkIndex);

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

  /**
   * Manages the download of all encrypted chunks for a given file manifest.
   * It downloads chunks in parallel and reports overall progress.
   */
  async downloadEncryptedChunks(
    manifest: FileManifestForJs,
    seederAddresses: string[],
    onProgress: (progress: {
      percentage: number;
      speed: string;
      eta: string;
    }) => void
  ): Promise<void> {
    const totalChunks = manifest.chunks.length;
    let downloadedChunks = 0;
    const totalSize = manifest.chunks.reduce(
      (sum, chunk) => sum + chunk.encryptedSize,
      0
    );
    let bytesDownloaded = 0;
    const startTime = Date.now();

    // Create a download promise for each encrypted chunk. This allows for parallel downloads.
    const downloadPromises = manifest.chunks.map((chunkInfo) => {
      // Use the helper function to download each individual chunk.
      return this.initiateChunkDownload(
        chunkInfo.encryptedHash,
        seederAddresses
      ).then(() => {
        // This code runs every time a single chunk download completes successfully.
        downloadedChunks++;
        bytesDownloaded += chunkInfo.encryptedSize;
        const percentage = (downloadedChunks / totalChunks) * 100;

        // Calculate speed and ETA for the UI
        const elapsedTime = (Date.now() - startTime) / 1000; // in seconds
        const speedBps = elapsedTime > 0 ? bytesDownloaded / elapsedTime : 0;
        const remainingBytes = totalSize - bytesDownloaded;
        const etaSeconds =
          speedBps > 0 ? Math.round(remainingBytes / speedBps) : 0;

        // Update the UI via the progress callback
        onProgress({
          percentage,
          speed: `${Math.round(speedBps / 1024)} KB/s`,
          eta: `${etaSeconds}s`,
        });
      });
    });

    // Promise.all waits for every single chunk download to complete before continuing.
    await Promise.all(downloadPromises);
  }

  /**
   * Helper function to download a single encrypted chunk from the network.
   * It tries to connect to one of the available seeders and request the chunk.
   */
  async initiateChunkDownload(
    chunkHash: string,
    seeders: string[]
  ): Promise<void> {
    if (seeders.length === 0) {
      throw new Error(`No seeders available to download chunk ${chunkHash}`);
    }

    // A more advanced implementation could try multiple seeders if one fails.
    const seederId = seeders[0];

    try {
      await invoke("request_file_chunk", {
        fileHash: chunkHash,
        peerId: seederId,
      });
      console.log(`Successfully received and stored chunk: ${chunkHash}`);
    } catch (error) {
      console.error(
        `Failed to request chunk ${chunkHash} from peer ${seederId}:`,
        error
      );
      throw error;
    }
  }

  private async handleDhtMessage(message: any): Promise<void> {
    // Handle WebRTC signaling messages received through DHT
    if (message.message?.type === "webrtc_signaling") {
      const signalingData = message.message;
      const fromPeer = message.from;

      try {
        switch (signalingData.signalingType) {
          case "offer":
            await this.handleIncomingOffer(fromPeer, signalingData);
            break;
          case "answer":
            await this.handleIncomingAnswer(fromPeer, signalingData);
            break;
          case "candidate":
            await this.handleIncomingCandidate(fromPeer, signalingData);
            break;
          default:
            console.warn(
              "Unknown WebRTC signaling type:",
              signalingData.signalingType
            );
        }
      } catch (error) {
        console.error("Error handling WebRTC signaling message:", error);
      }
    }
  }

  private async handleIncomingOffer(
    fromPeer: string,
    signalingData: any
  ): Promise<void> {
    console.log("Received WebRTC offer from peer:", fromPeer);

    // Create WebRTC session for incoming connection
    const webrtcSession = createWebRTCSession({
      isInitiator: false,
      peerId: fromPeer,
      onMessage: (data) => {
        this.handleIncomingChunkFromSession(fromPeer, data);
      },
      onConnectionStateChange: (state) => {
        console.log(`WebRTC connection state for ${fromPeer}: ${state}`);
      },
      onDataChannelOpen: () => {
        console.log(`Data channel opened for peer: ${fromPeer}`);
      },
      onError: (error) => {
        console.error(`WebRTC error for peer ${fromPeer}:`, error);
      },
    });

    this.webrtcSessions.set(fromPeer, webrtcSession);

    // Accept the offer and create answer
    try {
      const answer = await webrtcSession.acceptOfferCreateAnswer(
        signalingData.sdp
      );

      // Send answer back through backend WebRTC coordination
      await invoke("send_webrtc_answer", {
        peerId: fromPeer,
        answer: JSON.stringify(answer),
      });

      console.log("Sent WebRTC answer to peer:", fromPeer);
    } catch (error) {
      console.error("Failed to handle WebRTC offer:", error);
      webrtcSession.close();
      this.webrtcSessions.delete(fromPeer);
    }
  }

  private async handleIncomingAnswer(
    fromPeer: string,
    signalingData: any
  ): Promise<void> {
    const webrtcSession = this.webrtcSessions.get(fromPeer);
    if (!webrtcSession) {
      console.warn("Received answer for unknown WebRTC session:", fromPeer);
      return;
    }

    try {
      await webrtcSession.acceptAnswer(signalingData.sdp);
      console.log("Accepted WebRTC answer from peer:", fromPeer);
    } catch (error) {
      console.error("Failed to accept WebRTC answer:", error);
      webrtcSession.close();
      this.webrtcSessions.delete(fromPeer);
    }
  }

  private handleIncomingCandidate(fromPeer: string, _signalingData: any): void {
    const webrtcSession = this.webrtcSessions.get(fromPeer);
    if (!webrtcSession) {
      console.warn(
        "Received ICE candidate for unknown WebRTC session:",
        fromPeer
      );
      return;
    }

    try {
      // ICE candidates are handled automatically by the WebRTC session
      console.log("Processing ICE candidate for peer:", fromPeer);
    } catch (error) {
      console.error("Failed to process ICE candidate:", error);
    }
  }

  private handleIncomingChunkFromSession(peerId: string, data: any): void {
    // Find the transfer associated with this peer
    for (const transfer of this.transfers.values()) {
      if (transfer.webrtcSession?.peerId === peerId) {
        this.handleIncomingChunk(transfer, data);
        break;
      }
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

      // Close WebRTC session if it exists and clean up from tracking
      if (transfer.webrtcSession) {
        transfer.webrtcSession.close();
        if (transfer.webrtcSession.peerId) {
          this.webrtcSessions.delete(transfer.webrtcSession.peerId);
        }
      }

      // Cancel streaming download session if active
      if (transfer.streamingSessionId) {
        invoke("cancel_streaming_download", {
          sessionId: transfer.streamingSessionId,
        }).catch((err) => console.error("Failed to cancel streaming download:", err));
        transfer.streamingSessionId = undefined;
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

  /**
   * Calculates SHA-256 hash of the provided data
   * @param data The data to hash
   * @returns Promise that resolves to the hex-encoded hash
   */
  private async calculateSHA256(data: Uint8Array): Promise<string> {
    // Use the Web Crypto API to calculate SHA-256
    // Convert to ArrayBuffer to ensure compatibility with crypto.subtle.digest
    const hashBuffer = await crypto.subtle.digest(
      "SHA-256",
      data.slice().buffer
    );
    const hashArray = Array.from(new Uint8Array(hashBuffer));

    // Convert to hex string
    return hashArray.map((b) => b.toString(16).padStart(2, "0")).join("");
  }
}

// Singleton instance
export const p2pFileTransferService = new P2PFileTransferService();