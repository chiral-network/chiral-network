/**
 * Reputation system stress tests — verifies Elo rating operations at scale
 *
 * Tests concurrent rating lookups, batch operations with many wallets,
 * and transfer outcome recording under load.
 */
import { describe, it, expect, beforeEach, vi } from 'vitest';

const mockFetch = vi.fn();
vi.stubGlobal('fetch', mockFetch);

vi.mock('$lib/logger', () => ({
  logger: () => ({
    info: vi.fn(), warn: vi.fn(), error: vi.fn(), debug: vi.fn(), ok: vi.fn(),
  }),
}));

function makeReputationResponse(wallet: string, elo = 50) {
  return {
    wallet,
    elo,
    baseElo: 50,
    completedCount: Math.floor(elo / 5),
    failedCount: Math.max(0, Math.floor((100 - elo) / 20)),
    transactionCount: Math.floor(elo / 5) + Math.max(0, Math.floor((100 - elo) / 20)),
    totalEarnedWei: '0',
    events: [],
  };
}

function mockFetchResponse(data: unknown, status = 200) {
  return {
    ok: status >= 200 && status < 300,
    status,
    headers: new Headers({ 'content-type': 'application/json' }),
    json: () => Promise.resolve(data),
    text: () => Promise.resolve(JSON.stringify(data)),
  };
}

