import { invoke } from "@tauri-apps/api/core";

export interface BandwidthStats {
  uploadBytes: number;
  downloadBytes: number;
  lastUpdated: number;
}

export interface BandwidthDataPoint {
  timestamp: number;
  uploadBytes: number;
  downloadBytes: number;
  uploadRateKbps: number;
  downloadRateKbps: number;
}

export interface PerformanceMetrics {
  avgDownloadSpeedKbps: number;
  avgUploadSpeedKbps: number;
  peakDownloadSpeedKbps: number;
  peakUploadSpeedKbps: number;
  totalConnections: number;
  successfulTransfers: number;
  failedTransfers: number;
  avgLatencyMs: number;
}

export interface NetworkActivity {
  activeUploads: number;
  activeDownloads: number;
  queuedDownloads: number;
  completedUploads: number;
  completedDownloads: number;
  totalPeersConnected: number;
  uniquePeersAllTime: number;
}

export interface ResourceContribution {
  storageContributedBytes: number;
  bandwidthContributedBytes: number;
  filesShared: number;
  totalSeedtimeHours: number;
  reputationScore: number;
}

export interface ContributionDataPoint {
  timestamp: number;
  bandwidthContributed: number;
  storageContributed: number;
  filesSeeded: number;
}

export class AnalyticsService {
  private static instance: AnalyticsService | null = null;
  private updateInterval: number | null = null;
  private updateCallbacks: Array<() => void> = [];

  private constructor() {}

  static getInstance(): AnalyticsService {
    if (!AnalyticsService.instance) {
      AnalyticsService.instance = new AnalyticsService();
    }
    return AnalyticsService.instance;
  }

  /**
   * Get current bandwidth statistics
   */
  async getBandwidthStats(): Promise<BandwidthStats> {
    try {
      return await invoke<BandwidthStats>("get_bandwidth_stats");
    } catch (error) {
      console.error("Failed to get bandwidth stats:", error);
      return {
        uploadBytes: 0,
        downloadBytes: 0,
        lastUpdated: Date.now() / 1000,
      };
    }
  }

  /**
   * Get bandwidth history
   */
  async getBandwidthHistory(limit?: number): Promise<BandwidthDataPoint[]> {
    try {
      return await invoke<BandwidthDataPoint[]>("get_bandwidth_history", { limit });
    } catch (error) {
      console.error("Failed to get bandwidth history:", error);
      return [];
    }
  }

  /**
   * Get performance metrics
   */
  async getPerformanceMetrics(): Promise<PerformanceMetrics> {
    try {
      return await invoke<PerformanceMetrics>("get_performance_metrics");
    } catch (error) {
      console.error("Failed to get performance metrics:", error);
      return {
        avgDownloadSpeedKbps: 0,
        avgUploadSpeedKbps: 0,
        peakDownloadSpeedKbps: 0,
        peakUploadSpeedKbps: 0,
        totalConnections: 0,
        successfulTransfers: 0,
        failedTransfers: 0,
        avgLatencyMs: 0,
      };
    }
  }

  /**
   * Get network activity
   */
  async getNetworkActivity(): Promise<NetworkActivity> {
    try {
      return await invoke<NetworkActivity>("get_network_activity");
    } catch (error) {
      console.error("Failed to get network activity:", error);
      return {
        activeUploads: 0,
        activeDownloads: 0,
        queuedDownloads: 0,
        completedUploads: 0,
        completedDownloads: 0,
        totalPeersConnected: 0,
        uniquePeersAllTime: 0,
      };
    }
  }

  /**
   * Get resource contribution
   */
  async getResourceContribution(): Promise<ResourceContribution> {
    try {
      return await invoke<ResourceContribution>("get_resource_contribution");
    } catch (error) {
      console.error("Failed to get resource contribution:", error);
      return {
        storageContributedBytes: 0,
        bandwidthContributedBytes: 0,
        filesShared: 0,
        totalSeedtimeHours: 0,
        reputationScore: 5.0,
      };
    }
  }

  /**
   * Get contribution history
   */
  async getContributionHistory(limit?: number): Promise<ContributionDataPoint[]> {
    try {
      return await invoke<ContributionDataPoint[]>("get_contribution_history", { limit });
    } catch (error) {
      console.error("Failed to get contribution history:", error);
      return [];
    }
  }

  /**
   * Reset all analytics statistics
   */
  async resetStats(): Promise<void> {
    try {
      await invoke("reset_analytics");
    } catch (error) {
      console.error("Failed to reset analytics:", error);
      throw error;
    }
  }

  /**
   * Start automatic updates
   */
  startAutoUpdate(intervalMs: number = 3000, callback?: () => void) {
    if (this.updateInterval) {
      this.stopAutoUpdate();
    }

    if (callback) {
      this.updateCallbacks.push(callback);
    }

    this.updateInterval = window.setInterval(() => {
      this.updateCallbacks.forEach((cb) => cb());
    }, intervalMs);
  }

  /**
   * Stop automatic updates
   */
  stopAutoUpdate() {
    if (this.updateInterval) {
      clearInterval(this.updateInterval);
      this.updateInterval = null;
    }
    this.updateCallbacks = [];
  }

  /**
   * Add update callback
   */
  onUpdate(callback: () => void) {
    this.updateCallbacks.push(callback);
  }

  /**
   * Remove update callback
   */
  offUpdate(callback: () => void) {
    const index = this.updateCallbacks.indexOf(callback);
    if (index > -1) {
      this.updateCallbacks.splice(index, 1);
    }
  }
}

export const analyticsService = AnalyticsService.getInstance();
