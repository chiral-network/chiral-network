import { invoke } from "@tauri-apps/api/core";
import { toHumanReadableSize } from "$lib/utils";

/**
 * Peer metrics interface matching the Rust struct
 */
export interface PeerMetrics {
  peer_id: string;
  address: string;
  latency_ms?: number;
  bandwidth_kbps?: number;
  reliability_score: number;
  uptime_score: number;
  success_rate: number;
  last_seen: number;
  transfer_count: number;
  successful_transfers: number;
  failed_transfers: number;
  total_bytes_transferred: number;
  encryption_support: boolean;
}

/**
 * Peer selection strategies
 */
export type PeerSelectionStrategy =
  | "fastest"
  | "reliable"
  | "bandwidth"
  | "balanced"
  | "encryption"
  | "load_balanced";

/**
 * Smart peer selection service for optimal file transfers
 */
export class PeerSelectionService {
  /**
   * Get recommended peers for downloading a specific file
   */
  static async getRecommendedPeersForFile(
    fileHash: string,
    fileSize: number,
    requireEncryption: boolean = false
  ): Promise<string[]> {
    try {
      const peers = await invoke<string[]>("get_recommended_peers_for_file", {
        fileHash,
        fileSize,
        requireEncryption,
      });
      return peers || [];
    } catch (error) {
      console.error("Failed to get recommended peers:", error);
      return [];
    }
  }

  /**
   * Record a successful file transfer for peer metrics
   */
  static async recordTransferSuccess(
    peerId: string,
    bytes: number,
    durationMs: number
  ): Promise<void> {
    try {
      await invoke("record_transfer_success", {
        peerId,
        bytes,
        durationMs,
      });
    } catch (error) {
      console.error("Failed to record transfer success:", error);
    }
  }

  /**
   * Record a failed file transfer for peer metrics
   */
  static async recordTransferFailure(
    peerId: string,
    error: string
  ): Promise<void> {
    try {
      await invoke("record_transfer_failure", {
        peerId,
        error,
      });
    } catch (error) {
      console.error("Failed to record transfer failure:", error);
    }
  }

  /**
   * Get all peer metrics for monitoring
   */
  static async getPeerMetrics(): Promise<PeerMetrics[]> {
    try {
      const metrics = await invoke<PeerMetrics[]>("get_peer_metrics");
      return metrics || [];
    } catch (error) {
      console.error("Failed to get peer metrics:", error);
      return [];
    }
  }

  /**
   * Select peers using a specific strategy
   */
  static async selectPeersWithStrategy(
    availablePeers: string[],
    count: number,
    strategy: PeerSelectionStrategy,
    requireEncryption: boolean = false
  ): Promise<string[]> {
    try {
      const peers = await invoke<string[]>("select_peers_with_strategy", {
        availablePeers,
        count,
        strategy,
        requireEncryption,
      });
      return peers || [];
    } catch (error) {
      console.error("Failed to select peers with strategy:", error);
      return [];
    }
  }

  /**
   * Set encryption support capability for a peer
   */
  static async setPeerEncryptionSupport(
    peerId: string,
    supported: boolean
  ): Promise<void> {
    try {
      await invoke("set_peer_encryption_support", {
        peerId,
        supported,
      });
    } catch (error) {
      console.error("Failed to set peer encryption support:", error);
    }
  }

  /**
   * Clean up inactive peer metrics
   */
  static async cleanupInactivePeers(
    maxAgeSeconds: number = 3600 // 1 hour default
  ): Promise<void> {
    try {
      await invoke("cleanup_inactive_peers", {
        maxAgeSeconds,
      });
    } catch (error) {
      console.error("Failed to cleanup inactive peers:", error);
    }
  }

