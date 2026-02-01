const WALLET_CACHE_KEY = "chiral_wallet_cache";

export type WalletSource = "create" | "import" | "keystore";

export type WalletCacheEntry = {
  address: string;
  name?: string;
  source: WalletSource;
  createdAt: number;
  lastUsed: number;
};

type WalletCache = Record<string, WalletCacheEntry>;

function readCache(): WalletCache {
  try {
    const raw = localStorage.getItem(WALLET_CACHE_KEY);
    return raw ? JSON.parse(raw) : {};
  } catch (error) {
    console.error("Error reading wallet cache:", error);
    return {};
  }
}

function writeCache(cache: WalletCache): void {
  try {
    localStorage.setItem(WALLET_CACHE_KEY, JSON.stringify(cache));
  } catch (error) {
    console.error("Error writing wallet cache:", error);
  }
}

export function saveWalletMetadata(
  address: string,
  options?: { name?: string; source?: WalletSource },
): void {
  if (!address) return;
  const normalized = address.toLowerCase();
  const now = Date.now();
  const cache = readCache();
  const existing = cache[normalized];

  cache[normalized] = {
    address,
    name: options?.name ?? existing?.name,
    source: options?.source ?? existing?.source ?? "keystore",
    createdAt: existing?.createdAt ?? now,
    lastUsed: now,
  };

  writeCache(cache);
}

export function touchWalletLastUsed(address: string): void {
  if (!address) return;
  const normalized = address.toLowerCase();
  const cache = readCache();
  const existing = cache[normalized];
  if (!existing) return;

  cache[normalized] = {
    ...existing,
    lastUsed: Date.now(),
  };

  writeCache(cache);
}

export function removeWalletMetadata(address: string): void {
  if (!address) return;
  const normalized = address.toLowerCase();
  const cache = readCache();
  if (!cache[normalized]) return;
  delete cache[normalized];
  writeCache(cache);
}
