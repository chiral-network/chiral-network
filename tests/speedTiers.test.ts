import { describe, it, expect } from 'vitest';
import { calculateCost, formatCost, formatSpeed } from '$lib/speedTiers';

describe('speedTiers', () => {
  describe('calculateCost', () => {
    it('should calculate cost: 10 MB = 0.01 CHI', () => {
      const cost = calculateCost(10_000_000);
      expect(cost).toBeCloseTo(0.01, 6);
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
      expect(cost).toBeLessThan(0.000001);
    });

    it('should scale linearly with file size', () => {
      const cost1 = calculateCost(100_000_000); // 100 MB
      const cost2 = calculateCost(200_000_000); // 200 MB
      expect(cost2).toBeCloseTo(cost1 * 2, 6);
    });

    it('should handle large file (1 GB) correctly', () => {
      const cost = calculateCost(1_000_000_000);
      // 1000 MB * 0.001 = 1 CHI
      expect(cost).toBeCloseTo(1.0, 6);
    });

    it('should cost 0.001 CHI per MB', () => {
      const cost = calculateCost(1_000_000); // 1 MB
      expect(cost).toBeCloseTo(0.001, 6);
    });
  });

  describe('formatCost', () => {
    it('should return "Free" for zero cost', () => {
      expect(formatCost(0)).toBe('Free');
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
