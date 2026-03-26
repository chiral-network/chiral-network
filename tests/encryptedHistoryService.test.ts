import { describe, it, expect, beforeEach, vi } from 'vitest';
import type { FileTransfer } from '$lib/chiralDropStore';

// ---- Mocks ----

// Mock logger
vi.mock('$lib/logger', () => ({
  logger: () => ({
    info: vi.fn(),
    warn: vi.fn(),
    error: vi.fn(),
    debug: vi.fn(),
    ok: vi.fn(),
  }),
}));

// walletAccount mock — we control the value returned by `get(walletAccount)`
const mockWalletStore = {
  subscribe: vi.fn((cb: (val: any) => void) => {
    cb(mockWalletStore._value);
    return () => {};
  }),
  _value: null as any,
};

vi.mock('$lib/stores', () => ({
  walletAccount: mockWalletStore,
}));

// Mock @tauri-apps/api/core (also in setup.ts, but we control per-test here)
const mockInvoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
  invoke: (...args: any[]) => mockInvoke(...args),
}));

// ---- Helpers ----

function makeTransfer(overrides: Partial<FileTransfer> = {}): FileTransfer {
  return {
    id: `transfer_${Date.now()}_${Math.random().toString(36).slice(2, 9)}`,
    fileName: 'test.txt',
    fileSize: 1024,
    fromPeerId: 'peer1',
    fromAlias: { displayName: 'Alice', emoji: 'A', color: '#ff0000' },
    toPeerId: 'peer2',
    toAlias: { displayName: 'Bob', emoji: 'B', color: '#0000ff' },
    status: 'completed',
    direction: 'outgoing',
    timestamp: Date.now(),
    ...overrides,
  } as FileTransfer;
}

const TEST_WALLET = {
  address: '0xAbCdEf1234567890AbCdEf1234567890AbCdEf12',
  privateKey: 'deadbeef1234567890abcdef1234567890abcdef1234567890abcdef12345678',
};

const TEST_WALLET_2 = {
  address: '0x9876543210FeDcBa9876543210FeDcBa98765432',
  privateKey: 'cafebabe1234567890abcdef1234567890abcdef1234567890abcdef12345678',
};

function setWallet(wallet: { address: string; privateKey: string } | null) {
  mockWalletStore._value = wallet;
}

// Simulate Tauri environment
function enableTauri() {
  (window as any).__TAURI__ = {};
}

function disableTauri() {
  delete (window as any).__TAURI__;
}

// ---- Tests ----

