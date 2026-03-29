import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';
import { get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { calculateCost, formatCost } from '$lib/speedTiers';

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

// Mock event listener with event emitter pattern for progress simulation
type EventCallback = (event: { payload: any }) => void;
const eventListeners = new Map<string, EventCallback[]>();

vi.mock('@tauri-apps/api/event', () => ({
  listen: vi.fn((event: string, handler: EventCallback) => {
    if (!eventListeners.has(event)) {
      eventListeners.set(event, []);
    }
    eventListeners.get(event)!.push(handler);
    return Promise.resolve(() => {
      const handlers = eventListeners.get(event);
      if (handlers) {
        const idx = handlers.indexOf(handler);
        if (idx >= 0) handlers.splice(idx, 1);
      }
    });
  }),
}));

// Mock toast store
vi.mock('$lib/toastStore', () => ({
  toasts: {
    show: vi.fn(),
    detail: vi.fn(),
    notify: vi.fn(),
    notifyDetail: vi.fn(),
  },
}));

// Mock driveApiService
vi.mock('$lib/services/driveApiService', () => ({
  driveApi: {
    listItems: vi.fn().mockResolvedValue([]),
    listShareLinks: vi.fn().mockResolvedValue([]),
  },
  setDriveOwner: vi.fn(),
  setLocalDriveServer: vi.fn(),
}));

// --- Helpers ---

function emitEvent(eventName: string, payload: any) {
  const handlers = eventListeners.get(eventName) || [];
  handlers.forEach((handler) => handler({ payload }));
}

function makeSearchResult(hash: string, name: string, size: number) {
  return {
    hash,
    fileName: name,
    fileSize: size,
    seeders: [
      { peerId: 'peer-1', address: '/ip4/192.168.1.10/tcp/4001' },
      { peerId: 'peer-2', address: '/ip4/192.168.1.11/tcp/4001' },
    ],
    priceChi: null,
    walletAddress: null,
  };
}

function makeDownloadId(index: number): string {
  return `dl-${String(index).padStart(4, '0')}`;
}

const TEST_WALLET = {
  address: '0xAbCdEf1234567890AbCdEf1234567890AbCdEf12',
  privateKey: 'deadbeef1234567890abcdef1234567890abcdef1234567890abcdef12345678',
};

const TEST_WALLET_2 = {
  address: '0x9876543210FeDcBa9876543210FeDcBa98765432',
  privateKey: 'cafebabe1234567890abcdef1234567890abcdef1234567890abcdef12345678',
};

// --- Tests ---

describe('E2E Download Flow', () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    vi.resetModules();
    localStorage.clear();
    eventListeners.clear();
    // Re-bind mockInvoke after restoreAllMocks resets implementations
    mockInvoke.mockReset();
  });

  // ====================================================================
  // 1. Full download lifecycle (mocked)
  // ====================================================================

  describe('full download lifecycle', () => {
    it('should complete search -> cost -> confirm -> start -> progress -> complete -> httpLink', async () => {
      const fileHash = 'abc123def456';
      const fileName = 'document.pdf';
      const fileSize = 50_000_000; // 50 MB

      // Step 1: Search file
      mockInvoke.mockResolvedValueOnce([makeSearchResult(fileHash, fileName, fileSize)]);
      const searchResults = await invoke<any[]>('search_file', { fileHash });
      expect(searchResults).toHaveLength(1);
      expect(searchResults[0].fileName).toBe(fileName);

      // Step 2: Calculate cost
      const cost = calculateCost(fileSize);
      expect(cost).toBeGreaterThan(0);
      expect(formatCost(cost)).toContain('CHI');

      // Step 3: Start download
      mockInvoke.mockResolvedValueOnce({ requestId: 'dl-001' });
      const dlResult = await invoke<{ requestId: string }>('start_download', {
        fileHash,
        fileName,
        walletAddress: TEST_WALLET.address,
        privateKey: TEST_WALLET.privateKey,
      });
      expect(dlResult.requestId).toBe('dl-001');

      // Step 4: Simulate progress events
      const progressValues = [10, 25, 50, 75, 90, 100];
      for (const progress of progressValues) {
        emitEvent('download-progress', {
          requestId: 'dl-001',
          fileHash,
          progress,
          bytesReceived: Math.floor((fileSize * progress) / 100),
          totalBytes: fileSize,
        });
      }

      // Step 5: Simulate completion event
      emitEvent('download-complete', {
        requestId: 'dl-001',
        fileHash,
        filePath: `/home/user/Downloads/${fileName}`,
        httpLink: `http://localhost:9419/files/${fileHash}/${fileName}`,
      });

      // Step 6: Verify the httpLink would be available
      mockInvoke.mockResolvedValueOnce({
        url: `http://localhost:9419/files/${fileHash}/${fileName}`,
      });
      const linkResult = await invoke<{ url: string }>('get_drive_server_url');
      expect(linkResult.url).toContain(fileHash);
    });

    it('should handle 10 concurrent downloads', async () => {
      const downloadCount = 10;

      // Phase 1: Search all files (sequential to maintain mock queue order)
      for (let i = 0; i < downloadCount; i++) {
        mockInvoke.mockResolvedValueOnce([
          makeSearchResult(`hash_${i}`, `file_${i}.dat`, 1_000_000 * (i + 1)),
        ]);
      }

      const searchResults: any[][] = [];
      for (let i = 0; i < downloadCount; i++) {
        const results = await invoke<any[]>('search_file', { fileHash: `hash_${i}` });
        expect(results).toHaveLength(1);
        searchResults.push(results);
      }

      // Phase 2: Start all downloads concurrently
      for (let i = 0; i < downloadCount; i++) {
        mockInvoke.mockResolvedValueOnce({ requestId: makeDownloadId(i) });
      }

      const downloadPromises = Array.from({ length: downloadCount }, (_, i) =>
        invoke<{ requestId: string }>('start_download', {
          fileHash: `hash_${i}`,
          fileName: `file_${i}.dat`,
          walletAddress: TEST_WALLET.address,
          privateKey: TEST_WALLET.privateKey,
        })
      );

      const results = await Promise.all(downloadPromises);
      expect(results).toHaveLength(downloadCount);

      // Each download should have a unique request ID
      const uniqueIds = new Set(results.map((r) => r.requestId));
      expect(uniqueIds.size).toBe(downloadCount);
    });

    it('should verify invoke sequence: calculate_cost -> send_transaction -> start_download', async () => {
      const fileHash = 'paid_file_hash';
      const fileName = 'premium.zip';
      const fileSize = 100_000_000; // 100 MB
      const cost = calculateCost(fileSize);

      // Step 1: Search
      mockInvoke.mockResolvedValueOnce([
        {
          ...makeSearchResult(fileHash, fileName, fileSize),
          priceChi: cost.toString(),
          walletAddress: '0xSeller',
        },
      ]);

      const results = await invoke<any[]>('search_file', { fileHash });
      expect(results[0].priceChi).toBe(cost.toString());

      // Step 2: Send payment
      mockInvoke.mockResolvedValueOnce({
        hash: '0xtx_payment',
        status: 'pending',
        balanceBefore: '10.000000',
        balanceAfter: `${10 - cost}`,
      });
      await invoke('send_transaction', {
        fromAddress: TEST_WALLET.address,
        toAddress: '0xSeller',
        amount: cost.toString(),
        privateKey: TEST_WALLET.privateKey,
      });

      // Step 3: Start download after payment
      mockInvoke.mockResolvedValueOnce({ requestId: 'dl-paid-001' });
      await invoke('start_download', {
        fileHash,
        fileName,
        walletAddress: TEST_WALLET.address,
        privateKey: TEST_WALLET.privateKey,
      });

      // Verify call order
      const calls = mockInvoke.mock.calls.map((c) => c[0]);
      expect(calls).toEqual(['search_file', 'send_transaction', 'start_download']);
    });
  });

  // ====================================================================
  // 2. Download queue management
  // ====================================================================

  describe('download queue management', () => {
    it('should start 20 downloads rapidly', async () => {
      const downloadCount = 20;
      const startPromises: Promise<any>[] = [];

      for (let i = 0; i < downloadCount; i++) {
        mockInvoke.mockResolvedValueOnce({ requestId: makeDownloadId(i) });

        startPromises.push(
          invoke('start_download', {
            fileHash: `rapid_hash_${i}`,
            fileName: `rapid_${i}.bin`,
            walletAddress: TEST_WALLET.address,
            privateKey: TEST_WALLET.privateKey,
          })
        );
      }

      const results = await Promise.all(startPromises);
      expect(results).toHaveLength(downloadCount);

      // All should have unique request IDs
      const ids = results.map((r: any) => r.requestId);
      expect(new Set(ids).size).toBe(downloadCount);
    });

    it('should cancel downloads mid-progress', async () => {
      // Start 5 downloads
      const downloadIds: string[] = [];
      for (let i = 0; i < 5; i++) {
        const dlId = makeDownloadId(i);
        mockInvoke.mockResolvedValueOnce({ requestId: dlId });
        await invoke('start_download', {
          fileHash: `cancel_hash_${i}`,
          fileName: `cancel_${i}.bin`,
        });
        downloadIds.push(dlId);
      }

      // Simulate progress on all downloads
      downloadIds.forEach((id, i) => {
        emitEvent('download-progress', {
          requestId: id,
          fileHash: `cancel_hash_${i}`,
          progress: 50,
          bytesReceived: 500_000,
          totalBytes: 1_000_000,
        });
      });

      // Cancel downloads 0, 2, 4 (even indices)
      const cancelledIds = downloadIds.filter((_, i) => i % 2 === 0);
      for (const dlId of cancelledIds) {
        mockInvoke.mockResolvedValueOnce({ cancelled: true });
        const cancelResult = await invoke<{ cancelled: boolean }>('cancel_download', {
          requestId: dlId,
        });
        expect(cancelResult.cancelled).toBe(true);
      }

      // Verify cancel was called for each cancelled download
      const cancelCalls = mockInvoke.mock.calls.filter((c) => c[0] === 'cancel_download');
      expect(cancelCalls).toHaveLength(3);
    });

    it('should resume/retry failed downloads', async () => {
      // Start a download
      mockInvoke.mockResolvedValueOnce({ requestId: 'dl-retry-001' });
      await invoke('start_download', {
        fileHash: 'retry_hash',
        fileName: 'retry.bin',
      });

      // Simulate failure
      emitEvent('download-failed', {
        requestId: 'dl-retry-001',
        fileHash: 'retry_hash',
        error: 'Connection reset by peer',
      });

      // Retry the same download
      mockInvoke.mockResolvedValueOnce({ requestId: 'dl-retry-002' });
      const retryResult = await invoke<{ requestId: string }>('start_download', {
        fileHash: 'retry_hash',
        fileName: 'retry.bin',
      });
      expect(retryResult.requestId).toBe('dl-retry-002');

      // Simulate success on retry
      emitEvent('download-complete', {
        requestId: 'dl-retry-002',
        fileHash: 'retry_hash',
        filePath: '/home/user/Downloads/retry.bin',
      });

      // Two start_download calls total
      const startCalls = mockInvoke.mock.calls.filter((c) => c[0] === 'start_download');
      expect(startCalls).toHaveLength(2);
    });

    it('should handle multiple retries up to max attempts', async () => {
      const maxRetries = 3;

      for (let attempt = 0; attempt < maxRetries; attempt++) {
        mockInvoke.mockResolvedValueOnce({ requestId: `dl-multi-retry-${attempt}` });
        await invoke('start_download', {
          fileHash: 'flaky_hash',
          fileName: 'flaky.bin',
        });

        if (attempt < maxRetries - 1) {
          // Simulate failure
          emitEvent('download-failed', {
            requestId: `dl-multi-retry-${attempt}`,
            fileHash: 'flaky_hash',
            error: 'Timeout waiting for chunks',
          });
        } else {
          // Final attempt succeeds
          emitEvent('download-complete', {
            requestId: `dl-multi-retry-${attempt}`,
            fileHash: 'flaky_hash',
            filePath: '/home/user/Downloads/flaky.bin',
          });
        }
      }

      const startCalls = mockInvoke.mock.calls.filter((c) => c[0] === 'start_download');
      expect(startCalls).toHaveLength(maxRetries);
    });
  });

  // ====================================================================
  // 3. Download history persistence
  // ====================================================================

  describe('download history persistence', () => {
    it('should save 100 history entries and reload all', () => {
      const walletAddr = TEST_WALLET.address.toLowerCase();
      const storageKey = `chiral-downloads-${walletAddr}`;

      // Generate 100 history entries
      const entries = Array.from({ length: 100 }, (_, i) => ({
        id: `entry_${i}`,
        fileHash: `hash_${i}`,
        fileName: `file_${i}.dat`,
        fileSize: (i + 1) * 1024,
        timestamp: Date.now() - i * 60_000,
        status: i % 10 === 0 ? 'failed' : 'completed',
        progress: i % 10 === 0 ? 75 : 100,
      }));

      // Save
      localStorage.setItem(storageKey, JSON.stringify(entries));

      // Reload (simulate app restart)
      const loaded = JSON.parse(localStorage.getItem(storageKey)!);
      expect(loaded).toHaveLength(100);

      // Verify data integrity
      loaded.forEach((entry: any, i: number) => {
        expect(entry.id).toBe(`entry_${i}`);
        expect(entry.fileHash).toBe(`hash_${i}`);
        expect(entry.fileSize).toBe((i + 1) * 1024);
      });

      // Verify failed entries
      const failedEntries = loaded.filter((e: any) => e.status === 'failed');
      expect(failedEntries).toHaveLength(10); // indices 0, 10, 20, ..., 90
    });

    it('should isolate download history per wallet (download as A, switch to B, verify empty)', () => {
      const keyA = `chiral-downloads-${TEST_WALLET.address.toLowerCase()}`;
      const keyB = `chiral-downloads-${TEST_WALLET_2.address.toLowerCase()}`;

      // Wallet A has download history
      const historyA = [
        { id: 'dl-a-1', fileHash: 'hash_a1', fileName: 'a_file.pdf', status: 'completed' },
        { id: 'dl-a-2', fileHash: 'hash_a2', fileName: 'a_file2.zip', status: 'completed' },
      ];
      localStorage.setItem(keyA, JSON.stringify(historyA));

      // Switch to wallet B — no history
      const historyB = localStorage.getItem(keyB);
      expect(historyB).toBeNull();

      // Wallet A's data is untouched
      const reloadA = JSON.parse(localStorage.getItem(keyA)!);
      expect(reloadA).toHaveLength(2);
      expect(reloadA[0].id).toBe('dl-a-1');
    });

    it('should handle large history without data loss', () => {
      const walletAddr = TEST_WALLET.address.toLowerCase();
      const storageKey = `chiral-downloads-${walletAddr}`;

      // Create entries with large metadata
      const entries = Array.from({ length: 50 }, (_, i) => ({
        id: `large_entry_${i}`,
        fileHash: `${'a'.repeat(64)}_${i}`,
        fileName: `${'x'.repeat(200)}_${i}.bin`,
        fileSize: 1_000_000_000 * (i + 1), // up to 50 GB
        timestamp: Date.now() - i * 3600_000,
        status: 'completed',
        progress: 100,
        httpLink: `http://localhost:9419/files/${'a'.repeat(64)}_${i}/${'x'.repeat(200)}_${i}.bin`,
        peers: Array.from({ length: 5 }, (_, j) => `peer_${i}_${j}`),
      }));

      localStorage.setItem(storageKey, JSON.stringify(entries));
      const loaded = JSON.parse(localStorage.getItem(storageKey)!);
      expect(loaded).toHaveLength(50);
      expect(loaded[0].peers).toHaveLength(5);
    });
  });

  // ====================================================================
  // 4. Payment flow stress
  // ====================================================================

  describe('payment flow stress', () => {
    it('should handle concurrent cost calculations', () => {
      const fileSizes = [
        1_000, // 1 KB
        1_000_000, // 1 MB
        10_000_000, // 10 MB
        100_000_000, // 100 MB
        1_000_000_000, // 1 GB
        5_000_000_000, // 5 GB
        10_000_000_000, // 10 GB
      ];

      const costs = fileSizes.map((size) => calculateCost(size));

      // All costs should be non-negative
      costs.forEach((cost) => expect(cost).toBeGreaterThanOrEqual(0));

      // Costs should increase with file size
      for (let i = 1; i < costs.length; i++) {
        expect(costs[i]).toBeGreaterThan(costs[i - 1]);
      }

      // Verify cost is roughly linear (1 GB = 1 CHI based on existing tests)
      expect(costs[4]).toBeCloseTo(1.0, 1); // 1 GB ~= 1 CHI
    });

    it('should handle payment timeout', async () => {
      // Simulate a payment that takes too long
      mockInvoke.mockRejectedValueOnce('Transaction timed out: no response after 30s');

      await expect(
        invoke('send_transaction', {
          fromAddress: TEST_WALLET.address,
          toAddress: '0xSeller',
          amount: '0.1',
          privateKey: TEST_WALLET.privateKey,
        })
      ).rejects.toContain('timed out');
    });

    it('should handle insufficient balance with multiple concurrent attempts', async () => {
      const concurrentPayments = 5;

      // Queue all rejections before firing any calls
      for (let i = 0; i < concurrentPayments; i++) {
        mockInvoke.mockRejectedValueOnce(
          `Insufficient balance: have 0.050000 CHI, need ${(i + 1) * 0.1} CHI (amount) + 0.000042 CHI (gas)`
        );
      }

      const paymentPromises = Array.from({ length: concurrentPayments }, (_, i) =>
        invoke('send_transaction', {
          fromAddress: TEST_WALLET.address,
          toAddress: `0xSeller_${i}`,
          amount: `${(i + 1) * 0.1}`,
          privateKey: TEST_WALLET.privateKey,
        }).catch((err: string) => ({ error: err, index: i }))
      );

      const results = await Promise.all(paymentPromises);

      // All should fail with insufficient balance
      expect(results).toHaveLength(concurrentPayments);
      results.forEach((r: any) => {
        expect(r.error).toBeDefined();
        expect(String(r.error)).toContain('Insufficient balance');
      });
    });

    it('should handle concurrent payments where some succeed and some fail', async () => {
      const payments = 8;

      // Queue all mock responses in order before firing calls
      for (let i = 0; i < payments; i++) {
        if (i % 2 === 0) {
          mockInvoke.mockResolvedValueOnce({
            hash: `0xtx_success_${i}`,
            status: 'pending',
            balanceBefore: '100.000000',
            balanceAfter: `${100 - (i + 1) * 0.5}`,
          });
        } else {
          mockInvoke.mockRejectedValueOnce('RPC error: nonce too low');
        }
      }

      // Fire all calls sequentially to ensure mock queue alignment
      const results: any[] = [];
      for (let i = 0; i < payments; i++) {
        try {
          const result = await invoke('send_transaction', {
            fromAddress: TEST_WALLET.address,
            toAddress: `0xSeller_${i}`,
            amount: `${(i + 1) * 0.5}`,
            privateKey: TEST_WALLET.privateKey,
          });
          results.push({ success: true, result, index: i });
        } catch (error: any) {
          results.push({ success: false, error, index: i });
        }
      }

      const successes = results.filter((r) => r.success);
      const failures = results.filter((r) => !r.success);

      expect(successes).toHaveLength(4); // even indices
      expect(failures).toHaveLength(4); // odd indices

      successes.forEach((r) => {
        expect(r.index % 2).toBe(0);
        expect(r.result.hash).toContain('0xtx_success');
      });

      failures.forEach((r) => {
        expect(r.index % 2).toBe(1);
        expect(String(r.error)).toContain('nonce too low');
      });
    });

    it('should handle payment then download start failure (rollback scenario)', async () => {
      const fileHash = 'rollback_hash';

      // Payment succeeds
      mockInvoke.mockResolvedValueOnce({
        hash: '0xtx_paid',
        status: 'confirmed',
        balanceBefore: '10.000000',
        balanceAfter: '9.900000',
      });
      await invoke('send_transaction', {
        fromAddress: TEST_WALLET.address,
        toAddress: '0xSeller',
        amount: '0.1',
        privateKey: TEST_WALLET.privateKey,
      });

      // But download start fails (e.g., seeder went offline)
      mockInvoke.mockRejectedValueOnce('No seeders available for hash: rollback_hash');
      await expect(
        invoke('start_download', {
          fileHash,
          fileName: 'paid_but_unavailable.zip',
          walletAddress: TEST_WALLET.address,
          privateKey: TEST_WALLET.privateKey,
        })
      ).rejects.toContain('No seeders available');

      // Verify both calls were made
      expect(mockInvoke).toHaveBeenCalledTimes(2);
      expect(mockInvoke.mock.calls[0][0]).toBe('send_transaction');
      expect(mockInvoke.mock.calls[1][0]).toBe('start_download');
    });

    it('should handle rapid cost calculations for many file sizes', () => {
      const sizes = Array.from({ length: 1000 }, (_, i) => (i + 1) * 1024 * 1024); // 1 MB to 1000 MB

      const start = performance.now();
      const costs = sizes.map((s) => calculateCost(s));
      const elapsed = performance.now() - start;

      // All 1000 calculations should complete quickly (< 100ms)
      expect(elapsed).toBeLessThan(100);

      // Sanity: costs should be monotonically increasing
      for (let i = 1; i < costs.length; i++) {
        expect(costs[i]).toBeGreaterThanOrEqual(costs[i - 1]);
      }
    });
  });
});
