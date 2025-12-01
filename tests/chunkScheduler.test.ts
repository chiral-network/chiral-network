import { describe, it, expect, beforeEach, vi } from 'vitest';
import { ChunkScheduler, ChunkState } from '../src/lib/transfer/chunkScheduler';

describe('ChunkScheduler', () => {
  let scheduler: ChunkScheduler;

  beforeEach(() => {
    scheduler = new ChunkScheduler({
      maxConcurrentPerPeer: 2,
      chunkTimeoutMs: 1000,
      maxRetries: 2,
      peerSelectionStrategy: 'load-balanced',
    });
  });

  it('initializes with manifest', () => {
    const manifest = {
      chunks: [
        { index: 0, size: 1000 },
        { index: 1, size: 1000 },
        { index: 2, size: 1000 },
      ],
    };

    scheduler.initScheduler(manifest);
    const state = scheduler.getSchedulerState();
    
    expect(state.totalChunks).toBe(3);
    expect(state.completedChunks).toBe(0);
    expect(state.chunkStates.every(s => s === ChunkState.UNREQUESTED)).toBe(true);
  });

  it('adds and removes peers', () => {
    scheduler.addPeer('peer1', 3);
    scheduler.addPeer('peer2');
    
    const peers = scheduler.getPeers();
    expect(peers.size).toBe(2);
    expect(peers.get('peer1')?.maxConcurrent).toBe(3);
    expect(peers.get('peer2')?.maxConcurrent).toBe(2); // default
    
    scheduler.removePeer('peer1');
    expect(scheduler.getPeers().size).toBe(1);
  });

  it('generates chunk requests within peer limits', () => {
    const manifest = {
      chunks: [
        { index: 0, size: 1000 },
        { index: 1, size: 1000 },
        { index: 2, size: 1000 },
      ],
    };

    scheduler.initScheduler(manifest);
    scheduler.addPeer('peer1', 1); // limit to 1 concurrent
    scheduler.addPeer('peer2', 1);

    const requests = scheduler.getNextRequests(5);
    
    expect(requests.length).toBe(2); // limited by peer concurrency
    expect(requests[0].peerId).not.toBe(requests[1].peerId); // different peers
    expect(requests.every(r => r.chunkIndex >= 0 && r.chunkIndex <= 2)).toBe(true);
  });

  it('handles chunk success and updates peer metrics', () => {
    const manifest = { chunks: [{ index: 0, size: 1000 }] };
    scheduler.initScheduler(manifest);
    scheduler.addPeer('peer1');

    const requests = scheduler.getNextRequests(1);
    expect(requests.length).toBe(1);
    expect(requests[0].chunkIndex).toBe(0);

    // Simulate successful chunk
    scheduler.onChunkReceived(0);
    
    const state = scheduler.getSchedulerState();
    expect(state.completedChunks).toBe(1);
    expect(state.activeRequestCount).toBe(0);
    expect(scheduler.isComplete()).toBe(true);
  });

  it('handles chunk failure and retries', () => {
    const manifest = { chunks: [{ index: 0, size: 1000 }] };
    scheduler.initScheduler(manifest);
    scheduler.addPeer('peer1');

    // First request
    let requests = scheduler.getNextRequests(1);
    expect(requests.length).toBe(1);

    // Simulate failure
    scheduler.onChunkFailed(0, false);
    
    let state = scheduler.getSchedulerState();
    expect(state.activeRequestCount).toBe(0);
    expect(state.completedChunks).toBe(0);

    // Should allow retry
    requests = scheduler.getNextRequests(1);
    expect(requests.length).toBe(1);
    expect(requests[0].chunkIndex).toBe(0);

    // Fail again
    scheduler.onChunkFailed(0, false);

    // Third attempt (max retries = 2)
    requests = scheduler.getNextRequests(1);
    expect(requests.length).toBe(0); // exceeded max retries
  });

  it('handles timeouts', async () => {
    vi.useFakeTimers();
    const startTime = Date.now();
    
    const manifest = { chunks: [{ index: 0, size: 1000 }] };
    scheduler.initScheduler(manifest);
    scheduler.addPeer('peer1');

    // Make request
    const requests = scheduler.getNextRequests(1);
    expect(requests.length).toBe(1);
    expect(scheduler.getActiveRequests().size).toBe(1);

    // Advance time past timeout (config timeout is 1000ms)
    vi.advanceTimersByTime(1500);

    // Next call should handle timeout - call it multiple times to ensure processing
    scheduler.getNextRequests(0); // Just trigger timeout handling
    expect(scheduler.getActiveRequests().size).toBe(0);

    vi.useRealTimers();
  });

  it('respects peer availability', () => {
    const manifest = {
      chunks: [{ index: 0, size: 1000 }, { index: 1, size: 1000 }],
    };
    scheduler.initScheduler(manifest);
    scheduler.addPeer('peer1');
    scheduler.addPeer('peer2');

    // Mark peer2 as unavailable
    scheduler.updatePeerHealth('peer2', false);

    const requests = scheduler.getNextRequests(5);
    
    // Should only use peer1
    expect(requests.every(r => r.peerId === 'peer1')).toBe(true);
  });

  it('balances load across peers', () => {
    const manifest = {
      chunks: [
        { index: 0, size: 1000 },
        { index: 1, size: 1000 },
        { index: 2, size: 1000 },
        { index: 3, size: 1000 },
      ],
    };
    scheduler.initScheduler(manifest);
    scheduler.addPeer('peer1', 2);
    scheduler.addPeer('peer2', 2);

    const requests = scheduler.getNextRequests(4);
    
    // Should distribute across both peers
    const peer1Requests = requests.filter(r => r.peerId === 'peer1').length;
    const peer2Requests = requests.filter(r => r.peerId === 'peer2').length;
    
    expect(peer1Requests).toBe(2);
    expect(peer2Requests).toBe(2);
  });

  it('handles corrupted chunks', () => {
    const manifest = { chunks: [{ index: 0, size: 1000 }] };
    scheduler.initScheduler(manifest);
    scheduler.addPeer('peer1');

    const requests = scheduler.getNextRequests(1);
    expect(requests.length).toBe(1);

    // Mark as corrupted (won't retry)
    scheduler.onChunkFailed(0, true);
    
    const state = scheduler.getSchedulerState();
    expect(state.chunkStates[0]).toBe(ChunkState.CORRUPTED);

    // Should not generate new requests for corrupted chunk
    const newRequests = scheduler.getNextRequests(1);
    expect(newRequests.length).toBe(0);
  });

  it('updates peer response times', () => {
    scheduler.addPeer('peer1');
    
    const initialAvgTime = scheduler.getPeers().get('peer1')?.avgResponseTime;
    
    scheduler.updatePeerHealth('peer1', true, 500);
    
    const updatedAvgTime = scheduler.getPeers().get('peer1')?.avgResponseTime;
    expect(updatedAvgTime).not.toBe(initialAvgTime);
    expect(updatedAvgTime).toBeLessThan(initialAvgTime!);
  });
});
