import { describe, it, expect, beforeEach, vi } from 'vitest';
import * as shim from '../src/lib/transfer/chunkScheduler';
import { invoke } from '@tauri-apps/api/tauri';

// Mock the tauri invoke function so tests only verify the shim's behavior
vi.mock('@tauri-apps/api/tauri', () => ({
  invoke: vi.fn(),
}));

beforeEach(() => {
  vi.clearAllMocks();
});

describe('chunkScheduler shim', () => {
  it('initScheduler calls init_scheduler with manifest', async () => {
    const manifest = { chunks: [{ index: 0, size: 1000 }] };
    (invoke as any).mockResolvedValueOnce(undefined);

    await shim.initScheduler(manifest as any);

    expect(invoke).toHaveBeenCalledWith('init_scheduler', { manifest });
  });

  it('addPeer calls add_peer with args', async () => {
    (invoke as any).mockResolvedValueOnce(undefined);
    await shim.addPeer('peerA', 2);
    expect(invoke).toHaveBeenCalledWith('add_peer', { peerId: 'peerA', maxConcurrent: 2 });
  });

  it('removePeer calls remove_peer', async () => {
    (invoke as any).mockResolvedValueOnce(undefined);
    await shim.removePeer('peerA');
    expect(invoke).toHaveBeenCalledWith('remove_peer', { peerId: 'peerA' });
  });

  it('updatePeerHealth calls update_peer_health', async () => {
    (invoke as any).mockResolvedValueOnce(undefined);
    await shim.updatePeerHealth('peerA', true, 123);
    expect(invoke).toHaveBeenCalledWith('update_peer_health', { peerId: 'peerA', available: true, responseTimeMs: 123 });
  });

  it('onChunkReceived calls on_chunk_received', async () => {
    (invoke as any).mockResolvedValueOnce(undefined);
    await shim.onChunkReceived(5);
    expect(invoke).toHaveBeenCalledWith('on_chunk_received', { chunkIndex: 5 });
  });

  it('onChunkFailed calls on_chunk_failed with markCorrupted flag', async () => {
    (invoke as any).mockResolvedValueOnce(undefined);
    await shim.onChunkFailed(7, true);
    expect(invoke).toHaveBeenCalledWith('on_chunk_failed', { chunkIndex: 7, markCorrupted: true });
  });

  it('getNextRequests forwards result from backend', async () => {
    const now = Date.now();
    const fake: any = [
      { chunkIndex: 0, peerId: 'peerA', requestedAt: now, timeoutMs: 30000 },
    ];
    (invoke as any).mockResolvedValueOnce(fake);

    const res = await shim.getNextRequests(3);
    expect(invoke).toHaveBeenCalledWith('get_next_requests', { maxRequests: 3 });
    expect(res).toEqual(fake);
  });

  it('getPeers returns peer list from backend', async () => {
    const peers = [
      { peerId: 'p1', available: true, lastSeen: Date.now(), pendingRequests: 0, maxConcurrent: 2, avgResponseTime: 1000, failureCount: 0 },
    ];
    (invoke as any).mockResolvedValueOnce(peers);

    const res = await shim.getPeers();
    expect(invoke).toHaveBeenCalledWith('get_peers');
    expect(res).toEqual(peers);
  });
});
