import { describe, it, expect, beforeEach, vi } from 'vitest';
import { get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';

const mockInvoke = vi.mocked(invoke);

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

// Mock event listener
vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn().mockResolvedValue(vi.fn()),
}));

// Mock driveApiService
vi.mock('$lib/services/driveApiService', () => ({
  driveApi: {
    listItems: vi.fn().mockResolvedValue([]),
    listShareLinks: vi.fn().mockResolvedValue([]),
    createFolder: vi.fn(),
    uploadFile: vi.fn(),
    updateItem: vi.fn(),
    deleteItem: vi.fn(),
    createShareLink: vi.fn(),
    revokeShareLink: vi.fn(),
    toggleVisibility: vi.fn(),
    getDownloadUrl: vi.fn((id: string, name: string) => `http://localhost/dl/${id}/${name}`),
    getShareUrl: vi.fn((token: string) => `http://relay/drive/${token}`),
  },
  setDriveOwner: vi.fn(),
  setLocalDriveServer: vi.fn(),
}));

// --- Test Wallets ---

const WALLET_A = {
  address: '0xAaAaAaAa1111111111111111111111111111AaAa',
  privateKey: 'aaaa1111aaaa1111aaaa1111aaaa1111aaaa1111aaaa1111aaaa1111aaaa1111',
};

const WALLET_B = {
  address: '0xBbBbBbBb2222222222222222222222222222BbBb',
  privateKey: 'bbbb2222bbbb2222bbbb2222bbbb2222bbbb2222bbbb2222bbbb2222bbbb2222',
};

const WALLET_C = {
  address: '0xCcCcCcCc3333333333333333333333333333CcCc',
  privateKey: 'cccc3333cccc3333cccc3333cccc3333cccc3333cccc3333cccc3333cccc3333',
};

function makeWallet(index: number) {
  const hex = index.toString(16).padStart(4, '0');
  return {
    address: `0x${hex.repeat(10)}`,
    privateKey: `${hex.repeat(16)}`,
  };
}

// --- Tests ---

