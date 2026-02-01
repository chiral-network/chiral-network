import { writable } from 'svelte/store';

export interface WalletAccount {
  address: string;
  privateKey: string;
}

export const walletAccount = writable<WalletAccount | null>(null);
export const isAuthenticated = writable<boolean>(false);
export const networkConnected = writable<boolean>(false);

export interface PeerInfo {
  id: string;
  address: string;
  multiaddrs?: string[];
  lastSeen: Date | number;
}

export const peers = writable<PeerInfo[]>([]);
export const networkStats = writable({
  connectedPeers: 0,
  totalPeers: 0
});
