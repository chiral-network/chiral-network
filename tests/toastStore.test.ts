import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { toasts, showToast } from '$lib/toastStore';
import { settings } from '$lib/stores';
import { get } from 'svelte/store';

describe('toastStore', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    // Clear any existing toasts
    const current = get(toasts);
    current.forEach(t => toasts.remove(t.id));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  describe('toasts.show', () => {
    it('should add a toast to the store', () => {
      toasts.show('Test message', 'info');
      const current = get(toasts);
      expect(current).toHaveLength(1);
      expect(current[0].message).toBe('Test message');
      expect(current[0].type).toBe('info');
    });

    it('should auto-dismiss toast after duration', () => {
      toasts.show('Temporary', 'info', 3000);
      expect(get(toasts)).toHaveLength(1);

      vi.advanceTimersByTime(3000);
      expect(get(toasts)).toHaveLength(0);
    });

    it('should default to info type', () => {
      toasts.show('Default type');
      const current = get(toasts);
      expect(current[0].type).toBe('info');
    });

    it('should use type-specific default durations', () => {
      toasts.show('Success toast', 'success');
      expect(get(toasts)).toHaveLength(1);

      // success default is 4000ms
      vi.advanceTimersByTime(3999);
      expect(get(toasts)).toHaveLength(1);

      vi.advanceTimersByTime(1);
      expect(get(toasts)).toHaveLength(0);
    });

    it('should use 8000ms default for error toasts', () => {
      toasts.show('Error toast', 'error');
      expect(get(toasts)).toHaveLength(1);

      vi.advanceTimersByTime(7999);
      expect(get(toasts)).toHaveLength(1);

      vi.advanceTimersByTime(1);
      expect(get(toasts)).toHaveLength(0);
    });

    it('should support custom duration override', () => {
      toasts.show('Custom duration', 'info', 2000);
      expect(get(toasts)).toHaveLength(1);

      vi.advanceTimersByTime(2000);
      expect(get(toasts)).toHaveLength(0);
    });

    it('should support success type', () => {
      toasts.show('Success!', 'success');
      expect(get(toasts)[0].type).toBe('success');
    });

    it('should support error type', () => {
      toasts.show('Error!', 'error');
      expect(get(toasts)[0].type).toBe('error');
    });

    it('should support warning type', () => {
      toasts.show('Warning!', 'warning');
      expect(get(toasts)[0].type).toBe('warning');
    });

    it('should assign unique IDs to each toast', () => {
      toasts.show('First');
      toasts.show('Second');
      const current = get(toasts);
      expect(current[0].id).not.toBe(current[1].id);
    });

    it('should stack multiple toasts', () => {
      toasts.show('First');
      toasts.show('Second');
      toasts.show('Third');
      expect(get(toasts)).toHaveLength(3);
    });

    it('should return the toast id', () => {
      const id = toasts.show('Test');
      expect(typeof id).toBe('number');
    });

    it('should store duration and createdAt', () => {
      toasts.show('Test', 'info', 3000);
      const current = get(toasts);
      expect(current[0].duration).toBe(3000);
      expect(current[0].createdAt).toBeGreaterThan(0);
    });
  });

  describe('toasts.detail', () => {
    it('should add a toast with description', () => {
      toasts.detail('Title', 'Description body', 'success');
      const current = get(toasts);
      expect(current).toHaveLength(1);
      expect(current[0].message).toBe('Title');
      expect(current[0].description).toBe('Description body');
      expect(current[0].type).toBe('success');
    });

    it('should auto-dismiss after type-specific duration', () => {
      toasts.detail('Error', 'Something broke', 'error');
      expect(get(toasts)).toHaveLength(1);

      vi.advanceTimersByTime(8000);
      expect(get(toasts)).toHaveLength(0);
    });

    it('should support custom duration', () => {
      toasts.detail('Title', 'Body', 'info', 1500);
      vi.advanceTimersByTime(1500);
      expect(get(toasts)).toHaveLength(0);
    });
  });

  describe('toasts.remove', () => {
    it('should remove a specific toast by ID', () => {
      toasts.show('First');
      toasts.show('Second');
      const current = get(toasts);
      const firstId = current[0].id;

      toasts.remove(firstId);
      const after = get(toasts);
      expect(after).toHaveLength(1);
      expect(after[0].message).toBe('Second');
    });

    it('should not crash when removing non-existent ID', () => {
      toasts.show('Only');
      toasts.remove(99999);
      expect(get(toasts)).toHaveLength(1);
    });
  });

  describe('showToast (deprecated helper)', () => {
    it('should add a toast via showToast helper', () => {
      showToast('Helper toast', 'success');
      const current = get(toasts);
      expect(current).toHaveLength(1);
      expect(current[0].message).toBe('Helper toast');
      expect(current[0].type).toBe('success');
    });

    it('should preserve warning type (no longer converts to info)', () => {
      showToast('Warning toast', 'warning');
      const current = get(toasts);
      expect(current[0].type).toBe('warning');
    });

    it('should respect custom duration', () => {
      showToast('Short toast', 'info', 1000);
      expect(get(toasts)).toHaveLength(1);

      vi.advanceTimersByTime(1000);
      expect(get(toasts)).toHaveLength(0);
    });
  });

  describe('toasts.notify', () => {
    it('should show toast when notification setting is enabled', () => {
      settings.update(s => ({
        ...s,
        notifications: { ...s.notifications, downloadComplete: true },
      }));
      toasts.notify('downloadComplete', 'Download done', 'success');
      expect(get(toasts)).toHaveLength(1);
      expect(get(toasts)[0].message).toBe('Download done');
    });

    it('should suppress toast when notification setting is disabled', () => {
      settings.update(s => ({
        ...s,
        notifications: { ...s.notifications, downloadComplete: false },
      }));
      toasts.notify('downloadComplete', 'Download done', 'success');
      expect(get(toasts)).toHaveLength(0);
    });

    it('should default to showing when setting is not explicitly false', () => {
      toasts.notify('miningBlock', 'Block mined', 'success');
      expect(get(toasts)).toHaveLength(1);
    });
  });

  describe('toasts.notifyDetail', () => {
    it('should show detail toast when notification setting is enabled', () => {
      settings.update(s => ({
        ...s,
        notifications: { ...s.notifications, networkStatus: true },
      }));
      toasts.notifyDetail('networkStatus', 'Connected', 'P2P network active', 'success');
      const current = get(toasts);
      expect(current).toHaveLength(1);
      expect(current[0].message).toBe('Connected');
      expect(current[0].description).toBe('P2P network active');
    });

    it('should suppress detail toast when notification setting is disabled', () => {
      settings.update(s => ({
        ...s,
        notifications: { ...s.notifications, networkStatus: false },
      }));
      toasts.notifyDetail('networkStatus', 'Connected', 'P2P network active', 'success');
      expect(get(toasts)).toHaveLength(0);
    });
  });
});
