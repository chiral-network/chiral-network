import { invoke } from "@tauri-apps/api/core";
import type { PeerInfo } from "$lib/stores";

export interface BackendPeerMetrics {
  peer_id: string;
  address: string;
  nickname?: string;
  last_seen: number;
  joined_at: number;
  reliability_score: number;
  bandwidth_score: number;
  latency_ms: number;
  total_downloads: number;
  total_uploads: number;
  bytes_transferred: number;
  encryption_support: boolean;
  malicious_reports: number;
  connection_count: number;
  location?: string;
}

export class PeerService {
  private static instance: PeerService | null = null;

  private constructor() {}

  static getInstance(): PeerService {
    if (!PeerService.instance) {
      PeerService.instance = new PeerService();
    }
    return PeerService.instance;
  }

  /**
   * Get all connected peers with their detailed information
   */
  async getConnectedPeers(): Promise<PeerInfo[]> {
    try {
      // Check if running in Tauri environment
      const isTauri =
        typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

      if (!isTauri) {
        // Return mock data for web development
        console.log("PeerService: Running in web mode, returning mock data");
        return this.getMockPeers();
      }

      // Check if DHT is running before attempting to get peers
      const isDhtRunning = await invoke<boolean>("is_dht_running").catch(
        () => false
      );
      if (!isDhtRunning) {
        return [];
      }

      // Get connected peer IDs from DHT
      const connectedPeerIds = await invoke<string[]>(
        "get_dht_connected_peers"
      );

      if (connectedPeerIds.length === 0) {
        return [];
      }

      // Try to get detailed metrics for all peers
      let peerMetrics: BackendPeerMetrics[] = [];
      try {
        peerMetrics = await invoke<BackendPeerMetrics[]>("get_peer_metrics");
      } catch (metricsError) {
        // Silently handle metrics error
      }

      // Filter metrics to only include connected peers and transform to PeerInfo format
      const connectedPeers: PeerInfo[] = [];

      for (const peerId of connectedPeerIds) {
        const metrics = peerMetrics.find((m) => m.peer_id === peerId);
        if (metrics) {
          // Use detailed metrics if available
          connectedPeers.push(this.transformBackendMetricsToPeerInfo(metrics));
        } else {
          // Create basic peer info if no metrics available
          connectedPeers.push(this.createBasicPeerInfo(peerId));
        }
      }

      return connectedPeers;
    } catch (error) {
      console.error("PeerService: Failed to get connected peers:", error);
      // Only return empty array on error, don't fall back to mock data
      return [];
    }
  }

  /**
   * Get information for a specific peer
   */
  async getPeerInfo(peerId: string): Promise<PeerInfo | null> {
    try {
      const isTauri =
        typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

      if (!isTauri) {
        const mockPeers = this.getMockPeers();
        return mockPeers.find((p) => p.id === peerId) || null;
      }

      const peerMetrics =
        await invoke<BackendPeerMetrics[]>("get_peer_metrics");
      const metrics = peerMetrics.find((m) => m.peer_id === peerId);

      if (!metrics) {
        return null;
      }

      return this.transformBackendMetricsToPeerInfo(metrics);
    } catch (error) {
      console.error("Failed to get peer info:", error);
      return null;
    }
  }

  /**
   * Disconnect from a specific peer
   */
  async disconnectFromPeer(peerId: string): Promise<void> {
    try {
      const isTauri =
        typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

      if (!isTauri) {
        console.log("Mock: Disconnecting from peer", peerId);
        return;
      }

      await invoke("disconnect_from_peer", { peerId });
    } catch (error) {
      console.error("Failed to disconnect from peer:", error);
      throw error;
    }
  }

  /**
   * Connect to a new peer
   */
  async connectToPeer(peerAddress: string): Promise<void> {
    try {
      const isTauri =
        typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

      if (!isTauri) {
        console.log("Mock: Connecting to peer", peerAddress);
        return;
      }

      await invoke("connect_to_peer", { peerAddress });
    } catch (error) {
      console.error("Failed to connect to peer:", error);
      throw error;
    }
  }

  /**
   * Report a peer as malicious
   */
  async reportMaliciousPeer(peerId: string, severity: string): Promise<void> {
    try {
      const isTauri =
        typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

      if (!isTauri) {
        console.log("Mock: Reporting malicious peer", peerId, severity);
        return;
      }

      await invoke("report_malicious_peer", { peerId, severity });
    } catch (error) {
      console.error("Failed to report malicious peer:", error);
      throw error;
    }
  }

