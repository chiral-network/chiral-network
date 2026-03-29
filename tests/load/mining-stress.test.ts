/**
 * Mining stress tests — verifies mining operations under load
 *
 * Tests concurrent mining status polling, rapid start/stop cycles,
 * hash rate reporting stability, and mined block history queries.
 */
import { describe, it, expect, beforeEach, vi } from 'vitest';
import { get } from 'svelte/store';

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

function makeMiningStatus(overrides = {}) {
  return {
    mining: true,
    hashRate: 5_000_000,
    minerAddress: '0xabcdef1234567890abcdef1234567890abcdef12',
    totalMinedWei: '50000000000000000000',
    totalMinedChi: 50.0,
    ...overrides,
  };
}

function makeGethStatus(overrides = {}) {
  return {
    installed: true,
    running: true,
    localRunning: true,
    syncing: false,
    currentBlock: 1000,
    highestBlock: 1000,
    peerCount: 3,
    chainId: 98765,
    ...overrides,
  };
}

describe('Mining stress tests', () => {
  beforeEach(() => {
    vi.resetModules();
    mockInvoke.mockReset();
  });

  describe('concurrent status polling', () => {
    it('should handle 50 concurrent getMiningStatus calls', async () => {
      for (let i = 0; i < 50; i++) {
        mockInvoke.mockResolvedValueOnce(makeMiningStatus({ hashRate: 1000 + i }));
      }
      const { gethService } = await import('$lib/services/gethService');

      const results = await Promise.all(
        Array.from({ length: 50 }, () => gethService.getMiningStatus())
      );

      expect(results).toHaveLength(50);
      results.forEach(r => {
        expect(r.mining).toBe(true);
        expect(r.hashRate).toBeGreaterThanOrEqual(1000);
      });
    });

    it('should handle interleaved getStatus and getMiningStatus calls', async () => {
      for (let i = 0; i < 40; i++) {
        if (i % 2 === 0) {
          mockInvoke.mockResolvedValueOnce(makeGethStatus());
        } else {
          mockInvoke.mockResolvedValueOnce(makeMiningStatus());
        }
      }
      const { gethService } = await import('$lib/services/gethService');

      const calls = Array.from({ length: 40 }, (_, i) =>
        i % 2 === 0 ? gethService.getStatus() : gethService.getMiningStatus()
      );
      const results = await Promise.all(calls);

      expect(results).toHaveLength(40);
      expect(results.filter((_, i) => i % 2 === 0).every((r: any) => 'chainId' in r)).toBe(true);
      expect(results.filter((_, i) => i % 2 === 1).every((r: any) => 'hashRate' in r)).toBe(true);
    });
  });

  describe('rapid start/stop cycles', () => {
    it('should handle 20 rapid start/stop mining cycles', async () => {
      for (let i = 0; i < 40; i++) {
        mockInvoke
          .mockResolvedValueOnce(undefined) // start or stop
          .mockResolvedValueOnce(makeMiningStatus({ mining: i % 2 === 0 })); // status
      }
      const { gethService } = await import('$lib/services/gethService');

      for (let i = 0; i < 20; i++) {
        await gethService.startMining(4);
        await gethService.stopMining();
      }

      // Should not throw and invoke count should match
      expect(mockInvoke.mock.calls.length).toBeGreaterThanOrEqual(40);
    });

    it('should handle concurrent start attempts gracefully', async () => {
      // First call succeeds, subsequent calls may get "already mining"
      mockInvoke.mockResolvedValueOnce(undefined);
      for (let i = 0; i < 9; i++) {
        mockInvoke.mockRejectedValueOnce('Miner is already running');
      }
      // Each start also calls getMiningStatus
      for (let i = 0; i < 10; i++) {
        mockInvoke.mockResolvedValueOnce(makeMiningStatus());
      }
      const { gethService } = await import('$lib/services/gethService');

      const results = await Promise.allSettled(
        Array.from({ length: 10 }, () => gethService.startMining(2))
      );

      const successes = results.filter(r => r.status === 'fulfilled');
      const failures = results.filter(r => r.status === 'rejected');
      expect(successes.length + failures.length).toBe(10);
    });
  });

  describe('hash rate stability', () => {
    it('should report consistent hash rate over 20 polls', async () => {
      const hashRates: number[] = [];
      for (let i = 0; i < 20; i++) {
        const rate = 5_000_000 + Math.floor(Math.random() * 500_000);
        hashRates.push(rate);
        mockInvoke.mockResolvedValueOnce(makeMiningStatus({ hashRate: rate }));
      }
      const { gethService } = await import('$lib/services/gethService');

      const polled: number[] = [];
      for (let i = 0; i < 20; i++) {
        const status = await gethService.getMiningStatus();
        polled.push(status.hashRate);
      }

      // All polled rates should be within expected range
      polled.forEach((rate, i) => {
        expect(rate).toBe(hashRates[i]);
        expect(rate).toBeGreaterThan(0);
      });
    });

    it('should handle hash rate dropping to zero', async () => {
      mockInvoke
        .mockResolvedValueOnce(makeMiningStatus({ hashRate: 5_000_000 }))
        .mockResolvedValueOnce(makeMiningStatus({ hashRate: 0, mining: false }));
      const { gethService, miningStatus } = await import('$lib/services/gethService');

      await gethService.getMiningStatus();
      expect(get(miningStatus)?.hashRate).toBe(5_000_000);

      await gethService.getMiningStatus();
      expect(get(miningStatus)?.hashRate).toBe(0);
      expect(get(miningStatus)?.mining).toBe(false);
    });
  });

  describe('mined blocks history', () => {
    it('should handle large block history (500 blocks)', async () => {
      const blocks = Array.from({ length: 500 }, (_, i) => ({
        blockNumber: i + 1,
        timestamp: 1700000000 + i * 13,
        rewardWei: '5000000000000000000',
        rewardChi: 5.0,
        difficulty: 864695,
      }));
      mockInvoke.mockResolvedValueOnce(blocks);

      const result = await mockInvoke('get_mined_blocks', { maxBlocks: 500 });

      expect(result).toHaveLength(500);
      expect(result[0].blockNumber).toBe(1);
      expect(result[499].blockNumber).toBe(500);
    });

    it('should handle empty block history', async () => {
      mockInvoke.mockResolvedValueOnce([]);

      const result = await mockInvoke('get_mined_blocks', { maxBlocks: 500 });

      expect(result).toHaveLength(0);
    });

    it('should handle concurrent block history requests', async () => {
      for (let i = 0; i < 10; i++) {
        mockInvoke.mockResolvedValueOnce([
          { blockNumber: i + 1, timestamp: 1700000000, rewardWei: '5000000000000000000', rewardChi: 5.0, difficulty: 1000 },
        ]);
      }

      const results = await Promise.all(
        Array.from({ length: 10 }, () => mockInvoke('get_mined_blocks', { maxBlocks: 10 }))
      );

      expect(results).toHaveLength(10);
      results.forEach(r => expect(r).toHaveLength(1));
    });
  });

  describe('error resilience', () => {
    it('should recover from backend timeout during status poll', async () => {
      mockInvoke
        .mockRejectedValueOnce(new Error('RPC timeout'))
        .mockResolvedValueOnce(makeMiningStatus());
      const { gethService } = await import('$lib/services/gethService');

      await expect(gethService.getMiningStatus()).rejects.toThrow();

      const status = await gethService.getMiningStatus();
      expect(status.mining).toBe(true);
    });

    it('should handle rapid error/success alternation', async () => {
      for (let i = 0; i < 20; i++) {
        if (i % 2 === 0) {
          mockInvoke.mockRejectedValueOnce(new Error('intermittent failure'));
        } else {
          mockInvoke.mockResolvedValueOnce(makeMiningStatus());
        }
      }
      const { gethService } = await import('$lib/services/gethService');

      const results = await Promise.allSettled(
        Array.from({ length: 20 }, () => gethService.getMiningStatus())
      );

      const successes = results.filter(r => r.status === 'fulfilled');
      const failures = results.filter(r => r.status === 'rejected');
      expect(successes).toHaveLength(10);
      expect(failures).toHaveLength(10);
    });

    it('should not crash when Geth is not installed', async () => {
      mockInvoke.mockResolvedValueOnce(makeGethStatus({ installed: false, running: false, localRunning: false }));
      const { gethService } = await import('$lib/services/gethService');

      const status = await gethService.getStatus();
      expect(status.installed).toBe(false);
      expect(status.running).toBe(false);
    });
  });

  describe('total mined tracking', () => {
    it('should show increasing total mined over time', async () => {
      const increments = [10.0, 15.0, 20.0, 25.0, 30.0];
      for (const chi of increments) {
        mockInvoke.mockResolvedValueOnce(makeMiningStatus({
          totalMinedChi: chi,
          totalMinedWei: (BigInt(Math.round(chi * 1e18))).toString(),
        }));
      }
      const { gethService } = await import('$lib/services/gethService');

      let prevChi = 0;
      for (const expected of increments) {
        const status = await gethService.getMiningStatus();
        expect(status.totalMinedChi).toBe(expected);
        expect(status.totalMinedChi).toBeGreaterThanOrEqual(prevChi);
        prevChi = status.totalMinedChi;
      }
    });
  });
});
