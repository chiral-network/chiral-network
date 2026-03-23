import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

/** Format byte count as human-readable string (B, KB, MB, GB, TB) */
export function formatBytes(bytes: number | undefined | null): string {
  if (!bytes || bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
}

/** Format wei string as CHI display (e.g. "0.001 CHI" or "Free") */
export function formatPriceWei(weiStr: string | undefined | null): string {
  if (!weiStr || weiStr === '0') return 'Free';
  try {
    const wei = BigInt(weiStr);
    if (wei === 0n) return 'Free';
    const whole = wei / 1_000_000_000_000_000_000n;
    const frac = wei % 1_000_000_000_000_000_000n;
    if (frac === 0n) return `${whole} CHI`;
    const fracStr = frac.toString().padStart(18, '0').replace(/0+$/, '');
    const decimals = fracStr.length > 6 ? fracStr.slice(0, 6) : fracStr;
    return `${whole}.${decimals} CHI`;
  } catch {
    return 'Free';
  }
}
