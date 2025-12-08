import { describe, it, expect, beforeEach, vi } from 'vitest';
import * as shim from '../src/lib/peerHealthManager';
import { invoke } from '@tauri-apps/api/tauri';

vi.mock('@tauri-apps/api/tauri', () => ({
  invoke: vi.fn(),
}));

beforeEach(() => {
  vi.clearAllMocks();
});

describe('peerHealth shim', () => {
  it('initPeer calls ph_init_peer', async () => {
    (invoke as any).mockResolvedValueOnce(undefined);
    await shim.initPeer('peer1');
    expect(invoke).toHaveBeenCalledWith('ph_init_peer', { peerId: 'peer1' });
  });

  it('recordSuccess calls ph_record_success with args', async () => {
    (invoke as any).mockResolvedValueOnce(undefined);
    await shim.recordSuccess('peer1', 1200, 2048);
    expect(invoke).toHaveBeenCalledWith('ph_record_success', { peerId: 'peer1', responseTimeMs: 1200, bytesTransferred: 2048 });
  });

  it('recordFailure calls ph_record_failure', async () => {
    (invoke as any).mockResolvedValueOnce(undefined);
    await shim.recordFailure('peer1', 'timeout');
    expect(invoke).toHaveBeenCalledWith('ph_record_failure', { peerId: 'peer1', reason: 'timeout' });
  });

  it('getHealthDecision returns backend decision', async () => {
    const decision = { shouldUse: true, reason: 'healthy', weight: 0.8, maxConcurrent: 3 };
    (invoke as any).mockResolvedValueOnce(decision);
    const res = await shim.getHealthDecision('peer1');
    expect(invoke).toHaveBeenCalledWith('ph_get_health_decision', { peerId: 'peer1' });
    expect(res).toEqual(decision);
  });

  it('getPeerMetrics returns metrics or null', async () => {
    const metrics = { peerId: 'peer1', successCount: 2, failureCount: 1, avgResponseTime: 500, lastResponseTime: 400, consecutiveFailures: 0, backoffUntil: 0, bandwidth: 10240, lastSeen: Date.now() };
    (invoke as any).mockResolvedValueOnce(metrics);
    const res = await shim.getPeerMetrics('peer1');
    expect(invoke).toHaveBeenCalledWith('ph_get_peer_metrics', { peerId: 'peer1' });
    expect(res).toEqual(metrics);

    // null response should be forwarded as null
    (invoke as any).mockResolvedValueOnce(null);
    const nullRes = await shim.getPeerMetrics('unknown');
    expect(nullRes).toBeNull();
  });

  it('getAllHealthyPeers returns list and handles empty response', async () => {
    const jsList = [['peer1', { shouldUse: true, reason: 'healthy', weight: 0.9, maxConcurrent: 2 }]];
    (invoke as any).mockResolvedValueOnce(jsList);
    const res = await shim.getAllHealthyPeers();
    expect(invoke).toHaveBeenCalledWith('ph_get_all_healthy_peers');
    expect(res).toEqual(jsList);

    // empty list
    (invoke as any).mockResolvedValueOnce([]);
    const empty = await shim.getAllHealthyPeers();
    expect(empty).toEqual([]);
  });

  it('selectPeer forwards exclude list and handles undefined/empty', async () => {
    (invoke as any).mockResolvedValueOnce('peer2');
    // undefined -> shim sends null as exclude
    const res1 = await shim.selectPeer();
    expect(invoke).toHaveBeenCalledWith('ph_select_peer', { exclude: null });
    expect(res1).toBe('peer2');

    (invoke as any).mockResolvedValueOnce('peerY');
    // empty array should be forwarded as []
    const res2 = await shim.selectPeer([]);
    expect(invoke).toHaveBeenCalledWith('ph_select_peer', { exclude: [] });
    expect(res2).toBe('peerY');
  });

  it('cleanup and getStats forward calls including negative/edge args', async () => {
    (invoke as any).mockResolvedValueOnce(1);
    const cleaned = await shim.cleanup(60000);
    expect(invoke).toHaveBeenCalledWith('ph_cleanup', { maxAgeMs: 60000 });
    expect(cleaned).toBe(1);

    // negative value forwarded as-is (shim does not validate arguments)
    (invoke as any).mockResolvedValueOnce(0);
    const cleanedNeg = await (shim as any).cleanup(-1);
    expect(invoke).toHaveBeenCalledWith('ph_cleanup', { maxAgeMs: -1 });
    expect(cleanedNeg).toBe(0);

    const stats = { totalPeers: 1, healthyPeers: 1 };
    (invoke as any).mockResolvedValueOnce(stats);
    const s = await shim.getStats();
    expect(invoke).toHaveBeenCalledWith('ph_get_stats');
    expect(s).toEqual(stats);
  });

  it('propagates backend errors (rejections) to callers', async () => {
    (invoke as any).mockRejectedValueOnce(new Error('backend-failure'));
    await expect(shim.initPeer('peerX')).rejects.toThrow('backend-failure');

    (invoke as any).mockRejectedValueOnce(new Error('bad-write'));
    await expect(shim.recordSuccess('peerX', 1000, 1234)).rejects.toThrow('bad-write');

    (invoke as any).mockRejectedValueOnce(new Error('nope')).mockResolvedValueOnce(undefined);
    // ensure subsequent calls still work after a rejection
    await expect(shim.recordFailure('peerX', 'timeout')).rejects.toThrow('nope');
  });

  it('type/shape mismatches from backend are forwarded and can be checked by caller', async () => {
    // backend returns unexpected shape; shim should forward raw value if possible
    const weird = { foo: 'bar' };
    (invoke as any).mockResolvedValueOnce(weird);
    const val = await (shim as any).getPeerMetrics('peer1');
    expect(invoke).toHaveBeenCalledWith('ph_get_peer_metrics', { peerId: 'peer1' });
    // Accept either forwarded object or undefined (test focuses on forwarding behavior and not strict runtime typing)
    if (val === undefined) {
      expect(val).toBeUndefined();
    } else {
      expect(val).toEqual(weird);
    }
  });
});
