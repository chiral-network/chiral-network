import { ethers } from 'ethers';
import { get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { walletAccount } from '$lib/stores';

const DEFAULT_CHAIN_ID = 98765; // Fallback Chiral Network Chain ID

// Cache for the chain ID
let cachedChainId: number | null = null;

// Check if running in Tauri
function isTauriEnvironment(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

/**
 * Get the chain ID from the backend, with caching
 */
export async function getChainId(): Promise<number> {
  if (cachedChainId !== null) {
    return cachedChainId;
  }

  if (isTauriEnvironment()) {
    try {
      cachedChainId = await invoke<number>('get_chain_id');
      return cachedChainId;
    } catch (error) {
      console.warn('Failed to get chain ID from backend, using default:', error);
      return DEFAULT_CHAIN_ID;
    }
  }
  return DEFAULT_CHAIN_ID;
}

export interface TransactionRequest {
  from: string;
  to: string;
  value: string; // Amount in ETH/CHR as string
  gasLimit: number;
  gasPrice: number; // in Wei
  nonce?: number;
}

export interface WalletBalance {
  balance: string;
  balanceWei: string;
  pendingBalance?: string;
}

/**
 * Wallet service singleton for balance tracking and transactions
 */
class WalletService {
  private balanceCache: Map<string, { balance: string; timestamp: number }> = new Map();
  private readonly CACHE_TTL = 30000; // 30 seconds cache

  /**
   * Get wallet balance from backend
   */
  async getBalance(address: string): Promise<string> {
    if (!address) {
      return '0.00';
    }

    // Check cache
    const cached = this.balanceCache.get(address.toLowerCase());
    if (cached && Date.now() - cached.timestamp < this.CACHE_TTL) {
      return cached.balance;
    }

    if (isTauriEnvironment()) {
      try {
        const result = await invoke<WalletBalance>('get_wallet_balance', { address });
        const balance = result.balance || '0.00';

        // Update cache
        this.balanceCache.set(address.toLowerCase(), {
          balance,
          timestamp: Date.now()
        });

        return balance;
      } catch (error) {
        console.error('Failed to fetch balance from backend:', error);
        // Return cached value if available, otherwise 0
        return cached?.balance || '0.00';
      }
    }

    // In non-Tauri environment, return mock balance for development
    return '0.00';
  }

  /**
   * Clear balance cache for an address
   */
  clearCache(address?: string): void {
    if (address) {
      this.balanceCache.delete(address.toLowerCase());
    } else {
      this.balanceCache.clear();
    }
  }

  /**
   * Force refresh balance (bypass cache)
   */
  async refreshBalance(address: string): Promise<string> {
    this.clearCache(address);
    return this.getBalance(address);
  }
}

// Export singleton instance
export const walletService = new WalletService();

/**
 * Sign a transaction using the stored wallet
 */
export async function signTransaction(txRequest: TransactionRequest): Promise<string> {
  const account = get(walletAccount);

  if (!account?.privateKey) {
    throw new Error('No wallet available for signing');
  }

  // Create ethers wallet from private key
  const walletInstance = new ethers.Wallet(account.privateKey);

  // Convert value from ETH string to Wei
  const valueWei = ethers.parseEther(txRequest.value);

  // Get chain ID from backend
  const chainId = await getChainId();

  // Build transaction
  const transaction: ethers.TransactionRequest = {
    to: txRequest.to,
    value: valueWei,
    gasLimit: BigInt(txRequest.gasLimit),
    gasPrice: BigInt(txRequest.gasPrice),
    nonce: txRequest.nonce,
    chainId: chainId,
    type: 0, // Legacy transaction type
  };

  try {
    // Sign the transaction
    const signedTx = await walletInstance.signTransaction(transaction);
    return signedTx;
  } catch (error) {
    console.error('Transaction signing failed:', error);
    throw new Error('Failed to sign transaction: ' + (error instanceof Error ? error.message : 'Unknown error'));
  }
}

/**
 * Validate Ethereum address format
 */
export function isValidAddress(address: string): boolean {
  try {
    ethers.getAddress(address); // Will throw if invalid
    return true;
  } catch {
    return false;
  }
}

/**
 * Format Wei to ETH for display
 */
export function formatEther(wei: string | number): string {
  return ethers.formatEther(wei.toString());
}

/**
 * Parse ETH to Wei
 */
export function parseEther(eth: string): string {
  return ethers.parseEther(eth).toString();
}
