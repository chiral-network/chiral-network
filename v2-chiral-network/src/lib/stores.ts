import { writable } from 'svelte/store';

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
  reducedMotion: boolean;
  compactMode: boolean;
  downloadDirectory: string; // empty string = system default Downloads folder
  notifications: NotificationSettings;
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
  reducedMotion: false,
  compactMode: false,
  downloadDirectory: '',
  notifications: { ...defaultNotifications }
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