describe('encryptedHistoryService', () => {
  beforeEach(() => {
    vi.resetModules();
    vi.clearAllMocks();
    localStorage.clear();
    mockInvoke.mockReset();
    setWallet(null);
    disableTauri();
  });

  // ----------------------------------------------------------------
  // getLocalCacheKey
  // ----------------------------------------------------------------
  describe('getLocalCacheKey', () => {
    it('should return a wallet-specific lowercase key', async () => {
      // getLocalCacheKey is not exported, so we test it indirectly via saveHistoryToDht
      // which stores to localStorage using getLocalCacheKey(wallet.address).
      enableTauri();
      setWallet(TEST_WALLET);
      mockInvoke.mockResolvedValue(undefined);

      const { saveHistoryToDht } = await import('$lib/encryptedHistoryService');
      const transfer = makeTransfer({ id: 'key-test-1' });
      await saveHistoryToDht([transfer]);

      const expectedKey = `chiraldrop_history_encrypted_${TEST_WALLET.address.toLowerCase()}`;
      const stored = localStorage.getItem(expectedKey);
      expect(stored).not.toBeNull();
      // Uppercase variant should not exist
      const wrongKey = `chiraldrop_history_encrypted_${TEST_WALLET.address}`;
      if (wrongKey !== expectedKey) {
        expect(localStorage.getItem(wrongKey)).toBeNull();
      }
    });

    it('should produce different keys for different wallets', async () => {
      enableTauri();
      mockInvoke.mockResolvedValue(undefined);

      // Save with wallet 1
      setWallet(TEST_WALLET);
      const mod1 = await import('$lib/encryptedHistoryService');
      await mod1.saveHistoryToDht([makeTransfer({ id: 'w1-t' })]);

      const key1 = `chiraldrop_history_encrypted_${TEST_WALLET.address.toLowerCase()}`;
      const key2 = `chiraldrop_history_encrypted_${TEST_WALLET_2.address.toLowerCase()}`;

      expect(key1).not.toBe(key2);
      expect(localStorage.getItem(key1)).not.toBeNull();
      expect(localStorage.getItem(key2)).toBeNull();
    });
  });

  // ----------------------------------------------------------------
  // loadHistoryFromDht
  // ----------------------------------------------------------------
  describe('loadHistoryFromDht', () => {
    it('should return empty array when no wallet is connected', async () => {
      setWallet(null);
      const { loadHistoryFromDht } = await import('$lib/encryptedHistoryService');
      const result = await loadHistoryFromDht();
      expect(result).toEqual([]);
    });

    it('should load unencrypted fallback from localStorage when no wallet', async () => {
      setWallet(null);
      const transfers = [makeTransfer({ id: 'local-1' })];
      localStorage.setItem('chiraldrop_history_plain', JSON.stringify(transfers));

      const { loadHistoryFromDht } = await import('$lib/encryptedHistoryService');
      const result = await loadHistoryFromDht();
      expect(result).toHaveLength(1);
      expect(result[0].id).toBe('local-1');
    });

    it('should load from DHT when Tauri is available', async () => {
      enableTauri();
      setWallet(TEST_WALLET);

      // First save so we have valid encrypted data
      mockInvoke.mockResolvedValue(undefined);
      const { saveHistoryToDht, loadHistoryFromDht } = await import('$lib/encryptedHistoryService');

      const transfers = [makeTransfer({ id: 'dht-1' })];
      await saveHistoryToDht(transfers);

      // Capture what was stored in the invoke call
      const storeCalls = mockInvoke.mock.calls.filter(
        (c: any[]) => c[0] === 'store_dht_value'
      );
      expect(storeCalls).toHaveLength(1);
      const encryptedData = storeCalls[0][1].value;

      // Now simulate DHT returning that encrypted data
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'get_dht_value') return Promise.resolve(encryptedData);
        return Promise.resolve(undefined);
      });

      const result = await loadHistoryFromDht();
      expect(result).toHaveLength(1);
      expect(result[0].id).toBe('dht-1');
    });

    it('should fall back to localStorage cache when DHT lookup fails', async () => {
      enableTauri();
      setWallet(TEST_WALLET);

      // First, save valid encrypted data to localStorage cache
      mockInvoke.mockResolvedValue(undefined);
      const { saveHistoryToDht, loadHistoryFromDht } = await import('$lib/encryptedHistoryService');

      const transfers = [makeTransfer({ id: 'fallback-1' })];
      await saveHistoryToDht(transfers);

      // Now simulate DHT lookup throwing an error
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'get_dht_value') return Promise.reject(new Error('DHT offline'));
        return Promise.resolve(undefined);
      });

      const result = await loadHistoryFromDht();
      expect(result).toHaveLength(1);
      expect(result[0].id).toBe('fallback-1');
    });

    it('should return empty array when no data exists anywhere', async () => {
      enableTauri();
      setWallet(TEST_WALLET);

      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'get_dht_value') return Promise.reject(new Error('not found'));
        return Promise.resolve(undefined);
      });

      const { loadHistoryFromDht } = await import('$lib/encryptedHistoryService');
      const result = await loadHistoryFromDht();
      expect(result).toEqual([]);
    });

    it('should clear stale cache and fall back to plain local on decryption error', async () => {
      enableTauri();
      setWallet(TEST_WALLET);

      // Put garbage encrypted data in the local cache
      const cacheKey = `chiraldrop_history_encrypted_${TEST_WALLET.address.toLowerCase()}`;
      localStorage.setItem(cacheKey, 'not-valid-encrypted-data');

      // Put plain fallback data
      const plainTransfers = [makeTransfer({ id: 'plain-fallback' })];
      localStorage.setItem('chiraldrop_history_plain', JSON.stringify(plainTransfers));

      // DHT returns nothing
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'get_dht_value') return Promise.reject(new Error('not found'));
        return Promise.resolve(undefined);
      });

      const { loadHistoryFromDht } = await import('$lib/encryptedHistoryService');
      const result = await loadHistoryFromDht();

      // Stale cache should be cleared
      expect(localStorage.getItem(cacheKey)).toBeNull();
      // Should fall back to plain local
      expect(result).toHaveLength(1);
      expect(result[0].id).toBe('plain-fallback');
    });

    it('should fall back to localStorage when not in Tauri environment', async () => {
      disableTauri();
      setWallet(TEST_WALLET);

      // We need valid encrypted data in localStorage. Use Tauri temporarily to create it.
      enableTauri();
      mockInvoke.mockResolvedValue(undefined);
      const mod = await import('$lib/encryptedHistoryService');
      const transfers = [makeTransfer({ id: 'no-tauri-1' })];
      await mod.saveHistoryToDht(transfers);

      // Now disable Tauri and reload module
      disableTauri();
      vi.resetModules();
      // Re-mock since we reset modules
      const { loadHistoryFromDht } = await import('$lib/encryptedHistoryService');
      const result = await loadHistoryFromDht();
      expect(result).toHaveLength(1);
      expect(result[0].id).toBe('no-tauri-1');
    });
  });

  // ----------------------------------------------------------------
  // saveHistoryToDht
  // ----------------------------------------------------------------
  describe('saveHistoryToDht', () => {
    it('should encrypt and save to DHT via invoke', async () => {
      enableTauri();
      setWallet(TEST_WALLET);
      mockInvoke.mockResolvedValue(undefined);

      const { saveHistoryToDht } = await import('$lib/encryptedHistoryService');
      const transfers = [makeTransfer({ id: 'save-1' })];
      await saveHistoryToDht(transfers);

      expect(mockInvoke).toHaveBeenCalledWith('store_dht_value', {
        key: `chiraldrop_history_${TEST_WALLET.address.toLowerCase()}`,
        value: expect.any(String),
      });
    });

    it('should also cache locally with wallet-specific key', async () => {
      enableTauri();
      setWallet(TEST_WALLET);
      mockInvoke.mockResolvedValue(undefined);

      const { saveHistoryToDht } = await import('$lib/encryptedHistoryService');
      await saveHistoryToDht([makeTransfer({ id: 'cache-1' })]);

      const cacheKey = `chiraldrop_history_encrypted_${TEST_WALLET.address.toLowerCase()}`;
      expect(localStorage.getItem(cacheKey)).not.toBeNull();
    });

    it('should fall back to unencrypted localStorage when no wallet', async () => {
      setWallet(null);

      const { saveHistoryToDht } = await import('$lib/encryptedHistoryService');
      const transfers = [makeTransfer({ id: 'no-wallet-1' })];
      await saveHistoryToDht(transfers);

      // Should not invoke DHT
      expect(mockInvoke).not.toHaveBeenCalled();

      // Should save to plain localStorage
      const stored = localStorage.getItem('chiraldrop_history_plain');
      expect(stored).not.toBeNull();
      const parsed = JSON.parse(stored!);
      expect(parsed).toHaveLength(1);
      expect(parsed[0].id).toBe('no-wallet-1');
    });

    it('should fall back to localStorage on DHT invoke error', async () => {
      enableTauri();
      setWallet(TEST_WALLET);
      mockInvoke.mockRejectedValue(new Error('DHT write failed'));

      // Mock crypto.subtle to also fail so the whole try block errors out
      const originalImportKey = crypto.subtle.importKey;
      vi.spyOn(crypto.subtle, 'importKey').mockRejectedValueOnce(
        new Error('Crypto error')
      );

      const { saveHistoryToDht } = await import('$lib/encryptedHistoryService');
      const transfers = [makeTransfer({ id: 'error-save-1' })];
      await saveHistoryToDht(transfers);

      // Should fall back to plain localStorage
      const stored = localStorage.getItem('chiraldrop_history_plain');
      expect(stored).not.toBeNull();
      const parsed = JSON.parse(stored!);
      expect(parsed[0].id).toBe('error-save-1');

      vi.spyOn(crypto.subtle, 'importKey').mockRestore();
    });

    it('should produce encrypted (non-plaintext) data in cache', async () => {
      enableTauri();
      setWallet(TEST_WALLET);
      mockInvoke.mockResolvedValue(undefined);

      const { saveHistoryToDht } = await import('$lib/encryptedHistoryService');
      const transfers = [makeTransfer({ id: 'enc-check' })];
      await saveHistoryToDht(transfers);

      const cacheKey = `chiraldrop_history_encrypted_${TEST_WALLET.address.toLowerCase()}`;
      const stored = localStorage.getItem(cacheKey)!;

      // The stored value should be base64, not readable JSON
      expect(() => JSON.parse(stored)).toThrow();
      expect(stored).not.toContain('enc-check');
    });
  });

  // ----------------------------------------------------------------
  // syncHistory
  // ----------------------------------------------------------------
  describe('syncHistory', () => {
    it('should return local history when no wallet is connected', async () => {
      setWallet(null);

      const { syncHistory } = await import('$lib/encryptedHistoryService');
      const local = [makeTransfer({ id: 'local-only' })];
      const result = await syncHistory(local);
      expect(result).toEqual(local);
    });

    it('should merge local and DHT histories deduping by ID', async () => {
      enableTauri();
      setWallet(TEST_WALLET);
      mockInvoke.mockResolvedValue(undefined);

      const { saveHistoryToDht, syncHistory } = await import('$lib/encryptedHistoryService');

      // Save some history to DHT first
      const dhtTransfers = [
        makeTransfer({ id: 'shared', timestamp: 1000 }),
        makeTransfer({ id: 'dht-only', timestamp: 900 }),
      ];
      await saveHistoryToDht(dhtTransfers);

      // Capture encrypted data for the DHT mock
      const storeCalls = mockInvoke.mock.calls.filter(
        (c: any[]) => c[0] === 'store_dht_value'
      );
      const encryptedData = storeCalls[0][1].value;

      // Mock DHT to return the saved data, then accept the merged save
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'get_dht_value') return Promise.resolve(encryptedData);
        return Promise.resolve(undefined);
      });

      const localTransfers = [
        makeTransfer({ id: 'shared', timestamp: 2000 }), // newer
        makeTransfer({ id: 'local-only', timestamp: 1500 }),
      ];

      const result = await syncHistory(localTransfers);

      // Should have 3 unique entries
      expect(result).toHaveLength(3);
      const ids = result.map((t: any) => t.id);
      expect(ids).toContain('shared');
      expect(ids).toContain('dht-only');
      expect(ids).toContain('local-only');

      // 'shared' should use local version (timestamp 2000 > 1000)
      const shared = result.find((t: any) => t.id === 'shared');
      expect(shared!.timestamp).toBe(2000);
    });

    it('should prefer DHT version when it is more recent', async () => {
      enableTauri();
      setWallet(TEST_WALLET);
      mockInvoke.mockResolvedValue(undefined);

      const { saveHistoryToDht, syncHistory } = await import('$lib/encryptedHistoryService');

      // DHT has newer version
      const dhtTransfers = [
        makeTransfer({ id: 'dup', timestamp: 5000, status: 'completed' }),
      ];
      await saveHistoryToDht(dhtTransfers);

      const storeCalls = mockInvoke.mock.calls.filter(
        (c: any[]) => c[0] === 'store_dht_value'
      );
      const encryptedData = storeCalls[0][1].value;

      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'get_dht_value') return Promise.resolve(encryptedData);
        return Promise.resolve(undefined);
      });

      // Local has older version
      const localTransfers = [
        makeTransfer({ id: 'dup', timestamp: 1000, status: 'pending' }),
      ];

      const result = await syncHistory(localTransfers);
      expect(result).toHaveLength(1);
      // DHT entry is added first, then local tries to overwrite but 1000 < 5000
      // so the DHT version (timestamp 5000) should remain
      expect(result[0].timestamp).toBe(5000);
    });

    it('should sort merged results by timestamp descending', async () => {
      enableTauri();
      setWallet(TEST_WALLET);
      mockInvoke.mockResolvedValue(undefined);

      const { saveHistoryToDht, syncHistory } = await import('$lib/encryptedHistoryService');

      const dhtTransfers = [
        makeTransfer({ id: 't1', timestamp: 1000 }),
        makeTransfer({ id: 't3', timestamp: 3000 }),
      ];
      await saveHistoryToDht(dhtTransfers);

      const storeCalls = mockInvoke.mock.calls.filter(
        (c: any[]) => c[0] === 'store_dht_value'
      );
      const encryptedData = storeCalls[0][1].value;

      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'get_dht_value') return Promise.resolve(encryptedData);
        return Promise.resolve(undefined);
      });

      const localTransfers = [
        makeTransfer({ id: 't2', timestamp: 2000 }),
      ];

      const result = await syncHistory(localTransfers);
      expect(result).toHaveLength(3);
      expect(result[0].timestamp).toBe(3000);
      expect(result[1].timestamp).toBe(2000);
      expect(result[2].timestamp).toBe(1000);
    });

    it('should save merged history back to DHT', async () => {
      enableTauri();
      setWallet(TEST_WALLET);
      mockInvoke.mockResolvedValue(undefined);

      const { saveHistoryToDht, syncHistory } = await import('$lib/encryptedHistoryService');

      // No DHT data
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'get_dht_value') return Promise.reject(new Error('not found'));
        return Promise.resolve(undefined);
      });

      const localTransfers = [makeTransfer({ id: 'save-back' })];
      await syncHistory(localTransfers);

      // store_dht_value should have been called (saving merged result)
      const storeCalls = mockInvoke.mock.calls.filter(
        (c: any[]) => c[0] === 'store_dht_value'
      );
      expect(storeCalls.length).toBeGreaterThanOrEqual(1);
    });

    it('should return local history on sync error', async () => {
      enableTauri();
      setWallet(TEST_WALLET);

      // Make crypto.subtle.importKey fail to trigger catch
      vi.spyOn(crypto.subtle, 'importKey').mockRejectedValueOnce(
        new Error('Crypto unavailable')
      );

      const { syncHistory } = await import('$lib/encryptedHistoryService');
      const local = [makeTransfer({ id: 'error-sync' })];
      const result = await syncHistory(local);
      expect(result).toEqual(local);

      vi.spyOn(crypto.subtle, 'importKey').mockRestore();
    });
  });

  // ----------------------------------------------------------------
  // Wallet switch scenario
  // ----------------------------------------------------------------
  describe('wallet switch scenario', () => {
    it('should use different cache keys for different wallets', async () => {
      enableTauri();
      mockInvoke.mockResolvedValue(undefined);

      const { saveHistoryToDht } = await import('$lib/encryptedHistoryService');

      // Save with wallet 1
      setWallet(TEST_WALLET);
      await saveHistoryToDht([makeTransfer({ id: 'w1-transfer' })]);

      const key1 = `chiraldrop_history_encrypted_${TEST_WALLET.address.toLowerCase()}`;
      expect(localStorage.getItem(key1)).not.toBeNull();

      // Save with wallet 2
      setWallet(TEST_WALLET_2);
      await saveHistoryToDht([makeTransfer({ id: 'w2-transfer' })]);

      const key2 = `chiraldrop_history_encrypted_${TEST_WALLET_2.address.toLowerCase()}`;
      expect(localStorage.getItem(key2)).not.toBeNull();

      // Both keys should exist and be different
      expect(key1).not.toBe(key2);
      expect(localStorage.getItem(key1)).not.toBe(localStorage.getItem(key2));
    });

    it('should not decrypt data from a different wallet cache', async () => {
      enableTauri();
      mockInvoke.mockResolvedValue(undefined);

      // Save with wallet 1
      setWallet(TEST_WALLET);
      const mod = await import('$lib/encryptedHistoryService');
      await mod.saveHistoryToDht([makeTransfer({ id: 'cross-wallet' })]);

      // Copy wallet 1's cache to wallet 2's key (simulating stale/wrong cache)
      const key1 = `chiraldrop_history_encrypted_${TEST_WALLET.address.toLowerCase()}`;
      const key2 = `chiraldrop_history_encrypted_${TEST_WALLET_2.address.toLowerCase()}`;
      const wallet1Data = localStorage.getItem(key1)!;
      localStorage.setItem(key2, wallet1Data);

      // Put plain fallback
      localStorage.setItem(
        'chiraldrop_history_plain',
        JSON.stringify([makeTransfer({ id: 'plain-fallback-2' })])
      );

      // Switch to wallet 2 and try to load
      setWallet(TEST_WALLET_2);
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'get_dht_value') return Promise.reject(new Error('not found'));
        return Promise.resolve(undefined);
      });

      const result = await mod.loadHistoryFromDht();

      // Decryption should fail (different key), stale cache cleared, falls back to plain
      expect(localStorage.getItem(key2)).toBeNull();
      expect(result).toHaveLength(1);
      expect(result[0].id).toBe('plain-fallback-2');
    });

    it('should isolate history per wallet during sync', async () => {
      enableTauri();
      mockInvoke.mockResolvedValue(undefined);

      const { saveHistoryToDht, syncHistory } = await import('$lib/encryptedHistoryService');

      // Save wallet 1 history
      setWallet(TEST_WALLET);
      const w1Transfers = [makeTransfer({ id: 'w1-sync', timestamp: 1000 })];
      await saveHistoryToDht(w1Transfers);

      // Capture wallet 1 encrypted data
      const w1StoreCalls = mockInvoke.mock.calls.filter(
        (c: any[]) => c[0] === 'store_dht_value'
      );
      const w1Encrypted = w1StoreCalls[0][1].value;

      // Switch to wallet 2
      setWallet(TEST_WALLET_2);
      mockInvoke.mockReset();

      // DHT returns nothing for wallet 2
      mockInvoke.mockImplementation((cmd: string) => {
        if (cmd === 'get_dht_value') return Promise.reject(new Error('not found'));
        return Promise.resolve(undefined);
      });

      const w2Local = [makeTransfer({ id: 'w2-sync', timestamp: 2000 })];
      const result = await syncHistory(w2Local);

      // Should only contain wallet 2's local data, not wallet 1's
      expect(result).toHaveLength(1);
      expect(result[0].id).toBe('w2-sync');
    });
  });
});
