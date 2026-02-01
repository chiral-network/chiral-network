import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import {
  transferStore,
  subscribeToTransferEvents,
} from "$lib/stores/transferEventsStore";
import type { Protocol } from "./contentProtocols";

// ============================================================================
// Types
// ============================================================================

export type TransferProtocol =
  | "WebRTC"
  | "BitTorrent"
  | "FTP"
  | "HTTP"
  | "ED2K";

export interface StartDownloadOptions {
  fileHash: string;
  fileName: string;
  fileSize: number;
  protocol: TransferProtocol;
  outputPath: string;
  sources?: DownloadSourceInfo[];
}

export interface DownloadSourceInfo {
  type: Protocol;
  address: string;
  peerId?: string;
  url?: string;
}

export interface StartUploadOptions {
  filePath: string;
  protocol: TransferProtocol;
  pricePerMb?: number;
  ftpConfig?: FtpUploadConfig;
}

export interface FtpUploadConfig {
  url: string;
  username?: string;
  password?: string;
  useTls?: boolean;
}

// Protocol-specific event payloads (for normalization)
interface TorrentEventPayload {
  type: "Progress" | "Complete" | "Added" | "Removed";
  info_hash?: string;
  name?: string;
  progress?: number;
  download_speed?: number;
  upload_speed?: number;
  downloaded?: number;
  total_size?: number;
  peers?: number;
}

// NOTE: WebRTC payload types have been removed.
// WebRTC events now use the unified TransferEventPayload types via AppEventBus.

interface MultiSourceProgressPayload {
  fileHash: string;
  fileName?: string;
  downloadedBytes: number;
  totalBytes: number;
  speedBps: number;
  activeSources: number;
  etaSeconds?: number;
}

interface MultiSourceCompletedPayload {
  fileHash: string;
  fileName: string;
  outputPath: string;
  fileSize: number;
  durationSeconds?: number;
}

interface MultiSourceStartedPayload {
  fileHash: string;
  fileName: string;
  fileSize: number;
  totalChunks: number;
  availableSources: number;
}

interface MultiSourceFailedPayload {
  fileHash: string;
  error: string;
  downloadedBytes?: number;
}

// ============================================================================
// Transfer Service Class
// ============================================================================

class TransferService {
  private initialized = false;
  private unlistenFunctions: UnlistenFn[] = [];

  // Track transfers that come through protocol-specific events
  // (not through the unified transfer:event channel)
  private protocolTransfers = new Map<
    string,
    {
      fileName: string;
      fileSize: number;
      protocol: TransferProtocol;
      startedAt: number;
    }
  >();

  /**
   * Initialize the transfer service.
   * This subscribes to ALL transfer-related events (unified + protocol-specific).
   * Should be called once when the app starts (e.g., in App.svelte onMount).
   */
  async initialize(): Promise<void> {
    if (this.initialized) {
      console.warn("TransferService already initialized");
      return;
    }

    try {
      // 1. Subscribe to unified transfer:event (handles FTP and future unified protocols)
      await subscribeToTransferEvents();

      // 2. Subscribe to protocol-specific events and normalize them
      await this.subscribeToProtocolEvents();

      this.initialized = true;
      console.log("TransferService initialized successfully");
    } catch (error) {
      console.error("Failed to initialize TransferService:", error);
      throw error;
    }
  }

  /**
   * Subscribe to all protocol-specific events that bypass the unified system.
   * Note: WebRTC events are now handled via the unified transfer:* channels
   * and don't need separate listeners here.
   */
  private async subscribeToProtocolEvents(): Promise<void> {
    // BitTorrent events
    const unlistenTorrent = await listen<TorrentEventPayload>(
      "torrent_event",
      (event) => {
        this.handleTorrentEvent(event.payload);
      },
    );
    this.unlistenFunctions.push(unlistenTorrent);

    // NOTE: WebRTC events (webrtc_download_progress, webrtc_download_complete)
    // are now emitted via the unified transfer:* channels from AppEventBus,
    // so they're handled by subscribeToTransferEvents() instead.

    // Multi-source download events
    const unlistenMSStarted = await listen<MultiSourceStartedPayload>(
      "multi_source_download_started",
      (event) => {
        this.handleMultiSourceStarted(event.payload);
      },
    );
    this.unlistenFunctions.push(unlistenMSStarted);

    const unlistenMSProgress = await listen<MultiSourceProgressPayload>(
      "multi_source_progress_update",
      (event) => {
        this.handleMultiSourceProgress(event.payload);
      },
    );
    this.unlistenFunctions.push(unlistenMSProgress);

    const unlistenMSCompleted = await listen<MultiSourceCompletedPayload>(
      "multi_source_download_completed",
      (event) => {
        this.handleMultiSourceCompleted(event.payload);
      },
    );
    this.unlistenFunctions.push(unlistenMSCompleted);

    const unlistenMSFailed = await listen<MultiSourceFailedPayload>(
      "multi_source_download_failed",
      (event) => {
        this.handleMultiSourceFailed(event.payload);
      },
    );
    this.unlistenFunctions.push(unlistenMSFailed);

    // HTTP download progress
    const unlistenHTTP = await listen<{
      fileHash: string;
      downloadedBytes: number;
      totalBytes: number;
      speedBps: number;
    }>("http_download_progress", (event) => {
      this.handleHTTPProgress(event.payload);
    });
    this.unlistenFunctions.push(unlistenHTTP);
  }

