import { describe, it, expect, vi, beforeEach } from 'vitest';
import { invoke } from '@tauri-apps/api/core';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

const mockedInvoke = vi.mocked(invoke);

describe('networkEndpointConfig', () => {
  beforeEach(async () => {
    vi.clearAllMocks();
    delete (window as any).__TAURI_INTERNALS__;
    const { resetNetworkEndpointConfig } = await import('$lib/services/networkEndpointConfig');
    resetNetworkEndpointConfig();
  });

  it('normalizes configured endpoint URLs and CDN servers', async () => {
    const {
      normalizeNetworkEndpointConfig,
      DEFAULT_NETWORK_ENDPOINT_CONFIG,
    } = await import('$lib/services/networkEndpointConfig');

    const config = normalizeNetworkEndpointConfig({
      relayBaseUrl: 'https://relay.example/',
      ratingBaseUrl: 'https://ratings.example//',
      driveRelayBaseUrl: 'https://drive.example/',
      cdnSearchBaseUrls: ['https://cdn-a.example/', '  ', 'https://cdn-b.example//'],
      cdnServers: [
        { url: 'https://cdn-a.example/', name: ' Primary ', region: ' Region ' },
      ],
    });

    expect(config.relayBaseUrl).toBe('https://relay.example');
    expect(config.ratingBaseUrl).toBe('https://ratings.example');
    expect(config.driveRelayBaseUrl).toBe('https://drive.example');
    expect(config.cdnSearchBaseUrls).toEqual(['https://cdn-a.example', 'https://cdn-b.example']);
    expect(config.cdnServers).toEqual([
      { url: 'https://cdn-a.example', name: 'Primary', region: 'Region' },
    ]);
    expect(DEFAULT_NETWORK_ENDPOINT_CONFIG.relayBaseUrl).toBe('http://130.245.173.73:8080');
  });

  it('loads endpoint config from the active Tauri network', async () => {
    (window as any).__TAURI_INTERNALS__ = {};
    mockedInvoke.mockResolvedValueOnce({
      relayBaseUrl: 'https://relay.net',
      ratingBaseUrl: 'https://ratings.net',
      driveRelayBaseUrl: 'https://drive.net',
      cdnSearchBaseUrls: ['https://cdn-search.net'],
      cdnServers: [{ url: 'https://cdn.net', name: 'Configured CDN', region: 'Lab' }],
    });

    const {
      loadNetworkEndpointConfig,
      getRelayBaseUrl,
      getCdnServers,
    } = await import('$lib/services/networkEndpointConfig');

    const config = await loadNetworkEndpointConfig();

    expect(mockedInvoke).toHaveBeenCalledWith('get_active_network');
    expect(config.ratingBaseUrl).toBe('https://ratings.net');
    expect(getRelayBaseUrl()).toBe('https://relay.net');
    expect(getCdnServers()).toEqual([{ url: 'https://cdn.net', name: 'Configured CDN', region: 'Lab' }]);
  });

  it('lets services override endpoints without mutating defaults', async () => {
    const {
      DEFAULT_NETWORK_ENDPOINT_CONFIG,
      getDriveRelayBaseUrl,
      resetNetworkEndpointConfig,
      setNetworkEndpointConfig,
    } = await import('$lib/services/networkEndpointConfig');

    setNetworkEndpointConfig({ driveRelayBaseUrl: 'https://drive-relay.example/' });

    expect(getDriveRelayBaseUrl()).toBe('https://drive-relay.example');
    expect(DEFAULT_NETWORK_ENDPOINT_CONFIG.driveRelayBaseUrl).toBe('http://130.245.173.73:8080');

    resetNetworkEndpointConfig();
    expect(getDriveRelayBaseUrl()).toBe(DEFAULT_NETWORK_ENDPOINT_CONFIG.driveRelayBaseUrl);
  });
});
