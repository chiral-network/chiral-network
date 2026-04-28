/**
 * Helper for browser-side fetches that hit Chiral servers.
 *
 * Phase 3 of version enforcement: every server-bound HTTP call has to
 * carry an `X-Chiral-Client-Version` header so the gateway middleware
 * can reject out-of-date clients. Rust-side calls get the header
 * automatically via the shared `rpc_client.rs` default headers; calls
 * made directly from the browser (CDN status/pricing/upload, headless
 * search probes, etc.) need to inject it themselves — which is what
 * this helper is for.
 *
 * Usage:
 *   await fetchWithVersion(`${cdn}/api/cdn/pricing?sizeMb=...`);
 *   await fetchWithVersion(url, { method: 'DELETE' });
 */
import { get } from 'svelte/store';
import { versionStatus } from './stores/versionStore';

/// Returns the current client version string, or `'0.0.0'` if the
/// version store hasn't initialised yet (e.g. browser dev). Servers
/// treat `0.0.0` as "unknown" and only reject if `min_required` is
/// also above 0.
export function chiralClientVersion(): string {
  return get(versionStatus).currentVersion ?? '0.0.0';
}

/// Build a `RequestInit` that carries the version header alongside any
/// caller-supplied headers / method / body / etc.
export function withVersionHeader(init?: RequestInit): RequestInit {
  return {
    ...(init ?? {}),
    headers: {
      ...(init?.headers ?? {}),
      'X-Chiral-Client-Version': chiralClientVersion(),
    },
  };
}

/// Drop-in `fetch` wrapper. The first arg is unchanged; the second arg
/// has the version header merged in.
export function fetchWithVersion(
  input: RequestInfo | URL,
  init?: RequestInit,
): Promise<Response> {
  return fetch(input, withVersionHeader(init));
}
