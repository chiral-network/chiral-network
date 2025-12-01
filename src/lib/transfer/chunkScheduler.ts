/**
 * Chunk Scheduler for multi-source downloads
 * 
 * Manages which chunks to request from which peers, handles timeouts,
 * reassignment, and coordinates with reassembly state.
 */

export interface ChunkRequest {
  chunkIndex: number;
  peerId: string;
  requestedAt: number;
  timeoutMs: number;
}

export interface PeerInfo {
  peerId: string;
  available: boolean;
  lastSeen: number;
  pendingRequests: number;
  maxConcurrent: number;
  avgResponseTime: number;
  failureCount: number;
}

export interface SchedulerConfig {
  maxConcurrentPerPeer: number;
  chunkTimeoutMs: number;
  maxRetries: number;
  peerSelectionStrategy: 'round-robin' | 'fastest-first' | 'load-balanced';
}

export interface ChunkManifest {
  chunks: Array<{
    index: number;
    size: number;
    checksum?: string;
  }>;
}

export enum ChunkState {
  UNREQUESTED = "UNREQUESTED",
  REQUESTED = "REQUESTED", 
  RECEIVED = "RECEIVED",
  CORRUPTED = "CORRUPTED",
}

export class ChunkScheduler {
  private config: SchedulerConfig;
  private peers = new Map<string, PeerInfo>();
  private activeRequests = new Map<number, ChunkRequest>();
  private chunkStates: ChunkState[] = [];
  private retryCount = new Map<number, number>();

  constructor(config: Partial<SchedulerConfig> = {}) {
    this.config = {
      maxConcurrentPerPeer: 3,
      chunkTimeoutMs: 30000,
      maxRetries: 3,
      peerSelectionStrategy: 'load-balanced',
      ...config,
    };
  }

  initScheduler(manifest: ChunkManifest): void {
    this.chunkStates = manifest.chunks.map(() => ChunkState.UNREQUESTED);
    this.activeRequests.clear();
    this.retryCount.clear();
  }

  addPeer(peerId: string, maxConcurrent?: number): void {
    this.peers.set(peerId, {
      peerId,
      available: true,
      lastSeen: Date.now(),
      pendingRequests: 0,
      maxConcurrent: maxConcurrent ?? this.config.maxConcurrentPerPeer,
      avgResponseTime: 1000, // initial estimate
      failureCount: 0,
    });
  }

  removePeer(peerId: string): void {
    // Cancel all active requests from this peer
    for (const [chunkIndex, request] of this.activeRequests.entries()) {
      if (request.peerId === peerId) {
        this.activeRequests.delete(chunkIndex);
        this.chunkStates[chunkIndex] = ChunkState.UNREQUESTED;
      }
    }
    this.peers.delete(peerId);
  }

  updatePeerHealth(peerId: string, available: boolean, responseTimeMs?: number): void {
    const peer = this.peers.get(peerId);
    if (!peer) return;

    peer.available = available;
    peer.lastSeen = Date.now();
    
    if (responseTimeMs !== undefined) {
      // Exponential moving average
      peer.avgResponseTime = peer.avgResponseTime * 0.8 + responseTimeMs * 0.2;
    }

    if (!available) {
      peer.failureCount += 1;
    }
  }

  onChunkReceived(chunkIndex: number): void {
    const request = this.activeRequests.get(chunkIndex);
    if (request) {
      const peer = this.peers.get(request.peerId);
      if (peer) {
        peer.pendingRequests = Math.max(0, peer.pendingRequests - 1);
        const responseTime = Date.now() - request.requestedAt;
        this.updatePeerHealth(request.peerId, true, responseTime);
      }
      this.activeRequests.delete(chunkIndex);
    }
    this.chunkStates[chunkIndex] = ChunkState.RECEIVED;
  }

