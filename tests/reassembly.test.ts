// Mock the Tauri invoke function before importing modules that use it
import { vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

import { describe, it, expect, beforeEach } from 'vitest';
import { reassemblyManager, ChunkState } from '../src/lib/reassembly';

import { invoke } from '@tauri-apps/api/core';

beforeEach(() => {
  (invoke as any).mockReset?.();
});

describe('ReassemblyManager', () => {
  it('initializes and computes offsets', () => {
    const manifest = {
      fileSize: 3000,
      chunks: [
        { index: 0, encryptedSize: 1000 },
        { index: 1, encryptedSize: 1000 },
        { index: 2, encryptedSize: 1000 },
      ],
    };

    reassemblyManager.initReassembly('t1', manifest, '/tmp/t1');
    const state = reassemblyManager.getState('t1');
    expect(state).not.toBeNull();
    expect(state!.offsets).toEqual([0, 1000, 2000]);
    // initial per-chunk states should be UNREQUESTED
    expect(state!.chunkStates.every((s: any) => s === ChunkState.UNREQUESTED)).toBe(true);
  });

  it('accepts a valid chunk and persists via invoke', async () => {
    const manifest = {
      fileSize: 200,
      chunks: [{ index: 0, encryptedSize: 100 }, { index: 1, encryptedSize: 100 }],
    };

    reassemblyManager.initReassembly('t2', manifest, '/tmp/t2');

    // Mock invoke success
    (invoke as any).mockResolvedValue(true);

    const chunk = new Uint8Array([1,2,3]);
    const ok = await reassemblyManager.acceptChunk('t2', 0, chunk);
    expect(ok).toBe(true);

    const state = reassemblyManager.getState('t2');
    expect(state!.receivedChunks.includes(0)).toBe(true);
    // ensure chunk state set to RECEIVED
    expect(state!.chunkStates[0]).toBe(ChunkState.RECEIVED);
  });

  it('rejects a chunk with bad checksum', async () => {
    const manifest = {
      fileSize: 200,
      chunks: [{ index: 0, encryptedSize: 100, checksum: 'deadbeef' }],
    };

    reassemblyManager.initReassembly('t3', manifest, '/tmp/t3');

    const chunk = new Uint8Array([9,9,9]);
    const ok = await reassemblyManager.acceptChunk('t3', 0, chunk);
    expect(ok).toBe(false);

    const state = reassemblyManager.getState('t3');
    expect(state!.corruptedChunks.includes(0)).toBe(true);
    expect(state!.chunkStates[0]).toBe(ChunkState.CORRUPTED);
  });

  it('finalize calls backend verify_and_finalize', async () => {
    const manifest = {
      fileSize: 200,
      chunks: [{ index: 0, encryptedSize: 100 }, { index: 1, encryptedSize: 100 }],
    };

    reassemblyManager.initReassembly('t4', manifest, '/tmp/t4');

    // Pretend both chunks received using public API
    reassemblyManager.markChunkReceived('t4', 0);
    reassemblyManager.markChunkReceived('t4', 1);

    (invoke as any).mockResolvedValue({ ok: true });

    const ok = await reassemblyManager.finalize('t4', '/final/path');
    expect(ok).toBe(true);

    const after = reassemblyManager.getState('t4');
    expect(after).toBeNull();
  });

  it('respects bounded write queue and concurrency', async () => {
    const manifest = {
      fileSize: 300,
      chunks: [
        { index: 0, encryptedSize: 100 },
        { index: 1, encryptedSize: 100 },
        { index: 2, encryptedSize: 100 },
      ],
    };

    // use maxConcurrentWrites = 1 to test serialization
    reassemblyManager.initReassembly('t5', manifest, '/tmp/t5', 1);

    // Create deferred resolves for each invoke call
    const resolves: Array<(v: any) => void> = [];
    const invokePromises: Promise<any>[] = [];
    (invoke as any).mockImplementation(() => {
      const p = new Promise((res) => {
        resolves.push(res);
      });
      invokePromises.push(p);
      return p;
    });

    // Start three acceptChunk calls but don't await them yet
    const p0 = reassemblyManager.acceptChunk('t5', 0, new Uint8Array([1]));
    const p1 = reassemblyManager.acceptChunk('t5', 1, new Uint8Array([2]));
    const p2 = reassemblyManager.acceptChunk('t5', 2, new Uint8Array([3]));

    // allow microtasks to settle so enqueue/processing runs
    await Promise.resolve();

    // With concurrency=1, only one write should be in flight and the rest queued
    let s = reassemblyManager.getState('t5')!;
    expect(s.writeInFlight).toBe(1);
    expect(s.writeQueueLength).toBe(2);

    // First chunk should have been marked REQUESTED (before write completes)
    expect(s.chunkStates[0]).toBe(ChunkState.REQUESTED);
    // Other chunks should also be REQUESTED (they're enqueued)
    expect(s.chunkStates[1]).toBe(ChunkState.REQUESTED);
    expect(s.chunkStates[2]).toBe(ChunkState.REQUESTED);

    // Fulfill first write
    resolves[0](true);
    await p0; // wait for acceptChunk resolution

    // Now one queued job should have started: still 1 in flight, queue length 1
    s = reassemblyManager.getState('t5')!;
    expect(s.writeInFlight).toBe(1);
    expect(s.writeQueueLength).toBe(1);
    // first chunk should be RECEIVED
    expect(s.chunkStates[0]).toBe(ChunkState.RECEIVED);

    // Fulfill remaining writes
    resolves[1](true);
    await p1;
    resolves[2](true);
    await p2;

    // After all completed, no in-flight writes and empty queue
    s = reassemblyManager.getState('t5')!;
    expect(s.writeInFlight).toBe(0);
    expect(s.writeQueueLength).toBe(0);
    expect(s.chunkStates.every((st: any) => st === ChunkState.RECEIVED)).toBe(true);
  });

  it('enforces hard queue length cap', async () => {
    const manifest = {
      fileSize: 300,
      chunks: [
        { index: 0, encryptedSize: 100 },
        { index: 1, encryptedSize: 100 },
      ],
    };

    // small maxQueueLength to trigger backpressure
    reassemblyManager.initReassembly('t6', manifest, '/tmp/t6', 1, 1);

    // Make invoke never resolve to keep the queue occupied
    (invoke as any).mockImplementation(() => new Promise(() => {}));

    // First accept should enqueue
    const p0 = reassemblyManager.acceptChunk('t6', 0, new Uint8Array([1]));
    await Promise.resolve();
    const s = reassemblyManager.getState('t6')!;
    expect(s.writeQueueLength + s.writeInFlight).toBeGreaterThan(0);

    // Second accept should exceed maxQueueLength and throw
    await expect(reassemblyManager.acceptChunk('t6', 1, new Uint8Array([2]))).rejects.toThrow();
  });

  it('emits events on chunk state changes and progress', async () => {
    const manifest = {
      fileSize: 200,
      chunks: [{ index: 0, encryptedSize: 100 }, { index: 1, encryptedSize: 100 }],
    };
    reassemblyManager.initReassembly('t7', manifest, '/tmp/t7');

    const events: any[] = [];
    reassemblyManager.on('chunkState', (p) => events.push(['state', p]));
    reassemblyManager.on('progress', (p) => events.push(['progress', p]));

    (invoke as any).mockResolvedValue(true);

    await reassemblyManager.acceptChunk('t7', 0, new Uint8Array([1]));

    // should have emitted REQUESTED then RECEIVED and a progress event
    expect(events.length).toBeGreaterThanOrEqual(2);
    expect(events.some((e) => e[0] === 'state' && e[1].state === ChunkState.REQUESTED)).toBe(true);
    expect(events.some((e) => e[0] === 'state' && e[1].state === ChunkState.RECEIVED)).toBe(true);
    expect(events.some((e) => e[0] === 'progress')).toBe(true);
  });
});
