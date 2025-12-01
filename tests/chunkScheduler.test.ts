import { describe, it, expect, beforeEach, vi } from 'vitest';
import { ChunkScheduler, ChunkState } from '../src/lib/transfer/chunkScheduler';

//
// ChunkScheduler Test Suite
// -------------------------
// This suite covers initialization, peer tracking, scheduling logic,
// timeout behavior, retry handling, corruption handling, and load balancing.
//

describe('ChunkScheduler', () => {
  let scheduler: ChunkScheduler;

  beforeEach(() => {
    // Re-create a fresh scheduler before each test
    scheduler = new ChunkScheduler({
      maxConcurrentPerPeer: 2,      // allow up to 2 concurrent chunks per peer
      chunkTimeoutMs: 1000,         // chunk timeout used for timeout tests
      maxRetries: 2,                // maximum number of retry attempts
      peerSelectionStrategy: 'load-balanced',
    });
  });

  it('initializes with manifest', () => {
    // Tests that scheduler loads a manifest and sets all chunks to UNREQUESTED
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
    // Tests addPeer() and removePeer() logic
    scheduler.addPeer('peer1', 3);
    scheduler.addPeer('peer2');
    
    const peers = scheduler.getPeers();
    expect(peers.size).toBe(2);
    expect(peers.get('peer1')?.maxConcurrent).toBe(3);
    expect(peers.get('peer2')?.maxConcurrent).toBe(2); // default value
    
    scheduler.removePeer('peer1');
    expect(scheduler.getPeers().size).toBe(1);
  });

  it('generates chunk requests within peer limits', () => {
    // Ensures scheduler respects peer concurrency caps
    const manifest = {
      chunks: [
        { index: 0, size: 1000 },
        { index: 1, size: 1000 },
        { index: 2, size: 1000 },
      ],
    };

    scheduler.initScheduler(manifest);
    scheduler.addPeer('peer1', 1);
    scheduler.addPeer('peer2', 1);

    const requests = scheduler.getNextRequests(5);
    
    expect(requests.length).toBe(2);
    expect(requests[0].peerId).not.toBe(requests[1].peerId); // load-balanced
    expect(requests.every(r => r.chunkIndex >= 0 && r.chunkIndex <= 2)).toBe(true);
  });

  it('handles chunk success and updates peer metrics', () => {
    // Tests success path + completion tracking
    const manifest = { chunks: [{ index: 0, size: 1000 }] };
    scheduler.initScheduler(manifest);
    scheduler.addPeer('peer1');

    const requests = scheduler.getNextRequests(1);
    expect(requests.length).toBe(1);

    scheduler.onChunkReceived(0); // simulate a successful chunk
    
    const state = scheduler.getSchedulerState();
    expect(state.completedChunks).toBe(1);
    expect(state.activeRequestCount).toBe(0);
    expect(scheduler.isComplete()).toBe(true);
  });

  it('handles chunk failure and retries', () => {
    // Verifies retry behavior when chunks fail
    const manifest = { chunks: [{ index: 0, size: 1000 }] };
    scheduler.initScheduler(manifest);
    scheduler.addPeer('peer1');

    let requests = scheduler.getNextRequests(1);
    expect(requests.length).toBe(1);

    scheduler.onChunkFailed(0, false); // first failure
    
    let state = scheduler.getSchedulerState();
    expect(state.activeRequestCount).toBe(0);
    expect(state.completedChunks).toBe(0);

    requests = scheduler.getNextRequests(1); // retry #1
    expect(requests.length).toBe(1);

    scheduler.onChunkFailed(0, false); // second failure

    requests = scheduler.getNextRequests(1); // retry limit reached
    expect(requests.length).toBe(0);
  });

  it('handles timeouts', async () => {
    // Tests timeout logic using fake timers
    vi.useFakeTimers();
    
    const manifest = { chunks: [{ index: 0, size: 1000 }] };
    scheduler.initScheduler(manifest);
    scheduler.addPeer('peer1');

    const requests = scheduler.getNextRequests(1);
    expect(requests.length).toBe(1);
    expect(scheduler.getActiveRequests().size).toBe(1);

    // Advance time past timeout
    vi.advanceTimersByTime(1500);

    // Timeout cleanup occurs when requesting next work
    scheduler.getNextRequests(0);
    expect(scheduler.getActiveRequests().size).toBe(0);

    vi.useRealTimers();
  });

  it('respects peer availability', () => {
    // Ensures offline peers are ignored
    const manifest = {
      chunks: [{ index: 0, size: 1000 }, { index: 1, size: 1000 }],
    };
    scheduler.initScheduler(manifest);
    scheduler.addPeer('peer1');
    scheduler.addPeer('peer2');

    scheduler.updatePeerHealth('peer2', false); // mark peer2 unavailable

    const requests = scheduler.getNextRequests(5);
    expect(requests.every(r => r.peerId === 'peer1')).toBe(true);
  });

  it('balances load across peers', () => {
    // Ensures scheduler distributes work evenly
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
    
    const peer1Requests = requests.filter(r => r.peerId === 'peer1').length;
    const peer2Requests = requests.filter(r => r.peerId === 'peer2').length;
    
    expect(peer1Requests).toBe(2);
    expect(peer2Requests).toBe(2);
  });

  it('handles corrupted chunks', () => {
    // Tests behavior when a chunk is marked corrupted (no retries)
    const manifest = { chunks: [{ index: 0, size: 1000 }] };
    scheduler.initScheduler(manifest);
    scheduler.addPeer('peer1');

    const requests = scheduler.getNextRequests(1);
    expect(requests.length).toBe(1);

    scheduler.onChunkFailed(0, true); // corruption path
    
    const state = scheduler.getSchedulerState();
    expect(state.chunkStates[0]).toBe(ChunkState.CORRUPTED);

    const newRequests = scheduler.getNextRequests(1);
    expect(newRequests.length).toBe(0);
  });

  it('updates peer response times', () => {
    // Tests simple moving-average update of peer response times
    scheduler.addPeer('peer1');
    
    const initialAvgTime = scheduler.getPeers().get('peer1')?.avgResponseTime;

    scheduler.updatePeerHealth('peer1', true, 500);

    const updatedAvgTime = scheduler.getPeers().get('peer1')?.avgResponseTime;

    expect(updatedAvgTime).not.toBe(initialAvgTime);
    expect(updatedAvgTime).toBeLessThan(initialAvgTime!);
  });
});
