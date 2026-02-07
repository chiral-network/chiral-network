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
import { reputationService } from "./reputationService";

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
function saveTransactionsToStorage(txs: Transaction[]) {
  try {
    const serialized = JSON.stringify(txs);
    localStorage.setItem("chiral_transactions", serialized);
    console.log(
      `[SAVE] Saved ${txs.length} transactions to localStorage (${(serialized.length / 1024).toFixed(2)} KB)`
    );
  } catch (error) {
    console.error("Failed to save transactions to localStorage:", error);
  }
}

function loadTransactionsFromStorage(): Transaction[] {
  try {
    const saved = localStorage.getItem("chiral_transactions");
    if (!saved) {
      return [];
    }

    const parsed = JSON.parse(saved);
    // Convert date strings back to Date objects
    const transactions = parsed.map((tx: any) => ({
      ...tx,
      date: new Date(tx.date),
    }));

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

    // Note: Do NOT load balance from localStorage here.
    // The blockchain RPC is the source of truth for balance, not localStorage.
    // Loading a cached balance would overwrite the fresh value from refreshBalance()
    // and cause cross-device balance mismatches.

    // Load transactions from storage (these are local payment records, not blockchain state)
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

    return parseFloat(finalCost.toFixed(8));
  }

  /**
   * Fetch dynamic network metrics and calculate real-time price per MB
   * based on current network conditions
   */
  static async getDynamicPricePerMB(normalizationFactor = 1): Promise<number> {
    try {
      const stats = await invoke<{
        network_difficulty: number;
        network_hashrate: number;
        active_miners: number;
        power_usage: number;
      }>("get_full_network_stats");

      const { network_hashrate, active_miners, power_usage } = stats;

      // Base price per MB in Chiral tokens
      const basePricePerMB = 0.001;

      // Calculate network load factor (more miners = slightly higher price due to demand)
      // Clamped between 0.5x and 2x the base price
      const minerFactor = Math.min(Math.max(active_miners / 10, 0.5), 2.0);

      // Calculate efficiency factor based on power usage relative to hashrate
      // Higher power usage per hash = slightly higher price
      // Default to 1.0 if we can't calculate
      let efficiencyFactor = 1.0;
      if (network_hashrate > 0 && power_usage > 0) {
        // Normalize: assume 100W per 1MH/s is baseline efficiency
        const baselineEfficiency = 100 / 1_000_000; // W per H/s
        const actualEfficiency = power_usage / network_hashrate;
        // Ratio clamped between 0.8x and 1.5x
        efficiencyFactor = Math.min(
          Math.max(actualEfficiency / baselineEfficiency, 0.8),
          1.5
        );
      }

      // Final price calculation
      const pricePerMB =
        basePricePerMB * minerFactor * efficiencyFactor * normalizationFactor;

      // Ensure price stays within reasonable bounds: 0.0001 to 1.0 Chiral per MB
      const finalPrice = Math.min(Math.max(pricePerMB, 0.0001), 1.0);

      return parseFloat(finalPrice.toFixed(8));
    } catch (error) {
      // fallback to static price from settings when network pricing unavailable
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
        console.log("[WARN] Payment already processed for file:", fileHash);
        return {
          success: false,
          error: "Payment already processed for this file",
        };
      }

      const amount = await this.calculateDownloadCost(fileSize);

      if (!seederAddress || !this.WALLET_ADDRESS_REGEX.test(seederAddress)) {
        console.error("[X] Invalid seeder wallet address for payment", {
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

      console.log("[PAY] Processing download payment:", {
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
        console.log("[LINK] On-chain payment submitted:", {
          transactionHash,
          seederAddress,
          amount,
        });
      } catch (chainError: any) {
        const errorMessage =
          chainError?.message ||
          chainError?.toString() ||
          "Failed to submit on-chain payment";
        console.error("[X] Ethereum payment transaction failed:", chainError);
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
      console.log("[SEND] Balance Update:", {
        before: currentWallet.balance,
        deducting: amount,
        after: newBalance,
        calculation: `${currentWallet.balance} - ${amount} = ${newBalance}`,
      });

      wallet.update((w) => ({
        ...w,
        balance: newBalance,
      }));
      console.log("[OK] Wallet store updated with new balance:", newBalance);

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

      console.log("[NOTE] Creating transaction:", newTransaction);

      // Add transaction to history with persistence
      transactions.update((txs) => {
        const updated = [newTransaction, ...txs];
        console.log("[OK] Updated transactions array length:", updated.length);
        saveTransactionsToStorage(updated);
        return updated;
      });

      // Mark this file as paid to prevent duplicate payments
      this.processedPayments.add(fileHash);
      console.log("[OK] Marked file as paid:", fileHash);

      // Publish reputation verdict for successful payment (downloader perspective)
      // Get our own peer ID first for the issuer_id
      let downloaderPeerId = currentWallet.address; // Fallback to wallet address
      try {
        downloaderPeerId = await invoke<string>("get_peer_id");
        console.log("[STATS] Got downloader peer ID:", downloaderPeerId);
      } catch (err) {
        console.warn(
          "Could not get peer ID for issuer_id, using wallet address:",
          err
        );
      }

      // Publish reputation verdict using signed message system (see docs/SIGNED_TRANSACTION_MESSAGES.md)
      try {
        console.log(
          "[STATS] Attempting to publish reputation verdict for downloader->seeder"
        );
        console.log("[STATS] seederPeerId:", seederPeerId);
        console.log("[STATS] seederAddress:", seederAddress);
        console.log("[STATS] Using target_id:", seederPeerId || seederAddress);
        console.log("[STATS] Using issuer_id:", downloaderPeerId);

        await reputationService.publishVerdict({
          target_id: seederPeerId || seederAddress,
          tx_hash: transactionHash,
          outcome: "good",
          details: `Successful payment for file: ${fileName}`,
          metric: "transaction",
          issued_at: Math.floor(Date.now() / 1000),
          issuer_id: downloaderPeerId,
          issuer_seq_no: transactionId,
        });

        console.log(
          "[OK] Published good reputation verdict for seeder:",
          seederPeerId || seederAddress
        );
      } catch (reputationError) {
        console.error(
          "[X] Failed to publish reputation verdict:",
          reputationError
        );
        // Don't fail the payment if reputation update fails
      }

      // Notify backend about the payment - this will send P2P message to the seeder
      try {
        console.log(
          "[OUT] Sending payment notification with downloaderPeerId:",
          downloaderPeerId
        );
        console.log("[OUT] Type of downloaderPeerId:", typeof downloaderPeerId);
        console.log(
          "[OUT] Is downloaderPeerId a peer ID?",
          downloaderPeerId?.startsWith("12D3Koo")
        );

        await invoke("record_download_payment", {
          fileHash,
          fileName,
          fileSize,
          seederWalletAddress: seederAddress,
          seederPeerId: seederPeerId || seederAddress, // Fallback to wallet address if no peer ID
          downloaderAddress: currentWallet.address || "unknown",
          downloaderPeerId,
          amount,
          transactionId,
          transactionHash,
        });
        console.log("[OK] Payment notification sent to seeder:", seederAddress);
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
    downloaderPeerId: string,
    transactionHash?: string
  ): Promise<{ success: boolean; transactionId?: number; error?: string }> {
    try {
      // Generate unique key for this payment receipt
      const paymentKey = `${fileHash}-${downloaderAddress}`;

      // Check if we already received this payment
      if (this.receivedPayments.has(paymentKey)) {
        console.log("[WARN] Payment already received for:", paymentKey);
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

      // Update wallet totals from transaction history for immediate UI feedback.
      // The actual balance will be refreshed from the blockchain RPC on the next poll.
      wallet.update((w) => {
        const allTxs = get(transactions);
        const totalReceived = allTxs
          .filter((tx) => tx.status === "success" && tx.type === "received")
          .reduce((sum, tx) => sum + tx.amount, 0);
        const totalSpent = allTxs
          .filter((tx) => tx.status === "success" && tx.type === "sent")
          .reduce((sum, tx) => sum + tx.amount, 0);

        return {
          ...w,
          totalEarned: totalReceived,
          totalSpent: totalSpent,
        };
      });

      // Mark this payment as received
      this.receivedPayments.add(paymentKey);
      console.log("[OK] Marked payment as received:", paymentKey);

      // Publish reputation verdict for successful payment (seeder perspective)
      // Get our own peer ID first for the issuer_id
      let seederPeerId = currentWallet.address; // Fallback to wallet address
      try {
        seederPeerId = await invoke<string>("get_peer_id");
        console.log("[STATS] Got seeder peer ID:", seederPeerId);
      } catch (err) {
        console.warn(
          "Could not get peer ID for issuer_id, using wallet address:",
          err
        );
      }

      // Publish reputation verdict using signed message system (see docs/SIGNED_TRANSACTION_MESSAGES.md)
      try {
        console.log(
          "[STATS] Attempting to publish reputation verdict for seeder->downloader"
        );
        console.log("[STATS] downloaderPeerId:", downloaderPeerId);
        console.log("[STATS] downloaderAddress:", downloaderAddress);
        console.log(
          "[STATS] Using target_id:",
          downloaderPeerId || downloaderAddress
        );
        console.log("[STATS] Using issuer_id:", seederPeerId);

        await reputationService.publishVerdict({
          target_id: downloaderPeerId || downloaderAddress,
          tx_hash: transactionHash || null,
          outcome: "good",
          details: `Payment received for file: ${fileName}`,
          metric: "transaction",
          issued_at: Math.floor(Date.now() / 1000),
          issuer_id: seederPeerId,
          issuer_seq_no: transactionId,
        });

        console.log(
          "[OK] Published good reputation verdict for downloader:",
          downloaderPeerId || downloaderAddress
        );
      } catch (reputationError) {
        console.error(
          "[X] Failed to publish reputation verdict:",
          reputationError
        );
        // Don't fail the payment if reputation update fails
      }

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

      console.log("[PAY] Seeder payment credited:", {
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
    console.log(`Download cost: ${amount.toFixed(4)} Chiral`);

    let pricePerMb = await this.getDynamicPricePerMB(1.2);
    if (!Number.isFinite(pricePerMb) || pricePerMb <= 0) {
      pricePerMb = 0.001;
    }

    return {
      amount,
      pricePerMb: Number(pricePerMb.toFixed(8)),
      sizeInMB,
      formattedAmount: `${amount.toFixed(4)} Chiral`,
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
      console.log("[WARN] Payment notification polling already running");
      return;
    }

    console.log("[SYNC] Starting payment notification polling...");

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
      console.log("[STOP] Stopped payment notification polling");
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
      console.log("[PAY] Payment notification received:", notification);

      // Credit the seeder's wallet
      const result = await this.creditSeederPayment(
        notification.file_hash,
        notification.file_name,
        notification.file_size,
        notification.downloader_address,
        notification.transaction_hash
      );

      if (result.success) {
        console.log("[OK] Payment credited successfully");
      } else {
        console.warn("[WARN] Failed to credit payment:", result.error);
      }
    } catch (error) {
      console.error("Error handling payment notification:", error);
    }
  }
}

// Export singleton instance
export const paymentService = PaymentService;
