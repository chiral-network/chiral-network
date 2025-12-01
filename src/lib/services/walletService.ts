import { ethers } from 'ethers';
import { get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';
import { etcAccount } from '$lib/stores';

const DEFAULT_CHAIN_ID = 98765; // Fallback Chiral Network Chain ID

// Cache for the chain ID
let cachedChainId: number | null = null;

/**
 * Get the chain ID from the backend, with caching
 */
export async function getChainId(): Promise<number> {
  if (cachedChainId !== null) {
    return cachedChainId;
  }

  const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
  if (isTauri) {
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

/**
 * Sign a transaction using the stored wallet
 */
export async function signTransaction(txRequest: TransactionRequest): Promise<string> {
  const account = get(etcAccount);

  if (!account?.private_key) {
    throw new Error('No wallet available for signing');
  }

  // Create ethers wallet from private key
  const walletInstance = new ethers.Wallet(account.private_key);

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
