interface CachedBalance {
  balance: string;
  timestamp: number;
}

interface BalanceCache {
  [address: string]: CachedBalance;
}

const CACHE_KEY = 'chiral_keystore_balance_cache';
const CACHE_STALE_MS = 5 * 60 * 1000; // 5 minutes

export function getCachedBalance(address: string): CachedBalance | null {
  try {
    const cache = localStorage.getItem(CACHE_KEY);
    if (!cache) return null;
    const parsed: BalanceCache = JSON.parse(cache);
    return parsed[address.toLowerCase()] || null;
  } catch (error) {
    console.error('Error reading balance cache:', error);
    return null;
  }
}

export function setCachedBalance(address: string, balance: string): void {
  try {
    const cache = localStorage.getItem(CACHE_KEY);
    const parsed: BalanceCache = cache ? JSON.parse(cache) : {};
    parsed[address.toLowerCase()] = {
      balance,
      timestamp: Date.now()
    };
    localStorage.setItem(CACHE_KEY, JSON.stringify(parsed));
  } catch (error) {
    console.error('Error saving balance cache:', error);
  }
}

export function isCacheStale(cachedBalance: CachedBalance): boolean {
  return Date.now() - cachedBalance.timestamp > CACHE_STALE_MS;
}

export function clearBalanceCache(): void {
  localStorage.removeItem(CACHE_KEY);
}

export function formatRelativeTime(timestamp: number): string {
  const minutes = Math.floor((Date.now() - timestamp) / 60000);

  if (minutes < 1) return 'just now';
  if (minutes < 60) return `${minutes}m ago`;

  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours}h ago`;

  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}