  // ============================================================================
  // Protocol Event Handlers (Normalization Layer)
  // ============================================================================

  private handleTorrentEvent(payload: TorrentEventPayload): void {
    if (!payload.info_hash) return;

    const transferId = `torrent-${payload.info_hash}`;

    switch (payload.type) {
      case "Added":
        // Torrent added - create queued transfer
        transferStore.handleEvent({
          type: "queued",
          transferId,
          fileHash: payload.info_hash,
          fileName: payload.name || "Unknown Torrent",
          fileSize: payload.total_size || 0,
          outputPath: "",
          priority: "normal",
          queuedAt: Date.now(),
        });
        this.protocolTransfers.set(transferId, {
          fileName: payload.name || "Unknown Torrent",
          fileSize: payload.total_size || 0,
          protocol: "BitTorrent",
          startedAt: Date.now(),
        });
        break;

      case "Progress":
        transferStore.handleEvent({
          type: "progress",
          transferId,
          downloadedBytes: payload.downloaded || 0,
          completedChunks: 0,
          progressPercentage: (payload.progress || 0) * 100,
          downloadSpeedBps: payload.download_speed || 0,
          uploadSpeedBps: payload.upload_speed || 0,
          activeSources: payload.peers || 0,
        });
        break;

      case "Complete":
        transferStore.handleEvent({
          type: "completed",
          transferId,
          completedAt: Date.now(),
          fileSize: payload.total_size || 0,
          durationSeconds: this.calculateDuration(transferId),
        });
        this.protocolTransfers.delete(transferId);
        break;

      case "Removed":
        transferStore.handleEvent({
          type: "canceled",
          transferId,
          canceledAt: Date.now(),
          downloadedBytes: payload.downloaded || 0,
        });
        this.protocolTransfers.delete(transferId);
        break;
    }
  }

  // NOTE: handleWebRTCProgress and handleWebRTCComplete have been removed.
  // WebRTC events are now emitted via the unified transfer:* channels from AppEventBus
  // and handled automatically by the transferEventsStore.

  private handleMultiSourceStarted(payload: MultiSourceStartedPayload): void {
    const transferId = `multi-${payload.fileHash}`;

    transferStore.handleEvent({
      type: "started",
      transferId,
      fileHash: payload.fileHash,
      fileName: payload.fileName,
      fileSize: payload.fileSize,
      totalChunks: payload.totalChunks,
      startedAt: Date.now(),
      availableSources: [],
    });

    this.protocolTransfers.set(transferId, {
      fileName: payload.fileName,
      fileSize: payload.fileSize,
      protocol: "WebRTC", // Multi-source typically uses WebRTC/Bitswap
      startedAt: Date.now(),
    });
  }

  private handleMultiSourceProgress(payload: MultiSourceProgressPayload): void {
    const transferId = `multi-${payload.fileHash}`;

    const progress =
      payload.totalBytes > 0
        ? (payload.downloadedBytes / payload.totalBytes) * 100
        : 0;

    transferStore.handleEvent({
      type: "progress",
      transferId,
      downloadedBytes: payload.downloadedBytes,
      completedChunks: 0,
      progressPercentage: progress,
      downloadSpeedBps: payload.speedBps,
      uploadSpeedBps: 0,
      activeSources: payload.activeSources,
      etaSeconds: payload.etaSeconds,
    });
  }

  private handleMultiSourceCompleted(
    payload: MultiSourceCompletedPayload,
  ): void {
    const transferId = `multi-${payload.fileHash}`;

    transferStore.handleEvent({
      type: "completed",
      transferId,
      completedAt: Date.now(),
      fileSize: payload.fileSize,
      durationSeconds:
        payload.durationSeconds || this.calculateDuration(transferId),
    });
    this.protocolTransfers.delete(transferId);
  }

  private handleMultiSourceFailed(payload: MultiSourceFailedPayload): void {
    const transferId = `multi-${payload.fileHash}`;

    transferStore.handleEvent({
      type: "failed",
      transferId,
      failedAt: Date.now(),
      error: payload.error,
      errorCategory: "download_error",
      retryPossible: true,
      downloadedBytes: payload.downloadedBytes || 0,
    });
    this.protocolTransfers.delete(transferId);
  }

