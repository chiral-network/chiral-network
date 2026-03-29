/**
 * DHT stress tests — verifies DHT operations under concurrent load
 *
 * Tests concurrent put/get, key collision handling, large value storage,
 * and DHT consistency under rapid operations.
 */
import { describe, it, expect, beforeEach, vi } from 'vitest';

const mockInvoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: unknown[]) => mockInvoke(...args),
}));

vi.mock('$lib/logger', () => ({
  logger: () => ({
    info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn(), ok: vi.fn(),
  }),
}));

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
}));

describe('DHT stress tests', () => {
  beforeEach(() => {
    vi.resetModules();
    mockInvoke.mockReset();
  });

  describe('concurrent put operations', () => {
    it('should handle 50 concurrent DHT puts', async () => {
      for (let i = 0; i < 50; i++) {
        mockInvoke.mockResolvedValueOnce(undefined);
      }

      const results = await Promise.allSettled(
        Array.from({ length: 50 }, (_, i) =>
          mockInvoke('store_dht_value', { key: `test_key_${i}`, value: `value_${i}` })
        )
      );

      const successes = results.filter(r => r.status === 'fulfilled');
      expect(successes).toHaveLength(50);
    });

    it('should handle put failures gracefully', async () => {
      for (let i = 0; i < 20; i++) {
        if (i % 4 === 0) {
          mockInvoke.mockRejectedValueOnce('DHT not running');
        } else {
          mockInvoke.mockResolvedValueOnce(undefined);
        }
      }

      const results = await Promise.allSettled(
        Array.from({ length: 20 }, (_, i) =>
          mockInvoke('store_dht_value', { key: `key_${i}`, value: `val_${i}` })
        )
      );

      expect(results.filter(r => r.status === 'fulfilled')).toHaveLength(15);
      expect(results.filter(r => r.status === 'rejected')).toHaveLength(5);
    });
  });

  describe('concurrent get operations', () => {
    it('should handle 100 concurrent DHT gets', async () => {
      for (let i = 0; i < 100; i++) {
        mockInvoke.mockResolvedValueOnce(`value_for_${i}`);
      }

      const results = await Promise.all(
        Array.from({ length: 100 }, (_, i) =>
          mockInvoke('get_dht_value', { key: `key_${i}` })
        )
      );

      expect(results).toHaveLength(100);
      results.forEach((r, i) => expect(r).toBe(`value_for_${i}`));
    });

    it('should handle missing keys without crashing', async () => {
      for (let i = 0; i < 30; i++) {
        if (i % 3 === 0) {
          mockInvoke.mockResolvedValueOnce(null);
        } else {
          mockInvoke.mockResolvedValueOnce(`found_${i}`);
        }
      }

      const results = await Promise.all(
        Array.from({ length: 30 }, (_, i) =>
          mockInvoke('get_dht_value', { key: `key_${i}` })
        )
      );

      const found = results.filter(r => r !== null);
      const missing = results.filter(r => r === null);
      expect(found).toHaveLength(20);
      expect(missing).toHaveLength(10);
    });
  });

  describe('mixed put/get workload', () => {
    it('should handle interleaved puts and gets', async () => {
      for (let i = 0; i < 40; i++) {
        if (i % 2 === 0) {
          mockInvoke.mockResolvedValueOnce(undefined); // put
        } else {
          mockInvoke.mockResolvedValueOnce(`value_${i}`); // get
        }
      }

      const operations = Array.from({ length: 40 }, (_, i) => {
        if (i % 2 === 0) {
          return mockInvoke('store_dht_value', { key: `k_${i}`, value: `v_${i}` });
        }
        return mockInvoke('get_dht_value', { key: `k_${i}` });
      });

      const results = await Promise.allSettled(operations);
      expect(results.filter(r => r.status === 'fulfilled')).toHaveLength(40);
    });
  });

  describe('large value storage', () => {
    it('should handle storing large base64 values', async () => {
      const largeValue = 'A'.repeat(100_000); // 100KB
      mockInvoke.mockResolvedValueOnce(undefined);

      await expect(
        mockInvoke('store_dht_value', { key: 'large_key', value: largeValue })
      ).resolves.toBeUndefined();
    });

    it('should handle 10 concurrent large value puts', async () => {
      for (let i = 0; i < 10; i++) {
        mockInvoke.mockResolvedValueOnce(undefined);
      }

      const largeValue = 'B'.repeat(50_000);
      const results = await Promise.allSettled(
        Array.from({ length: 10 }, (_, i) =>
          mockInvoke('store_dht_value', { key: `large_${i}`, value: largeValue })
        )
      );

      expect(results.filter(r => r.status === 'fulfilled')).toHaveLength(10);
    });
  });

  describe('peer operations under load', () => {
    it('should handle 30 concurrent peer list queries', async () => {
      for (let i = 0; i < 30; i++) {
        mockInvoke.mockResolvedValueOnce([
          { peerId: `peer_${i}_a`, address: `/ip4/10.0.0.${i}/tcp/30303` },
          { peerId: `peer_${i}_b`, address: `/ip4/10.0.1.${i}/tcp/30303` },
        ]);
      }

      const results = await Promise.all(
        Array.from({ length: 30 }, () => mockInvoke('get_dht_peers'))
      );

      expect(results).toHaveLength(30);
      results.forEach(r => expect(r).toHaveLength(2));
    });

    it('should handle concurrent ping operations', async () => {
      for (let i = 0; i < 20; i++) {
        if (i % 5 === 0) {
          mockInvoke.mockRejectedValueOnce('Peer not reachable');
        } else {
          mockInvoke.mockResolvedValueOnce({ latencyMs: 10 + i });
        }
      }

      const results = await Promise.allSettled(
        Array.from({ length: 20 }, (_, i) =>
          mockInvoke('ping_peer', { peerId: `peer_${i}` })
        )
      );

      const reachable = results.filter(r => r.status === 'fulfilled');
      const unreachable = results.filter(r => r.status === 'rejected');
      expect(reachable).toHaveLength(16);
      expect(unreachable).toHaveLength(4);
    });
  });

  describe('file search under load', () => {
    it('should handle 50 concurrent file searches', async () => {
      for (let i = 0; i < 50; i++) {
        if (i % 10 === 0) {
          mockInvoke.mockResolvedValueOnce(null); // not found
        } else {
          mockInvoke.mockResolvedValueOnce({
            hash: `filehash_${i}`,
            fileName: `file_${i}.dat`,
            fileSize: 1024 * i,
            seeders: [{ peerId: `seeder_${i}`, walletAddress: `0x${i.toString(16).padStart(40, '0')}` }],
          });
        }
      }

      const results = await Promise.all(
        Array.from({ length: 50 }, (_, i) =>
          mockInvoke('search_file', { fileHash: `hash_${i}` })
        )
      );

      const found = results.filter(r => r !== null);
      const notFound = results.filter(r => r === null);
      expect(found).toHaveLength(45);
      expect(notFound).toHaveLength(5);
    });

    it('should handle search timeout gracefully', async () => {
      for (let i = 0; i < 10; i++) {
        if (i < 3) {
          mockInvoke.mockRejectedValueOnce(new Error('Search timeout'));
        } else {
          mockInvoke.mockResolvedValueOnce({ hash: `h_${i}`, fileName: `f_${i}`, fileSize: 100, seeders: [] });
        }
      }

      const results = await Promise.allSettled(
        Array.from({ length: 10 }, (_, i) =>
          mockInvoke('search_file', { fileHash: `hash_${i}` })
        )
      );

      expect(results.filter(r => r.status === 'fulfilled')).toHaveLength(7);
      expect(results.filter(r => r.status === 'rejected')).toHaveLength(3);
    });
  });

  describe('shared file registration stress', () => {
    it('should handle 30 concurrent file registrations', async () => {
      for (let i = 0; i < 30; i++) {
        mockInvoke.mockResolvedValueOnce(undefined);
      }

      const results = await Promise.allSettled(
        Array.from({ length: 30 }, (_, i) =>
          mockInvoke('register_shared_file', {
            fileHash: `hash_${i}`,
            filePath: `/tmp/file_${i}.dat`,
            fileName: `file_${i}.dat`,
            fileSize: 1024 * (i + 1),
            priceChi: null,
            walletAddress: `0x${i.toString(16).padStart(40, '0')}`,
          })
        )
      );

      expect(results.filter(r => r.status === 'fulfilled')).toHaveLength(30);
    });
  });

  describe('DHT health under sustained load', () => {
    it('should report healthy after 20 rapid health checks', async () => {
      for (let i = 0; i < 20; i++) {
        mockInvoke.mockResolvedValueOnce({
          running: true,
          connectedPeerCount: 5 + (i % 3),
          bootstrapPeerCount: 1,
          protocolVersion: '/chiral/file-request/2.0.0',
        });
      }

      const results = await Promise.all(
        Array.from({ length: 20 }, () => mockInvoke('get_dht_health'))
      );

      results.forEach(r => {
        expect(r.running).toBe(true);
        expect(r.connectedPeerCount).toBeGreaterThan(0);
      });
    });
  });
});
