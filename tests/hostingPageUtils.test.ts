import { describe, it, expect } from 'vitest';
import {
  formatHostedFileSize,
  formatHostedTimeAgo,
  buildHostedSiteUrl,
  buildHostedLocalUrl,
  getTotalHostedSiteSize,
  resolveHostingPort,
} from '$lib/utils/hostingPageUtils';

describe('hostingPageUtils', () => {
  describe('formatHostedFileSize', () => {
    it('formats byte values across units', () => {
      expect(formatHostedFileSize(0)).toBe('0 B');
      expect(formatHostedFileSize(1023)).toBe('1023 B');
      expect(formatHostedFileSize(1024)).toBe('1 KB');
      expect(formatHostedFileSize(5 * 1024 * 1024)).toBe('5 MB');
      expect(formatHostedFileSize(3 * 1024 * 1024 * 1024)).toBe('3 GB');
    });
  });

  describe('formatHostedTimeAgo', () => {
    it('formats relative age in seconds/minutes/hours/days', () => {
      const now = 1_700_000_000;
      expect(formatHostedTimeAgo(now - 10, now)).toBe('just now');
      expect(formatHostedTimeAgo(now - 180, now)).toBe('3m ago');
      expect(formatHostedTimeAgo(now - 7200, now)).toBe('2h ago');
      expect(formatHostedTimeAgo(now - 172800, now)).toBe('2d ago');
    });
  });

  describe('buildHostedSiteUrl', () => {
    it('prefers relay URL when published', () => {
      const url = buildHostedSiteUrl('site-1', 'https://relay.example/site-1', '127.0.0.1:8080', 8080);
      expect(url).toBe('https://relay.example/site-1');
    });

    it('falls back to localhost URL when relay URL is missing', () => {
      const url = buildHostedSiteUrl('site-2', null, '10.0.0.3:9090', 8080);
      expect(url).toBe('http://localhost:9090/sites/site-2/');
    });
  });

  describe('buildHostedLocalUrl', () => {
    it('uses running server address when available', () => {
      expect(buildHostedLocalUrl('127.0.0.1:8080', 9000)).toBe('http://127.0.0.1:8080');
    });

    it('uses fallback localhost port when server is not running', () => {
      expect(buildHostedLocalUrl(null, 9000)).toBe('http://localhost:9000');
    });
  });

  describe('getTotalHostedSiteSize', () => {
    it('sums all file sizes', () => {
      expect(
        getTotalHostedSiteSize([
          { size: 100 },
          { size: 2500 },
          { size: 400 },
        ]),
      ).toBe(3000);
    });
  });

  describe('resolveHostingPort', () => {
    it('returns valid saved port', () => {
      expect(resolveHostingPort('9419')).toBe(9419);
    });

    it('falls back for empty/invalid/out-of-range values', () => {
      expect(resolveHostingPort(null)).toBe(8080);
      expect(resolveHostingPort('')).toBe(8080);
      expect(resolveHostingPort('abc')).toBe(8080);
      expect(resolveHostingPort('0')).toBe(8080);
      expect(resolveHostingPort('70000')).toBe(8080);
    });
  });
});
