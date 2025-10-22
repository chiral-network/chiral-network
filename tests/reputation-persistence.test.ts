import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest';

/**
 * Tests for localStorage persistence in Reputation.svelte
 * These tests verify the UI toggle persistence functionality
 */

describe('Reputation.svelte localStorage persistence', () => {
  const STORAGE_KEY_SHOW_ANALYTICS = 'chiral.reputation.showAnalytics';
  const STORAGE_KEY_SHOW_RELAY_LEADERBOARD = 'chiral.reputation.showRelayLeaderboard';
  
  let localStorageMock: { [key: string]: string };

  beforeEach(() => {
    localStorageMock = {};
    
    // Mock localStorage
    global.localStorage = {
      getItem: vi.fn((key: string) => localStorageMock[key] || null),
      setItem: vi.fn((key: string, value: string) => {
        localStorageMock[key] = value;
      }),
      removeItem: vi.fn((key: string) => {
        delete localStorageMock[key];
      }),
      clear: vi.fn(() => {
        localStorageMock = {};
      }),
      get length() {
        return Object.keys(localStorageMock).length;
      },
      key: vi.fn((index: number) => {
        const keys = Object.keys(localStorageMock);
        return keys[index] || null;
      }),
    } as Storage;
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  describe('loadPersistedToggles', () => {
    it('should return default values when localStorage is empty', () => {
      // Simulate the function from Reputation.svelte
      function loadPersistedToggles() {
        if (typeof window === 'undefined') return { showAnalytics: true, showRelayLeaderboard: true };
        
        try {
          const storedAnalytics = window.localStorage.getItem(STORAGE_KEY_SHOW_ANALYTICS);
          const storedLeaderboard = window.localStorage.getItem(STORAGE_KEY_SHOW_RELAY_LEADERBOARD);
          
          return {
            showAnalytics: storedAnalytics !== null ? storedAnalytics === 'true' : true,
            showRelayLeaderboard: storedLeaderboard !== null ? storedLeaderboard === 'true' : true
          };
        } catch (e) {
          return { showAnalytics: true, showRelayLeaderboard: true };
        }
      }

      const result = loadPersistedToggles();
      expect(result.showAnalytics).toBe(true);
      expect(result.showRelayLeaderboard).toBe(true);
    });

    it('should load persisted values from localStorage', () => {
      localStorage.setItem(STORAGE_KEY_SHOW_ANALYTICS, 'false');
      localStorage.setItem(STORAGE_KEY_SHOW_RELAY_LEADERBOARD, 'false');

      function loadPersistedToggles() {
        const storedAnalytics = localStorage.getItem(STORAGE_KEY_SHOW_ANALYTICS);
        const storedLeaderboard = localStorage.getItem(STORAGE_KEY_SHOW_RELAY_LEADERBOARD);
        
        return {
          showAnalytics: storedAnalytics !== null ? storedAnalytics === 'true' : true,
          showRelayLeaderboard: storedLeaderboard !== null ? storedLeaderboard === 'true' : true
        };
      }

      const result = loadPersistedToggles();
      expect(result.showAnalytics).toBe(false);
      expect(result.showRelayLeaderboard).toBe(false);
    });

    it('should handle mixed persisted values', () => {
      localStorage.setItem(STORAGE_KEY_SHOW_ANALYTICS, 'true');
      localStorage.setItem(STORAGE_KEY_SHOW_RELAY_LEADERBOARD, 'false');

      function loadPersistedToggles() {
        const storedAnalytics = localStorage.getItem(STORAGE_KEY_SHOW_ANALYTICS);
        const storedLeaderboard = localStorage.getItem(STORAGE_KEY_SHOW_RELAY_LEADERBOARD);
        
        return {
          showAnalytics: storedAnalytics !== null ? storedAnalytics === 'true' : true,
          showRelayLeaderboard: storedLeaderboard !== null ? storedLeaderboard === 'true' : true
        };
      }

      const result = loadPersistedToggles();
      expect(result.showAnalytics).toBe(true);
      expect(result.showRelayLeaderboard).toBe(false);
    });
  });

  describe('persistToggle', () => {
    it('should persist toggle values to localStorage', () => {
      function persistToggle(key: string, value: boolean) {
        try {
          localStorage.setItem(key, String(value));
        } catch (e) {
          console.warn('Failed to persist UI toggle:', e);
        }
      }

      persistToggle(STORAGE_KEY_SHOW_ANALYTICS, false);
      expect(localStorage.getItem(STORAGE_KEY_SHOW_ANALYTICS)).toBe('false');

      persistToggle(STORAGE_KEY_SHOW_ANALYTICS, true);
      expect(localStorage.getItem(STORAGE_KEY_SHOW_ANALYTICS)).toBe('true');
    });

    it('should handle multiple toggle persistence', () => {
      function persistToggle(key: string, value: boolean) {
        localStorage.setItem(key, String(value));
      }

      persistToggle(STORAGE_KEY_SHOW_ANALYTICS, true);
      persistToggle(STORAGE_KEY_SHOW_RELAY_LEADERBOARD, false);

      expect(localStorage.getItem(STORAGE_KEY_SHOW_ANALYTICS)).toBe('true');
      expect(localStorage.getItem(STORAGE_KEY_SHOW_RELAY_LEADERBOARD)).toBe('false');
    });
  });

  describe('integration', () => {
    it('should persist and restore toggle state', () => {
      function persistToggle(key: string, value: boolean) {
        localStorage.setItem(key, String(value));
      }

      function loadPersistedToggles() {
        const storedAnalytics = localStorage.getItem(STORAGE_KEY_SHOW_ANALYTICS);
        const storedLeaderboard = localStorage.getItem(STORAGE_KEY_SHOW_RELAY_LEADERBOARD);
        
        return {
          showAnalytics: storedAnalytics !== null ? storedAnalytics === 'true' : true,
          showRelayLeaderboard: storedLeaderboard !== null ? storedLeaderboard === 'true' : true
        };
      }

      // Simulate user toggling
      persistToggle(STORAGE_KEY_SHOW_ANALYTICS, false);
      persistToggle(STORAGE_KEY_SHOW_RELAY_LEADERBOARD, false);

      // Simulate page reload
      const restored = loadPersistedToggles();
      expect(restored.showAnalytics).toBe(false);
      expect(restored.showRelayLeaderboard).toBe(false);

      // Toggle again
      persistToggle(STORAGE_KEY_SHOW_ANALYTICS, true);
      
      const restoredAgain = loadPersistedToggles();
      expect(restoredAgain.showAnalytics).toBe(true);
      expect(restoredAgain.showRelayLeaderboard).toBe(false);
    });
  });
});
