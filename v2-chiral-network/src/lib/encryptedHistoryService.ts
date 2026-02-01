import { get } from 'svelte/store';
import { walletAccount } from './stores';
import type { FileTransfer } from './chiralDropStore';

const HISTORY_DHT_PREFIX = 'chiraldrop_history_';
const HISTORY_LOCAL_KEY = 'chiraldrop_history_encrypted';

/**
 * Encrypted History Service
 *
 * Uses AES-256-GCM encryption with a key derived from the wallet's private key.
 * History is stored on the DHT for cross-device access.
 * Falls back to localStorage when offline.
 */

/**
 * Derive an encryption key from the wallet's private key using PBKDF2
 */
async function deriveKey(privateKey: string): Promise<CryptoKey> {
  const encoder = new TextEncoder();
  const keyMaterial = await crypto.subtle.importKey(
    'raw',
    encoder.encode(privateKey),
    'PBKDF2',
    false,
    ['deriveKey']
  );

  // Use wallet address hash as salt for deterministic key derivation
  const wallet = get(walletAccount);
  const salt = encoder.encode(wallet?.address || 'chiraldrop');

  return crypto.subtle.deriveKey(
    {
      name: 'PBKDF2',
      salt,
      iterations: 100000,
      hash: 'SHA-256'
    },
    keyMaterial,
    { name: 'AES-GCM', length: 256 },
    false,
    ['encrypt', 'decrypt']
  );
}

/**
 * Encrypt data using AES-256-GCM
 */
async function encrypt(data: string, key: CryptoKey): Promise<string> {
  const encoder = new TextEncoder();
  const iv = crypto.getRandomValues(new Uint8Array(12));

  const encrypted = await crypto.subtle.encrypt(
    { name: 'AES-GCM', iv },
    key,
    encoder.encode(data)
  );

  // Combine IV and ciphertext, then base64 encode
  const combined = new Uint8Array(iv.length + encrypted.byteLength);
  combined.set(iv);
  combined.set(new Uint8Array(encrypted), iv.length);

  return btoa(String.fromCharCode(...combined));
}

/**
 * Decrypt data using AES-256-GCM
 */
async function decrypt(encryptedData: string, key: CryptoKey): Promise<string> {
  const combined = Uint8Array.from(atob(encryptedData), c => c.charCodeAt(0));

  // Extract IV (first 12 bytes) and ciphertext
  const iv = combined.slice(0, 12);
  const ciphertext = combined.slice(12);

  const decrypted = await crypto.subtle.decrypt(
    { name: 'AES-GCM', iv },
    key,
    ciphertext
  );

  return new TextDecoder().decode(decrypted);
}

/**
 * Generate DHT key for storing history
 */
function getDhtKey(walletAddress: string): string {
  return `${HISTORY_DHT_PREFIX}${walletAddress.toLowerCase()}`;
}

/**
 * Check if Tauri is available
 */
function isTauri(): boolean {
  return typeof window !== 'undefined' && '__TAURI__' in window;
}

/**
 * Save history to DHT (encrypted)
 */
export async function saveHistoryToDht(history: FileTransfer[]): Promise<void> {
  const wallet = get(walletAccount);
  if (!wallet) {
    console.warn('No wallet connected, cannot save to DHT');
    // Fall back to localStorage
    saveHistoryToLocal(history);
    return;
  }

  try {
    const key = await deriveKey(wallet.privateKey);
    const encryptedData = await encrypt(JSON.stringify(history), key);

    if (isTauri()) {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('store_dht_value', {
        key: getDhtKey(wallet.address),
        value: encryptedData
      });
      console.log('History saved to DHT');
    }

    // Always also save locally as cache
    localStorage.setItem(HISTORY_LOCAL_KEY, encryptedData);
  } catch (error) {
    console.error('Failed to save history to DHT:', error);
    // Fall back to localStorage
    saveHistoryToLocal(history);
  }
}

/**
 * Load history from DHT (encrypted)
 */
export async function loadHistoryFromDht(): Promise<FileTransfer[]> {
  const wallet = get(walletAccount);
  if (!wallet) {
    console.warn('No wallet connected, loading from local cache');
    return loadHistoryFromLocal();
  }

  try {
    let encryptedData: string | null = null;

    if (isTauri()) {
      const { invoke } = await import('@tauri-apps/api/core');
      try {
        encryptedData = await invoke('get_dht_value', {
          key: getDhtKey(wallet.address)
        });
      } catch (e) {
        console.log('DHT value not found, checking local cache');
      }
    }

    // Fall back to local cache if DHT lookup failed
    if (!encryptedData) {
      encryptedData = localStorage.getItem(HISTORY_LOCAL_KEY);
    }

    if (!encryptedData) {
      return [];
    }

    const key = await deriveKey(wallet.privateKey);
    const decryptedData = await decrypt(encryptedData, key);
    return JSON.parse(decryptedData);
  } catch (error) {
    console.error('Failed to load history from DHT:', error);
    return loadHistoryFromLocal();
  }
}

/**
 * Save history to localStorage (unencrypted fallback for when wallet not connected)
 */
function saveHistoryToLocal(history: FileTransfer[]): void {
  try {
    localStorage.setItem('chiraldrop_history_plain', JSON.stringify(history));
  } catch (error) {
    console.error('Failed to save history to localStorage:', error);
  }
}

/**
 * Load history from localStorage (unencrypted fallback)
 */
function loadHistoryFromLocal(): FileTransfer[] {
  try {
    const stored = localStorage.getItem('chiraldrop_history_plain');
    if (stored) {
      return JSON.parse(stored);
    }
  } catch (error) {
    console.error('Failed to load history from localStorage:', error);
  }
  return [];
}

/**
 * Sync history between local and DHT
 * Call this when wallet is connected
 */
export async function syncHistory(localHistory: FileTransfer[]): Promise<FileTransfer[]> {
  const wallet = get(walletAccount);
  if (!wallet) {
    return localHistory;
  }

  try {
    // Try to load from DHT first
    const dhtHistory = await loadHistoryFromDht();

    // Merge histories (dedupe by ID, prefer more recent)
    const mergedMap = new Map<string, FileTransfer>();

    // Add DHT history first
    for (const transfer of dhtHistory) {
      mergedMap.set(transfer.id, transfer);
    }

    // Add local history, overwriting if more recent
    for (const transfer of localHistory) {
      const existing = mergedMap.get(transfer.id);
      if (!existing || transfer.timestamp > existing.timestamp) {
        mergedMap.set(transfer.id, transfer);
      }
    }

    // Sort by timestamp descending
    const merged = Array.from(mergedMap.values())
      .sort((a, b) => b.timestamp - a.timestamp);

    // Save merged history back to DHT
    await saveHistoryToDht(merged);

    return merged;
  } catch (error) {
    console.error('Failed to sync history:', error);
    return localHistory;
  }
}
