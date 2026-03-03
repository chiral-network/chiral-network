import { writable } from 'svelte/store';
import type { HostingConfig } from '$lib/types/hosting';

// Check if we're in a browser environment
const browser = typeof window !== 'undefined';

export interface WalletAccount {
  address: string;
  privateKey: string;
}

export const walletAccount = writable<WalletAccount | null>(null);
export const isAuthenticated = writable<boolean>(false);
export const networkConnected = writable<boolean>(false);

export interface PeerInfo {
  id: string;
  address: string;
  multiaddrs?: string[];
  lastSeen: Date | number;
}

export const peers = writable<PeerInfo[]>([]);
export const networkStats = writable({
  connectedPeers: 0,
  totalPeers: 0
});

// ============================================================================
// Settings Store
// ============================================================================

export type ThemeMode = 'light' | 'dark' | 'system';
export type ColorTheme = 'blue' | 'purple' | 'green' | 'red' | 'orange';
export type NavStyle = 'navbar' | 'sidebar';

export interface NotificationSettings {
  downloadComplete: boolean;
  downloadFailed: boolean;
  peerConnected: boolean;
  peerDisconnected: boolean;
  miningBlock: boolean;
  paymentReceived: boolean;
  networkStatus: boolean;
  fileShared: boolean;
}

export interface AppSettings {
  theme: ThemeMode;
  colorTheme: ColorTheme;
  navStyle: NavStyle;
  reducedMotion: boolean;
  compactMode: boolean;
  downloadDirectory: string; // empty string = system default Downloads folder
  notifications: NotificationSettings;
  hostingConfig: HostingConfig;
}

const defaultNotifications: NotificationSettings = {
  downloadComplete: true,
  downloadFailed: true,
  peerConnected: false,
  peerDisconnected: false,
  miningBlock: true,
  paymentReceived: true,
  networkStatus: true,
  fileShared: true
};

const defaultSettings: AppSettings = {
  theme: 'system',
  colorTheme: 'blue',
  navStyle: 'navbar',
  reducedMotion: false,
  compactMode: false,
  downloadDirectory: '',
  notifications: { ...defaultNotifications },
  hostingConfig: {
    enabled: false,
    maxStorageBytes: 10 * 1024 * 1024 * 1024, // 10 GB
    pricePerMbPerDayWei: '1000000000000000',   // 0.001 CHI per MB/day
    minDepositWei: '100000000000000000',        // 0.1 CHI
  }
};

function createSettingsStore() {
  // Load settings from localStorage
  const stored = browser ? localStorage.getItem('chiral-settings') : null;
  const initial: AppSettings = stored ? { ...defaultSettings, ...JSON.parse(stored) } : defaultSettings;

  const { subscribe, set, update } = writable<AppSettings>(initial);

  return {
    subscribe,
    set: (value: AppSettings) => {
      if (browser) {
        localStorage.setItem('chiral-settings', JSON.stringify(value));
      }
      set(value);
    },
    update: (fn: (settings: AppSettings) => AppSettings) => {
      update((current) => {
        const updated = fn(current);
        if (browser) {
          localStorage.setItem('chiral-settings', JSON.stringify(updated));
        }
        return updated;
      });
    },
    reset: () => {
      if (browser) {
        localStorage.removeItem('chiral-settings');
      }
      set(defaultSettings);
    }
  };
}

export const settings = createSettingsStore();

// Derived dark mode state (resolves 'system' to actual preference)
function createDarkModeStore() {
  const { subscribe, set } = writable<boolean>(false);

  if (browser) {
    // Initialize based on current settings and system preference
    const updateDarkMode = (theme: ThemeMode) => {
      if (theme === 'system') {
        set(window.matchMedia('(prefers-color-scheme: dark)').matches);
      } else {
        set(theme === 'dark');
      }
    };

    // Subscribe to settings changes
    settings.subscribe((s) => updateDarkMode(s.theme));

    // Listen for system theme changes
    window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', (e) => {
      settings.subscribe((s) => {
        if (s.theme === 'system') {
          set(e.matches);
        }
      })();
    });
  }

  return { subscribe };
}

export const isDarkMode = createDarkModeStore();

// ============================================================================
// Blacklist Store
// ============================================================================

export interface BlacklistEntry {
  address: string;
  reason: string;
  addedAt: number;
}

function createBlacklistStore() {
  const stored = browser ? localStorage.getItem('chiral-blacklist') : null;
  const initial: BlacklistEntry[] = stored ? JSON.parse(stored) : [];

  const { subscribe, set, update } = writable<BlacklistEntry[]>(initial);

  const save = (entries: BlacklistEntry[]) => {
    if (browser) {
      localStorage.setItem('chiral-blacklist', JSON.stringify(entries));
    }
  };

  return {
    subscribe,
    set: (value: BlacklistEntry[]) => {
      save(value);
      set(value);
    },
    update: (fn: (entries: BlacklistEntry[]) => BlacklistEntry[]) => {
      update((current) => {
        const updated = fn(current);
        save(updated);
        return updated;
      });
    },
    add: (address: string, reason: string) => {
      update((current) => {
        if (current.some(e => e.address.toLowerCase() === address.toLowerCase())) {
          return current;
        }
        const updated = [...current, { address, reason, addedAt: Date.now() }];
        save(updated);
        return updated;
      });
    },
    remove: (address: string) => {
      update((current) => {
        const updated = current.filter(e => e.address.toLowerCase() !== address.toLowerCase());
        save(updated);
        return updated;
      });
    }
  };
}

export const blacklist = createBlacklistStore();