  /**
   * Transform backend peer metrics to frontend PeerInfo format
   */
  private transformBackendMetricsToPeerInfo(
    metrics: BackendPeerMetrics
  ): PeerInfo {
    // Calculate reputation from reliability and other factors with safe fallbacks
    const reliabilityScore = metrics.reliability_score || 0;
    const bandwidthScore = metrics.bandwidth_score || 0;
    const maliciousReports = metrics.malicious_reports || 0;

    const reputation =
      Math.min(
        5.0,
        Math.max(
          0,
          (reliabilityScore * 3 +
            bandwidthScore * 1.5 +
            Math.max(0, 1 - maliciousReports * 0.2)) /
            1.1
        )
      ) || 3.0; // Default to 3.0 if calculation results in NaN

    // Determine status based on last seen time
    const lastSeenMs = metrics.last_seen * 1000; // Convert to milliseconds
    const now = Date.now();
    const timeDiff = now - lastSeenMs;

    let status: "online" | "offline" | "away";
    if (timeDiff < 30000) {
      // Less than 30 seconds
      status = "online";
    } else if (timeDiff < 300000) {
      // Less than 5 minutes
      status = "away";
    } else {
      status = "offline";
    }

    // Generate address from peer ID (simplified)
    const address = this.generateAddressFromPeerId(metrics.peer_id);

    // Use joined_at if valid, otherwise use lastSeen as fallback
    const joinedAtMs = metrics.joined_at && metrics.joined_at > 0 
      ? metrics.joined_at * 1000 
      : lastSeenMs;

    return {
      id: metrics.peer_id,
      address,
      nickname: metrics.nickname || undefined,
      status,
      reputation,
      sharedFiles: metrics.total_uploads,
      totalSize: metrics.bytes_transferred,
      joinDate: new Date(joinedAtMs),
      lastSeen: new Date(lastSeenMs),
      location: metrics.location || this.inferLocationFromAddress(address),
    };
  }

  /**
   * Generate a readable address from peer ID
   */
  private generateAddressFromPeerId(peerId: string): string {
    // Return the actual peer ID instead of a fake IP address
    // This is what users need to identify and connect to peers
    return peerId;
  }

  /**
   * Infer geographic location from peer ID (mock implementation)
   */
  private inferLocationFromAddress(peerId: string): string {
    // Use a simple hash-based approach to assign locations pseudo-randomly
    // but consistently for the same peer ID
    const hash = peerId.split("").reduce((acc, char) => {
      return acc + char.charCodeAt(0);
    }, 0);

    const locations = ["US-East", "US-West", "EU-West", "Asia-Pacific"];
    return locations[hash % locations.length];
  }

  /**
   * Create basic peer info when detailed metrics are not available
   */
  private createBasicPeerInfo(peerId: string): PeerInfo {
    const address = this.generateAddressFromPeerId(peerId);
    // Create a friendly nickname from the peer ID
    const shortId = peerId.slice(-8); // Last 8 characters
    const nickname = `Peer_${shortId}`;
    const now = new Date();

    return {
      id: peerId,
      address,
      nickname,
      status: "online", // Assume online since they're connected
      reputation: 3.0, // Default neutral reputation
      sharedFiles: 0, // Unknown
      totalSize: 0, // Unknown
      joinDate: now, // Current time as join date
      lastSeen: now, // Current time
      location: this.inferLocationFromAddress(peerId),
    };
  }

  /**
   * Get mock peers for development/fallback
   */
  private getMockPeers(): PeerInfo[] {
    return [
      {
        id: "peer1",
        address: "192.168.1.50:8080",
        nickname: "AliceNode",
        status: "online",
        reputation: 4.8,
        sharedFiles: 150,
        totalSize: 5368709120,
        joinDate: new Date("2024-01-01"),
        lastSeen: new Date(),
        location: "US-East",
      },
      {
        id: "peer2",
        address: "10.0.0.25:8080",
        nickname: "BobStorage",
        status: "offline",
        reputation: 4.5,
        sharedFiles: 89,
        totalSize: 2147483648,
        joinDate: new Date("2024-02-15"),
        lastSeen: new Date(Date.now() - 3 * 24 * 60 * 60 * 1000),
        location: "EU-West",
      },
      {
        id: "peer3",
        address: "172.16.0.100:8080",
        nickname: "CharlieShare",
        status: "away",
        reputation: 4.2,
        sharedFiles: 45,
        totalSize: 1073741824,
        joinDate: new Date("2024-03-01"),
        lastSeen: new Date(Date.now() - 3600000),
        location: "Asia-Pacific",
      },
    ];
  }
}

export const peerService = PeerService.getInstance();
