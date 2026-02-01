const WALLET_NAMES_KEY = 'chiral_wallet_names';

interface WalletNames {
  [address: string]: string;
}

export function getWalletName(address: string): string | null {
  try {
    const cache = localStorage.getItem(WALLET_NAMES_KEY);
    if (!cache) return null;
    const parsed: WalletNames = JSON.parse(cache);
    return parsed[address.toLowerCase()] || null;
  } catch (error) {
    console.error('Error reading wallet name cache:', error);
    return null;
  }
}

export function setWalletName(address: string, name: string): void {
  try {
    const cache = localStorage.getItem(WALLET_NAMES_KEY);
    const parsed: WalletNames = cache ? JSON.parse(cache) : {};
    parsed[address.toLowerCase()] = name;
    localStorage.setItem(WALLET_NAMES_KEY, JSON.stringify(parsed));
  } catch (error) {
    console.error('Error saving wallet name:', error);
  }
}

export function removeWalletName(address: string): void {
  try {
    const cache = localStorage.getItem(WALLET_NAMES_KEY);
    if (!cache) return;
    const parsed: WalletNames = JSON.parse(cache);
    delete parsed[address.toLowerCase()];
    localStorage.setItem(WALLET_NAMES_KEY, JSON.stringify(parsed));
  } catch (error) {
    console.error('Error removing wallet name:', error);
  }
}

export function getAllWalletNames(): WalletNames {
  try {
    const cache = localStorage.getItem(WALLET_NAMES_KEY);
    return cache ? JSON.parse(cache) : {};
  } catch (error) {
    console.error('Error reading all wallet names:', error);
    return {};
  }
}
