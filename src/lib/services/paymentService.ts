/**
 * Payment Service
 *
 * Handles Chiral payments for file downloads, including:
 * - Calculating download costs based on file size
 * - Deducting balance from downloader
 * - Crediting balance to uploader/seeder
 * - Recording transactions for both parties
 */

import { wallet, transactions, type Transaction } from "$lib/stores";
import { get } from "svelte/store";
import { invoke } from "@tauri-apps/api/core";
import { walletService } from "$lib/wallet";

// type FullNetworkStats = {
//   network_difficulty: number
//   network_hashrate: number
//   active_miners: number
//   power_usage: number
//   current_block: number
//   peer_count: number
//   blocks_mined?: number
// }

// const stats = await invoke<FullNetworkStats>('get_network_stats', {
//   address: $etcAccount?.address
// })

// Helper functions for localStorage persistence
function saveWalletToStorage(walletData: any) {
  try {
    localStorage.setItem("chiral_wallet", JSON.stringify(walletData));
  } catch (error) {
    console.error("Failed to save wallet to localStorage:", error);
  }
}

function saveTransactionsToStorage(txs: Transaction[]) {
  try {
    const serialized = JSON.stringify(txs);
    localStorage.setItem("chiral_transactions", serialized);
    console.log(`💾 Saved ${txs.length} transactions to localStorage (${(serialized.length / 1024).toFixed(2)} KB)`);
  } catch (error) {
    console.error("Failed to save transactions to localStorage:", error);
  }
}

function loadWalletFromStorage() {
  try {
    const saved = localStorage.getItem("chiral_wallet");
    return saved ? JSON.parse(saved) : null;
  } catch (error) {
    console.error("Failed to load wallet from localStorage:", error);
    return null;
  }
}

