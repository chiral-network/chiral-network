import { writable, get } from 'svelte/store';
import { dhtService } from '$lib/dhtService';
import { logger } from '$lib/logger';
import { isAuthenticated, walletAccount } from '$lib/stores';
import { transferHistory, pendingTransfers, nearbyPeers, selectedPeer } from '$lib/chiralDropStore';

const log = logger('Logout');

export const logoutModalOpen = writable(false);
export const loggingOut = writable(false);

export function requestLogout(): void {
  logoutModalOpen.set(true);
}

export function cancelLogout(): void {
  if (get(loggingOut)) return;
  logoutModalOpen.set(false);
}

export async function confirmLogout(): Promise<void> {
  if (get(loggingOut)) return;
  loggingOut.set(true);

  try {
    // Stop DHT with a 5-second timeout so logout never hangs
    await Promise.race([
      dhtService.stop(),
      new Promise((_, reject) => setTimeout(() => reject(new Error('DHT stop timed out')), 5000))
    ]);
  } catch (error) {
    log.warn('Failed to stop DHT during logout:', error);
  } finally {
    // Clear all in-memory stores to prevent data leaking to next session
    transferHistory.set([]);
    pendingTransfers.set([]);
    nearbyPeers.set([]);
    selectedPeer.set(null);

    walletAccount.set(null);
    isAuthenticated.set(false);
    logoutModalOpen.set(false);
    loggingOut.set(false);
  }
}