  /**
   * Get the best peer for a specific use case
   */
  static async getBestPeerForDownload(
    availablePeers: string[],
    fileSize: number,
    preferEncryption: boolean = false
  ): Promise<string | null> {
    if (availablePeers.length === 0) {
      return null;
    }

    // For small files, prioritize low latency
    // For large files, prioritize high bandwidth
    const strategy: PeerSelectionStrategy =
      fileSize > 100 * 1024 * 1024
        ? "bandwidth" // >100MB use bandwidth
        : "fastest"; // <100MB use fastest

    const selectedPeers = await this.selectPeersWithStrategy(
      availablePeers,
      1,
      preferEncryption ? "encryption" : strategy,
      preferEncryption
    );

    return selectedPeers.length > 0 ? selectedPeers[0] : null;
  }

  /**
   * Get multiple peers for parallel downloading
   */
  static async getPeersForParallelDownload(
    availablePeers: string[],
    fileSize: number,
    maxPeers: number = 5,
    requireEncryption: boolean = false
  ): Promise<string[]> {
    if (availablePeers.length === 0) {
      return [];
    }

    // For very large files, use load balancing to distribute across peers
    const strategy: PeerSelectionStrategy =
      fileSize > 500 * 1024 * 1024
        ? "load_balanced" // >500MB use load balancing
        : "balanced"; // <500MB use balanced selection

    const peerCount = Math.min(maxPeers, availablePeers.length);

    return await this.selectPeersWithStrategy(
      availablePeers,
      peerCount,
      strategy,
      requireEncryption
    );
  }

  /**
   * Format peer metrics for display
   */
  static formatPeerMetrics(metrics: PeerMetrics): Record<string, string> {
    return {
      "Peer ID": metrics.peer_id.slice(0, 12) + "...",
      Address: metrics.address,
      Latency: metrics.latency_ms ? `${metrics.latency_ms}ms` : "Unknown",
      Bandwidth: metrics.bandwidth_kbps
        ? `${Math.round(metrics.bandwidth_kbps / 1024)} MB/s`
        : "Unknown",
      Reliability: `${Math.round(metrics.reliability_score * 100)}%`,
      "Success Rate": `${Math.round(metrics.success_rate * 100)}%`,
      Transfers: `${metrics.successful_transfers}/${metrics.transfer_count}`,
      "Data Transferred": this.formatBytes(metrics.total_bytes_transferred),
      Encryption: metrics.encryption_support ? "Yes" : "No",
      "Last Seen": new Date(metrics.last_seen * 1000).toLocaleString(),
    };
  }

  /**
   * Format bytes to human readable format
   */
  static formatBytes(bytes: number): string {
    if (!Number.isFinite(bytes) || bytes <= 0) {
      return "0 Bytes";
    }

    const formatted = toHumanReadableSize(bytes, 2);
    return formatted.endsWith(" B")
      ? formatted.replace(" B", " Bytes")
      : formatted;
  }

  /**
   * Get peer health score (0-100)
   */
  static getPeerHealthScore(metrics: PeerMetrics): number {
    const reliabilityWeight = 0.4;
    const latencyWeight = 0.3;
    const successRateWeight = 0.3;

    const latencyScore = metrics.latency_ms
      ? Math.max(0, 1 - metrics.latency_ms / 1000) // Normalize to 0-1, 1000ms = 0 score
      : 0.5;

    return Math.round(
      (reliabilityWeight * metrics.reliability_score +
        latencyWeight * latencyScore +
        successRateWeight * metrics.success_rate) *
        100
    );
  }

  /**
   * Auto-cleanup inactive peers periodically
   */
  static startPeriodicCleanup(intervalMinutes: number = 60): () => void {
    const intervalMs = intervalMinutes * 60 * 1000;
    const maxAgeSeconds = intervalMinutes * 60 * 2; // Clean peers older than 2x interval

    const intervalId = setInterval(() => {
      this.cleanupInactivePeers(maxAgeSeconds);
    }, intervalMs);

    // Return cleanup function
    return () => clearInterval(intervalId);
  }
}

// Export for use in other parts of the application
export default PeerSelectionService;
