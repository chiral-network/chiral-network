import { describe, it, expect, beforeEach, vi } from 'vitest';
import { get } from 'svelte/store';

// We need to re-import stores fresh for each test group
// since they read from localStorage on initialization
describe('stores', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.resetModules();
  });

  describe('walletAccount', () => {
    it('should initialize as null', async () => {
      const { walletAccount } = await import('$lib/stores');
      expect(get(walletAccount)).toBeNull();
    });

    it('should store wallet data when set', async () => {
      const { walletAccount } = await import('$lib/stores');
      walletAccount.set({
        address: '0x1234567890abcdef',
        privateKey: 'abc123'
      });
      const wallet = get(walletAccount);
      expect(wallet?.address).toBe('0x1234567890abcdef');
      expect(wallet?.privateKey).toBe('abc123');
    });
  });

  describe('isAuthenticated', () => {
    it('should initialize as false', async () => {
      const { isAuthenticated } = await import('$lib/stores');
      expect(get(isAuthenticated)).toBe(false);
    });

    it('should be settable to true', async () => {
      const { isAuthenticated } = await import('$lib/stores');
      isAuthenticated.set(true);
      expect(get(isAuthenticated)).toBe(true);
    });
  });

  describe('networkConnected', () => {
    it('should initialize as false', async () => {
      const { networkConnected } = await import('$lib/stores');
      expect(get(networkConnected)).toBe(false);
    });
  });

  describe('peers', () => {
    it('should initialize as empty array', async () => {
      const { peers } = await import('$lib/stores');
      expect(get(peers)).toEqual([]);
    });

    it('should accept PeerInfo objects', async () => {
      const { peers } = await import('$lib/stores');
      peers.set([{
        id: 'peer1',
        address: '192.168.1.1',
        multiaddrs: ['/ip4/192.168.1.1/tcp/4001'],
        lastSeen: Date.now(),
      }]);
      expect(get(peers)).toHaveLength(1);
      expect(get(peers)[0].id).toBe('peer1');
    });
  });

  describe('networkStats', () => {
    it('should initialize with zero peers', async () => {
      const { networkStats } = await import('$lib/stores');
      const stats = get(networkStats);
      expect(stats.connectedPeers).toBe(0);
      expect(stats.totalPeers).toBe(0);
    });

    it('should update peer counts', async () => {
      const { networkStats } = await import('$lib/stores');
      networkStats.set({ connectedPeers: 5, totalPeers: 10 });
      const stats = get(networkStats);
      expect(stats.connectedPeers).toBe(5);
      expect(stats.totalPeers).toBe(10);
    });
  });

  describe('settings', () => {
    it('should initialize with default settings', async () => {
      const { settings } = await import('$lib/stores');
      const s = get(settings);
      expect(s.theme).toBe('system');
      expect(s.reducedMotion).toBe(false);
      expect(s.compactMode).toBe(false);
    });

    it('should persist settings to localStorage', async () => {
      const { settings } = await import('$lib/stores');
      settings.set({ theme: 'dark', reducedMotion: true, compactMode: false });
      const stored = JSON.parse(localStorage.getItem('chiral-settings')!);
      expect(stored.theme).toBe('dark');
      expect(stored.reducedMotion).toBe(true);
    });

    it('should load settings from localStorage', async () => {
      localStorage.setItem('chiral-settings', JSON.stringify({
        theme: 'dark',
        reducedMotion: false,
        compactMode: true
      }));
      const { settings } = await import('$lib/stores');
      const s = get(settings);
      expect(s.theme).toBe('dark');
      expect(s.compactMode).toBe(true);
    });

    it('should merge partial localStorage with defaults', async () => {
      localStorage.setItem('chiral-settings', JSON.stringify({
        theme: 'light'
      }));
      const { settings } = await import('$lib/stores');
      const s = get(settings);
      expect(s.theme).toBe('light');
      expect(s.reducedMotion).toBe(false);  // default
      expect(s.compactMode).toBe(false);    // default
    });

    it('should support update function', async () => {
      const { settings } = await import('$lib/stores');
      settings.update(s => ({ ...s, compactMode: true }));
      expect(get(settings).compactMode).toBe(true);
      // Should also persist
      const stored = JSON.parse(localStorage.getItem('chiral-settings')!);
      expect(stored.compactMode).toBe(true);
    });

    it('should reset to defaults', async () => {
      const { settings } = await import('$lib/stores');
      settings.set({ theme: 'dark', reducedMotion: true, compactMode: true });
      settings.reset();
      const s = get(settings);
      expect(s.theme).toBe('system');
      expect(s.reducedMotion).toBe(false);
      expect(s.compactMode).toBe(false);
      expect(localStorage.getItem('chiral-settings')).toBeNull();
    });
  });

  describe('isDarkMode', () => {
    it('should be a boolean store', async () => {
      const { isDarkMode } = await import('$lib/stores');
      expect(typeof get(isDarkMode)).toBe('boolean');
    });

    it('should resolve dark theme to true', async () => {
      localStorage.setItem('chiral-settings', JSON.stringify({
        theme: 'dark',
        reducedMotion: false,
        compactMode: false
      }));
      const { isDarkMode } = await import('$lib/stores');
      expect(get(isDarkMode)).toBe(true);
    });

    it('should resolve light theme to false', async () => {
      localStorage.setItem('chiral-settings', JSON.stringify({
        theme: 'light',
        reducedMotion: false,
        compactMode: false
      }));
      const { isDarkMode } = await import('$lib/stores');
      expect(get(isDarkMode)).toBe(false);
    });
  });
});
