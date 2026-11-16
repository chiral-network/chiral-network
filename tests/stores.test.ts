import { describe, expect, it } from 'vitest';
import { get } from 'svelte/store';
import { clearSession, normalizeSettings, session, sessionEpoch } from '$lib/stores';

describe('inference preferences', () => {
  it('retains supported appearance and notification choices', () => {
    expect(normalizeSettings({ theme: 'dark', colorTheme: 'purple', navStyle: 'sidebar', reducedMotion: true, notifications: { inference: false } })).toEqual({
      theme: 'dark', colorTheme: 'purple', navStyle: 'sidebar', reducedMotion: true,
      notifications: { inference: false, funding: true, networkStatus: true },
    });
  });
  it('drops legacy runtime and secret fields instead of restoring them', () => {
    const settings = normalizeSettings({ autoStartMining: true, privateKey: 'never-store', downloadDirectory: '/tmp', theme: 'invalid' });
    expect(settings).not.toHaveProperty('privateKey');
    expect(settings).not.toHaveProperty('autoStartMining');
    expect(settings).not.toHaveProperty('downloadDirectory');
    expect(settings.theme).toBe('system');
  });
  it('defaults to the top navigation bar and rejects unknown layouts', () => {
    expect(normalizeSettings({}).navStyle).toBe('navbar');
    expect(normalizeSettings({ navStyle: 'floating' }).navStyle).toBe('navbar');
  });
  it('normalizes absent and malformed preference roots', () => {
    expect(normalizeSettings(null)).toEqual(normalizeSettings('bad'));
  });
  it('clears public session state and invalidates mounted page data on logout', () => {
    const oldEpoch = get(sessionEpoch);
    session.set({ address: '0x123', configured: true });
    clearSession();
    expect(get(session)).toEqual({ address: null, configured: false });
    expect(get(sessionEpoch)).toBe(oldEpoch + 1);
  });
});
