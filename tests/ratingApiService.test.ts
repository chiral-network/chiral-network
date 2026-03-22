import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

// Mock fetch globally
const mockFetch = vi.fn();
vi.stubGlobal('fetch', mockFetch);

describe('ratingApiService', () => {
  beforeEach(() => {
    vi.resetModules();
    mockFetch.mockReset();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe('setRatingOwner', () => {
    it('should set the owner for subsequent requests', async () => {
      const { setRatingOwner, ratingApi } = await import('$lib/services/ratingApiService');
      setRatingOwner('0xABCDEF1234567890ABCDEF1234567890ABCDEF12');

      mockFetch.mockResolvedValueOnce({
        ok: true,
        headers: new Headers({ 'content-type': 'application/json' }),
        json: async () => ({ wallet: '0xtest', elo: 50, events: [] }),
      });

      await ratingApi.getReputation('0xtest');

      expect(mockFetch).toHaveBeenCalledOnce();
      const [, init] = mockFetch.mock.calls[0];
      expect(init.headers['X-Owner']).toBe('0xABCDEF1234567890ABCDEF1234567890ABCDEF12');
    });

    it('should not include X-Owner header when owner is empty', async () => {
      const { setRatingOwner, ratingApi } = await import('$lib/services/ratingApiService');
      setRatingOwner('');

      mockFetch.mockResolvedValueOnce({
        ok: true,
        headers: new Headers({ 'content-type': 'application/json' }),
        json: async () => ({ wallet: '0xtest', elo: 50, events: [] }),
      });

      await ratingApi.getReputation('0xtest');
      const [, init] = mockFetch.mock.calls[0];
      expect(init.headers['X-Owner']).toBeUndefined();
    });
  });

  describe('recordTransferOutcome', () => {
    it('should POST to /api/ratings/transfer', async () => {
      const { ratingApi, setRatingOwner } = await import('$lib/services/ratingApiService');
      setRatingOwner('0xowner');

      mockFetch.mockResolvedValueOnce({
        ok: true,
        headers: new Headers({ 'content-type': 'application/json' }),
        json: async () => ({
          id: 'evt-1',
          transferId: 't-1',
          seederWallet: '0xseeder',
          downloaderWallet: '0xowner',
          fileHash: 'abc123',
          amountWei: '1000',
          outcome: 'completed',
          createdAt: 1700000000,
          updatedAt: 1700000000,
        }),
      });

      const result = await ratingApi.recordTransferOutcome(
        't-1', '0xseeder', 'abc123', 'completed', '1000', '0xtxhash'
      );

      const [url, init] = mockFetch.mock.calls[0];
      expect(url).toContain('/api/ratings/transfer');
      expect(init.method).toBe('POST');
      const body = JSON.parse(init.body);
      expect(body.transferId).toBe('t-1');
      expect(body.seederWallet).toBe('0xseeder');
      expect(body.outcome).toBe('completed');
      expect(body.txHash).toBe('0xtxhash');
    });

    it('should send null txHash when not provided', async () => {
      const { ratingApi, setRatingOwner } = await import('$lib/services/ratingApiService');
      setRatingOwner('0xowner');

      mockFetch.mockResolvedValueOnce({
        ok: true,
        headers: new Headers({ 'content-type': 'application/json' }),
        json: async () => ({ id: 'evt-1', outcome: 'failed' }),
      });

      await ratingApi.recordTransferOutcome('t-1', '0xseeder', 'abc', 'failed');
      const body = JSON.parse(mockFetch.mock.calls[0][1].body);
      expect(body.txHash).toBeNull();
      expect(body.amountWei).toBe('0');
    });

    it('should throw on HTTP error', async () => {
      const { ratingApi } = await import('$lib/services/ratingApiService');

      mockFetch.mockResolvedValueOnce({
        ok: false,
        status: 400,
        statusText: 'Bad Request',
        text: async () => 'transferId is required',
      });

      await expect(
        ratingApi.recordTransferOutcome('', '0xseeder', 'abc', 'completed')
      ).rejects.toThrow('transferId is required');
    });
  });

  describe('getReputation', () => {
    it('should GET reputation and normalize response', async () => {
      const { ratingApi } = await import('$lib/services/ratingApiService');

      mockFetch.mockResolvedValueOnce({
        ok: true,
        headers: new Headers({ 'content-type': 'application/json' }),
        json: async () => ({
          wallet: '0xwallet',
          elo: 72.5,
          base_elo: 50,
          completed_count: 10,
          failed_count: 2,
          transaction_count: 12,
          total_earned_wei: '5000000000000000000',
          events: [
            {
              id: 'e-1',
              transfer_id: 't-1',
              seeder_wallet: '0xwallet',
              downloader_wallet: '0xother',
              file_hash: 'abc',
              amount_wei: '1000000000000000000',
              outcome: 'completed',
              created_at: 1700000000,
              updated_at: 1700000000,
            },
          ],
        }),
      });

      const result = await ratingApi.getReputation('0xwallet');

      expect(result.wallet).toBe('0xwallet');
      expect(result.elo).toBe(72.5);
      expect(result.baseElo).toBe(50);
      expect(result.completedCount).toBe(10);
      expect(result.failedCount).toBe(2);
      expect(result.transactionCount).toBe(12);
      expect(result.totalEarnedWei).toBe('5000000000000000000');
      expect(result.events).toHaveLength(1);
      expect(result.events[0].transferId).toBe('t-1');
      expect(result.events[0].seederWallet).toBe('0xwallet');
    });

    it('should handle missing fields with defaults', async () => {
      const { ratingApi } = await import('$lib/services/ratingApiService');

      mockFetch.mockResolvedValueOnce({
        ok: true,
        headers: new Headers({ 'content-type': 'application/json' }),
        json: async () => ({ wallet: '0xwallet' }),
      });

      const result = await ratingApi.getReputation('0xwallet');
      expect(result.elo).toBe(50);
      expect(result.completedCount).toBe(0);
      expect(result.events).toHaveLength(0);
    });

    it('should encode wallet address in URL', async () => {
      const { ratingApi } = await import('$lib/services/ratingApiService');

      mockFetch.mockResolvedValueOnce({
        ok: true,
        headers: new Headers({ 'content-type': 'application/json' }),
        json: async () => ({ wallet: '0xtest', events: [] }),
      });

      await ratingApi.getReputation('0xABC');
      expect(mockFetch.mock.calls[0][0]).toContain('/api/ratings/0xABC');
    });
  });

  describe('getBatchReputation', () => {
    it('should POST wallets and return reputation map', async () => {
      const { ratingApi } = await import('$lib/services/ratingApiService');

      mockFetch.mockResolvedValueOnce({
        ok: true,
        headers: new Headers({ 'content-type': 'application/json' }),
        json: async () => ({
          reputations: {
            '0xA': { elo: 60, completedCount: 5, failedCount: 1, transactionCount: 6, totalEarnedWei: '100' },
            '0xB': { elo: 45, completedCount: 2, failedCount: 3, transactionCount: 5, totalEarnedWei: '50' },
          },
        }),
      });

      const result = await ratingApi.getBatchReputation(['0xA', '0xB']);
      expect(Object.keys(result)).toHaveLength(2);
      expect(result['0xA'].elo).toBe(60);
      expect(result['0xB'].elo).toBe(45);
    });

    it('should return empty object when response has no reputations', async () => {
      const { ratingApi } = await import('$lib/services/ratingApiService');

      mockFetch.mockResolvedValueOnce({
        ok: true,
        headers: new Headers({ 'content-type': 'application/json' }),
        json: async () => ({}),
      });

      const result = await ratingApi.getBatchReputation(['0xA']);
      expect(result).toEqual({});
    });
  });
});
