import { describe, it, expect } from 'vitest';
import {
  calculateCost,
  calculateCostWei,
  formatCost,
  formatSpeed,
  LAUNCH_DOWNLOAD_COST_PER_MB_WEI,
  PLATFORM_FEE_BPS,
  PLATFORM_FEE_PERCENT,
  calculatePlatformFee,
  calculatePlatformFeeWei,
  splitPaymentWei,
} from '$lib/speedTiers';

describe('speedTiers', () => {
  describe('calculateCost', () => {
    it('should calculate cost: 10 MB = 0.1 CHI', () => {
      const cost = calculateCost(10_000_000);
      expect(cost).toBeCloseTo(0.1, 6);
    });

    it('should return 0 for zero bytes', () => {
      expect(calculateCost(0)).toBe(0);
    });

    it('should return 0 for negative bytes', () => {
      expect(calculateCost(-100)).toBe(0);
    });

    it('should handle 1 byte file (tiny non-zero cost)', () => {
      const cost = calculateCost(1);
      expect(cost).toBeGreaterThan(0);
      expect(cost).toBeLessThan(0.00001);
      expect(calculateCostWei(1)).toBe(10_000_000_000n);
    });

    it('should scale linearly with file size', () => {
      const cost1 = calculateCost(100_000_000); // 100 MB
      const cost2 = calculateCost(200_000_000); // 200 MB
      expect(cost2).toBeCloseTo(cost1 * 2, 6);
    });

    it('should handle large file (1 GB) correctly', () => {
      const cost = calculateCost(1_000_000_000);
      // 1000 MB * 0.01 = 10 CHI
      expect(cost).toBeCloseTo(10.0, 6);
    });

    it('should cost 0.01 CHI per MB', () => {
      const cost = calculateCost(1_000_000); // 1 MB
      expect(cost).toBeCloseTo(0.01, 6);
      expect(calculateCostWei(1_000_000)).toBe(BigInt(LAUNCH_DOWNLOAD_COST_PER_MB_WEI));
    });

    it('should handle 500 MB file', () => {
      const cost = calculateCost(500_000_000);
      expect(cost).toBeCloseTo(5.0, 6);
    });
  });

  describe('platformFee', () => {
    it('should be 0.5%', () => {
      expect(PLATFORM_FEE_BPS).toBe(50);
      expect(PLATFORM_FEE_PERCENT).toBe(0.5);
    });

    it('should calculate 0.5% fee on 1 CHI', () => {
      const fee = calculatePlatformFee(1.0);
      expect(fee).toBeCloseTo(0.005, 6);
    });

    it('should calculate 0.5% fee on 100 CHI', () => {
      const fee = calculatePlatformFee(100.0);
      expect(fee).toBeCloseTo(0.5, 6);
    });

    it('should calculate 0.5% fee on 0.01 CHI', () => {
      const fee = calculatePlatformFee(0.01);
      expect(fee).toBeCloseTo(0.00005, 6);
    });

    it('should return 0 fee for 0 amount', () => {
      expect(calculatePlatformFee(0)).toBe(0);
      expect(calculatePlatformFeeWei(0n)).toBe(0n);
    });

    it('should split the listed price instead of adding markup', () => {
      const totalWei = 1_000_000_000_000_000_000n;
      const split = splitPaymentWei(totalWei);
      expect(split.platformFeeWei).toBe(5_000_000_000_000_000n);
      expect(split.sellerWei).toBe(995_000_000_000_000_000n);
      expect(split.sellerWei + split.platformFeeWei).toBe(totalWei);
    });

    it('should round fee up and preserve the split invariant for tiny totals', () => {
      expect(splitPaymentWei(0n)).toEqual({ sellerWei: 0n, platformFeeWei: 0n });
      expect(splitPaymentWei(1n)).toEqual({ sellerWei: 0n, platformFeeWei: 1n });
    });

    it('should split download cost for 100 MB', () => {
      const totalWei = calculateCostWei(100_000_000); // 100 MB = 1.0 CHI
      const split = splitPaymentWei(totalWei);
      expect(totalWei).toBe(1_000_000_000_000_000_000n);
      expect(split.platformFeeWei).toBe(5_000_000_000_000_000n);
      expect(split.sellerWei + split.platformFeeWei).toBe(totalWei);
    });

    it('should split download cost for 1 GB', () => {
      const totalWei = calculateCostWei(1_000_000_000); // 1 GB = 10 CHI
      const split = splitPaymentWei(totalWei);
      expect(split.platformFeeWei).toBe(50_000_000_000_000_000n);
      expect(split.sellerWei + split.platformFeeWei).toBe(totalWei);
    });

    it('fee should always be less than base cost', () => {
      for (const amount of [0.001, 0.01, 0.1, 1, 10, 100, 1000]) {
        const fee = calculatePlatformFee(amount);
        expect(fee).toBeLessThan(amount);
      }
    });
  });

  describe('formatCost', () => {
    it('should return "Free" for zero cost', () => {
      expect(formatCost(0)).toBe('Free');
    });

    it('should return "< 0.000001 CHI" for very tiny amounts', () => {
      expect(formatCost(0.0000001)).toBe('< 0.000001 CHI');
    });

    it('should format normal cost with trailing zeros trimmed', () => {
      expect(formatCost(0.01)).toBe('0.01 CHI');
    });

    it('should format larger costs correctly', () => {
      expect(formatCost(1.5)).toBe('1.5 CHI');
    });

    it('should trim trailing zeros', () => {
      expect(formatCost(0.010000)).toBe('0.01 CHI');
    });

    it('should trim trailing dot after zero removal', () => {
      expect(formatCost(1.0)).toBe('1 CHI');
    });
  });

  describe('formatSpeed', () => {
    it('should format zero speed', () => {
      expect(formatSpeed(0)).toBe('0 B/s');
    });

    it('should format bytes range', () => {
      expect(formatSpeed(512)).toBe('512 B/s');
    });

    it('should format at KB boundary', () => {
      expect(formatSpeed(1024)).toBe('1.0 KB/s');
    });

    it('should format at MB boundary', () => {
      expect(formatSpeed(1024 * 1024)).toBe('1.0 MB/s');
    });
  });
});
