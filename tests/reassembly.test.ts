// Mock the Tauri invoke function before importing modules that use it
import { vi } from 'vitest';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

import { describe, it, expect, beforeEach } from 'vitest';
import { reassemblyManager } from '../src/lib/transfer/reassembly';

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
    expect(state!.receivedChunks.has(0)).toBe(true);
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
    expect(state!.corruptedChunks.has(0)).toBe(true);
  });

  it('finalize calls backend verify_and_finalize', async () => {
    const manifest = {
      fileSize: 200,
      chunks: [{ index: 0, encryptedSize: 100 }, { index: 1, encryptedSize: 100 }],
    };

    reassemblyManager.initReassembly('t4', manifest, '/tmp/t4');

    // Pretend both chunks received
    const state = reassemblyManager.getState('t4')!;
    state.receivedChunks.add(0);
    state.receivedChunks.add(1);

    (invoke as any).mockResolvedValue({ ok: true });

    const ok = await reassemblyManager.finalize('t4', '/final/path');
    expect(ok).toBe(true);

    const after = reassemblyManager.getState('t4');
    expect(after).toBeNull();
  });
});