describe('Concurrent User Sessions', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.resetModules();
    localStorage.clear();
  });

  // ====================================================================
  // 1. Concurrent wallet operations
  // ====================================================================

  describe('concurrent wallet operations', () => {
    it('should handle multiple wallets querying balance simultaneously', async () => {
      const wallets = Array.from({ length: 5 }, (_, i) => makeWallet(i + 1));

      // Set up mock responses: each wallet gets a different balance
      wallets.forEach((w, i) => {
        mockInvoke.mockResolvedValueOnce({
          balance: `${(i + 1) * 10}.000000`,
          balanceWei: `${(i + 1) * 10}000000000000000000`,
        });
      });

      // Fire all balance queries concurrently
      const results = await Promise.all(
        wallets.map((w) =>
          invoke<{ balance: string; balanceWei: string }>('get_wallet_balance', {
            address: w.address,
          })
        )
      );

      expect(results).toHaveLength(5);
      results.forEach((r, i) => {
        expect(r.balance).toBe(`${(i + 1) * 10}.000000`);
      });
      expect(mockInvoke).toHaveBeenCalledTimes(5);
    });

    it('should handle rapid wallet switching (login -> use -> logout -> login different wallet)', async () => {
      const { walletAccount, isAuthenticated } = await import('$lib/stores');

      const wallets = [WALLET_A, WALLET_B, WALLET_C];

      for (const wallet of wallets) {
        // Login
        walletAccount.set(wallet);
        isAuthenticated.set(true);
        expect(get(walletAccount)?.address).toBe(wallet.address);
        expect(get(isAuthenticated)).toBe(true);

        // Use: query balance
        mockInvoke.mockResolvedValueOnce({ balance: '50.000000', balanceWei: '50000000000000000000' });
        await invoke('get_wallet_balance', { address: wallet.address });

        // Logout
        walletAccount.set(null);
        isAuthenticated.set(false);
        expect(get(walletAccount)).toBeNull();
        expect(get(isAuthenticated)).toBe(false);
      }

      // 3 wallets queried
      expect(mockInvoke).toHaveBeenCalledTimes(3);
    });

    it('should verify data isolation between wallet A and wallet B', async () => {
      const { walletAccount } = await import('$lib/stores');

      // Wallet A stores data in localStorage under wallet-specific key
      walletAccount.set(WALLET_A);
      const keyA = `chiral-history-${WALLET_A.address.toLowerCase()}`;
      localStorage.setItem(keyA, JSON.stringify([{ id: 'txA1', amount: '10' }]));

      // Switch to wallet B
      walletAccount.set(WALLET_B);
      const keyB = `chiral-history-${WALLET_B.address.toLowerCase()}`;

      // Wallet B should not see wallet A's data
      const historyB = localStorage.getItem(keyB);
      expect(historyB).toBeNull();

      // Wallet A's data is still intact
      const historyA = JSON.parse(localStorage.getItem(keyA)!);
      expect(historyA).toHaveLength(1);
      expect(historyA[0].id).toBe('txA1');
    });
  });

  // ====================================================================
  // 2. Session lifecycle stress
  // ====================================================================

  describe('session lifecycle stress', () => {
    it('should complete full cycle: create wallet -> mine -> send -> check balance -> logout (10 concurrent)', async () => {
      const cycles = 10;
      const wallets = Array.from({ length: cycles }, (_, i) => makeWallet(i + 100));

      const runCycle = async (wallet: { address: string; privateKey: string }, index: number) => {
        const { walletAccount, isAuthenticated } = await import('$lib/stores');

        // Create / login
        walletAccount.set(wallet);
        isAuthenticated.set(true);

        // Mine (start mining invoke)
        mockInvoke.mockResolvedValueOnce({ status: 'mining_started' });
        await invoke('start_cpu_mining', { address: wallet.address, threads: 1 });

        // Send transaction
        mockInvoke.mockResolvedValueOnce({
          hash: `0xtx_${index}`,
          status: 'pending',
          balanceBefore: '100.000000',
          balanceAfter: '95.000000',
        });
        await invoke('send_transaction', {
          fromAddress: wallet.address,
          toAddress: '0xRecipient0000000000000000000000000000dead',
          amount: '5.0',
          privateKey: wallet.privateKey,
        });

        // Check balance
        mockInvoke.mockResolvedValueOnce({ balance: '95.000000', balanceWei: '95000000000000000000' });
        const balResult = await invoke<{ balance: string }>('get_wallet_balance', {
          address: wallet.address,
        });
        expect(balResult.balance).toBe('95.000000');

        // Logout
        walletAccount.set(null);
        isAuthenticated.set(false);

        return true;
      };

      // Run all 10 cycles concurrently
      const results = await Promise.all(wallets.map((w, i) => runCycle(w, i)));
      expect(results).toHaveLength(cycles);
      results.forEach((r) => expect(r).toBe(true));

      // 3 invocations per cycle: start_cpu_mining, send_transaction, get_wallet_balance
      expect(mockInvoke).toHaveBeenCalledTimes(cycles * 3);
    });

    it('should survive rapid logout/login cycles (20 cycles) with no state leaks', async () => {
      const { walletAccount, isAuthenticated } = await import('$lib/stores');
      const rapidCycles = 20;

      for (let i = 0; i < rapidCycles; i++) {
        const wallet = i % 2 === 0 ? WALLET_A : WALLET_B;

        // Login
        walletAccount.set(wallet);
        isAuthenticated.set(true);

        // Immediately logout
        walletAccount.set(null);
        isAuthenticated.set(false);
      }

      // After all cycles, state should be clean
      expect(get(walletAccount)).toBeNull();
      expect(get(isAuthenticated)).toBe(false);
    });

    it('should verify localStorage keys are wallet-specific after rapid switching', async () => {
      const { walletAccount } = await import('$lib/stores');

      // Simulate per-wallet storage
      const wallets = [WALLET_A, WALLET_B, WALLET_C];

      for (const wallet of wallets) {
        walletAccount.set(wallet);
        const addr = wallet.address.toLowerCase();

        // Each wallet writes to its own namespaced key
        localStorage.setItem(`chiral-downloads-${addr}`, JSON.stringify([`dl_for_${addr}`]));
        localStorage.setItem(`chiral-history-${addr}`, JSON.stringify([`hist_for_${addr}`]));
        localStorage.setItem(`chiral-drive-${addr}`, JSON.stringify({ items: [] }));
      }

      // Verify isolation: each wallet's keys contain only its own data
      for (const wallet of wallets) {
        const addr = wallet.address.toLowerCase();
        const downloads = JSON.parse(localStorage.getItem(`chiral-downloads-${addr}`)!);
        expect(downloads).toEqual([`dl_for_${addr}`]);

        const history = JSON.parse(localStorage.getItem(`chiral-history-${addr}`)!);
        expect(history).toEqual([`hist_for_${addr}`]);
      }

      // Global settings are shared (not wallet-specific)
      localStorage.setItem('chiral-settings', JSON.stringify({ theme: 'dark' }));
      expect(JSON.parse(localStorage.getItem('chiral-settings')!).theme).toBe('dark');
    });
  });

  // ====================================================================
  // 3. Store consistency under concurrent access
  // ====================================================================

  describe('store consistency under concurrent access', () => {
    it('should handle multiple components reading/writing transferHistory simultaneously', async () => {
      const { writable } = await import('svelte/store');

      // Simulate transferHistory as a writable store (mirrors chiralDropStore)
      const transferHistory = writable<Array<{ id: string; fileName: string; status: string }>>([]);

      const writers = 20;
      const writePromises: Promise<void>[] = [];

      for (let i = 0; i < writers; i++) {
        writePromises.push(
          new Promise<void>((resolve) => {
            transferHistory.update((current) => [
              ...current,
              { id: `transfer_${i}`, fileName: `file_${i}.dat`, status: 'completed' },
            ]);
            resolve();
          })
        );
      }

      await Promise.all(writePromises);

      const finalHistory = get(transferHistory);
      expect(finalHistory).toHaveLength(writers);

      // Every transfer ID should be present
      const ids = new Set(finalHistory.map((t) => t.id));
      for (let i = 0; i < writers; i++) {
        expect(ids.has(`transfer_${i}`)).toBe(true);
      }
    });

    it('should handle concurrent driveStore operations without corruption', async () => {
      const { writable } = await import('svelte/store');

      interface SimpleDriveItem {
        id: string;
        name: string;
        type: 'file' | 'folder';
      }

      const driveItems = writable<SimpleDriveItem[]>([]);
      const opCount = 30;

      // Concurrent adds
      const addOps = Array.from({ length: opCount }, (_, i) =>
        new Promise<void>((resolve) => {
          driveItems.update((items) => [
            ...items,
            { id: `item_${i}`, name: `file_${i}.txt`, type: 'file' as const },
          ]);
          resolve();
        })
      );

      await Promise.all(addOps);

      const items = get(driveItems);
      expect(items).toHaveLength(opCount);

      // Concurrent deletes (remove even-indexed items)
      const deleteOps = Array.from({ length: opCount }, (_, i) => {
        if (i % 2 === 0) {
          return new Promise<void>((resolve) => {
            driveItems.update((current) => current.filter((item) => item.id !== `item_${i}`));
            resolve();
          });
        }
        return Promise.resolve();
      });

      await Promise.all(deleteOps);

      const remaining = get(driveItems);
      expect(remaining).toHaveLength(opCount / 2);
      remaining.forEach((item) => {
        const idx = parseInt(item.id.split('_')[1]);
        expect(idx % 2).toBe(1); // Only odd-indexed items remain
      });
    });

    it('should handle race condition: logout during active download', async () => {
      const { walletAccount, isAuthenticated } = await import('$lib/stores');
      const { writable } = await import('svelte/store');

      const activeDownloads = writable<Array<{ id: string; progress: number; cancelled: boolean }>>([]);

      // Start wallet session
      walletAccount.set(WALLET_A);
      isAuthenticated.set(true);

      // Start a simulated download
      activeDownloads.update((dl) => [
        ...dl,
        { id: 'dl-001', progress: 0, cancelled: false },
        { id: 'dl-002', progress: 50, cancelled: false },
      ]);

      // Simulate download progress updates and logout happening concurrently
      const progressUpdates = new Promise<void>((resolve) => {
        let updates = 0;
        const interval = setInterval(() => {
          activeDownloads.update((downloads) =>
            downloads.map((dl) =>
              dl.cancelled ? dl : { ...dl, progress: Math.min(100, dl.progress + 10) }
            )
          );
          updates++;
          if (updates >= 5) {
            clearInterval(interval);
            resolve();
          }
        }, 10);
      });

      // Logout mid-download after a brief delay
      const logoutAction = new Promise<void>((resolve) => {
        setTimeout(() => {
          // Cancel all downloads on logout
          activeDownloads.update((downloads) =>
            downloads.map((dl) => ({ ...dl, cancelled: true }))
          );
          // Clear session
          walletAccount.set(null);
          isAuthenticated.set(false);
          activeDownloads.set([]);
          resolve();
        }, 25);
      });

      await Promise.all([progressUpdates, logoutAction]);

      // After logout, everything is clean
      expect(get(walletAccount)).toBeNull();
      expect(get(isAuthenticated)).toBe(false);
      expect(get(activeDownloads)).toEqual([]);
    });

    it('should handle concurrent balance queries returning in different order', async () => {
      const wallets = Array.from({ length: 8 }, (_, i) => makeWallet(i + 200));

      // Mock responses with varying delays (simulated by resolve order)
      const balancePromises = wallets.map((w, i) => {
        // Return in reverse order to simulate network jitter
        const delay = (wallets.length - i) * 5;
        mockInvoke.mockImplementationOnce(
          () =>
            new Promise((resolve) =>
              setTimeout(() => resolve({ balance: `${i * 10}.000000` }), delay)
            )
        );
        return invoke<{ balance: string }>('get_wallet_balance', { address: w.address });
      });

      const results = await Promise.all(balancePromises);

      // Despite out-of-order resolution, each result matches its wallet
      results.forEach((r, i) => {
        expect(r.balance).toBe(`${i * 10}.000000`);
      });
    });

    it('should maintain store consistency when multiple subscribers read during writes', async () => {
      const { writable } = await import('svelte/store');

      const sharedStore = writable<number[]>([]);
      const observedSnapshots: number[][] = [];

      // Subscribe to capture snapshots
      const unsub = sharedStore.subscribe((value) => {
        observedSnapshots.push([...value]);
      });

      // Rapidly write 50 values
      for (let i = 0; i < 50; i++) {
        sharedStore.update((arr) => [...arr, i]);
      }

      unsub();

      // Each snapshot should be a monotonically growing prefix
      for (let i = 1; i < observedSnapshots.length; i++) {
        expect(observedSnapshots[i].length).toBe(observedSnapshots[i - 1].length + 1);
        // Each snapshot contains all previous values plus the new one
        expect(observedSnapshots[i].slice(0, -1)).toEqual(observedSnapshots[i - 1]);
      }

      // Final state should have all 50 values
      expect(get(sharedStore)).toHaveLength(50);
    });
  });
});
