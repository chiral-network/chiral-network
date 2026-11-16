import type { ComponentType } from 'svelte';

export type IconComponent = ComponentType;
export type Tone = 'primary' | 'green' | 'amber' | 'red' | 'purple' | 'orange' | 'sky' | 'gray';

// Icon-pill backgrounds + icon colors (the "p-2 rounded-lg bg-*-100" idiom).
// Full literal class strings so Tailwind's scanner keeps them.
export const pillTone: Record<Tone, string> = {
  primary: 'bg-primary-100 dark:bg-primary-900/30 text-primary-600 dark:text-primary-400',
  green: 'bg-green-100 dark:bg-green-900/30 text-green-600 dark:text-green-400',
  amber: 'bg-amber-100 dark:bg-amber-900/30 text-amber-600 dark:text-amber-400',
  red: 'bg-red-100 dark:bg-red-900/30 text-red-600 dark:text-red-400',
  purple: 'bg-purple-100 dark:bg-purple-900/30 text-purple-600 dark:text-purple-400',
  orange: 'bg-orange-100 dark:bg-orange-900/30 text-orange-600 dark:text-orange-400',
  sky: 'bg-sky-100 dark:bg-sky-900/30 text-sky-600 dark:text-sky-400',
  gray: 'bg-gray-100 dark:bg-gray-700 text-gray-500 dark:text-gray-400',
};

/** Badge class for a provider-account / funding status string. */
export function statusBadge(status: string | null | undefined): string {
  switch (status) {
    case 'active': case 'free': case 'ready': case 'live': case 'completed': case 'confirmed': return 'badge-green';
    case 'pending': case 'draining': case 'drained': case 'submitted': case 'broadcast': return 'badge-amber';
    case 'frozen': case 'orphaned': case 'failed': case 'rejected': case 'reversed': case 'exhausted': return 'badge-red';
    default: return 'badge-gray';
  }
}

export function shortAddress(value: string | null | undefined, head = 6, tail = 4): string {
  if (!value) return '';
  return value.length <= head + tail + 1 ? value : `${value.slice(0, head)}…${value.slice(-tail)}`;
}
