import { invoke } from "@tauri-apps/api/core";
import { toHumanReadableSize } from "$lib/utils";
import {ReputationStore} from "$lib/reputationStore";
import { get } from 'svelte/store';
import { blacklist } from '$lib/stores';
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
  protocols: string[];
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
    // Get blacklist from store
    const blacklistedAddresses = get(blacklist).map(entry => entry.chiral_address);
    
    //  Pass blacklist to backend
    const peers = await invoke<string[]>("select_peers_with_strategy", {
      availablePeers,
      count,
      strategy,
      requireEncryption,
      blacklistedPeers: blacklistedAddresses 
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
   * Filter peers to ensure they support at least one of the required protocols.
   */
  static filterPeersByProtocol(
    peers: PeerMetrics[],
    requiredProtocols: string[]
  ): PeerMetrics[] {
    if (requiredProtocols.length === 0) {
      return peers; // No protocol requirement, return all
    }

    return peers.filter(peer => {
      if (!peer.protocols || peer.protocols.length === 0) {
        // For backward compatibility or if identify info is missing, assume support.
        // The connection will fail later if the protocol is not actually supported.
        return true;
      }
      // Check if the peer supports at least one of the required protocols.
      return requiredProtocols.some(requiredProto =>
        peer.protocols.some(peerProto => peerProto.includes(requiredProto))
      );
    });
  }

  // Define the required protocols for transfers
  private static TRANSFER_PROTOCOLS = [
    "/chiral/webrtc-signaling/1.0.0", // For WebRTC
    "/ipfs/bitswap", // For Bitswap
  ];

  private static rep = ReputationStore.getInstance();

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

    // First, get metrics for all available peers
    const allMetrics = await this.getPeerMetrics();
    const availablePeerMetrics = allMetrics.filter(metric => availablePeers.includes(metric.peer_id));

    // Filter peers to ensure they support a valid transfer protocol
    const supportedPeers = this.filterPeersByProtocol(availablePeerMetrics, this.TRANSFER_PROTOCOLS);
    if (supportedPeers.length === 0) {
      return null; // No peers support the required protocols
    }

    // ✨ Pre-rank by local reputation composite (no external metrics needed)
    availablePeers.sort((a, b) => {
      const cb = this.rep.composite(b);
      const ca = this.rep.composite(a);
      // Higher composite first; stable if equal
      return cb - ca;
    });

    // Keep your original strategy selection (unchanged)
    const strategy: PeerSelectionStrategy =
      fileSize > 100 * 1024 * 1024
        ? "bandwidth"
        : "fastest";

    // Keep your original selection call (unchanged)
    const selectedPeers = await this.selectPeersWithStrategy(
      supportedPeers.map(p => p.peer_id), // Use the filtered list of peer IDs
      1,
      preferEncryption ? "encryption" : strategy,
      preferEncryption
    );

    return selectedPeers.length > 0 ? selectedPeers[0] : null;

  }
  //END OF GET BEST PEER 

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

    // Get metrics for all available peers to check for protocol support
    const allMetrics = await this.getPeerMetrics();
    const availablePeerMetrics = allMetrics.filter(metric => availablePeers.includes(metric.peer_id));

    // Filter out peers that don't support the necessary transfer protocols
    const supportedPeers = this.filterPeersByProtocol(availablePeerMetrics, this.TRANSFER_PROTOCOLS);
    if (supportedPeers.length === 0) {
      return [];
    }

    // For very large files, use load balancing to distribute across peers
    const strategy: PeerSelectionStrategy =
      fileSize > 500 * 1024 * 1024
        ? "load_balanced" // >500MB use load balancing
        : "balanced"; // <500MB use balanced selection

    const peerCount = Math.min(maxPeers, supportedPeers.length);

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


  /**
   * Composite score in [0,1], combining:
   *  - local reputation (Beta) 60%
   *  - freshness (last_seen)   25%
   *  - performance (latency)   15%
   */
  static compositeScoreFromMetrics(p: PeerMetrics): number {
    // keep the store updated with what we see
    this.rep.noteSeen(p.peer_id);
    if (typeof p.latency_ms === "number") {
      // don't mark success here; RTT success will be recorded where you actually connect/transfer
      // but we can gently update EMA if we want to reflect recent latency probes
      // (optional, comment out if you prefer only connection-based updates)
      // this.rep.success(p.peer_id, p.latency_ms);
    }

    // local rep components
    const repScore = this.rep.repScore(p.peer_id);
    const freshScore = (() => {
      const nowSec = Date.now() / 1000;
      const ageSec = Math.max(0, nowSec - (p.last_seen || 0));
      if (ageSec <= 60) return 1;
      if (ageSec >= 86400) return 0;
      return 1 - (ageSec - 60) / (86400 - 60);
    })();
    const perfScore = (() => {
      if (typeof p.latency_ms !== "number") return 0.5;
      const clamped = Math.max(100, Math.min(2000, p.latency_ms));
      return 1 - (clamped - 100) / (2000 - 100);
    })();

    return 0.6 * repScore + 0.25 * freshScore + 0.15 * perfScore;
  }

  //Adding so transfers can update reputation
  static notePeerSeen(peerId: string) {
    this.rep.noteSeen(peerId);
  }

  static notePeerSuccess(peerId: string, rttMs?: number) {
    this.rep.success(peerId, rttMs);
  }

  static notePeerFailure(peerId: string) {
    this.rep.failure(peerId);
  }

  /**
   * Record successful chunk download from a peer (for Bitswap reputation)
   */
  static async recordChunkDownloadSuccess(
    peerId: string,
    chunkSize: number
  ): Promise<void> {
    try {
      await invoke("record_chunk_download_success", {
        peerId,
        chunkSize,
      });
      // Also update local reputation store
      this.rep.success(peerId);
    } catch (error) {
      console.error("Failed to record chunk download success:", error);
    }
  }

  /**
   * Record failed chunk download from a peer (for Bitswap reputation)
   */
  static async recordChunkDownloadFailure(
    peerId: string,
    error: string
  ): Promise<void> {
    try {
      await invoke("record_chunk_download_failure", {
        peerId,
        error,
      });
      // Also update local reputation store
      this.rep.failure(peerId);
    } catch (error) {
      console.error("Failed to record chunk download failure:", error);
    }
  }

  /**
   * Record successful chunk upload to a peer (for Bitswap reputation)
   */
  static async recordChunkUploadSuccess(
    peerId: string,
    chunkSize: number
  ): Promise<void> {
    try {
      await invoke("record_chunk_upload_success", {
        peerId,
        chunkSize,
      });
      // Also update local reputation store
      this.rep.success(peerId);
    } catch (error) {
      console.error("Failed to record chunk upload success:", error);
    }
  }
}

// Export for use in other parts of the application
export default PeerSelectionService;