  onChunkFailed(chunkIndex: number, markCorrupted = false): void {
    const request = this.activeRequests.get(chunkIndex);
    if (request) {
      const peer = this.peers.get(request.peerId);
      if (peer) {
        peer.pendingRequests = Math.max(0, peer.pendingRequests - 1);
        peer.failureCount += 1;
      }
      this.activeRequests.delete(chunkIndex);
    }

    if (markCorrupted) {
      this.chunkStates[chunkIndex] = ChunkState.CORRUPTED;
    } else {
      this.chunkStates[chunkIndex] = ChunkState.UNREQUESTED;
    }

    // Increment retry count
    const retries = this.retryCount.get(chunkIndex) || 0;
    this.retryCount.set(chunkIndex, retries + 1);
  }

  getNextRequests(maxRequests = 10): ChunkRequest[] {
    const requests: ChunkRequest[] = [];
    const now = Date.now();

    // Handle timeouts first
    this.handleTimeouts(now);

    // Get available peers sorted by selection strategy
    const availablePeers = this.getAvailablePeers();
    if (availablePeers.length === 0) return requests;

    // Find chunks that need requesting
    const chunksToRequest = this.getChunksToRequest(maxRequests);
    
    let peerIndex = 0;
    for (const chunkIndex of chunksToRequest) {
      if (requests.length >= maxRequests) break;

      const peer = availablePeers[peerIndex % availablePeers.length];
      if (peer.pendingRequests >= peer.maxConcurrent) {
        peerIndex++;
        continue;
      }

      const request: ChunkRequest = {
        chunkIndex,
        peerId: peer.peerId,
        requestedAt: now,
        timeoutMs: this.config.chunkTimeoutMs,
      };

      requests.push(request);
      this.activeRequests.set(chunkIndex, request);
      this.chunkStates[chunkIndex] = ChunkState.REQUESTED;
      peer.pendingRequests++;
      
      peerIndex++;
    }

    return requests;
  }

  private handleTimeouts(now: number): void {
    const timedOutChunks: number[] = [];
    
    for (const [chunkIndex, request] of this.activeRequests.entries()) {
      if (now - request.requestedAt > request.timeoutMs) {
        timedOutChunks.push(chunkIndex);
      }
    }
    
    // Handle timeouts after iteration to avoid modifying map during iteration
    for (const chunkIndex of timedOutChunks) {
      this.onChunkFailed(chunkIndex, false);
    }
  }

  private getAvailablePeers(): PeerInfo[] {
    const available = Array.from(this.peers.values())
      .filter(p => p.available && p.pendingRequests < p.maxConcurrent);

    switch (this.config.peerSelectionStrategy) {
      case 'fastest-first':
        return available.sort((a, b) => a.avgResponseTime - b.avgResponseTime);
      
      case 'load-balanced':
        return available.sort((a, b) => 
          (a.pendingRequests / a.maxConcurrent) - (b.pendingRequests / b.maxConcurrent)
        );
      
      case 'round-robin':
      default:
        return available;
    }
  }

  private getChunksToRequest(maxChunks: number): number[] {
    const chunks: number[] = [];
    
    for (let i = 0; i < this.chunkStates.length && chunks.length < maxChunks; i++) {
      const state = this.chunkStates[i];
      const retries = this.retryCount.get(i) || 0;
      
      if (state === ChunkState.UNREQUESTED && retries < this.config.maxRetries) {
        chunks.push(i);
      }
    }
    
    return chunks;
  }

  getSchedulerState() {
    return {
      chunkStates: this.chunkStates.slice(),
      activeRequestCount: this.activeRequests.size,
      availablePeerCount: Array.from(this.peers.values()).filter(p => p.available).length,
      totalPeerCount: this.peers.size,
      completedChunks: this.chunkStates.filter(s => s === ChunkState.RECEIVED).length,
      totalChunks: this.chunkStates.length,
    };
  }

  isComplete(): boolean {
    return this.chunkStates.every(state => state === ChunkState.RECEIVED);
  }

  // Test/debug helpers
  getActiveRequests(): Map<number, ChunkRequest> {
    return new Map(this.activeRequests);
  }

  getPeers(): Map<string, PeerInfo> {
    return new Map(this.peers);
  }
}
