/**
 * Payment Service
 *
 * Handles Chiral payments for file downloads, including:
 * - Calculating download costs based on file size
 * - Deducting balance from downloader
 * - Crediting balance to uploader/seeder
 * - Recording transactions for both parties
 */

import { wallet, transactions, type Transaction, settings } from '$lib/stores';
import { get } from 'svelte/store';
import { invoke } from '@tauri-apps/api/core';

// Helper functions for localStorage persistence
function saveWalletToStorage(walletData: any) {
  try {
    localStorage.setItem('chiral_wallet', JSON.stringify(walletData));
  } catch (error) {
    console.error('Failed to save wallet to localStorage:', error);
  }
}

function saveTransactionsToStorage(txs: Transaction[]) {
  try {
    localStorage.setItem('chiral_transactions', JSON.stringify(txs));
  } catch (error) {
    console.error('Failed to save transactions to localStorage:', error);
  }
}

function loadWalletFromStorage() {
  try {
    const saved = localStorage.getItem('chiral_wallet');
    return saved ? JSON.parse(saved) : null;
  } catch (error) {
    console.error('Failed to load wallet from localStorage:', error);
    return null;
  }
}

function loadTransactionsFromStorage(): Transaction[] {
  try {
    const saved = localStorage.getItem('chiral_transactions');
    if (!saved) return [];

    const parsed = JSON.parse(saved);
    // Convert date strings back to Date objects
    return parsed.map((tx: any) => ({
      ...tx,
      date: new Date(tx.date)
    }));
  } catch (error) {
    console.error('Failed to load transactions from localStorage:', error);
    return [];
  }
}

export interface DownloadPayment {
  fileHash: string;
  fileName: string;
  fileSize: number; // in bytes
  seederAddress: string;
  downloaderId: string;
  timestamp: Date;
  amount: number; // in Chiral
}

export class PaymentService {
  private static initialized = false;
  private static processedPayments = new Set<string>(); // Track processed file hashes (for downloads)
  private static receivedPayments = new Set<string>(); // Track received payments (for uploads)

  /**
   * Initialize payment service and load persisted data (only runs once)
   */
  static initialize() {
    // Only initialize once
    if (this.initialized) {
      console.log('💾 Payment service already initialized, skipping...');
      return;
    }

    // Load wallet from storage - localStorage is the source of truth
    const savedWallet = loadWalletFromStorage();
    if (savedWallet && typeof savedWallet.balance === 'number') {
      wallet.update(w => ({ ...w, balance: savedWallet.balance }));
      console.log('💾 Restored wallet balance from localStorage:', savedWallet.balance);
    } else {
      console.log('💾 No saved wallet found, using current balance');
    }

    // Load transactions from storage
    const savedTransactions = loadTransactionsFromStorage();
    if (savedTransactions.length > 0) {
      transactions.set(savedTransactions);
      console.log('💾 Loaded transactions from storage:', savedTransactions.length);
    }

    this.initialized = true;
  }

  /**
   * Calculate the cost of downloading a file based on its size
   */
  static calculateDownloadCost(fileSizeInBytes: number): number {
    const pricePerMb = get(settings).pricePerMb || 0.001;
    const sizeInMB = fileSizeInBytes / (1024 * 1024);
    return parseFloat((sizeInMB * pricePerMb).toFixed(8)); // Support 8 decimal places
  }

  /**
   * Check if the downloader has sufficient balance
   */
  static hasSufficientBalance(amount: number): boolean {
    const currentBalance = get(wallet).balance;
    return currentBalance >= amount;
  }

