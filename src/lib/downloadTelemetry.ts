import { listen, type UnlistenFn } from '@tauri-apps/api/event';
import { get } from 'svelte/store';
import { t } from 'svelte-i18n';
import { showToast } from '$lib/toast';

type AttemptStatus = 'retrying' | 'success' | 'failed';

type DownloadAttemptPayload = {
  file_hash: string;
  attempt: number;
  max_attempts: number;
  status: AttemptStatus;
  duration_ms: number;
  timestamp: number;
};

let unlisten: UnlistenFn | null = null;

type TranslateFn = (key: string, params?: Record<string, unknown>) => string;

const getTranslator = (): TranslateFn => get(t) as TranslateFn;

const tr = (key: string, params?: Record<string, unknown>) => getTranslator()(key, params);

function summarizeHash(hash: string): string {
  if (!hash) return 'unknown';
  if (hash.length <= 12) return hash;
  return `${hash.slice(0, 6)}…${hash.slice(-4)}`;
}

function handleAttempt(payload: DownloadAttemptPayload) {
  const formattedHash = summarizeHash(payload.file_hash);

  switch (payload.status) {
    case 'retrying':
      showToast(
        tr('download.telemetry.retrying', {
          values: {
            hash: formattedHash,
            attempt: payload.attempt,
            max: payload.max_attempts
          }
        }),
        'info'
      );
      break;
    case 'success':
      if (payload.attempt > 1) {
        showToast(
          tr('download.telemetry.recovered', {
            values: {
              hash: formattedHash,
              retries: payload.attempt - 1,
              duration: Math.round(payload.duration_ms)
            }
          }),
          'success'
        );
      }
      break;
    case 'failed':
      showToast(
        tr('download.telemetry.failed', {
          values: {
            hash: formattedHash,
            attempts: payload.attempt
          }
        }),
        'error'
      );
      break;
    default:
      break;
  }
}

export async function initDownloadTelemetry() {
  if (typeof window === 'undefined') return;
  if (unlisten) return;

  try {
    unlisten = await listen<DownloadAttemptPayload>('download_attempt', (event) => {
      const payload = event.payload;
      if (!payload) return;
      handleAttempt(payload);
    });
  } catch (error) {
    console.error('Failed to bind download_attempt listener', error);
  }
}

export function disposeDownloadTelemetry() {
  if (unlisten) {
    unlisten();
    unlisten = null;
  }
}

