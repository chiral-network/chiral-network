import { describe, it, expect } from 'vitest';
import { toHumanReadableSize, formatRelativeTime } from '../src/lib/utils';

describe('toHumanReadableSize', () => {
  describe('Basic size conversions', () => {
    it('should format 0 bytes', () => {
      expect(toHumanReadableSize(0)).toBe('0 B');
    });

    it('should format bytes without decimals', () => {
      expect(toHumanReadableSize(1)).toBe('1 B');
      expect(toHumanReadableSize(500)).toBe('500 B');
      expect(toHumanReadableSize(1023)).toBe('1023 B');
    });

    it('should format kilobytes with 1 decimal by default', () => {
      expect(toHumanReadableSize(1024)).toBe('1.0 KB');
      expect(toHumanReadableSize(1536)).toBe('1.5 KB');
      expect(toHumanReadableSize(2048)).toBe('2.0 KB');
      expect(toHumanReadableSize(10240)).toBe('10.0 KB');
    });

    it('should format megabytes with 1 decimal by default', () => {
      expect(toHumanReadableSize(1024 * 1024)).toBe('1.0 MB');
      expect(toHumanReadableSize(1.5 * 1024 * 1024)).toBe('1.5 MB');
      expect(toHumanReadableSize(100 * 1024 * 1024)).toBe('100.0 MB');
    });

    it('should format gigabytes with 1 decimal by default', () => {
      expect(toHumanReadableSize(1024 * 1024 * 1024)).toBe('1.0 GB');
      expect(toHumanReadableSize(2.5 * 1024 * 1024 * 1024)).toBe('2.5 GB');
      expect(toHumanReadableSize(500 * 1024 * 1024 * 1024)).toBe('500.0 GB');
    });

    it('should format terabytes with 1 decimal by default', () => {
      expect(toHumanReadableSize(1024 * 1024 * 1024 * 1024)).toBe('1.0 TB');
      expect(toHumanReadableSize(5.25 * 1024 * 1024 * 1024 * 1024)).toBe('5.3 TB');
    });

    it('should format petabytes with 1 decimal by default', () => {
      expect(toHumanReadableSize(1024 * 1024 * 1024 * 1024 * 1024)).toBe('1.0 PB');
      expect(toHumanReadableSize(3.7 * 1024 * 1024 * 1024 * 1024 * 1024)).toBe('3.7 PB');
    });

    it('should cap at petabytes for extremely large values', () => {
      const hugeValue = 10000 * 1024 * 1024 * 1024 * 1024 * 1024;
      const result = toHumanReadableSize(hugeValue);
      expect(result).toContain('PB');
      expect(result).not.toContain('EB');
    });
  });

  describe('Custom fraction digits', () => {
    it('should respect custom fractionDigits parameter', () => {
      expect(toHumanReadableSize(1536, 0)).toBe('2 KB');
      expect(toHumanReadableSize(1536, 2)).toBe('1.50 KB');
      expect(toHumanReadableSize(1536, 3)).toBe('1.500 KB');
    });

    it('should use 0 decimals for bytes even with fractionDigits set', () => {
      expect(toHumanReadableSize(500, 2)).toBe('500 B');
      expect(toHumanReadableSize(1023, 5)).toBe('1023 B');
    });

    it('should handle fractionDigits = 0 for larger units', () => {
      expect(toHumanReadableSize(1.5 * 1024 * 1024, 0)).toBe('2 MB');
      expect(toHumanReadableSize(2.9 * 1024 * 1024, 0)).toBe('3 MB');
    });

    it('should handle large fractionDigits values', () => {
      expect(toHumanReadableSize(1024, 10)).toBe('1.0000000000 KB');
    });
  });

  describe('Edge cases and invalid inputs', () => {
    it('should handle negative numbers', () => {
      expect(toHumanReadableSize(-100)).toBe('0 B');
      expect(toHumanReadableSize(-1024)).toBe('0 B');
    });

    it('should handle NaN', () => {
      expect(toHumanReadableSize(NaN)).toBe('0 B');
    });

    it('should handle Infinity', () => {
      expect(toHumanReadableSize(Infinity)).toBe('0 B');
      expect(toHumanReadableSize(-Infinity)).toBe('0 B');
    });

    it('should handle very small positive numbers', () => {
      expect(toHumanReadableSize(0.1)).toBe('0 B');
      expect(toHumanReadableSize(0.9)).toBe('1 B');
    });

    it('should handle decimal byte values', () => {
      expect(toHumanReadableSize(1.5)).toBe('2 B');
      expect(toHumanReadableSize(999.9)).toBe('1000 B');
    });
  });

  describe('Boundary values', () => {
    it('should handle exact unit boundaries', () => {
      expect(toHumanReadableSize(1024)).toBe('1.0 KB');
      expect(toHumanReadableSize(1024 * 1024)).toBe('1.0 MB');
      expect(toHumanReadableSize(1024 * 1024 * 1024)).toBe('1.0 GB');
    });

    it('should handle values just below unit boundaries', () => {
      expect(toHumanReadableSize(1023)).toBe('1023 B');
      expect(toHumanReadableSize(1024 * 1024 - 1)).toBe('1024.0 KB');
    });

    it('should handle values just above unit boundaries', () => {
      expect(toHumanReadableSize(1025)).toBe('1.0 KB');
      expect(toHumanReadableSize(1024 * 1024 + 1)).toBe('1.0 MB');
    });
  });

  describe('Real-world file sizes', () => {
    it('should format common file sizes correctly', () => {
      // Small text file
      expect(toHumanReadableSize(4096)).toBe('4.0 KB');
      
      // Image file
      expect(toHumanReadableSize(2.5 * 1024 * 1024)).toBe('2.5 MB');
      
      // Video file
      expect(toHumanReadableSize(750 * 1024 * 1024)).toBe('750.0 MB');
      
      // HD movie
      expect(toHumanReadableSize(4.7 * 1024 * 1024 * 1024)).toBe('4.7 GB');
      
      // Large dataset
      expect(toHumanReadableSize(2 * 1024 * 1024 * 1024 * 1024)).toBe('2.0 TB');
    });
  });
});

