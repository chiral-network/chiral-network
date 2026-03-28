import { writable } from 'svelte/store';
import { dhtService } from '$lib/dhtService';
import { logger } from '$lib/logger';
import { isAuthenticated, walletAccount } from '$lib/stores';
import { transferHistory, pendingTransfers, nearbyPeers, selectedPeer } from '$lib/chiralDropStore';

const log = logger('Logout');

export const logoutModalOpen = writable(false);

export function requestLogout(): void {
  logoutModalOpen.set(true);
}

export function cancelLogout(): void {
  logoutModalOpen.set(false);
}

export async function confirmLogout(): Promise<void> {
  try {
    await dhtService.stop();
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
  }
}
