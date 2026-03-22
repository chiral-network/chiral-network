import { describe, it, expect } from 'vitest';
import { cn, formatBytes, formatPriceWei } from '$lib/utils';

describe('cn (class name utility)', () => {
  it('should merge simple class names', () => {
    expect(cn('foo', 'bar')).toBe('foo bar');
  });

  it('should handle conditional classes with clsx syntax', () => {
    expect(cn('base', true && 'active')).toBe('base active');
    expect(cn('base', false && 'active')).toBe('base');
  });

  it('should merge conflicting Tailwind classes', () => {
    // tailwind-merge should resolve conflicts
    expect(cn('p-2', 'p-4')).toBe('p-4');
    expect(cn('text-red-500', 'text-blue-500')).toBe('text-blue-500');
  });

  it('should handle array inputs', () => {
    expect(cn(['foo', 'bar'])).toBe('foo bar');
  });

  it('should handle object inputs', () => {
    expect(cn({ foo: true, bar: false, baz: true })).toBe('foo baz');
  });

  it('should handle undefined and null inputs', () => {
    expect(cn('foo', undefined, null, 'bar')).toBe('foo bar');
  });

  it('should handle empty string', () => {
    expect(cn('')).toBe('');
  });

  it('should handle no arguments', () => {
    expect(cn()).toBe('');
  });

  it('should handle mixed Tailwind utilities', () => {
    expect(cn('bg-red-500 text-white', 'bg-blue-500')).toBe('text-white bg-blue-500');
  });

  it('should handle responsive prefixes', () => {
    expect(cn('md:p-2', 'md:p-4')).toBe('md:p-4');
  });

  it('should handle dark mode prefixes', () => {
    expect(cn('dark:bg-gray-800', 'dark:bg-gray-900')).toBe('dark:bg-gray-900');
  });
});

describe('formatBytes', () => {
  it('should return "0 B" for 0', () => {
    expect(formatBytes(0)).toBe('0 B');
  });

  it('should return "0 B" for null/undefined', () => {
    expect(formatBytes(null)).toBe('0 B');
    expect(formatBytes(undefined)).toBe('0 B');
  });

  it('should format bytes', () => {
    expect(formatBytes(500)).toBe('500 B');
    expect(formatBytes(1)).toBe('1 B');
  });

  it('should format kilobytes', () => {
    expect(formatBytes(1024)).toBe('1 KB');
    expect(formatBytes(1536)).toBe('1.5 KB');
  });

  it('should format megabytes', () => {
    expect(formatBytes(1048576)).toBe('1 MB');
    expect(formatBytes(1572864)).toBe('1.5 MB');
  });

  it('should format gigabytes', () => {
    expect(formatBytes(1073741824)).toBe('1 GB');
  });

  it('should format terabytes', () => {
    expect(formatBytes(1099511627776)).toBe('1 TB');
  });

  it('should round to 2 decimal places', () => {
    expect(formatBytes(1288490189)).toBe('1.2 GB');
  });
});

describe('formatPriceWei', () => {
  it('should return "Free" for "0"', () => {
    expect(formatPriceWei('0')).toBe('Free');
  });

  it('should return "Free" for empty/null/undefined', () => {
    expect(formatPriceWei('')).toBe('Free');
    expect(formatPriceWei(null)).toBe('Free');
    expect(formatPriceWei(undefined)).toBe('Free');
  });

  it('should format 1 CHI (1e18 wei)', () => {
    expect(formatPriceWei('1000000000000000000')).toBe('1 CHI');
  });

  it('should format whole CHI amounts', () => {
    expect(formatPriceWei('5000000000000000000')).toBe('5 CHI');
    expect(formatPriceWei('100000000000000000000')).toBe('100 CHI');
  });

  it('should format fractional CHI', () => {
    expect(formatPriceWei('500000000000000000')).toBe('0.5 CHI');
    expect(formatPriceWei('2500000000000000000')).toBe('2.5 CHI');
  });

  it('should format very small amounts', () => {
    const result = formatPriceWei('1');
    expect(result).toContain('CHI');
    expect(result).not.toBe('Free');
  });

  it('should truncate to 6 decimal places max', () => {
    const result = formatPriceWei('1000000000');
    expect(result).toMatch(/^0\.\d{1,6} CHI$/);
  });

  it('should return "Free" for invalid input', () => {
    expect(formatPriceWei('not_a_number')).toBe('Free');
  });
});
