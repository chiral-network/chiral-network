import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

const WEI_PER_CHI = 1_000_000_000_000_000_000n;
const MAX_U128_WEI = 340_282_366_920_938_463_463_374_607_431_768_211_455n;
const CHI_DECIMAL_RE = /^(?:\d+(?:\.\d*)?|\.\d+)$/;

/** Parse a user-entered CHI decimal price into a wei string. Empty means free. */
export function parseChiPriceToWei(price: string | null | undefined): string {
  const trimmed = String(price ?? '').trim();
  if (!trimmed) return '0';
  if (!CHI_DECIMAL_RE.test(trimmed)) {
    throw new Error('Price must be a decimal CHI amount');
  }

  const [wholePart, fracPart = ''] = trimmed.split('.');
  if (fracPart.length > 18) {
    throw new Error('Price supports at most 18 decimal places');
  }

  const wholeWei = BigInt(wholePart || '0') * WEI_PER_CHI;
  const fracWei = fracPart ? BigInt(fracPart.padEnd(18, '0')) : 0n;
  const wei = wholeWei + fracWei;
  if (wei > MAX_U128_WEI) {
    throw new Error('Price exceeds maximum supported amount');
  }
  return wei.toString();
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
