import { get, writable } from 'svelte/store';

export interface CdnEndpointConfig {
  url: string;
  name: string;
  region: string;
}

export interface NetworkEndpointConfig {
  relayBaseUrl: string;
  ratingBaseUrl: string;
  driveRelayBaseUrl: string;
  cdnSearchBaseUrls: string[];
  cdnServers: CdnEndpointConfig[];
}

interface ActiveNetworkInfo {
  relayBaseUrl?: unknown;
  ratingBaseUrl?: unknown;
  driveRelayBaseUrl?: unknown;
  cdnSearchBaseUrls?: unknown;
  cdnServers?: unknown;
}

export const DEFAULT_NETWORK_ENDPOINT_CONFIG: NetworkEndpointConfig = {
  relayBaseUrl: 'http://130.245.173.73:8080',
  ratingBaseUrl: 'http://130.245.173.73:8080',
  driveRelayBaseUrl: 'http://130.245.173.73:8080',
  cdnSearchBaseUrls: ['http://130.245.173.73:9420', 'http://130.245.173.231:9420'],
  cdnServers: [
    { url: 'http://130.245.173.73:9420', name: 'CDN Primary (US East)', region: 'New York' },
    { url: 'http://130.245.173.231:9420', name: 'CDN Secondary (US East)', region: 'Stony Brook' },
  ],
};

const configStore = writable<NetworkEndpointConfig>(cloneConfig(DEFAULT_NETWORK_ENDPOINT_CONFIG));
let loadPromise: Promise<NetworkEndpointConfig> | null = null;

function cloneConfig(config: NetworkEndpointConfig): NetworkEndpointConfig {
  return {
    relayBaseUrl: config.relayBaseUrl,
    ratingBaseUrl: config.ratingBaseUrl,
    driveRelayBaseUrl: config.driveRelayBaseUrl,
    cdnSearchBaseUrls: [...config.cdnSearchBaseUrls],
    cdnServers: config.cdnServers.map((server) => ({ ...server })),
  };
}

function normalizeBaseUrl(value: unknown, fallback: string): string {
  if (typeof value !== 'string') return fallback;
  const trimmed = value.trim().replace(/\/+$/, '');
  return trimmed || fallback;
}

function normalizeUrlList(value: unknown, fallback: string[]): string[] {
  if (!Array.isArray(value)) return [...fallback];
  const urls = value
    .map((url) => normalizeBaseUrl(url, ''))
    .filter((url): url is string => url.length > 0);
  return urls.length > 0 ? urls : [...fallback];
}

function normalizeCdnServers(value: unknown, fallback: CdnEndpointConfig[]): CdnEndpointConfig[] {
  if (!Array.isArray(value)) return fallback.map((server) => ({ ...server }));
  const servers = value
    .map((server): CdnEndpointConfig | null => {
      if (!server || typeof server !== 'object') return null;
      const raw = server as Record<string, unknown>;
      const url = normalizeBaseUrl(raw.url, '');
      if (!url) return null;
      return {
        url,
        name: typeof raw.name === 'string' && raw.name.trim() ? raw.name.trim() : url,
        region: typeof raw.region === 'string' ? raw.region.trim() : '',
      };
    })
    .filter((server): server is CdnEndpointConfig => server !== null);
  return servers.length > 0 ? servers : fallback.map((server) => ({ ...server }));
}

export function normalizeNetworkEndpointConfig(raw: unknown): NetworkEndpointConfig {
  const parsed = (raw || {}) as ActiveNetworkInfo;
  const defaults = DEFAULT_NETWORK_ENDPOINT_CONFIG;
  return {
    relayBaseUrl: normalizeBaseUrl(parsed.relayBaseUrl, defaults.relayBaseUrl),
    ratingBaseUrl: normalizeBaseUrl(parsed.ratingBaseUrl, defaults.ratingBaseUrl),
    driveRelayBaseUrl: normalizeBaseUrl(parsed.driveRelayBaseUrl, defaults.driveRelayBaseUrl),
    cdnSearchBaseUrls: normalizeUrlList(parsed.cdnSearchBaseUrls, defaults.cdnSearchBaseUrls),
    cdnServers: normalizeCdnServers(parsed.cdnServers, defaults.cdnServers),
  };
}

export const networkEndpointConfig = {
  subscribe: configStore.subscribe,
};

export function setNetworkEndpointConfig(config: Partial<NetworkEndpointConfig>): NetworkEndpointConfig {
  const normalized = normalizeNetworkEndpointConfig({
    ...get(configStore),
    ...config,
  });
  configStore.set(normalized);
  loadPromise = Promise.resolve(cloneConfig(normalized));
  return cloneConfig(normalized);
}

export function resetNetworkEndpointConfig(): NetworkEndpointConfig {
  const defaults = cloneConfig(DEFAULT_NETWORK_ENDPOINT_CONFIG);
  configStore.set(defaults);
  loadPromise = null;
  return cloneConfig(defaults);
}

export function getNetworkEndpointConfig(): NetworkEndpointConfig {
  return cloneConfig(get(configStore));
}

function canInvokeTauri(): boolean {
  return typeof window !== 'undefined' && ('__TAURI__' in window || '__TAURI_INTERNALS__' in window);
}

export async function loadNetworkEndpointConfig(): Promise<NetworkEndpointConfig> {
  if (loadPromise) return cloneConfig(await loadPromise);
  if (!canInvokeTauri()) {
    return getNetworkEndpointConfig();
  }
  loadPromise = (async () => {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const activeNetwork = await invoke<ActiveNetworkInfo>('get_active_network');
      const normalized = normalizeNetworkEndpointConfig(activeNetwork);
      configStore.set(normalized);
      return cloneConfig(normalized);
    } catch {
      return getNetworkEndpointConfig();
    }
  })();
  return cloneConfig(await loadPromise);
}

export function getRelayBaseUrl(): string {
  return get(configStore).relayBaseUrl;
}

export async function getRelayBaseUrlAsync(): Promise<string> {
  return (await loadNetworkEndpointConfig()).relayBaseUrl;
}

export function getRatingBaseUrl(): string {
  return get(configStore).ratingBaseUrl;
}

export async function getRatingBaseUrlAsync(): Promise<string> {
  return (await loadNetworkEndpointConfig()).ratingBaseUrl;
}

export function getDriveRelayBaseUrl(): string {
  return get(configStore).driveRelayBaseUrl;
}

export async function getDriveRelayBaseUrlAsync(): Promise<string> {
  return (await loadNetworkEndpointConfig()).driveRelayBaseUrl;
}

export function getCdnSearchBaseUrls(): string[] {
  return [...get(configStore).cdnSearchBaseUrls];
}

export async function getCdnSearchBaseUrlsAsync(): Promise<string[]> {
  return [...(await loadNetworkEndpointConfig()).cdnSearchBaseUrls];
}

export function getCdnServers(): CdnEndpointConfig[] {
  return get(configStore).cdnServers.map((server) => ({ ...server }));
}

export async function getCdnServersAsync(): Promise<CdnEndpointConfig[]> {
  return (await loadNetworkEndpointConfig()).cdnServers.map((server) => ({ ...server }));
}