  /**
   * Process payment for a file download
   * This deducts from the downloader's balance and creates a transaction
   */
  static async processDownloadPayment(
    fileHash: string,
    fileName: string,
    fileSize: number,
    seederAddress: string
  ): Promise<{ success: boolean; transactionId?: number; error?: string }> {
    try {
      // Check if this file has already been paid for
      if (this.processedPayments.has(fileHash)) {
        console.log('⚠️ Payment already processed for file:', fileHash);
        return {
          success: false,
          error: 'Payment already processed for this file'
        };
      }

      const amount = this.calculateDownloadCost(fileSize);

      // Check if user has sufficient balance
      if (!this.hasSufficientBalance(amount)) {
        return {
          success: false,
          error: `Insufficient balance. Need ${amount.toFixed(4)} Chiral, have ${get(wallet).balance.toFixed(4)} Chiral`
        };
      }

      // Get current wallet state
      const currentWallet = get(wallet);
      const currentTransactions = get(transactions);

      console.log('💰 Processing download payment:', {
        currentBalance: currentWallet.balance,
        amount,
        fileName,
        seederAddress,
        currentTransactionCount: currentTransactions.length
      });

      // Generate unique transaction ID
      const transactionId = currentTransactions.length > 0
        ? Math.max(...currentTransactions.map(tx => tx.id)) + 1
        : 1;

      // Deduct from downloader's balance (support 8 decimal places)
      const newBalance = parseFloat((currentWallet.balance - amount).toFixed(8));
      console.log('💸 Balance Update:', {
        before: currentWallet.balance,
        deducting: amount,
        after: newBalance,
        calculation: `${currentWallet.balance} - ${amount} = ${newBalance}`
      });

      wallet.update(w => {
        const updated = {
          ...w,
          balance: newBalance
          // Note: totalSpent is automatically calculated from transactions store
        };
        saveWalletToStorage(updated);
        console.log('✅ Wallet store updated and saved to localStorage');
        return updated;
      });

      // Create transaction record for downloader
      const newTransaction: Transaction = {
        id: transactionId,
        type: 'sent',
        amount: amount,
        to: seederAddress,
        from: currentWallet.address,
        date: new Date(),
        description: `Download: ${fileName}`,
        status: 'completed'
      };

      console.log('📝 Creating transaction:', newTransaction);

      // Add transaction to history with persistence
      transactions.update(txs => {
        const updated = [newTransaction, ...txs];
        console.log('✅ Updated transactions array length:', updated.length);
        saveTransactionsToStorage(updated);
        return updated;
      });

      // Mark this file as paid to prevent duplicate payments
      this.processedPayments.add(fileHash);
      console.log('✅ Marked file as paid:', fileHash);

      // Notify backend about the payment - this will emit an event to the seeder
      try {
        await invoke('record_download_payment', {
          fileHash,
          fileName,
          fileSize,
          seederAddress,
          downloaderAddress: currentWallet.address || 'unknown',
          amount,
          transactionId
        });
        console.log('✅ Backend notified of payment to seeder');
      } catch (invokeError) {
        console.warn('Failed to notify backend of payment:', invokeError);
        // Continue anyway - frontend state is updated
      }

      return {
        success: true,
        transactionId
      };
    } catch (error) {
      console.error('Error processing download payment:', error);
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Unknown error occurred'
      };
    }
  }

  /**
   * Credit payment to seeder when someone downloads their file
   * This is called when the seeder receives a download payment
   */
  static async creditSeederPayment(
    fileHash: string,
    fileName: string,
    fileSize: number,
    downloaderAddress: string
  ): Promise<{ success: boolean; transactionId?: number; error?: string }> {
    try {
      // Generate unique key for this payment receipt
      const paymentKey = `${fileHash}-${downloaderAddress}`;

      // Check if we already received this payment
      if (this.receivedPayments.has(paymentKey)) {
        console.log('⚠️ Payment already received for:', paymentKey);
        return {
          success: false,
          error: 'Payment already received'
        };
      }

      const amount = this.calculateDownloadCost(fileSize);

      // Get current wallet state
      const currentWallet = get(wallet);
      const currentTransactions = get(transactions);

      // Generate unique transaction ID
      const transactionId = currentTransactions.length > 0
        ? Math.max(...currentTransactions.map(tx => tx.id)) + 1
        : 1;

      // Credit to seeder's balance (support 8 decimal places)
      wallet.update(w => {
        const updated = {
          ...w,
          balance: parseFloat((w.balance + amount).toFixed(8))
          // Note: totalEarned is automatically calculated from mining rewards
        };
        saveWalletToStorage(updated);
        return updated;
      });

      // Create transaction record for seeder
      const newTransaction: Transaction = {
        id: transactionId,
        type: 'received',
        amount: amount,
        from: downloaderAddress,
        to: currentWallet.address,
        date: new Date(),
        description: `Upload payment: ${fileName}`,
        status: 'completed'
      };

      // Add transaction to history with persistence
      transactions.update(txs => {
        const updated = [newTransaction, ...txs];
        saveTransactionsToStorage(updated);
        return updated;
      });

      // Mark this payment as received
      this.receivedPayments.add(paymentKey);
      console.log('✅ Marked payment as received:', paymentKey);

      // Notify backend about the payment receipt
      try {
        await invoke('record_seeder_payment', {
          fileHash,
          fileName,
          fileSize,
          downloaderAddress,
          amount,
          transactionId
        });
      } catch (invokeError) {
        console.warn('Failed to persist seeder payment to backend:', invokeError);
        // Continue anyway - frontend state is updated
      }

      console.log('💰 Seeder payment credited:', {
        amount: amount.toFixed(8),
        from: downloaderAddress,
        file: fileName,
        newBalance: get(wallet).balance.toFixed(8)
      });

      return {
        success: true,
        transactionId
      };
    } catch (error) {
      console.error('Error crediting seeder payment:', error);
      return {
        success: false,
        error: error instanceof Error ? error.message : 'Unknown error occurred'
      };
    }
  }

  /**
   * Get payment details for a file without processing it
   */
  static getPaymentDetails(fileSizeInBytes: number): {
    amount: number;
    pricePerMb: number;
    sizeInMB: number;
    formattedAmount: string;
  } {
    const pricePerMb = get(settings).pricePerMb || 0.001;
    const sizeInMB = fileSizeInBytes / (1024 * 1024);
    const amount = this.calculateDownloadCost(fileSizeInBytes);

    return {
      amount,
      pricePerMb,
      sizeInMB,
      formattedAmount: `${amount.toFixed(6)} Chiral`
    };
  }

  /**
   * Validate if a payment can be processed
   */
  static validatePayment(fileSizeInBytes: number): {
    valid: boolean;
    amount: number;
    error?: string;
  } {
    const amount = this.calculateDownloadCost(fileSizeInBytes);
    const currentBalance = get(wallet).balance;

    if (amount <= 0) {
      return {
        valid: false,
        amount: 0,
        error: 'Invalid file size'
      };
    }

    if (!this.hasSufficientBalance(amount)) {
      return {
        valid: false,
        amount,
        error: `Insufficient balance. Need ${amount.toFixed(4)} Chiral, have ${currentBalance.toFixed(4)} Chiral`
      };
    }

    return {
      valid: true,
      amount
    };
  }
}

// Export singleton instance
export const paymentService = PaymentService;
