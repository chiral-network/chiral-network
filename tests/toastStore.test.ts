import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { get } from 'svelte/store';
import { toasts } from '$lib/toastStore';
import { normalizeSettings, settings } from '$lib/stores';

describe('notification behavior', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    for (const toast of get(toasts)) toasts.remove(toast.id);
    settings.set(normalizeSettings({}));
  });
  afterEach(() => vi.useRealTimers());
  it('dismisses an action notification after its duration', () => {
    toasts.detail('Account opened', 'Provider-specific credit is available', 'success', 3000);
    expect(get(toasts)[0].description).toBe('Provider-specific credit is available');
    vi.advanceTimersByTime(3000);
    expect(get(toasts)).toHaveLength(0);
  });
  it('respects inference notification preferences but still reports explicit actions', () => {
    settings.update(s => ({ ...s, notifications: { ...s.notifications, inference: false } }));
    toasts.notify('inference', 'Completed');
    expect(get(toasts)).toHaveLength(0);
    toasts.show('Credential revoked', 'success');
    expect(get(toasts)).toHaveLength(1);
  });
  it('can dismiss one notification without clearing unrelated alerts', () => {
    const first = toasts.show('Funding pending', 'warning');
    toasts.show('Engine unavailable', 'error');
    toasts.remove(first);
    expect(get(toasts).map(t => t.message)).toEqual(['Engine unavailable']);
  });
});
