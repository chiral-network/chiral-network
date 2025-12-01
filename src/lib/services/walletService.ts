import { ethers } from 'ethers';
import { get } from 'svelte/store';
import { etcAccount } from '$lib/stores';
import { invoke } from '@tauri-apps/api/core';

// Default chain ID, can be overridden by fetching from backend
let CHAIN_ID = 98765; // Chiral Network Chain ID

// Check if running in Tauri environment
const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

/**
 * Fetch the chain ID from the running Geth node
 */
export async function fetchChainId(): Promise<number> {
  if (!isTauri) {
    return CHAIN_ID;
  }
  
  try {
    const chainId = await invoke('get_network_chain_id') as number;
    CHAIN_ID = chainId;
    return chainId;
  } catch (error) {
    console.warn('Failed to fetch chain ID, using default:', error);
    return CHAIN_ID;
  }
}

/**
 * Get the current chain ID
 */
export function getChainId(): number {
  return CHAIN_ID;
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

  // Build transaction
  const transaction: ethers.TransactionRequest = {
    to: txRequest.to,
    value: valueWei,
    gasLimit: BigInt(txRequest.gasLimit),
    gasPrice: BigInt(txRequest.gasPrice),
    nonce: txRequest.nonce,
    chainId: CHAIN_ID,
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
