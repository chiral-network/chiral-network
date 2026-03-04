import { ethers } from 'ethers';

const WORD_LIST = ethers.Mnemonic.fromEntropy(ethers.randomBytes(16)).wordlist;

/**
 * Generate a 12-word mnemonic phrase
 */
export function generateMnemonic(): string {
  const entropy = ethers.randomBytes(16); // 128 bits for 12 words
  const mnemonic = ethers.Mnemonic.fromEntropy(entropy);
  return mnemonic.phrase;
}

/**
 * Create wallet from mnemonic
 */
export function createWalletFromMnemonic(mnemonic: string): { address: string; privateKey: string } {
  const wallet = ethers.Wallet.fromPhrase(mnemonic);
  return {
    address: wallet.address,
    privateKey: wallet.privateKey
  };
}

/**
 * Create wallet from private key
 */
export function createWalletFromPrivateKey(privateKey: string): { address: string; privateKey: string } {
  // Ensure private key has 0x prefix
  const formattedKey = privateKey.startsWith('0x') ? privateKey : `0x${privateKey}`;
  const wallet = new ethers.Wallet(formattedKey);
  return {
    address: wallet.address,
    privateKey: wallet.privateKey
  };
}

/**
 * Validate private key format
 */
export function isValidPrivateKey(privateKey: string): boolean {
  try {
    const formattedKey = privateKey.startsWith('0x') ? privateKey : `0x${privateKey}`;
    new ethers.Wallet(formattedKey);
    return true;
  } catch {
    return false;
  }
}

/**
 * Validate mnemonic phrase
 */
export function isValidMnemonic(mnemonic: string): boolean {
  return ethers.Mnemonic.isValidMnemonic(mnemonic);
}