function loadTransactionsFromStorage(): Transaction[] {
  try {
    const saved = localStorage.getItem("chiral_transactions");
    if (!saved) {
      console.log("📭 No transactions found in localStorage");
      return [];
    }

    const parsed = JSON.parse(saved);
    // Convert date strings back to Date objects
    const transactions = parsed.map((tx: any) => ({
      ...tx,
      date: new Date(tx.date),
    }));
    
    console.log(`📬 Loaded ${transactions.length} transactions from localStorage`);
    return transactions;
  } catch (error) {
    console.error("Failed to load transactions from localStorage:", error);
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
  private static pollingInterval: number | null = null;
  private static readonly POLL_INTERVAL_MS = 10000; // Poll every 10 seconds
  private static readonly WALLET_ADDRESS_REGEX = /^0x[a-fA-F0-9]{40}$/;

  /**
   * Initialize payment service and load persisted data (only runs once)
   */
  static initialize() {
    // Only initialize once
    if (this.initialized) {
      return;
    }

    // Load wallet from storage - localStorage is the source of truth
    const savedWallet = loadWalletFromStorage();
    if (savedWallet && typeof savedWallet.balance === "number") {
      wallet.update((w) => ({ ...w, balance: savedWallet.balance }));
    }

    // Load transactions from storage
    const savedTransactions = loadTransactionsFromStorage();
    if (savedTransactions.length > 0) {
      transactions.set(savedTransactions);
    }

    this.initialized = true;
  }

  /**
   * Calculate the cost of downloading a file based on its size
   */
  // static calculateDownloadCost(fileSizeInBytes: number): number {
  //   const pricePerMb = get(settings).pricePerMb || 0.001;
  //   const sizeInMB = fileSizeInBytes / (1024 * 1024);
  //   return parseFloat((sizeInMB * pricePerMb).toFixed(8)); // Support 8 decimal places
  // }

  //calculate dynamic download cost
  static async calculateDownloadCost(fileSizeInBytes: number): Promise<number> {
    const normalizationFactor = 1.2; // can be tuned based on desired pricing
    const dynamicPricePerMb =
      await this.getDynamicPricePerMB(normalizationFactor);

    const sizeInMB = fileSizeInBytes / (1024 * 1024);
    const cost = sizeInMB * dynamicPricePerMb;
    
    // Ensure minimum cost of 0.0001 Chiral for any file download
    const minimumCost = 0.0001;
    const finalCost = Math.max(cost, minimumCost);

    console.log(`📊 Download cost calculation:`, {
      fileSizeBytes: fileSizeInBytes,
      fileSizeMB: sizeInMB.toFixed(4),
      pricePerMB: dynamicPricePerMb.toFixed(8),
      calculatedCost: cost.toFixed(8),
      minimumCost: minimumCost.toFixed(8),
      finalCost: finalCost.toFixed(8)
    });

    return parseFloat(finalCost.toFixed(8));
  }

  /**
   * Fetch dynamic network metrics and calculate real-time price per MB
   * based on current Ethereum conditions
   */
  static async getDynamicPricePerMB(normalizationFactor = 1): Promise<number> {
    try {
      const stats = await invoke<{
        network_difficulty: number;
        network_hashrate: number;
        active_miners: number;
        power_usage: number;
      }>("get_full_network_stats");

      const {
        network_difficulty,
        network_hashrate,
        active_miners,
        power_usage,
      } = stats;

      console.log(`📈 Network stats for pricing:`, {
        difficulty: network_difficulty,
        hashrate: network_hashrate,
        miners: active_miners,
        power: power_usage
      });

      if (network_hashrate <= 0) {
        console.warn("⚠️ Network hashrate is 0, using fallback price");
        return 0.001;
      }

      // --- Average hash power per miner ---
      const avgHashPower =
        active_miners > 0 ? network_hashrate / active_miners : network_hashrate;

      // unit cost of one hash for this miner, normalized to the average mining power
      // basically for this miner, how expensive is each hash compared to the network average
      const baseHashCost = power_usage / Math.max(avgHashPower, 1);

      // --- Price per MB (scaled by difficulty) ---
      const pricePerMB =
        (baseHashCost / avgHashPower) *
        network_difficulty *
        normalizationFactor;

      console.log(`💵 Calculated price per MB: ${pricePerMB.toFixed(8)}`);
      return parseFloat(pricePerMB.toFixed(8));
    } catch (error) {
      console.warn("⚠️ Failed to fetch dynamic pricing from network:", error);
      // fallback to static price from settings
      return 0.001;
    }
  }

  /**
   * Check if the downloader has sufficient balance
   */
  static hasSufficientBalance(amount: number): boolean {
    const currentBalance = get(wallet).balance;
    return currentBalance >= amount;
  }

  /**
   * Validate that a string is a hex-encoded Ethereum wallet address
   */
  static isValidWalletAddress(address?: string | null): boolean {
    if (!address) {
      return false;
    }
    return this.WALLET_ADDRESS_REGEX.test(address);
  }

  /**
   * Process payment for a file download
   * This deducts from the downloader's balance and creates a transaction
   * @param seederAddress - Wallet address of the seeder (0x...)
   * @param seederPeerId - libp2p peer ID of the seeder
   */
  static async processDownloadPayment(
    fileHash: string,
    fileName: string,
    fileSize: number,
    seederAddress: string,
    seederPeerId?: string
  ): Promise<{
    success: boolean;
    transactionId?: number;
    transactionHash?: string;
    error?: string;
  }> {
    try {
      // Check if this file has already been paid for
      if (this.processedPayments.has(fileHash)) {
        console.log("⚠️ Payment already processed for file:", fileHash);
        return {
          success: false,
          error: "Payment already processed for this file",
        };
      }

      const amount = await this.calculateDownloadCost(fileSize);

      if (!seederAddress || !this.WALLET_ADDRESS_REGEX.test(seederAddress)) {
        console.error("❌ Invalid seeder wallet address for payment", {
          seederAddress,
          fileName,
          fileHash,
        });
        return {
          success: false,
          error: "Invalid seeder wallet address",
        };
      }

      // Check if user has sufficient balance
      if (!this.hasSufficientBalance(amount)) {
        return {
          success: false,
          error: `Insufficient balance. Need ${amount.toFixed(4)} Chiral, have ${get(wallet).balance.toFixed(4)} Chiral`,
        };
      }

      // Get current wallet state
      const currentWallet = get(wallet);
      const currentTransactions = get(transactions);
      let transactionHash = "";

      console.log("💰 Processing download payment:", {
        currentBalance: currentWallet.balance,
        amount,
        fileName,
        seederAddress,
        currentTransactionCount: currentTransactions.length,
      });

      try {
        const result = await invoke<string>("process_download_payment", {
          uploaderAddress: seederAddress,
          price: amount,
        });
        if (!result || typeof result !== "string") {
          throw new Error("Payment request did not return a transaction hash");
        }
        transactionHash = result;
        console.log("🔗 On-chain payment submitted:", {
          transactionHash,
          seederAddress,
          amount,
        });
      } catch (chainError: any) {
        const errorMessage =
          chainError?.message ||
          chainError?.toString() ||
          "Failed to submit on-chain payment";
        console.error("❌ Ethereum payment transaction failed:", chainError);
        return {
          success: false,
          error: errorMessage,
        };
      }

      // Generate unique transaction ID
      const transactionId =
        currentTransactions.length > 0
          ? Math.max(...currentTransactions.map((tx) => tx.id)) + 1
          : 1;

      // Deduct from downloader's balance (support 8 decimal places)
      const newBalance = parseFloat(
        (currentWallet.balance - amount).toFixed(8)
      );
      console.log("💸 Balance Update:", {
        before: currentWallet.balance,
        deducting: amount,
        after: newBalance,
        calculation: `${currentWallet.balance} - ${amount} = ${newBalance}`,
      });

      wallet.update((w) => {
        const updated = {
          ...w,
          balance: newBalance,
          // Note: totalSpent is automatically calculated from transactions store
        };
        saveWalletToStorage(updated);
        console.log("✅ Wallet store updated and saved to localStorage");
        return updated;
      });

      // Create transaction record for downloader
      const newTransaction: Transaction = {
        id: transactionId,
        type: "sent",
        amount: amount,
        to: seederAddress,
        from: currentWallet.address,
        txHash: transactionHash,
        date: new Date(),
        description: `Download: ${fileName}`,
        status: "success",
      };

      console.log("📝 Creating transaction:", newTransaction);

      // Add transaction to history with persistence
      transactions.update((txs) => {
        const updated = [newTransaction, ...txs];
        console.log("✅ Updated transactions array length:", updated.length);
        saveTransactionsToStorage(updated);
        return updated;
      });

      // Mark this file as paid to prevent duplicate payments
      this.processedPayments.add(fileHash);
      console.log("✅ Marked file as paid:", fileHash);

      // Notify backend about the payment - this will send P2P message to the seeder
      try {
        await invoke("record_download_payment", {
          fileHash,
          fileName,
          fileSize,
          seederWalletAddress: seederAddress,
          seederPeerId: seederPeerId || seederAddress, // Fallback to wallet address if no peer ID
          downloaderAddress: currentWallet.address || "unknown",
          amount,
          transactionId,
          transactionHash,
        });
        console.log("✅ Payment notification sent to seeder:", seederAddress);
      } catch (invokeError) {
        console.warn("Failed to send payment notification:", invokeError);
        // Continue anyway - frontend state is updated
      }

      return {
        success: true,
        transactionId,
        transactionHash,
      };
    } catch (error) {
      console.error("Error processing download payment:", error);
      return {
        success: false,
        error:
          error instanceof Error ? error.message : "Unknown error occurred",
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
    downloaderAddress: string,
    transactionHash?: string
  ): Promise<{ success: boolean; transactionId?: number; error?: string }> {
    try {
      // Generate unique key for this payment receipt
      const paymentKey = `${fileHash}-${downloaderAddress}`;

      // Check if we already received this payment
      if (this.receivedPayments.has(paymentKey)) {
        console.log("⚠️ Payment already received for:", paymentKey);
        return {
          success: false,
          error: "Payment already received",
        };
      }

      const amount = await this.calculateDownloadCost(fileSize);

      // Get current wallet state
      const currentWallet = get(wallet);
      const currentTransactions = get(transactions);

      // Generate unique transaction ID
      const transactionId =
        currentTransactions.length > 0
          ? Math.max(...currentTransactions.map((tx) => tx.id)) + 1
          : 1;

      // Create transaction record for seeder
      const newTransaction: Transaction = {
        id: transactionId,
        type: "received",
        amount: amount,
        from: downloaderAddress,
        to: currentWallet.address,
        txHash: transactionHash,
        date: new Date(),
        description: `Upload payment: ${fileName}`,
        status: "success",
      };

      // Add transaction to history with persistence
      transactions.update((txs) => {
        const updated = [newTransaction, ...txs];
        saveTransactionsToStorage(updated);
        return updated;
      });

      // Trigger wallet refresh to recalculate balance from transaction history
      // This ensures consistency with walletService polling
      try {
        await walletService.refreshBalance();
      } catch (error) {
        console.warn("Failed to refresh balance after payment:", error);
        // Fallback: manually calculate balance for immediate UI feedback
        wallet.update((w) => {
          const allTxs = get(transactions);
          const totalReceived = allTxs
            .filter((tx) => tx.status === "success" && tx.type === "received")
            .reduce((sum, tx) => sum + tx.amount, 0);
          const totalSpent = allTxs
            .filter((tx) => tx.status === "success" && tx.type === "sent")
            .reduce((sum, tx) => sum + tx.amount, 0);

          const updated = {
            ...w,
            balance: parseFloat((totalReceived - totalSpent).toFixed(8)),
            totalEarned: totalReceived,
            totalSpent: totalSpent,
          };
          saveWalletToStorage(updated);
          return updated;
        });
      }

      // Mark this payment as received
      this.receivedPayments.add(paymentKey);
      console.log("✅ Marked payment as received:", paymentKey);

      // Notify backend about the payment receipt
      try {
        await invoke("record_seeder_payment", {
          fileHash,
          fileName,
          fileSize,
          downloaderAddress,
          amount,
          transactionId,
        });
      } catch (invokeError) {
        console.warn(
          "Failed to persist seeder payment to backend:",
          invokeError
        );
        // Continue anyway - frontend state is updated
      }

      console.log("💰 Seeder payment credited:", {
        amount: amount.toFixed(8),
        from: downloaderAddress,
        file: fileName,
        newBalance: get(wallet).balance.toFixed(8),
      });

      return {
        success: true,
        transactionId,
      };
    } catch (error) {
      console.error("Error crediting seeder payment:", error);
      return {
        success: false,
        error:
          error instanceof Error ? error.message : "Unknown error occurred",
      };
    }
  }

  /**
   * Get payment details for a file without processing it
   */
  static async getPaymentDetails(fileSizeInBytes: number): Promise<{
    amount: number;
    pricePerMb: number;
    sizeInMB: number;
    formattedAmount: string;
  }> {
    const sizeInMB = fileSizeInBytes / (1024 * 1024);
    const amount = await this.calculateDownloadCost(fileSizeInBytes);
    console.log(`Download cost: ${amount.toFixed(8)} Chiral`);

    let pricePerMb = await this.getDynamicPricePerMB(1.2);
    if (!Number.isFinite(pricePerMb) || pricePerMb <= 0) {
      pricePerMb = 0.001;
    }

    return {
      amount,
      pricePerMb: Number(pricePerMb.toFixed(8)),
      sizeInMB,
      formattedAmount: `${amount.toFixed(8)} Chiral`,
    };
  }

  /**
   * Validate if a payment can be processed
   */
  static async validatePayment(fileSizeInBytes: number): Promise<{
    valid: boolean;
    amount: number;
    error?: string;
  }> {
    const amount = await this.calculateDownloadCost(fileSizeInBytes);
    const currentBalance = get(wallet).balance;

    if (amount <= 0) {
      return {
        valid: false,
        amount: 0,
        error: "Invalid file size",
      };
    }

    if (!this.hasSufficientBalance(amount)) {
      return {
        valid: false,
        amount,
        error: `Insufficient balance. Need ${amount.toFixed(4)} Chiral, have ${currentBalance.toFixed(4)} Chiral`,
      };
    }

    return {
      valid: true,
      amount,
    };
  }

  /**
   * Start polling for payment notifications from the DHT
   */
  static startPaymentNotificationPolling(): void {
    if (this.pollingInterval) {
      console.log("⚠️ Payment notification polling already running");
      return;
    }

    console.log("🔄 Starting payment notification polling...");

    // Poll immediately
    this.checkForPaymentNotifications();

    // Then poll every 10 seconds
    this.pollingInterval = window.setInterval(() => {
      this.checkForPaymentNotifications();
    }, this.POLL_INTERVAL_MS);
  }

  /**
   * Stop polling for payment notifications
   */
  static stopPaymentNotificationPolling(): void {
    if (this.pollingInterval) {
      clearInterval(this.pollingInterval);
      this.pollingInterval = null;
      console.log("🛑 Stopped payment notification polling");
    }
  }

  /**
   * Check for payment notifications from the DHT
   */
  private static async checkForPaymentNotifications(): Promise<void> {
    try {
      const currentWallet = get(wallet);
      if (!currentWallet.address) {
        return; // No wallet address to check
      }

      const notifications = (await invoke("check_payment_notifications", {
        walletAddress: currentWallet.address,
      })) as any[];

      if (notifications && notifications.length > 0) {
        for (const notification of notifications) {
          await this.handlePaymentNotification(notification);
        }
      }
    } catch (error) {
      // Silently handle errors - DHT might not be ready yet
      console.debug("Payment notification check failed:", error);
    }
  }

  /**
   * Handle a payment notification from the DHT
   */
  private static async handlePaymentNotification(
    notification: any
  ): Promise<void> {
    try {
      console.log("💰 Payment notification received:", notification);

      // Credit the seeder's wallet
      const result = await this.creditSeederPayment(
        notification.file_hash,
        notification.file_name,
        notification.file_size,
        notification.downloader_address,
        notification.transaction_hash
      );

      if (result.success) {
        console.log("✅ Payment credited successfully");
      } else {
        console.warn("⚠️ Failed to credit payment:", result.error);
      }
    } catch (error) {
      console.error("Error handling payment notification:", error);
    }
  }
}

// Export singleton instance
export const paymentService = PaymentService;