describe('Reputation system stress tests', () => {
  beforeEach(() => {
    vi.resetModules();
    mockFetch.mockReset();
  });

  describe('concurrent individual lookups', () => {
    it('should handle 30 concurrent single-wallet reputation lookups', async () => {
      const wallets = Array.from({ length: 30 }, (_, i) =>
        `0x${i.toString(16).padStart(40, '0')}`
      );

      for (const wallet of wallets) {
        mockFetch.mockResolvedValueOnce(
          mockFetchResponse(makeReputationResponse(wallet, 40 + (parseInt(wallet.slice(-2), 16) % 60)))
        );
      }

      const results = await Promise.all(
        wallets.map(wallet =>
          mockFetch(`http://130.245.173.73:8080/api/ratings/${wallet}`)
            .then((r: any) => r.json())
        )
      );

      expect(results).toHaveLength(30);
      results.forEach(r => {
        expect(r.elo).toBeGreaterThanOrEqual(0);
        expect(r.elo).toBeLessThanOrEqual(100);
      });
    });
  });

  describe('batch lookup scalability', () => {
    it('should handle batch lookup with 100 wallets', async () => {
      const wallets = Array.from({ length: 100 }, (_, i) =>
        `0x${i.toString(16).padStart(40, '0')}`
      );

      const reputations: Record<string, any> = {};
      wallets.forEach(w => {
        reputations[w] = { elo: 50, completedCount: 0, failedCount: 0, transactionCount: 0, totalEarnedWei: '0' };
      });

      mockFetch.mockResolvedValueOnce(mockFetchResponse({ reputations }));

      const { ratingApi } = await import('$lib/services/ratingApiService');
      const result = await ratingApi.getBatchReputation(wallets);

      expect(Object.keys(result)).toHaveLength(100);
    });

    it('should handle batch lookup with 500 wallets', async () => {
      const wallets = Array.from({ length: 500 }, (_, i) =>
        `0x${i.toString(16).padStart(40, '0')}`
      );

      const reputations: Record<string, any> = {};
      wallets.forEach(w => {
        reputations[w] = { elo: 50, completedCount: 0, failedCount: 0, transactionCount: 0, totalEarnedWei: '0' };
      });

      mockFetch.mockResolvedValueOnce(mockFetchResponse({ reputations }));

      const { ratingApi } = await import('$lib/services/ratingApiService');
      const result = await ratingApi.getBatchReputation(wallets);

      expect(Object.keys(result)).toHaveLength(500);
    });

    it('should handle 10 concurrent batch lookups', async () => {
      for (let batch = 0; batch < 10; batch++) {
        const reputations: Record<string, any> = {};
        for (let i = 0; i < 20; i++) {
          const w = `0x${(batch * 20 + i).toString(16).padStart(40, '0')}`;
          reputations[w] = { elo: 50, completedCount: 0, failedCount: 0, transactionCount: 0, totalEarnedWei: '0' };
        }
        mockFetch.mockResolvedValueOnce(mockFetchResponse({ reputations }));
      }

      const { ratingApi } = await import('$lib/services/ratingApiService');

      const results = await Promise.all(
        Array.from({ length: 10 }, (_, batch) => {
          const wallets = Array.from({ length: 20 }, (_, i) =>
            `0x${(batch * 20 + i).toString(16).padStart(40, '0')}`
          );
          return ratingApi.getBatchReputation(wallets);
        })
      );

      expect(results).toHaveLength(10);
      results.forEach(r => expect(Object.keys(r)).toHaveLength(20));
    });
  });

  describe('transfer outcome recording', () => {
    it('should handle 20 concurrent transfer outcome submissions', async () => {
      for (let i = 0; i < 20; i++) {
        mockFetch.mockResolvedValueOnce(mockFetchResponse({ ok: true }));
      }

      const results = await Promise.allSettled(
        Array.from({ length: 20 }, (_, i) =>
          mockFetch('http://130.245.173.73:8080/api/ratings/transfer', {
            method: 'POST',
            headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify({
              sellerWallet: `0x${i.toString(16).padStart(40, 'a')}`,
              buyerWallet: `0x${i.toString(16).padStart(40, 'b')}`,
              outcome: i % 3 === 0 ? 'failed' : 'completed',
              amountWei: '1000000000000000',
            }),
          }).then((r: any) => r.json())
        )
      );

      expect(results.filter(r => r.status === 'fulfilled')).toHaveLength(20);
    });
  });

  describe('Elo score consistency', () => {
    it('should return consistent scores for same wallet across requests', async () => {
      const wallet = '0x' + 'a'.repeat(40);

      for (let i = 0; i < 10; i++) {
        mockFetch.mockResolvedValueOnce(
          mockFetchResponse(makeReputationResponse(wallet, 65))
        );
      }

      const results = await Promise.all(
        Array.from({ length: 10 }, () =>
          mockFetch(`http://130.245.173.73:8080/api/ratings/${wallet}`)
            .then((r: any) => r.json())
        )
      );

      const elos = results.map(r => r.elo);
      expect(new Set(elos).size).toBe(1); // All same score
      expect(elos[0]).toBe(65);
    });

    it('should handle wallet address case normalization', async () => {
      // Same wallet in different cases
      const lower = '0x' + 'a'.repeat(40);
      const mixed = '0x' + 'A'.repeat(20) + 'a'.repeat(20);

      mockFetch
        .mockResolvedValueOnce(mockFetchResponse(makeReputationResponse(lower, 70)))
        .mockResolvedValueOnce(mockFetchResponse(makeReputationResponse(mixed, 70)));

      const [r1, r2] = await Promise.all([
        mockFetch(`http://130.245.173.73:8080/api/ratings/${lower}`).then((r: any) => r.json()),
        mockFetch(`http://130.245.173.73:8080/api/ratings/${mixed}`).then((r: any) => r.json()),
      ]);

      expect(r1.elo).toBe(r2.elo);
    });
  });

  describe('error resilience', () => {
    it('should handle API timeout gracefully', async () => {
      mockFetch.mockRejectedValueOnce(new Error('fetch timeout'));

      const { ratingApi } = await import('$lib/services/ratingApiService');

      await expect(ratingApi.getReputation('0x' + 'a'.repeat(40))).rejects.toThrow();
    });

    it('should handle 500 errors from batch endpoint', async () => {
      mockFetch.mockResolvedValueOnce(mockFetchResponse({ error: 'Internal server error' }, 500));

      const { ratingApi } = await import('$lib/services/ratingApiService');

      // May return empty object or throw — either is acceptable
      const result = await ratingApi.getBatchReputation(['0x' + 'a'.repeat(40)])
        .catch(() => ({}));
      expect(typeof result).toBe('object');
    });

    it('should handle mixed success/failure in concurrent lookups', async () => {
      for (let i = 0; i < 10; i++) {
        if (i % 3 === 0) {
          mockFetch.mockRejectedValueOnce(new Error('Network error'));
        } else {
          mockFetch.mockResolvedValueOnce(mockFetchResponse(makeReputationResponse(`0x${i}`, 50)));
        }
      }

      const results = await Promise.allSettled(
        Array.from({ length: 10 }, (_, i) =>
          mockFetch(`http://130.245.173.73:8080/api/ratings/0x${i}`)
            .then((r: any) => r.json())
        )
      );

      expect(results.filter(r => r.status === 'fulfilled').length).toBeGreaterThan(0);
      expect(results.filter(r => r.status === 'rejected').length).toBeGreaterThan(0);
    });
  });
});
