import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';
import { debounce } from '$lib/utils/debounce';

describe('debounce', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it('should delay function execution', () => {
    const mockFn = vi.fn();
    const debouncedFn = debounce(mockFn, 300);

    debouncedFn('test');
    expect(mockFn).not.toHaveBeenCalled();

    vi.advanceTimersByTime(299);
    expect(mockFn).not.toHaveBeenCalled();

    vi.advanceTimersByTime(1);
    expect(mockFn).toHaveBeenCalledTimes(1);
    expect(mockFn).toHaveBeenCalledWith('test');
  });

  it('should cancel previous call when invoked again', () => {
    const mockFn = vi.fn();
    const debouncedFn = debounce(mockFn, 300);

    debouncedFn('first');
    vi.advanceTimersByTime(100);
    
    debouncedFn('second');
    vi.advanceTimersByTime(100);
    
    debouncedFn('third');
    vi.advanceTimersByTime(300);

    expect(mockFn).toHaveBeenCalledTimes(1);
    expect(mockFn).toHaveBeenCalledWith('third');
  });

  it('should handle multiple arguments', () => {
    const mockFn = vi.fn();
    const debouncedFn = debounce(mockFn, 300);

    debouncedFn('arg1', 'arg2', 123);
    vi.advanceTimersByTime(300);

    expect(mockFn).toHaveBeenCalledTimes(1);
    expect(mockFn).toHaveBeenCalledWith('arg1', 'arg2', 123);
  });

  it('should allow multiple executions after delay', () => {
    const mockFn = vi.fn();
    const debouncedFn = debounce(mockFn, 300);

    debouncedFn('first');
    vi.advanceTimersByTime(300);
    expect(mockFn).toHaveBeenCalledTimes(1);

    debouncedFn('second');
    vi.advanceTimersByTime(300);
    expect(mockFn).toHaveBeenCalledTimes(2);

    expect(mockFn).toHaveBeenNthCalledWith(1, 'first');
    expect(mockFn).toHaveBeenNthCalledWith(2, 'second');
  });

  it('should work with zero delay', () => {
    const mockFn = vi.fn();
    const debouncedFn = debounce(mockFn, 0);

    debouncedFn('test');
    expect(mockFn).not.toHaveBeenCalled();

    vi.advanceTimersByTime(0);
    expect(mockFn).toHaveBeenCalledTimes(1);
    expect(mockFn).toHaveBeenCalledWith('test');
  });
});