describe('formatRelativeTime', () => {
  // Helper to get a timestamp N milliseconds from now
  const msFromNow = (ms: number) => Date.now() + ms;
  const msAgo = (ms: number) => Date.now() - ms;

  describe('Seconds', () => {
    it('should format times less than a minute in seconds', () => {
      expect(formatRelativeTime(msFromNow(5000))).toBe('in 5 seconds');
      expect(formatRelativeTime(msAgo(10000))).toBe('10 seconds ago');
      expect(formatRelativeTime(msAgo(30000))).toBe('30 seconds ago');
      expect(formatRelativeTime(msAgo(59000))).toBe('59 seconds ago');
    });

    it('should handle "just now" cases', () => {
      expect(formatRelativeTime(Date.now())).toBe('now');
      expect(formatRelativeTime(msAgo(500))).toBe('now');
      expect(formatRelativeTime(msFromNow(500))).toBe('in 1 second');
    });

    it('should use "1 second" (singular) appropriately', () => {
      expect(formatRelativeTime(msAgo(1000))).toBe('1 second ago');
      expect(formatRelativeTime(msFromNow(1000))).toBe('in 1 second');
    });
  });

  describe('Minutes', () => {
    it('should format times in minutes when >= 1 minute and < 1 hour', () => {
      expect(formatRelativeTime(msAgo(60000))).toBe('1 minute ago');
      expect(formatRelativeTime(msAgo(120000))).toBe('2 minutes ago');
      expect(formatRelativeTime(msAgo(30 * 60000))).toBe('30 minutes ago');
      expect(formatRelativeTime(msAgo(59 * 60000))).toBe('59 minutes ago');
    });

    it('should format future times in minutes', () => {
      expect(formatRelativeTime(msFromNow(5 * 60000))).toBe('in 5 minutes');
      expect(formatRelativeTime(msFromNow(15 * 60000))).toBe('in 15 minutes');
    });
  });

  describe('Hours', () => {
    it('should format times in hours when >= 1 hour and < 1 day', () => {
      expect(formatRelativeTime(msAgo(60 * 60000))).toBe('1 hour ago');
      expect(formatRelativeTime(msAgo(2 * 60 * 60000))).toBe('2 hours ago');
      expect(formatRelativeTime(msAgo(12 * 60 * 60000))).toBe('12 hours ago');
      expect(formatRelativeTime(msAgo(23 * 60 * 60000))).toBe('23 hours ago');
    });

    it('should format future times in hours', () => {
      expect(formatRelativeTime(msFromNow(3 * 60 * 60000))).toBe('in 3 hours');
      expect(formatRelativeTime(msFromNow(6 * 60 * 60000))).toBe('in 6 hours');
    });
  });

  describe('Days', () => {
    it('should format times in days when >= 1 day and < 1 week', () => {
      expect(formatRelativeTime(msAgo(24 * 60 * 60000))).toBe('yesterday');
      expect(formatRelativeTime(msAgo(2 * 24 * 60 * 60000))).toBe('2 days ago');
      expect(formatRelativeTime(msAgo(6 * 24 * 60 * 60000))).toBe('6 days ago');
    });

    it('should format future times in days', () => {
      expect(formatRelativeTime(msFromNow(24 * 60 * 60000))).toBe('tomorrow');
      expect(formatRelativeTime(msFromNow(2 * 24 * 60 * 60000))).toBe('in 2 days');
      expect(formatRelativeTime(msFromNow(5 * 24 * 60 * 60000))).toBe('in 5 days');
    });
  });

  describe('Weeks', () => {
    it('should format times in weeks when >= 1 week and < 1 month', () => {
      expect(formatRelativeTime(msAgo(7 * 24 * 60 * 60000))).toBe('last week');
      expect(formatRelativeTime(msAgo(14 * 24 * 60 * 60000))).toBe('2 weeks ago');
      expect(formatRelativeTime(msAgo(21 * 24 * 60 * 60000))).toBe('3 weeks ago');
    });

    it('should format future times in weeks', () => {
      expect(formatRelativeTime(msFromNow(7 * 24 * 60 * 60000))).toBe('next week');
      expect(formatRelativeTime(msFromNow(14 * 24 * 60 * 60000))).toBe('in 2 weeks');
    });
  });

  describe('Months', () => {
    it('should format times in months when >= 1 month and < 1 year', () => {
      expect(formatRelativeTime(msAgo(30 * 24 * 60 * 60000))).toBe('last month');
      expect(formatRelativeTime(msAgo(60 * 24 * 60 * 60000))).toBe('2 months ago');
      expect(formatRelativeTime(msAgo(180 * 24 * 60 * 60000))).toBe('6 months ago');
    });

    it('should format future times in months', () => {
      expect(formatRelativeTime(msFromNow(30 * 24 * 60 * 60000))).toBe('next month');
      expect(formatRelativeTime(msFromNow(90 * 24 * 60 * 60000))).toBe('in 3 months');
    });
  });

  describe('Years', () => {
    it('should format times in years when >= 1 year', () => {
      expect(formatRelativeTime(msAgo(365 * 24 * 60 * 60000))).toBe('last year');
      expect(formatRelativeTime(msAgo(730 * 24 * 60 * 60000))).toBe('2 years ago');
      expect(formatRelativeTime(msAgo(5 * 365 * 24 * 60 * 60000))).toBe('5 years ago');
    });

    it('should format future times in years', () => {
      expect(formatRelativeTime(msFromNow(365 * 24 * 60 * 60000))).toBe('next year');
      expect(formatRelativeTime(msFromNow(730 * 24 * 60 * 60000))).toBe('in 2 years');
    });
  });

  describe('Date object input', () => {
    it('should accept Date objects', () => {
      const fiveMinutesAgo = new Date(Date.now() - 5 * 60000);
      expect(formatRelativeTime(fiveMinutesAgo)).toBe('5 minutes ago');
    });

    it('should handle Date objects for various time ranges', () => {
      const yesterday = new Date(Date.now() - 24 * 60 * 60000);
      expect(formatRelativeTime(yesterday)).toBe('yesterday');

      const nextWeek = new Date(Date.now() + 7 * 24 * 60 * 60000);
      expect(formatRelativeTime(nextWeek)).toBe('next week');
    });
  });

  describe('Number input (timestamp handling)', () => {
    it('should handle millisecond timestamps (> 1e12)', () => {
      const msTimestamp = Date.now() - 3600000; // 1 hour ago
      expect(formatRelativeTime(msTimestamp)).toBe('1 hour ago');
    });

    it('should handle second timestamps (< 1e12) by converting to ms', () => {
      const secondTimestamp = Math.floor(Date.now() / 1000) - 3600; // 1 hour ago in seconds
      expect(formatRelativeTime(secondTimestamp)).toBe('1 hour ago');
    });

    it('should auto-detect and handle timestamp scale confusion', () => {
      // If a very large diff is detected, it tries alternate scale
      const veryOldSeconds = 1000000000; // Year 2001 in seconds
      const result = formatRelativeTime(veryOldSeconds);
      // Should try to correct the scale and give a reasonable result
      expect(result).toBeTruthy();
    });
  });

  describe('Edge cases and invalid inputs', () => {
    it('should handle NaN gracefully', () => {
      expect(formatRelativeTime(NaN)).toBe('now');
    });

    it('should handle Infinity gracefully', () => {
      expect(formatRelativeTime(Infinity)).toBe('now');
      expect(formatRelativeTime(-Infinity)).toBe('now');
    });

    it('should handle very large future dates', () => {
      const farFuture = Date.now() + (100 * 365 * 24 * 60 * 60000);
      const result = formatRelativeTime(farFuture);
      expect(result).toContain('years');
      expect(result).toContain('in');
    });

    it('should handle very old past dates', () => {
      const farPast = Date.now() - (50 * 365 * 24 * 60 * 60000);
      const result = formatRelativeTime(farPast);
      expect(result).toContain('years');
      expect(result).toContain('ago');
    });

    it('should handle zero as current time when interpreted as seconds', () => {
      // 0 as seconds (< 1e12) gets converted to 0ms, which is epoch start
      // This will be very far in the past
      const result = formatRelativeTime(0);
      expect(result).toContain('ago');
    });
  });

  describe('Boundary conditions', () => {
    it('should handle exact minute boundary', () => {
      expect(formatRelativeTime(msAgo(60000))).toBe('1 minute ago');
    });

    it('should handle exact hour boundary', () => {
      expect(formatRelativeTime(msAgo(3600000))).toBe('1 hour ago');
    });

    it('should handle exact day boundary', () => {
      expect(formatRelativeTime(msAgo(86400000))).toBe('yesterday');
    });

    it('should handle transition between units', () => {
      // Just under 1 minute should be in seconds
      expect(formatRelativeTime(msAgo(59999))).toBe('60 seconds ago');
      
      // Just at 1 minute should be in minutes
      expect(formatRelativeTime(msAgo(60000))).toBe('1 minute ago');
    });
  });

  describe('Real-world scenarios', () => {
    it('should format recent activity timestamps', () => {
      expect(formatRelativeTime(msAgo(30000))).toBe('30 seconds ago');
      
      expect(formatRelativeTime(msAgo(5 * 60000))).toBe('5 minutes ago');
      
      expect(formatRelativeTime(msAgo(2 * 3600000))).toBe('2 hours ago');
    });

    it('should format scheduled future events', () => {
      expect(formatRelativeTime(msFromNow(15 * 60000))).toBe('in 15 minutes');
      
      expect(formatRelativeTime(msFromNow(3 * 86400000))).toBe('in 3 days');
      
      expect(formatRelativeTime(msFromNow(14 * 86400000))).toBe('in 2 weeks');
    });
  });
});
