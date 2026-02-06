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
    // logical receipt is marked immediately
    expect(state!.receivedChunks.includes(0)).toBe(true);
    // ensure chunk state set to RECEIVED
    expect(state!.chunkStates[0]).toBe(ChunkState.RECEIVED);

    // ensure backend was invoked (drain runs async)
    await new Promise((r) => setTimeout(r, 0));
    expect((invoke as any).mock.calls.length).toBeGreaterThanOrEqual(1);
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

    // Start three acceptChunk calls
    const r0 = await reassemblyManager.acceptChunk('t5', 0, new Uint8Array([1]));
    const r1 = await reassemblyManager.acceptChunk('t5', 1, new Uint8Array([2]));
    const r2 = await reassemblyManager.acceptChunk('t5', 2, new Uint8Array([3]));

    expect(r0).toBe(true);
    expect(r1).toBe(true);
    expect(r2).toBe(true);

    // allow microtasks to settle so enqueue/processing runs
    await new Promise((r) => setTimeout(r, 0));

    // With concurrency=1, only one write should be in flight and the rest queued
    let s = reassemblyManager.getState('t5')!;
    expect(s.writeInFlight).toBe(1);
    expect(s.writeQueueLength).toBe(2);

    // Chunks are marked RECEIVED immediately after checksum validation
    expect(s.chunkStates[0]).toBe(ChunkState.RECEIVED);
    expect(s.chunkStates[1]).toBe(ChunkState.RECEIVED);
    expect(s.chunkStates[2]).toBe(ChunkState.RECEIVED);

    // Fulfill first write
    resolves[0](true);
    // allow drain to pick next
    await new Promise((r) => setTimeout(r, 0));

    // Now one queued job should have started: still 1 in flight, queue length 1
    s = reassemblyManager.getState('t5')!;
    expect(s.writeInFlight).toBe(1);
    expect(s.writeQueueLength).toBe(1);
    // first chunk should be RECEIVED
    expect(s.chunkStates[0]).toBe(ChunkState.RECEIVED);

    // Fulfill remaining writes
    resolves[1](true);
    await new Promise((r) => setTimeout(r, 0));
    resolves[2](true);
    await new Promise((r) => setTimeout(r, 0));

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
    const r0 = await reassemblyManager.acceptChunk('t6', 0, new Uint8Array([1]));
    expect(r0).toBe(true);
    await new Promise((r) => setTimeout(r, 0));
    const s = reassemblyManager.getState('t6')!;
    expect(s.writeQueueLength + s.writeInFlight).toBeGreaterThan(0);

    // Second accept should hit backpressure and return false (not throw)
    const r1 = await reassemblyManager.acceptChunk('t6', 1, new Uint8Array([2]));
    expect(r1).toBe(false);
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

  it('simulates crash and resume using saved bitmap', async () => {
    const manifest = {
      fileSize: 400,
      chunks: [
        { index: 0, encryptedSize: 100 },
        { index: 1, encryptedSize: 100 },
        { index: 2, encryptedSize: 100 },
        { index: 3, encryptedSize: 100 },
      ],
    };

    // First run: receive chunks 0 and 2 and assume bitmap was saved by backend
    reassemblyManager.initReassembly('t8', manifest, '/tmp/t8');

    // Mock invoke to succeed for writes
    (invoke as any).mockImplementation((cmd: string, args: any) => {
      if (cmd === 'write_chunk_temp') return Promise.resolve(true);
      return Promise.resolve(null);
    });

    await reassemblyManager.acceptChunk('t8', 0, new Uint8Array([1]));
    await reassemblyManager.acceptChunk('t8', 2, new Uint8Array([3]));

    const s = reassemblyManager.getState('t8')!;
    expect(s.receivedChunks.includes(0)).toBe(true);
    expect(s.receivedChunks.includes(2)).toBe(true);

    // Simulate process crash and restart: new transfer id 't8r' will resume using bitmap loaded from backend
    reassemblyManager.initReassembly('t8r', manifest, '/tmp/t8r');

    // Mock load_chunk_bitmap to return the saved chunks for original transfer 't8'
    (invoke as any).mockImplementation((cmd: string, args: any) => {
      if (cmd === 'load_chunk_bitmap') {
        return Promise.resolve([0, 2]);
      }
      if (cmd === 'write_chunk_temp') return Promise.resolve(true);
      return Promise.resolve(null);
    });

    // Load saved bitmap (simulated) and mark received chunks on resumed manager
    const loaded: number[] = await (invoke as any)('load_chunk_bitmap', { transferId: 't8' });
    expect(loaded).toEqual([0, 2]);

    for (const idx of loaded) {
      reassemblyManager.markChunkReceived('t8r', idx);
    }

    let sr = reassemblyManager.getState('t8r')!;
    expect(sr.receivedChunks.includes(0)).toBe(true);
    expect(sr.receivedChunks.includes(2)).toBe(true);

    // Now accept remaining chunks and finalize
    await reassemblyManager.acceptChunk('t8r', 1, new Uint8Array([2]));
    await reassemblyManager.acceptChunk('t8r', 3, new Uint8Array([4]));

    sr = reassemblyManager.getState('t8r')!;
    expect(sr.receivedChunks.length).toBe(4);
  });

  it('backpressure resumes after drain', async () => {
    const manifest = {
      fileSize: 200,
      chunks: [{ index: 0, encryptedSize: 100 }, { index: 1, encryptedSize: 100 }],
    };

    // Use a tiny queue so backpressure is triggered while first write is in-flight
    reassemblyManager.initReassembly('t9', manifest, '/tmp/t9', 1, 1);

    const resolves: Array<(v: any) => void> = [];
    (invoke as any).mockImplementation(() => {
      return new Promise((res) => resolves.push(res));
    });

    // First accept should enqueue and drain will pick it (in-flight)
    const r0 = await reassemblyManager.acceptChunk('t9', 0, new Uint8Array([1]));
    expect(r0).toBe(true);

    // allow drain to start and mark inFlight
    await new Promise((r) => setTimeout(r, 0));
    let s = reassemblyManager.getState('t9')!;
    expect(s.writeInFlight).toBe(1);

    // Second accept should see backpressure and return false
    const r1 = await reassemblyManager.acceptChunk('t9', 1, new Uint8Array([2]));
    expect(r1).toBe(false);

    // Resolve first write to free capacity
    resolves[0](true);
    await new Promise((r) => setTimeout(r, 0));

    // Now accept should succeed
    const r2 = await reassemblyManager.acceptChunk('t9', 1, new Uint8Array([2]));
    expect(r2).toBe(true);

    // Cleanup: resolve any outstanding invokes
    if (resolves.length > 1) {
      resolves.slice(1).forEach((fn) => fn(true));
    }
  });

  it('accepts chunk with valid checksum', async () => {
    const { createHash } = require('crypto');
    const chunk = new Uint8Array([1, 2, 3]);
    const checksum = createHash('sha256').update(Buffer.from(chunk)).digest('hex');

    const manifest = {
      fileSize: 3,
      chunks: [{ index: 0, encryptedSize: 3, checksum }],
    };

    reassemblyManager.initReassembly('t10', manifest, '/tmp/t10');

    // Mock invoke success
    (invoke as any).mockResolvedValue(true);

    const ok = await reassemblyManager.acceptChunk('t10', 0, chunk);
    expect(ok).toBe(true);

    const state = reassemblyManager.getState('t10')!;
    expect(state.receivedChunks.includes(0)).toBe(true);
    expect(state.chunkStates[0]).toBe(ChunkState.RECEIVED);

    // ensure backend was invoked (drain runs async)
    await new Promise((r) => setTimeout(r, 0));
    expect((invoke as any).mock.calls.length).toBeGreaterThanOrEqual(1);
  });

});
