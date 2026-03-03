/**
 * Color-coded console logger for browser dev tools.
 *
 * Usage:
 *   import { logger } from '$lib/logger';
 *   const log = logger('DHT');
 *   log.info('Connected to peer', peerId);
 *   log.error('Failed to connect', err);
 */

const COLORS = {
  info:  { badge: '#2563eb', text: '#1d4ed8', label: 'INFO' },
  warn:  { badge: '#d97706', text: '#b45309', label: 'WARN' },
  error: { badge: '#dc2626', text: '#b91c1c', label: 'ERROR' },
  debug: { badge: '#6b7280', text: '#4b5563', label: 'DEBUG' },
  ok:    { badge: '#16a34a', text: '#15803d', label: 'OK' },
} as const;

type Level = keyof typeof COLORS;

function fmt(level: Level, tag: string) {
  const c = COLORS[level];
  return [
    `%c ${c.label} %c ${tag} %c`,
    `background:${c.badge};color:#fff;font-weight:bold;border-radius:3px 0 0 3px;padding:1px 4px`,
    `background:#e5e7eb;color:#1f2937;font-weight:bold;border-radius:0 3px 3px 0;padding:1px 4px`,
    'background:transparent',
  ];
}

export function logger(tag: string) {
  return {
    info:  (...args: unknown[]) => console.log(...fmt('info', tag), ...args),
    warn:  (...args: unknown[]) => console.warn(...fmt('warn', tag), ...args),
    error: (...args: unknown[]) => console.error(...fmt('error', tag), ...args),
    debug: (...args: unknown[]) => console.debug(...fmt('debug', tag), ...args),
    ok:    (...args: unknown[]) => console.log(...fmt('ok', tag), ...args),
  };
}
