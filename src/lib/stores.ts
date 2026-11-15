import { writable } from 'svelte/store';

export type ThemeMode = 'light' | 'dark' | 'system';
export type ColorTheme = 'blue' | 'purple' | 'green' | 'red' | 'orange';
export interface NotificationSettings { inference: boolean; funding: boolean; networkStatus: boolean }
export interface AppSettings {
  theme: ThemeMode;
  colorTheme: ColorTheme;
  reducedMotion: boolean;
  notifications: NotificationSettings;
}
const defaults: AppSettings = {
  theme: 'system', colorTheme: 'blue', reducedMotion: false,
  notifications: { inference: true, funding: true, networkStatus: true },
};
export function normalizeSettings(value: unknown): AppSettings {
  const raw = (value && typeof value === 'object' ? value : {}) as Partial<AppSettings>;
  return {
    theme: ['light', 'dark', 'system'].includes(raw.theme ?? '') ? raw.theme! : defaults.theme,
    colorTheme: ['blue', 'purple', 'green', 'red', 'orange'].includes(raw.colorTheme ?? '') ? raw.colorTheme! : defaults.colorTheme,
    reducedMotion: raw.reducedMotion === true,
    notifications: {
      inference: raw.notifications?.inference !== false,
      funding: raw.notifications?.funding !== false,
      networkStatus: raw.notifications?.networkStatus !== false,
    },
  };
}
function load(): AppSettings {
  try { return normalizeSettings(JSON.parse(localStorage.getItem('chiral-inference-settings') ?? '{}')); }
  catch { return normalizeSettings({}); }
}
export const settings = writable<AppSettings>(load());
settings.subscribe(value => {
  try { localStorage.setItem('chiral-inference-settings', JSON.stringify(value)); } catch { /* Preferences remain session-local when storage is unavailable. */ }
});

// Only public identities and account metadata belong in the webview.
export const session = writable<{ address: string | null; configured: boolean }>({ address: null, configured: false });
export const sessionEpoch = writable(0);
export function clearSession() {
  session.set({ address: null, configured: false });
  sessionEpoch.update(value => value + 1);
}
