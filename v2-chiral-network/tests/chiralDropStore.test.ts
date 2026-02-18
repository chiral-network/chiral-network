import { describe, it, expect, beforeEach, vi } from 'vitest';
import { get } from 'svelte/store';

// Mock encryptedHistoryService before importing chiralDropStore
vi.mock('$lib/encryptedHistoryService', () => ({
  saveHistoryToDht: vi.fn().mockResolvedValue(undefined),
  loadHistoryFromDht: vi.fn().mockResolvedValue([]),
  syncHistory: vi.fn().mockImplementation((local: any[]) => Promise.resolve(local)),
}));

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

describe('chiralDropStore', () => {
  beforeEach(() => {
    localStorage.clear();
    vi.resetModules();
    vi.clearAllMocks();
  });

  describe('formatPriceWei', () => {
    it('should return "Free" for "0"', async () => {
      const { formatPriceWei } = await import('$lib/chiralDropStore');
      expect(formatPriceWei('0')).toBe('Free');
    });

    it('should return "Free" for empty string', async () => {
      const { formatPriceWei } = await import('$lib/chiralDropStore');
      expect(formatPriceWei('')).toBe('Free');
    });

    it('should format 1 CHI (1e18 wei)', async () => {
      const { formatPriceWei } = await import('$lib/chiralDropStore');
      expect(formatPriceWei('1000000000000000000')).toBe('1 CHI');
    });

    it('should format 0.5 CHI (5e17 wei)', async () => {
      const { formatPriceWei } = await import('$lib/chiralDropStore');
      const result = formatPriceWei('500000000000000000');
      expect(result).toBe('0.5 CHI');
    });

    it('should format 2.5 CHI', async () => {
      const { formatPriceWei } = await import('$lib/chiralDropStore');
      expect(formatPriceWei('2500000000000000000')).toBe('2.5 CHI');
    });

    it('should format very small amount (1 wei)', async () => {
      const { formatPriceWei } = await import('$lib/chiralDropStore');
      const result = formatPriceWei('1');
      expect(result).toContain('CHI');
      expect(result).not.toBe('Free');
    });

    it('should fallback to "X wei" for invalid input', async () => {
      const { formatPriceWei } = await import('$lib/chiralDropStore');
      const result = formatPriceWei('not_a_number');
      expect(result).toContain('wei');
    });
  });

  describe('formatFileSize', () => {
    it('should format 0 bytes', async () => {
      const { formatFileSize } = await import('$lib/chiralDropStore');
      expect(formatFileSize(0)).toBe('0 B');
    });

    it('should format bytes', async () => {
      const { formatFileSize } = await import('$lib/chiralDropStore');
      expect(formatFileSize(500)).toBe('500 B');
    });

    it('should format KB', async () => {
      const { formatFileSize } = await import('$lib/chiralDropStore');
      expect(formatFileSize(1024)).toBe('1 KB');
    });

    it('should format MB', async () => {
      const { formatFileSize } = await import('$lib/chiralDropStore');
      expect(formatFileSize(1048576)).toBe('1 MB');
    });

    it('should format GB', async () => {
      const { formatFileSize } = await import('$lib/chiralDropStore');
      expect(formatFileSize(1073741824)).toBe('1 GB');
    });

    it('should format fractional sizes', async () => {
      const { formatFileSize } = await import('$lib/chiralDropStore');
      expect(formatFileSize(1536)).toBe('1.5 KB');
    });
  });

  describe('generateTransferId', () => {
    it('should generate unique IDs', async () => {
      const { generateTransferId } = await import('$lib/chiralDropStore');
      const id1 = generateTransferId();
      const id2 = generateTransferId();
      expect(id1).not.toBe(id2);
    });

    it('should start with "transfer_" prefix', async () => {
      const { generateTransferId } = await import('$lib/chiralDropStore');
      const id = generateTransferId();
      expect(id).toMatch(/^transfer_/);
    });

    it('should contain a timestamp', async () => {
      const { generateTransferId } = await import('$lib/chiralDropStore');
      const before = Date.now();
      const id = generateTransferId();
      const after = Date.now();
      const parts = id.split('_');
      const timestamp = parseInt(parts[1], 10);
      expect(timestamp).toBeGreaterThanOrEqual(before);
      expect(timestamp).toBeLessThanOrEqual(after);
    });
  });

  describe('transfer lifecycle', () => {
    function makeTransfer(overrides = {}): any {
      return {
        id: `transfer_${Date.now()}_test`,
        fileName: 'test.txt',
        fileSize: 1024,
        fromPeerId: 'peer1',
        fromAlias: { displayName: 'Alice', emoji: '🐱', color: '#ff0000' },
        toPeerId: 'peer2',
        toAlias: { displayName: 'Bob', emoji: '🐶', color: '#0000ff' },
        status: 'pending' as const,
        direction: 'outgoing' as const,
        timestamp: Date.now(),
        ...overrides,
      };
    }

    it('should add transfer to pending', async () => {
      const { pendingTransfers, addPendingTransfer } = await import('$lib/chiralDropStore');
      const transfer = makeTransfer({ id: 'test-add-1' });
      addPendingTransfer(transfer);
      const pending = get(pendingTransfers);
      expect(pending).toHaveLength(1);
      expect(pending[0].id).toBe('test-add-1');
    });

    it('should move completed transfer to history', async () => {
      const { pendingTransfers, transferHistory, addPendingTransfer, updateTransferStatus } = await import('$lib/chiralDropStore');
      const transfer = makeTransfer({ id: 'test-complete-1' });
      addPendingTransfer(transfer);
      updateTransferStatus('test-complete-1', 'completed');
      expect(get(pendingTransfers)).toHaveLength(0);
      const history = get(transferHistory);
      expect(history.some((t: any) => t.id === 'test-complete-1')).toBe(true);
    });

    it('should move declined transfer to history', async () => {
      const { pendingTransfers, transferHistory, addPendingTransfer, updateTransferStatus } = await import('$lib/chiralDropStore');
      const transfer = makeTransfer({ id: 'test-decline-1' });
      addPendingTransfer(transfer);
      updateTransferStatus('test-decline-1', 'declined');
      expect(get(pendingTransfers)).toHaveLength(0);
      expect(get(transferHistory).some((t: any) => t.id === 'test-decline-1')).toBe(true);
    });

    it('should move failed transfer to history', async () => {
      const { pendingTransfers, transferHistory, addPendingTransfer, updateTransferStatus } = await import('$lib/chiralDropStore');
      const transfer = makeTransfer({ id: 'test-fail-1' });
      addPendingTransfer(transfer);
      updateTransferStatus('test-fail-1', 'failed');
      expect(get(pendingTransfers)).toHaveLength(0);
      expect(get(transferHistory).some((t: any) => t.id === 'test-fail-1')).toBe(true);
    });

    it('should update progress without moving to history for accepted status', async () => {
      const { pendingTransfers, addPendingTransfer, updateTransferStatus } = await import('$lib/chiralDropStore');
      const transfer = makeTransfer({ id: 'test-accept-1' });
      addPendingTransfer(transfer);
      updateTransferStatus('test-accept-1', 'accepted', 50);
      const pending = get(pendingTransfers);
      expect(pending).toHaveLength(1);
      expect(pending[0].status).toBe('accepted');
      expect(pending[0].progress).toBe(50);
    });

    it('should update payment tx hash via updateTransferPayment', async () => {
      const { pendingTransfers, addPendingTransfer, updateTransferPayment } = await import('$lib/chiralDropStore');
      const transfer = makeTransfer({ id: 'test-payment-1' });
      addPendingTransfer(transfer);
      updateTransferPayment('test-payment-1', '0xabc123');
      const pending = get(pendingTransfers);
      expect(pending[0].paymentTxHash).toBe('0xabc123');
    });

    it('should update transfer by file hash', async () => {
      const { pendingTransfers, transferHistory, addPendingTransfer, updateTransferByFileHash } = await import('$lib/chiralDropStore');
      const transfer = makeTransfer({ id: 'test-fh-1', fileHash: 'hash123' });
      addPendingTransfer(transfer);
      updateTransferByFileHash('hash123', 'completed', '0xtx', '10.0', '5.0');
      expect(get(pendingTransfers)).toHaveLength(0);
      const history = get(transferHistory);
      const found = history.find((t: any) => t.fileHash === 'hash123');
      expect(found).toBeDefined();
      expect(found.paymentTxHash).toBe('0xtx');
      expect(found.balanceBefore).toBe('10.0');
      expect(found.balanceAfter).toBe('5.0');
    });

    it('acceptTransfer should set status to accepted', async () => {
      const { pendingTransfers, addPendingTransfer, acceptTransfer } = await import('$lib/chiralDropStore');
      const transfer = makeTransfer({ id: 'test-shortcut-1' });
      addPendingTransfer(transfer);
      acceptTransfer('test-shortcut-1');
      expect(get(pendingTransfers)[0].status).toBe('accepted');
    });

    it('declineTransfer should move to history with declined status', async () => {
      const { pendingTransfers, transferHistory, addPendingTransfer, declineTransfer } = await import('$lib/chiralDropStore');
      const transfer = makeTransfer({ id: 'test-shortcut-2' });
      addPendingTransfer(transfer);
      declineTransfer('test-shortcut-2');
      expect(get(pendingTransfers)).toHaveLength(0);
      expect(get(transferHistory)[0].status).toBe('declined');
    });
  });

  describe('nearby peers', () => {
    it('should add a new nearby peer', async () => {
      const { nearbyPeers, addNearbyPeer } = await import('$lib/chiralDropStore');
      addNearbyPeer('12D3KooWTest1');
      const peers = get(nearbyPeers);
      expect(peers).toHaveLength(1);
      expect(peers[0].peerId).toBe('12D3KooWTest1');
    });

    it('should not duplicate existing peer, just update lastSeen', async () => {
      const { nearbyPeers, addNearbyPeer } = await import('$lib/chiralDropStore');
      addNearbyPeer('12D3KooWTest2');
      const firstSeen = get(nearbyPeers)[0].lastSeen;
      // Small delay to ensure timestamp difference
      await new Promise(r => setTimeout(r, 5));
      addNearbyPeer('12D3KooWTest2');
      const peers = get(nearbyPeers);
      expect(peers).toHaveLength(1);
      expect(peers[0].lastSeen).toBeGreaterThanOrEqual(firstSeen);
    });

    it('should remove peer by ID', async () => {
      const { nearbyPeers, addNearbyPeer, removeNearbyPeer } = await import('$lib/chiralDropStore');
      addNearbyPeer('12D3KooWTest3');
      addNearbyPeer('12D3KooWTest4');
      removeNearbyPeer('12D3KooWTest3');
      const peers = get(nearbyPeers);
      expect(peers).toHaveLength(1);
      expect(peers[0].peerId).toBe('12D3KooWTest4');
    });

    it('should handle removing non-existent peer gracefully', async () => {
      const { nearbyPeers, removeNearbyPeer } = await import('$lib/chiralDropStore');
      removeNearbyPeer('nonexistent');
      expect(get(nearbyPeers)).toHaveLength(0);
    });
  });

  describe('derived stores', () => {
    function makeTransfer(overrides = {}): any {
      return {
        id: `transfer_${Date.now()}_${Math.random()}`,
        fileName: 'test.txt',
        fileSize: 1024,
        fromPeerId: 'peer1',
        fromAlias: { displayName: 'Alice', emoji: '🐱', color: '#ff0000' },
        toPeerId: 'peer2',
        toAlias: { displayName: 'Bob', emoji: '🐶', color: '#0000ff' },
        status: 'pending' as const,
        direction: 'incoming' as const,
        timestamp: Date.now(),
        ...overrides,
      };
    }

    it('incomingPendingTransfers should filter incoming + pending', async () => {
      const { incomingPendingTransfers, addPendingTransfer } = await import('$lib/chiralDropStore');
      addPendingTransfer(makeTransfer({ direction: 'incoming', status: 'pending' }));
      addPendingTransfer(makeTransfer({ direction: 'outgoing', status: 'pending' }));
      addPendingTransfer(makeTransfer({ direction: 'incoming', status: 'accepted' }));
      expect(get(incomingPendingTransfers)).toHaveLength(1);
    });

    it('outgoingPendingTransfers should filter outgoing transfers', async () => {
      const { outgoingPendingTransfers, addPendingTransfer } = await import('$lib/chiralDropStore');
      addPendingTransfer(makeTransfer({ direction: 'outgoing', status: 'pending' }));
      addPendingTransfer(makeTransfer({ direction: 'outgoing', status: 'accepted' }));
      addPendingTransfer(makeTransfer({ direction: 'incoming', status: 'pending' }));
      expect(get(outgoingPendingTransfers)).toHaveLength(2);
    });
  });

  describe('pruneHistory', () => {
    it('should keep only last 100 history entries', async () => {
      const { transferHistory, pruneHistory } = await import('$lib/chiralDropStore');
      const bigHistory = Array.from({ length: 150 }, (_, i) => ({
        id: `t${i}`,
        fileName: `file${i}.txt`,
        fileSize: 100,
        fromPeerId: 'p1',
        fromAlias: { displayName: 'A', emoji: '🐱', color: '#f00' },
        toPeerId: 'p2',
        toAlias: { displayName: 'B', emoji: '🐶', color: '#00f' },
        status: 'completed' as const,
        direction: 'outgoing' as const,
        timestamp: Date.now() - i,
      }));
      transferHistory.set(bigHistory);
      pruneHistory();
      expect(get(transferHistory)).toHaveLength(100);
    });
  });

  describe('localStorage persistence', () => {
    it('should save history to localStorage on update', async () => {
      const { transferHistory } = await import('$lib/chiralDropStore');
      transferHistory.set([{
        id: 'persist-1',
        fileName: 'test.txt',
        fileSize: 100,
        fromPeerId: 'p1',
        fromAlias: { displayName: 'A', emoji: '🐱', color: '#f00' },
        toPeerId: 'p2',
        toAlias: { displayName: 'B', emoji: '🐶', color: '#00f' },
        status: 'completed' as const,
        direction: 'outgoing' as const,
        timestamp: Date.now(),
      }]);
      const stored = localStorage.getItem('chiraldrop_history');
      expect(stored).not.toBeNull();
      const parsed = JSON.parse(stored!);
      expect(parsed).toHaveLength(1);
      expect(parsed[0].id).toBe('persist-1');
    });

    it('should load history from localStorage on init', async () => {
      localStorage.setItem('chiraldrop_history', JSON.stringify([{
        id: 'loaded-1',
        fileName: 'loaded.txt',
        fileSize: 200,
        fromPeerId: 'p1',
        toPeerId: 'p2',
        status: 'completed',
        direction: 'incoming',
        timestamp: 1000,
      }]));
      const { transferHistory } = await import('$lib/chiralDropStore');
      const history = get(transferHistory);
      expect(history).toHaveLength(1);
      expect(history[0].id).toBe('loaded-1');
    });
  });
});
