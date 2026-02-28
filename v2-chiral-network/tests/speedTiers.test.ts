import { describe, it, expect } from 'vitest';
import {
  calculateCost,
  formatCost,
  getTierConfig,
  formatSpeed,
  TIERS,
  type SpeedTier,
} from '$lib/speedTiers';

describe('speedTiers', () => {
  describe('TIERS constant', () => {
    it('should have exactly 3 tiers', () => {
      expect(TIERS).toHaveLength(3);
    });

    it('should have standard, premium, and ultra tiers in order', () => {
      expect(TIERS[0].id).toBe('standard');
      expect(TIERS[1].id).toBe('premium');
      expect(TIERS[2].id).toBe('ultra');
    });

    it('standard tier should have 1 MB/s speed limit', () => {
      expect(TIERS[0].speedLimit).toBe(1024 * 1024);
    });

    it('premium tier should have 5 MB/s speed limit', () => {
      expect(TIERS[1].speedLimit).toBe(5 * 1024 * 1024);
    });

    it('ultra tier should have unlimited (0) speed limit', () => {
      expect(TIERS[2].speedLimit).toBe(0);
    });

    it('standard tier should cost 0.001 CHI per MB', () => {
      expect(TIERS[0].costPerMb).toBe(0.001);
    });

    it('premium tier should cost 0.005 CHI per MB', () => {
      expect(TIERS[1].costPerMb).toBe(0.005);
    });

    it('ultra tier should cost 0.01 CHI per MB', () => {
      expect(TIERS[2].costPerMb).toBe(0.01);
    });
  });

  describe('getTierConfig', () => {
    it('should return correct config for standard tier', () => {
      const config = getTierConfig('standard');
      expect(config.name).toBe('Standard');
      expect(config.speedLabel).toBe('1 MB/s');
    });

    it('should return correct config for premium tier', () => {
      const config = getTierConfig('premium');
      expect(config.name).toBe('Premium');
      expect(config.speedLabel).toBe('5 MB/s');
    });

    it('should return correct config for ultra tier', () => {
      const config = getTierConfig('ultra');
      expect(config.name).toBe('Ultra');
      expect(config.speedLabel).toBe('Unlimited');
    });
  });

  describe('calculateCost', () => {
    it('should calculate standard tier cost: 10 MB = 0.01 CHI', () => {
      const cost = calculateCost('standard', 10_000_000);
      expect(cost).toBeCloseTo(0.01, 6);
    });

    it('should calculate premium tier cost: 10 MB = 0.05 CHI', () => {
      const cost = calculateCost('premium', 10_000_000);
      expect(cost).toBeCloseTo(0.05, 6);
    });

    it('should calculate ultra tier cost: 10 MB = 0.1 CHI', () => {
      const cost = calculateCost('ultra', 10_000_000);
      expect(cost).toBeCloseTo(0.1, 6);
    });

    it('should return 0 for zero bytes on any tier', () => {
      expect(calculateCost('standard', 0)).toBe(0);
      expect(calculateCost('premium', 0)).toBe(0);
      expect(calculateCost('ultra', 0)).toBe(0);
    });

    it('should handle 1 byte file (tiny non-zero cost for paid tiers)', () => {
      const stdCost = calculateCost('standard', 1);
      expect(stdCost).toBeGreaterThan(0);
      expect(stdCost).toBeLessThan(0.000001);
    });

    it('should scale linearly with file size', () => {
      const cost1 = calculateCost('standard', 100_000_000); // 100 MB
      const cost2 = calculateCost('standard', 200_000_000); // 200 MB
      expect(cost2).toBeCloseTo(cost1 * 2, 6);
    });

    it('should handle large file (1 GB) correctly', () => {
      const cost = calculateCost('premium', 1_000_000_000);
      // 1000 MB * 0.005 = 5 CHI
      expect(cost).toBeCloseTo(5.0, 6);
    });
  });

  describe('formatCost', () => {
    it('should return "0 CHI" for zero cost', () => {
      expect(formatCost(0)).toBe('0 CHI');
    });

    it('should return "< 0.000001 CHI" for very tiny amounts', () => {
      expect(formatCost(0.0000001)).toBe('< 0.000001 CHI');
      expect(formatCost(0.00000001)).toBe('< 0.000001 CHI');
    });

    it('should format normal cost with trailing zeros trimmed', () => {
      expect(formatCost(0.001)).toBe('0.001 CHI');
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

    it('should handle exact 6 decimal places', () => {
      const result = formatCost(0.123456);
      expect(result).toBe('0.123456 CHI');
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

    it('should format fractional KB', () => {
      expect(formatSpeed(1536)).toBe('1.5 KB/s');
    });

    it('should format at MB boundary', () => {
      expect(formatSpeed(1024 * 1024)).toBe('1.0 MB/s');
    });

    it('should format fractional MB', () => {
      expect(formatSpeed(1.5 * 1024 * 1024)).toBe('1.5 MB/s');
    });

    it('should format just under KB boundary as bytes', () => {
      expect(formatSpeed(1023)).toBe('1023 B/s');
    });
  });
});
