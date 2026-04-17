import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { withTimeout } from '../src/lib/utils/withTimeout';

describe('withTimeout', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('resolves with the inner promise value when it settles in time', async () => {
    const p = Promise.resolve('ok');
    await expect(withTimeout(p, 1000, 'test')).resolves.toBe('ok');
  });

  it('rejects with the inner error when the inner promise rejects first', async () => {
    const p = Promise.reject(new Error('inner failure'));
    await expect(withTimeout(p, 1000, 'test')).rejects.toThrow('inner failure');
  });

  it('rejects with a descriptive timeout error when the timer fires first', async () => {
    const p = new Promise(() => {}); // never settles
    const pending = withTimeout(p, 200, 'search');
    vi.advanceTimersByTime(200);
    await expect(pending).rejects.toThrow('search timed out after 200ms');
  });

  it('clears the pending timer when the inner promise resolves (no stray callback)', async () => {
    let resolver!: (v: string) => void;
    const p = new Promise<string>((res) => { resolver = res; });

    const clearSpy = vi.spyOn(globalThis, 'clearTimeout');
    const pending = withTimeout(p, 5000, 'test');

    resolver('done');
    await expect(pending).resolves.toBe('done');
    // Without the finally-block clear, the 5s timer would still be queued.
    expect(clearSpy).toHaveBeenCalled();
  });

  it('clears the pending timer even when the inner promise rejects', async () => {
    let rejector!: (e: Error) => void;
    const p = new Promise<string>((_, rej) => { rejector = rej; });

    const clearSpy = vi.spyOn(globalThis, 'clearTimeout');
    const pending = withTimeout(p, 5000, 'test');

    rejector(new Error('boom'));
    await expect(pending).rejects.toThrow('boom');
    expect(clearSpy).toHaveBeenCalled();
  });

  it('includes the operation name in the timeout error message', async () => {
    const p = new Promise(() => {});
    const pending = withTimeout(p, 100, 'downloading file X');
    vi.advanceTimersByTime(100);
    await expect(pending).rejects.toThrow(/downloading file X timed out/);
  });
});
