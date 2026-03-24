import { writable } from 'svelte/store';
import { dhtService } from '$lib/dhtService';
import { logger } from '$lib/logger';
import { isAuthenticated, walletAccount } from '$lib/stores';

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
    walletAccount.set(null);
    isAuthenticated.set(false);
    logoutModalOpen.set(false);
  }
}
