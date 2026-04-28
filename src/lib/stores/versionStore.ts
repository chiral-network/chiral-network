import { writable, type Readable } from 'svelte/store';

/// Mirrors `version::VersionPolicy` from src-tauri/src/version.rs.
export interface VersionPolicy {
  minRequired: string;
  recommended: string;
  downloadUrl: string;
  message?: string | null;
  issuedAt: number;
  validUntil: number;
  signature: string;
}

export type VersionStatusKind = 'ok' | 'recommended' | 'required';

export interface VersionStatus {
  currentVersion: string;
  status: VersionStatusKind;
  policy: VersionPolicy;
}

const DEFAULT_POLICY: VersionPolicy = {
  minRequired: '0.0.0',
  recommended: '0.0.0',
  downloadUrl: 'https://github.com/chiral-network/chiral-network/releases/latest',
  issuedAt: 0,
  validUntil: 0,
  signature: '',
};

const DEFAULT_STATUS: VersionStatus = {
  currentVersion: 'unknown',
  status: 'ok',
  policy: DEFAULT_POLICY,
};

/// Re-poll cadence in ms. Hourly is plenty — version policy is not
/// expected to change at sub-minute granularity.
const POLL_INTERVAL_MS = 60 * 60 * 1000;

const internal = writable<VersionStatus>(DEFAULT_STATUS);

/// Public read-only handle. App-level UI (the UpdateGate) subscribes
/// here; nothing else writes to the store.
export const versionStatus: Readable<VersionStatus> = { subscribe: internal.subscribe };

let pollHandle: ReturnType<typeof setInterval> | null = null;

async function fetchOnce(): Promise<void> {
  if (typeof window === 'undefined') return;
  const inTauri = '__TAURI__' in window || '__TAURI_INTERNALS__' in window;
  if (!inTauri) return; // browser dev — leave defaults
  try {
    const { invoke } = await import('@tauri-apps/api/core');
    const status = await invoke<VersionStatus>('get_version_status');
    internal.set(status);
  } catch (e) {
    console.warn('[version] get_version_status failed:', e);
  }
}

/// Idempotent — call once at app startup. Subsequent calls are no-ops.
export function initVersionStore(): void {
  if (pollHandle !== null) return;
  void fetchOnce();
  pollHandle = setInterval(() => {
    void fetchOnce();
  }, POLL_INTERVAL_MS);
}