  private handleHTTPProgress(payload: {
    fileHash: string;
    downloadedBytes: number;
    totalBytes: number;
    speedBps: number;
  }): void {
    const transferId = `http-${payload.fileHash}`;

    // Ensure transfer exists
    if (!this.protocolTransfers.has(transferId)) {
      transferStore.handleEvent({
        type: "started",
        transferId,
        fileHash: payload.fileHash,
        fileName: "HTTP Download",
        fileSize: payload.totalBytes,
        totalChunks: 1,
        startedAt: Date.now(),
        availableSources: [],
      });
      this.protocolTransfers.set(transferId, {
        fileName: "HTTP Download",
        fileSize: payload.totalBytes,
        protocol: "HTTP",
        startedAt: Date.now(),
      });
    }

    const progress =
      payload.totalBytes > 0
        ? (payload.downloadedBytes / payload.totalBytes) * 100
        : 0;

    transferStore.handleEvent({
      type: "progress",
      transferId,
      downloadedBytes: payload.downloadedBytes,
      completedChunks: progress >= 100 ? 1 : 0,
      progressPercentage: progress,
      downloadSpeedBps: payload.speedBps,
      uploadSpeedBps: 0,
      activeSources: 1,
    });

    // Check for completion
    if (
      payload.downloadedBytes >= payload.totalBytes &&
      payload.totalBytes > 0
    ) {
      transferStore.handleEvent({
        type: "completed",
        transferId,
        completedAt: Date.now(),
        fileSize: payload.totalBytes,
        durationSeconds: this.calculateDuration(transferId),
      });
      this.protocolTransfers.delete(transferId);
    }
  }

  private calculateDuration(transferId: string): number {
    const transfer = this.protocolTransfers.get(transferId);
    if (!transfer) return 0;
    return Math.floor((Date.now() - transfer.startedAt) / 1000);
  }

  // ============================================================================
  // Public API
  // ============================================================================

  /**
   * Start a download using the specified protocol.
   * Returns a transfer ID that can be used for pause/resume/cancel.
   */
  async startDownload(opts: StartDownloadOptions): Promise<string> {
    // Delegate to backend - it will emit appropriate events
    const transferId = await invoke<string>("start_download", {
      fileHash: opts.fileHash,
      fileName: opts.fileName,
      fileSize: opts.fileSize,
      protocol: opts.protocol,
      outputPath: opts.outputPath,
      sources: opts.sources,
    });
    return transferId;
  }

  /**
   * Start an upload/seed using the specified protocol.
   * Returns a transfer ID.
   */
  async startUpload(opts: StartUploadOptions): Promise<string> {
    const transferId = await invoke<string>("start_upload", {
      filePath: opts.filePath,
      protocol: opts.protocol,
      pricePerMb: opts.pricePerMb,
      ftpConfig: opts.ftpConfig,
    });
    return transferId;
  }

  /**
   * Pause a transfer.
   */
  async pause(transferId: string): Promise<void> {
    await invoke("pause_transfer", { transferId });
  }

  /**
   * Resume a paused transfer.
   */
  async resume(transferId: string): Promise<void> {
    await invoke("resume_transfer", { transferId });
  }

  /**
   * Cancel a transfer.
   */
  async cancel(transferId: string, keepPartial = false): Promise<void> {
    await invoke("cancel_transfer", { transferId, keepPartial });
  }

  /**
   * Get the current status of a transfer.
   */
  getTransfer(transferId: string) {
    return transferStore.getTransfer(transferId);
  }

  /**
   * Remove a completed/failed transfer from the store.
   */
  dismissTransfer(transferId: string): void {
    transferStore.removeTransfer(transferId);
  }

  /**
   * Clear all finished transfers.
   */
  clearFinished(): void {
    transferStore.clearFinished();
  }

  /**
   * Clean up all event listeners.
   * Call this when the app is closing.
   */
  async cleanup(): Promise<void> {
    for (const unlisten of this.unlistenFunctions) {
      unlisten();
    }
    this.unlistenFunctions = [];
    this.protocolTransfers.clear();
    this.initialized = false;
  }

  /**
   * Check if the service is initialized.
   */
  isInitialized(): boolean {
    return this.initialized;
  }
}

// ============================================================================
// Singleton Export
// ============================================================================

export const transferService = new TransferService();

// Re-export store and derived stores for convenience
export {
  transferStore,
  activeTransfers,
  queuedTransfers,
  completedTransfers,
  failedTransfers,
  pausedTransfers,
  formatBytes,
  formatSpeed,
  formatETA,
  getStatusColor,
} from "$lib/stores/transferEventsStore";
