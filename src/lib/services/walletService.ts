import { ethers } from 'ethers';
import { get } from 'svelte/store';
import { etcAccount } from '$lib/stores';

const CHAIN_ID = 98765; // Chiral Network Chain ID

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
