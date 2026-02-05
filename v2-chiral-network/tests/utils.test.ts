import { describe, it, expect } from 'vitest';
import { cn } from '$lib/utils';

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
