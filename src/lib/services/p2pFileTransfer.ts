import { invoke } from "@tauri-apps/api/core";
import { createWebRTCSession } from "./webrtcService";
import { SignalingService } from "./signalingService";
import type { FileMetadata } from "../dht";
import type { FileManifestForJs } from "./encryption";

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
  requestedChunks?: Set<number>;
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
  private webrtcSessions = new Map<string, any>(); // peerId -> WebRTCSession
  private signalingService: SignalingService;

  constructor() {
    // Initialize signaling service for WebRTC coordination
    this.signalingService = new SignalingService();
  }

  async getFileMetadata(fileHash: string): Promise<any> {
    // Use the file hash to retrieve metadata from DHT
    return await invoke("get_file_metadata", { fileHash });
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
    transfer.requestedChunks = new Set();

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
                `WebRTC connection ${state}`
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

          // Use backend coordination with enhanced DHT signaling support
          await Promise.race([
            invoke("establish_webrtc_connection", {
              peerId: seederId,
              offer: JSON.stringify(offer),
              useDhtSignaling: true, // Enable DHT signaling as fallback
            }),
            this.createTimeoutPromise(
              15000,
              "WebRTC connection establishment timeout"
            ),
          ]);

          console.log("WebRTC connection established with peer:", seederId);
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

      if (message.type === "file_chunk") {
        // Handle incoming file chunk
        // Initialize chunks map if not exists
        if (!transfer.receivedChunks) {
          transfer.receivedChunks = new Map();
        }

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

        // Store the chunk data
        const chunkData = new Uint8Array(message.data);
        transfer.receivedChunks.set(message.chunk_index, chunkData);

        // Remove from requested chunks since it's now received
        transfer.requestedChunks?.delete(message.chunk_index);

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
