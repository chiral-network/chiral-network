import { invoke } from '@tauri-apps/api/core';

export interface ChunkInfo {
  chunkId: number;
  offset: number;
  size: number;
  hash: string;
}

export interface PeerAssignment {
  peerId: string;
  chunks: number[];
  status: 'Connecting' | 'Connected' | 'Downloading' | 'Failed' | 'Completed';
  connectedAt?: number;
  lastActivity?: number;
}

export interface MultiSourceProgress {
  fileHash: string;
  fileName: string;
  totalSize: number;
  downloadedSize: number;
  totalChunks: number;
  completedChunks: number;
  activePeers: number;
  downloadSpeedBps: number;
  etaSeconds?: number;
  peerAssignments: PeerAssignment[];
}

export interface MultiSourceDownloadOptions {
  maxPeers?: number;
  chunkSize?: number;
  preferMultiSource?: boolean;
  selectedPeers?: string[];  // Explicitly selected peers from peer selection modal
  peerAllocation?: Array<{peerId: string; percentage: number}>;  // Manual chunk allocation
}

export class MultiSourceDownloadService {
  
  /**
   * Start a multi-source download for a file
   */
  static async startDownload(
    fileHash: string,
    outputPath: string,
    options?: MultiSourceDownloadOptions
  ): Promise<string> {
    return invoke('start_multi_source_download', {
      fileHash,
      outputPath,
      maxPeers: options?.maxPeers,
      chunkSize: options?.chunkSize,
      selectedPeers: options?.selectedPeers,
      peerAllocation: options?.peerAllocation
    });
  }

  /**
   * Cancel an active multi-source download
   */
  static async cancelDownload(fileHash: string): Promise<void> {
    return invoke('cancel_multi_source_download', { fileHash });
  }

  /**
   * Get progress information for a multi-source download
   */
  static async getProgress(fileHash: string): Promise<MultiSourceProgress | null> {
    return invoke('get_multi_source_progress', { fileHash });
  }

  /**
   * Download a file with automatic multi-source detection
   * Falls back to single-source if multi-source is not beneficial
   */
  static async downloadFile(
    fileHash: string,
    outputPath: string,
    options?: MultiSourceDownloadOptions
  ): Promise<string> {
    return invoke('download_file_multi_source', {
      fileHash,
      outputPath,
      preferMultiSource: options?.preferMultiSource ?? true,
      maxPeers: options?.maxPeers
    });
  }

  /**
   * Format download speed for display
   */
  static formatSpeed(bytesPerSecond: number): string {
    const units = ['B/s', 'KB/s', 'MB/s', 'GB/s'];
    let speed = bytesPerSecond;
    let unitIndex = 0;
    
    while (speed >= 1024 && unitIndex < units.length - 1) {
      speed /= 1024;
      unitIndex++;
    }
    
    return `${speed.toFixed(1)} ${units[unitIndex]}`;
  }

  /**
   * Format file size for display
   */
  static formatFileSize(bytes: number): string {
    const units = ['B', 'KB', 'MB', 'GB', 'TB'];
    let size = bytes;
    let unitIndex = 0;
    
    while (size >= 1024 && unitIndex < units.length - 1) {
      size /= 1024;
      unitIndex++;
    }
    
    return `${size.toFixed(1)} ${units[unitIndex]}`;
  }

  /**
   * Format ETA for display
   */
  static formatETA(seconds?: number): string {
    if (!seconds || seconds <= 0) {
      return 'Unknown';
    }
    
    if (seconds < 60) {
      return `${Math.round(seconds)}s`;
    } else if (seconds < 3600) {
      const minutes = Math.round(seconds / 60);
      return `${minutes}m`;
    } else {
      const hours = Math.floor(seconds / 3600);
      const minutes = Math.round((seconds % 3600) / 60);
      return minutes > 0 ? `${hours}h ${minutes}m` : `${hours}h`;
    }
  }

  /**
   * Calculate download completion percentage
   */
  static getCompletionPercentage(progress: MultiSourceProgress): number {
    if (progress.totalSize === 0) return 0;
    return Math.round((progress.downloadedSize / progress.totalSize) * 100);
  }

  /**
   * Get status summary for a multi-source download
   */
  static getStatusSummary(progress: MultiSourceProgress): string {
    const percentage = this.getCompletionPercentage(progress);
    const speed = this.formatSpeed(progress.downloadSpeedBps);
    const eta = this.formatETA(progress.etaSeconds);
    
    return `${percentage}% - ${speed} - ${progress.activePeers} peers - ETA: ${eta}`;
  }

  /**
   * Check if a download should use multi-source based on file size and available peers
   */
  static shouldUseMultiSource(fileSize: number, availablePeers: number): boolean {
    const MIN_FILE_SIZE_FOR_MULTI_SOURCE = 1024 * 1024; // 1MB
    const MIN_PEERS_FOR_MULTI_SOURCE = 2;
    
    return fileSize >= MIN_FILE_SIZE_FOR_MULTI_SOURCE && availablePeers >= MIN_PEERS_FOR_MULTI_SOURCE;
  }

  /**
   * Get optimal chunk size based on file size
   */
  static getOptimalChunkSize(fileSize: number): number {
    if (fileSize < 1024 * 1024) {
      return 64 * 1024; // 64KB for small files
    } else if (fileSize < 100 * 1024 * 1024) {
      return 256 * 1024; // 256KB for medium files
    } else if (fileSize < 1024 * 1024 * 1024) {
      return 512 * 1024; // 512KB for large files
    } else {
      return 1024 * 1024; // 1MB for very large files
    }
  }

  /**
   * Get optimal number of peers based on file size
   */
  static getOptimalPeerCount(fileSize: number, availablePeers: number): number {
    let optimalPeers = 1;
    
    if (fileSize > 10 * 1024 * 1024) {
      optimalPeers = 2; // 2 peers for files > 10MB
    }
    if (fileSize > 100 * 1024 * 1024) {
      optimalPeers = 3; // 3 peers for files > 100MB
    }
    if (fileSize > 1024 * 1024 * 1024) {
      optimalPeers = 4; // 4 peers for files > 1GB
    }
    
    return Math.min(optimalPeers, availablePeers, 4); // Cap at 4 peers maximum
  }
}