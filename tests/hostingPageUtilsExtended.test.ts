import { describe, it, expect } from 'vitest';
import {
  formatPeerId,
  formatWeiAsChi,
  weiToChiNumber,
  chiToWeiString,
  formatDuration,
  timeRemaining,
  statusColor,
} from '$lib/utils/hostingPageUtils';

describe('hostingPageUtils (extended)', () => {
  describe('formatPeerId', () => {
    it('returns short IDs unchanged', () => {
      expect(formatPeerId('abc123')).toBe('abc123');
      expect(formatPeerId('exactly16chars!!')).toBe('exactly16chars!!');
    });

    it('truncates long IDs with ellipsis', () => {
      const long = '12QmYyQSo1JGpFHR3AR8DR4DMFA2hRpE1bkeb6hDeFnPi6';
      const result = formatPeerId(long);
      expect(result).toBe('12QmYyQS...eFnPi6');
      expect(result.length).toBeLessThan(long.length);
    });
  });

  describe('formatWeiAsChi', () => {
    it('returns Free for zero wei', () => {
      expect(formatWeiAsChi('0')).toBe('Free');
    });

    it('returns < 0.000001 CHI for very small amounts', () => {
      expect(formatWeiAsChi('1000')).toBe('< 0.000001 CHI');
    });

    it('formats large amounts correctly', () => {
      // 1 CHI = 1e18 wei
      expect(formatWeiAsChi('1000000000000000000')).toBe('1 CHI');
      expect(formatWeiAsChi('2500000000000000000')).toBe('2.5 CHI');
    });

    it('returns Free for invalid input', () => {
      expect(formatWeiAsChi('not-a-number')).toBe('Free');
      expect(formatWeiAsChi('')).toBe('Free');
    });
  });

  describe('weiToChiNumber', () => {
    it('converts valid wei to CHI number', () => {
      expect(weiToChiNumber('1000000000000000000', 0)).toBe(1);
      expect(weiToChiNumber('500000000000000000', 0)).toBe(0.5);
    });

    it('returns fallback for invalid input', () => {
      expect(weiToChiNumber('garbage', 99)).toBe(99);
      // Note: BigInt('') returns 0n, so empty string yields 0, not fallback
      expect(weiToChiNumber('', 42)).toBe(0);
    });
  });

  describe('chiToWeiString', () => {
    it('converts CHI to wei string', () => {
      expect(chiToWeiString(1, '0')).toBe('1000000000000000000');
      expect(chiToWeiString(0.5, '0')).toBe('500000000000000000');
    });

    it('roundtrips with weiToChiNumber', () => {
      const wei = chiToWeiString(2.5, '0');
      const chi = weiToChiNumber(wei, 0);
      expect(chi).toBe(2.5);
    });

    it('returns fallback for negative values', () => {
      expect(chiToWeiString(-1, '999')).toBe('999');
    });

    it('returns fallback for non-finite values', () => {
      expect(chiToWeiString(Infinity, '0')).toBe('0');
      expect(chiToWeiString(NaN, '0')).toBe('0');
    });
  });

  describe('formatDuration', () => {
    it('formats days', () => {
      expect(formatDuration(86400)).toBe('1 day');
      expect(formatDuration(86400 * 5)).toBe('5 days');
    });

    it('formats months', () => {
      expect(formatDuration(86400 * 30)).toBe('1.0 months');
      expect(formatDuration(86400 * 60)).toBe('2.0 months');
    });

    it('formats years', () => {
      expect(formatDuration(86400 * 365)).toBe('1.0 years');
      expect(formatDuration(86400 * 730)).toBe('2.0 years');
    });
  });

  describe('timeRemaining', () => {
    it('returns N/A when expiresAt is undefined', () => {
      expect(timeRemaining(undefined)).toBe('N/A');
    });

    it('returns N/A when expiresAt is 0', () => {
      expect(timeRemaining(0)).toBe('N/A');
    });

    it('returns Expired when past expiry', () => {
      const pastTimestamp = Math.floor(Date.now() / 1000) - 1000;
      expect(timeRemaining(pastTimestamp)).toBe('Expired');
    });

    it('returns formatted duration for future expiry', () => {
      const futureTimestamp = Math.floor(Date.now() / 1000) + 86400 * 10;
      const result = timeRemaining(futureTimestamp);
      expect(result).toMatch(/\d+ days?/);
    });
  });

  describe('statusColor', () => {
    it('returns blue classes for proposed', () => {
      expect(statusColor('proposed')).toContain('blue');
    });

    it('returns green classes for accepted', () => {
      expect(statusColor('accepted')).toContain('green');
    });

    it('returns emerald classes for active', () => {
      expect(statusColor('active')).toContain('emerald');
    });

    it('returns red classes for rejected', () => {
      expect(statusColor('rejected')).toContain('red');
    });

    it('returns gray classes for expired', () => {
      expect(statusColor('expired')).toContain('gray');
    });

    it('returns orange classes for cancelled', () => {
      expect(statusColor('cancelled')).toContain('orange');
    });

    it('returns gray classes for unknown status', () => {
      expect(statusColor('something-else')).toContain('gray');
    });
  });
});
