import { writable, derived, get } from 'svelte/store';
import { generateAlias, aliasFromPeerId, type UserAlias } from './aliasService';
import { saveHistoryToDht, loadHistoryFromDht, syncHistory } from './encryptedHistoryService';
import { walletAccount } from './stores';
import { logger } from './logger';

const log = logger('ChiralDrop');

// Transaction types
export interface FileTransfer {
  id: string;
  fileName: string;
  fileSize: number;
  fromPeerId: string;
  fromAlias: UserAlias;
  toPeerId: string;
  toAlias: UserAlias;
  status: 'pending' | 'accepted' | 'declined' | 'completed' | 'failed';
  direction: 'incoming' | 'outgoing';
  timestamp: number;
  progress?: number;
  // Pricing fields for paid transfers
  priceWei?: string;
  senderWallet?: string;
  fileHash?: string;
}

export interface NearbyPeer {
  peerId: string;
  alias: UserAlias;
  lastSeen: number;
  position: { x: number; y: number }; // Position on the map (0-100 percentage)
  wavePhase: number; // For animation
}

// User's peer ID (set when connected to network)
export const localPeerId = writable<string | null>(null);

// User's alias - derived from peer ID for consistency across clients
// Falls back to a random alias if not connected
const fallbackAlias = generateAlias();
export const userAlias = derived(localPeerId, ($peerId) => {
  if ($peerId) {
    return aliasFromPeerId($peerId);
  }
  return fallbackAlias;
});

// Writable store for components that need to set the peer ID
export function setLocalPeerId(peerId: string) {
  localPeerId.set(peerId);
}
export const nearbyPeers = writable<NearbyPeer[]>([]);
export const pendingTransfers = writable<FileTransfer[]>([]);
export const transferHistory = writable<FileTransfer[]>([]);
export const selectedPeer = writable<NearbyPeer | null>(null);

// Load transaction history from localStorage (temporary cache)
const HISTORY_STORAGE_KEY = 'chiraldrop_history';

function loadHistoryFromLocal(): FileTransfer[] {
  try {
    const stored = localStorage.getItem(HISTORY_STORAGE_KEY);
    if (stored) {
      const parsed = JSON.parse(stored);
      // Reconstruct alias objects from stored data
      return parsed.map((t: any) => ({
        ...t,
        fromAlias: t.fromAlias || aliasFromPeerId(t.fromPeerId),
        toAlias: t.toAlias || aliasFromPeerId(t.toPeerId)
      }));
    }
  } catch (e) {
    log.error('Failed to load transfer history from localStorage:', e);
  }
  return [];
}

function saveHistoryToLocal(history: FileTransfer[]) {
  try {
    localStorage.setItem(HISTORY_STORAGE_KEY, JSON.stringify(history));
  } catch (e) {
    log.error('Failed to save transfer history to localStorage:', e);
  }
}

// Initialize history from local storage first (fast)
transferHistory.set(loadHistoryFromLocal());

// Subscribe to history changes and persist both locally and to DHT
let saveTimeout: ReturnType<typeof setTimeout> | null = null;
transferHistory.subscribe((history) => {
  // Always save locally for quick access
  saveHistoryToLocal(history);

  // Debounce DHT saves to avoid excessive network traffic
  if (saveTimeout) {
    clearTimeout(saveTimeout);
  }
  saveTimeout = setTimeout(() => {
    saveHistoryToDht(history).catch((err) => {
      log.error('Failed to save history to DHT:', err);
    });
  }, 2000);
});

// When wallet connects, sync history from DHT
walletAccount.subscribe(async (wallet) => {
  if (wallet) {
    log.info('Wallet connected, syncing history from DHT...');
    try {
      const localHistory = get(transferHistory);
      const syncedHistory = await syncHistory(localHistory);
      // Update store with synced history (merged local + DHT)
      if (syncedHistory.length > 0) {
        transferHistory.set(syncedHistory);
      }
      log.info('History synced:', syncedHistory.length, 'transfers');
    } catch (err) {
      log.error('Failed to sync history:', err);
    }
  }
});

// Helper functions
export function addPendingTransfer(transfer: FileTransfer) {
  pendingTransfers.update((transfers) => [...transfers, transfer]);
}

export function updateTransferStatus(id: string, status: FileTransfer['status'], progress?: number) {
  pendingTransfers.update((transfers) => {
    const updated = transfers.map((t) =>
      t.id === id ? { ...t, status, progress: progress ?? t.progress } : t
    );
    return updated;
  });

  // If completed or final status, move to history
  if (status === 'completed' || status === 'declined' || status === 'failed') {
    const pending = get(pendingTransfers);
    const transfer = pending.find((t) => t.id === id);
    if (transfer) {
      transferHistory.update((history) => [{ ...transfer, status }, ...history]);
      pendingTransfers.update((transfers) => transfers.filter((t) => t.id !== id));
    }
  }
}

export function acceptTransfer(id: string) {
  updateTransferStatus(id, 'accepted');
}

export function declineTransfer(id: string) {
  updateTransferStatus(id, 'declined');
}

// Add a nearby peer with wave position
export function addNearbyPeer(peerId: string) {
  const alias = aliasFromPeerId(peerId);

  // Random position on the map (will be animated)
  const position = {
    x: 20 + Math.random() * 60, // Keep within center 60% of map
    y: 20 + Math.random() * 60
  };

  const wavePhase = Math.random() * Math.PI * 2; // Random starting phase for wave animation

  nearbyPeers.update((peers) => {
    // Check if peer already exists
    const existing = peers.find((p) => p.peerId === peerId);
    if (existing) {
      return peers.map((p) =>
        p.peerId === peerId ? { ...p, lastSeen: Date.now() } : p
      );
    }

    return [...peers, {
      peerId,
      alias,
      lastSeen: Date.now(),
      position,
      wavePhase
    }];
  });
}

export function removeNearbyPeer(peerId: string) {
  nearbyPeers.update((peers) => peers.filter((p) => p.peerId !== peerId));
}

export function selectPeer(peer: NearbyPeer | null) {
  selectedPeer.set(peer);
}

// Generate unique transfer ID
export function generateTransferId(): string {
  return `transfer_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`;
}

// Derived store for incoming pending transfers
export const incomingPendingTransfers = derived(
  pendingTransfers,
  ($pending) => $pending.filter((t) => t.direction === 'incoming' && t.status === 'pending')
);

// Derived store for outgoing pending transfers
export const outgoingPendingTransfers = derived(
  pendingTransfers,
  ($pending) => $pending.filter((t) => t.direction === 'outgoing')
);

// Clear old history (keep last 100 entries)
export function pruneHistory() {
  transferHistory.update((history) => history.slice(0, 100));
}

// Format wei price as CHR for display
export function formatPriceWei(wei: string): string {
  if (!wei || wei === '0') return 'Free';
  try {
    const weiNum = BigInt(wei);
    const whole = weiNum / BigInt(1e18);
    const frac = weiNum % BigInt(1e18);
    if (frac === BigInt(0)) return `${whole} CHR`;
    const fracStr = frac.toString().padStart(18, '0').replace(/0+$/, '');
    return `${whole}.${fracStr} CHR`;
  } catch {
    return `${wei} wei`;
  }
}

// Format file size for display
export function formatFileSize(bytes: number): string {
  if (bytes === 0) return '0 B';
  const k = 1024;
  const sizes = ['B', 'KB', 'MB', 'GB', 'TB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
}
