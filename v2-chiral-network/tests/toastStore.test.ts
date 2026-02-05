import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { toasts, showToast } from '$lib/toastStore';
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

    it('should default to 5000ms duration', () => {
      toasts.show('Default duration');
      expect(get(toasts)).toHaveLength(1);

      vi.advanceTimersByTime(4999);
      expect(get(toasts)).toHaveLength(1);

      vi.advanceTimersByTime(1);
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

  describe('showToast', () => {
    it('should add a toast via showToast helper', () => {
      showToast('Helper toast', 'success');
      const current = get(toasts);
      expect(current).toHaveLength(1);
      expect(current[0].message).toBe('Helper toast');
      expect(current[0].type).toBe('success');
    });

    it('should convert warning type to info', () => {
      showToast('Warning toast', 'warning');
      const current = get(toasts);
      expect(current[0].type).toBe('info');
    });

    it('should respect custom duration', () => {
      showToast('Short toast', 'info', 1000);
      expect(get(toasts)).toHaveLength(1);

      vi.advanceTimersByTime(1000);
      expect(get(toasts)).toHaveLength(0);
    });
  });
});
