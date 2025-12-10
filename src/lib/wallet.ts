import { invoke } from "@tauri-apps/api/core";
import { get } from "svelte/store";
import {
  blockReward,
  etcAccount,
  miningState,
  transactions,
  transactionPagination,
  miningPagination,
  wallet,
  blacklist,
  type ETCAccount,
  type Transaction,
  type WalletInfo,
} from "$lib/stores";
import { showToast } from "$lib/toast";
import { t } from "svelte-i18n";

type TranslateParams = { values?: Record<string, unknown>; default?: string };
type TranslateFn = (key: string, params?: TranslateParams) => string;

const tr: TranslateFn = (key, params) =>
  (get(t) as unknown as TranslateFn)(key, params);

const DEFAULT_POLL_INTERVAL = 15_000;

export interface WalletServiceOptions {
  pollIntervalMs?: number;
  autoStartPolling?: boolean;
}

export interface AccountCreationResult {
  address: string;
  private_key: string;
  blacklist?: unknown[];
}

export interface TotpSetupInfo {
  secret: string;
  otpauthUrl: string;
}

export interface WalletExportSnapshot {
  address: string | undefined;
  balance: number;
  totalEarned?: number;
  totalSpent?: number;
  pendingTransactions: number;
  exportDate: string;
  version: string;
  privateKey?: string;
}

export interface ApiRequestSignature {
  address: string;
  signature: string;
  timestamp: number;
  bodyHash: string;
  canonicalMessage: string;
}

export class WalletService {
  private initialized = false;
  private pollHandle: ReturnType<typeof setInterval> | null = null;
  private pollInterval = DEFAULT_POLL_INTERVAL;
  private unsubscribeAccount?: () => void;
  private readonly isTauri: boolean;
  private readonly seenHashes = new Set<string>();
  private isRestoringAccount = false; // Flag to prevent sync during account restoration
  private progressiveLoadHandle: ReturnType<typeof setTimeout> | null = null;
  private isProgressiveLoading = false;
  private blockMinedUnsubscribe?: () => void;

  constructor() {
    this.isTauri =
      typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
  }

  async initialize(options?: WalletServiceOptions): Promise<void> {
    if (this.initialized) {
      return;
    }

    this.initialized = true;
    this.pollInterval = options?.pollIntervalMs ?? DEFAULT_POLL_INTERVAL;

    if (this.isTauri) {
      // Fetch block reward from backend (single source of truth)
      try {
        const reward = await invoke<number>("get_block_reward");
        blockReward.set(reward);
      } catch (err) {
        console.warn(
          "Failed to fetch block reward from backend, using default:",
          err
        );
      }

      await this.bindBlockMinedEvents();
      await this.syncFromBackend();
      if (options?.autoStartPolling !== false) {
        this.startPolling();
      }
    }

    this.unsubscribeAccount = etcAccount.subscribe(async (account) => {
      if (!account || !this.isTauri || this.isRestoringAccount) {
        return;
      }
      try {
        await this.refreshTransactions();
        await this.refreshBalance();
      } catch (err) {
        console.error("WalletService refresh failed", err);
      }
    });
  }

  setRestoringAccount(restoring: boolean): void {
    this.isRestoringAccount = restoring;
  }

  shutdown(): void {
    if (this.pollHandle) {
      clearInterval(this.pollHandle);
      this.pollHandle = null;
    }
    if (this.unsubscribeAccount) {
      this.unsubscribeAccount();
      this.unsubscribeAccount = undefined;
    }
    this.stopProgressiveLoading();
    if (this.blockMinedUnsubscribe) {
      try {
        this.blockMinedUnsubscribe();
      } catch (error) {
        console.warn("Failed to unsubscribe block_mined listener", error);
      }
      this.blockMinedUnsubscribe = undefined;
    }
    this.initialized = false;
    this.seenHashes.clear();
  }

  isDesktopEnvironment(): boolean {
    return this.isTauri;
  }

  async signApiRequest(
    method: string,
    path: string,
    body?: Uint8Array | null,
    options?: { timestamp?: number }
  ): Promise<ApiRequestSignature> {
    if (!this.isTauri) {
      throw new Error(
        "Ethereum authentication headers require the desktop app"
      );
    }

    const payload = body && body.length > 0 ? Array.from(body) : null;
    const result = (await invoke("sign_api_request", {
      method,
      path,
      body: payload,
      timestamp: options?.timestamp ?? null,
    })) as ApiRequestSignature;

    return result;
  }

  private startPolling(): void {
    if (!this.isTauri || this.pollHandle) {
      return;
    }

    this.pollHandle = setInterval(async () => {
      // Check backend for active account
      try {
        const hasAccount = await invoke<boolean>("has_active_account");
        if (!hasAccount) {
          return;
        }
      } catch (error) {
        console.error("Failed to check account status:", error);
        return;
      }

      // IMPORTANT: refreshTransactions must run BEFORE refreshBalance
      // because refreshBalance depends on blocksFound set by refreshTransactions
      try {
        await this.refreshTransactions();
        await this.refreshBalance();
      } catch (err) {
        console.error("WalletService poll failed", err);
      }
    }, this.pollInterval);
  }

  private async syncFromBackend(): Promise<void> {
    // Skip sync if we're restoring an account
    if (this.isRestoringAccount) {
      console.log(
        "[syncFromBackend] Skipping sync - account is being restored"
      );
      return;
    }

    // Check if Geth is running before trying to sync
    if (this.isTauri) {
      try {
        const isRunning = await invoke<boolean>("is_geth_running");
        if (!isRunning) {
          return; // Silently skip if Geth is not running
        }
      } catch (error) {
        console.warn("Failed to check Geth status:", error);
        return;
      }
    }

    // Check backend for active account
    try {
      const hasAccount = await invoke<boolean>("has_active_account");
      if (!hasAccount) {
        return;
      }
    } catch (error) {
      console.error("Failed to check account status:", error);
      return;
    }

    // IMPORTANT: refreshTransactions must run BEFORE refreshBalance
    // because refreshBalance depends on blocksFound set by refreshTransactions
    try {
      await this.refreshTransactions();
      await this.refreshBalance();
    } catch (err) {
      console.error("WalletService sync failed", err);
    }
  }

  async refreshTransactions(): Promise<void> {
    if (!this.isTauri) {
      return;
    }

    // Skip if we're restoring an account
    if (this.isRestoringAccount) {
      return;
    }

    // Check if Geth is running before trying to query blockchain
    try {
      const isRunning = await invoke<boolean>("is_geth_running");
      if (!isRunning) {
        return; // Silently skip if Geth is not running
      }
    } catch (error) {
      return; // Can't check Geth status, skip
    }

    // Get account address from backend
    let accountAddress: string;
    try {
      accountAddress = await invoke<string>("get_active_account_address");
    } catch (error) {
      // No active account
      return;
    }

    // Check if account changed - if so, clear everything and reset
    const currentPaginationState = get(transactionPagination);
    if (
      currentPaginationState.accountAddress !== accountAddress &&
      currentPaginationState.accountAddress !== null
    ) {
      this.seenHashes.clear();
      this.stopProgressiveLoading();
      transactions.set([]); // Clear old account's transactions

      // Reset pagination state for new account
      transactionPagination.update((state) => ({
        ...state,
        accountAddress: null, // Will be set below
        oldestBlockScanned: null,
        hasMore: true,
        isLoading: false,
      }));

      // Reset mining pagination state for new account
      miningPagination.update((state) => ({
        ...state,
        accountAddress: null, // Will be set below
        oldestBlockScanned: null,
        hasMore: true,
        isLoading: false,
      }));
    }

    try {
      // Get current block number to track pagination
      const currentBlock = await invoke<number>("get_current_block");

      // Check pending transactions and update their status
      await this.updatePendingTransactions();

      // Get data in parallel: mining blocks AND transaction history
      const [blocks, totalBlockCount, txHistory] = await Promise.all([
        invoke("get_recent_mined_blocks_pub", {
          address: accountAddress,
          lookback: 2000,
          limit: 50,
        }) as Promise<
          Array<{
            hash: string;
            timestamp: number;
            number: number;
            reward?: number;
          }>
        >,
        invoke("get_blocks_mined", {
          address: accountAddress,
        }) as Promise<number>,
        invoke("get_transaction_history", {
          address: accountAddress,
          lookback: 1000, // Scan last 1000 blocks for transactions
        }) as Promise<
          Array<{
            hash: string;
            from: string;
            to: string | null;
            value: string;
            block_number: number;
            timestamp: number;
            status: string;
            tx_type: string;
            gas_used: string | null;
            gas_price: string | null;
          }>
        >,
      ]);

      // If backend returns no history but we already have imported history, keep existing
      const existingTxs = get(transactions);
      if (
        txHistory.length === 0 &&
        blocks.length === 0 &&
        existingTxs.length > 0
      ) {
        transactionPagination.update((state) => ({
          ...state,
          accountAddress,
          oldestBlockScanned: currentBlock,
          hasMore: true,
          isLoading: false,
        }));
        miningPagination.update((state) => ({
          ...state,
          accountAddress,
          oldestBlockScanned: currentBlock,
          hasMore: true,
          isLoading: false,
        }));
        return;
      }

      // Update total count AND rewards together to keep them consistent
      // During active mining, don't override - the backend counter is the source of truth
      // The backend counter is initialized from accurateTotals and incremented when new blocks are mined
      const reward = get(blockReward);
      miningState.update((state) => ({
        ...state,
        blocksFound: totalBlockCount,
        totalRewards: totalBlockCount * reward,
      }));

      // Process mining rewards
      for (const block of blocks) {
        if (this.seenHashes.has(block.hash)) {
          continue;
        }
        this.seenHashes.add(block.hash);
        this.pushRecentBlock({
          hash: block.hash,
          timestamp: new Date((block.timestamp || 0) * 1000),
          reward: block.reward ?? get(blockReward),
          block_number: block.number,
        });
      }

      // Process regular transactions (sent/received)
      const newTransactions: Transaction[] = [];
      for (const tx of txHistory) {
        if (this.seenHashes.has(tx.hash)) {
          continue; // Skip already seen transactions
        }
        this.seenHashes.add(tx.hash);

        // Convert Wei to Chiral (1 Chiral = 10^18 Wei)
        const valueInWei = BigInt(tx.value);
        const valueInChiral = Number(valueInWei) / 1e18;

        // Skip zero-value transactions (likely contract interactions)
        if (valueInChiral === 0) {
          continue;
        }

        // Convert gas data from hex to numbers
        const gasUsed = tx.gas_used
          ? parseInt(tx.gas_used.replace("0x", ""), 16)
          : undefined;

        const gasPrice = tx.gas_price
          ? parseInt(tx.gas_price.replace("0x", ""), 16) / 1e9 // Convert Wei to Gwei
          : undefined;

        // Calculate fee in Chiral (gas_used * gas_price in Wei, then convert to Chiral)
        const feeInWei =
          gasUsed && tx.gas_price
            ? gasUsed * parseInt(tx.gas_price.replace("0x", ""), 16)
            : undefined;
        const feeInChiral = feeInWei ? feeInWei / 1e18 : undefined;

        const transaction: Transaction = {
          id: Date.now() + Math.random(), // Unique ID
          type: tx.tx_type as "sent" | "received",
          amount: valueInChiral,
          from: tx.tx_type === "sent" ? accountAddress : tx.from,
          to: tx.tx_type === "received" ? accountAddress : (tx.to ?? ""),
          date: new Date(tx.timestamp * 1000),
          description:
            tx.tx_type === "sent"
              ? `Sent to ${tx.to?.slice(0, 10)}...`
              : `Received from ${tx.from.slice(0, 10)}...`,
          status: tx.status === "success" ? "success" : "failed",
          hash: tx.hash,
          block_number: tx.block_number,
          timestamp: tx.timestamp,
          gas_used: gasUsed,
          gas_price: gasPrice, // Store as Gwei for display
          fee: feeInChiral,
        };

        newTransactions.push(transaction);
      }

      // Add new transactions to store (sorted by date, newest first)
      if (newTransactions.length > 0) {
        transactions.update((list) => {
          const combined = [...newTransactions, ...list];
          // Remove duplicates by hash, preferring confirmed blockchain data over pending
          const uniqueMap = new Map();
          for (const tx of combined) {
            const key = tx.hash || tx.txHash || `${tx.id}`;

            if (!uniqueMap.has(key)) {
              uniqueMap.set(key, tx);
            } else {
              const existing = uniqueMap.get(key);
              // Prefer confirmed transactions over pending
              // Prefer transactions with block_number (from blockchain) over manual entries
              if (
                (tx.status !== "pending" && existing?.status === "pending") ||
                (tx.block_number && !existing?.block_number)
              ) {
                uniqueMap.set(key, tx);
              }
            }
          }
          return Array.from(uniqueMap.values()).sort(
            (a, b) => b.date.getTime() - a.date.getTime()
          );
        });
      }

      // Update pagination state - reset if account changed or first load
      transactionPagination.update((state) => {
        // Reset if account changed or this is the first load
        if (
          state.accountAddress !== accountAddress ||
          state.oldestBlockScanned === null
        ) {
          const oldestScanned = Math.max(0, currentBlock - 1000);
          return {
            ...state,
            accountAddress,
            oldestBlockScanned: oldestScanned,
            hasMore: oldestScanned > 0,
          };
        }
        // Otherwise, keep existing state (preserve progress)
        return state;
      });

      // Update mining pagination state - reset if account changed or first load
      miningPagination.update((state) => {
        // Reset if account changed or this is the first load
        if (
          state.accountAddress !== accountAddress ||
          state.oldestBlockScanned === null
        ) {
          const oldestScanned = Math.max(0, currentBlock - 2000);
          return {
            ...state,
            accountAddress,
            oldestBlockScanned: oldestScanned,
            hasMore: oldestScanned > 0,
          };
        }
        // Otherwise, keep existing state (preserve progress)
        return state;
      });
    } catch (error) {
      // Expected when Geth is not running - silently skip
      console.error("Failed to refresh transactions:", error);
    }
  }

  private async bindBlockMinedEvents(): Promise<void> {
    if (!this.isTauri || this.blockMinedUnsubscribe) {
      return;
    }

    try {
      const { listen } = await import("@tauri-apps/api/event");
      this.blockMinedUnsubscribe = await listen("block_mined", async () => {
        try {
          await this.refreshTransactions();
          await this.refreshBalance();
        } catch (error) {
          console.error("[WalletService] Failed to refresh after block_mined", error);
        }
      });
    } catch (error) {
      console.warn("[WalletService] Could not bind block_mined listener:", error);
    }
  }

  async updatePendingTransactions(): Promise<void> {
    const currentTransactions = get(transactions);
    const pendingTransactions = currentTransactions.filter(
      (tx) => tx.status === "pending" && tx.txHash
    );

    if (pendingTransactions.length === 0) {
      return;
    }

    // Check each pending transaction
    for (const tx of pendingTransactions) {
      try {
        const receipt = await invoke<any>("get_transaction_receipt", {
          txHash: tx.txHash,
        });

        // If receipt exists and is not null, transaction has been mined
        if (receipt) {
          const success = receipt.status === "0x1" || receipt.status === 1;

          transactions.update((txs) =>
            txs.map((t) =>
              t.txHash === tx.txHash
                ? {
                    ...t,
                    status: success
                      ? ("success" as const)
                      : ("failed" as const),
                  }
                : t
            )
          );

          // Update pending count
          if (tx.type === "sent") {
            wallet.update((w) => ({
              ...w,
              pendingTransactions: Math.max(
                0,
                (w.pendingTransactions ?? 0) - 1
              ),
            }));
          }
        }
      } catch (error) {
        // Transaction receipt not available yet or RPC error
        console.log(`Could not get receipt for ${tx.txHash}:`, error);
      }
    }
  }

  async refreshBalance(): Promise<void> {
    if (!this.isTauri) {
      console.log("[refreshBalance] Not in Tauri, skipping");
      return;
    }

    // Skip if we're restoring an account
    if (this.isRestoringAccount) {
      return;
    }

    // Check if Geth is running before trying to query blockchain
    try {
      const isRunning = await invoke<boolean>("is_geth_running");
      if (!isRunning) {
        console.log("[refreshBalance] Geth not running, skipping");
        return; // Silently skip if Geth is not running
      }
    } catch (error) {
      console.log("[refreshBalance] Could not check Geth status:", error);
      return; // Can't check Geth status, skip
    }

    // Get account address from backend
    let accountAddress: string;
    try {
      accountAddress = await invoke<string>("get_active_account_address");
    } catch (error) {
      console.log("[refreshBalance] No active account:", error);
      return;
    }

    try {
      // Try to get balance from geth
      let realBalance = 0;
      try {
        const balanceStr = (await invoke("get_account_balance", {
          address: accountAddress,
        })) as string;
        realBalance = parseFloat(balanceStr);
      } catch (e) {
        const errorMsg = String(e);
        // Only log if it's not a known blockchain state issue
        if (
          !errorMsg.includes("missing trie node") &&
          !errorMsg.includes("not available")
        ) {
          console.log("[refreshBalance] Could not get balance from geth:", e);
        }
      }

      const prevWallet = get(wallet);

      // If geth returns zero but we already have a non-zero balance (e.g., from an imported snapshot),
      // avoid clobbering it until real data is available.
      if (realBalance === 0 && prevWallet.balance > 0) {
        return;
      }

      // Reconcile any lingering pending/ submitted sent txs by checking their receipts
      const pendingSentTxs = get(transactions).filter(
        (tx) =>
          (tx.status === "pending" || tx.status === "submitted") &&
          tx.type === "sent" &&
          (tx.hash || tx.txHash)
      );

      if (pendingSentTxs.length > 0) {
        for (const tx of pendingSentTxs) {
          const hash = tx.hash || tx.txHash;
          if (!hash) continue;

          try {
            const receipt = await invoke<any>("get_transaction_receipt", {
              txHash: hash,
            });

            if (receipt && receipt.block_number !== null) {
              const status =
                receipt.status === "success" ? "success" : "failed";
              const confirmations = receipt.confirmations || 0;

              transactions.update((txs) =>
                txs.map((t) =>
                  t.txHash === hash || t.hash === hash
                    ? {
                        ...t,
                        status: status as "success" | "failed",
                        confirmations,
                        block_number: receipt.block_number ?? t.block_number,
                      }
                    : t
                )
              );

              wallet.update((w) => ({
                ...w,
                pendingTransactions: Math.max(
                  0,
                  (w.pendingTransactions ?? 0) - 1
                ),
              }));
            }
          } catch (err) {
            // Ignore receipt lookup errors; we'll try again on next poll
          }
        }
      }

      // Calculate pending sent transactions (after reconciliation)
      // Exclude transactions to blacklisted addresses - they shouldn't reduce available balance
      const blacklistedAddresses = new Set(
        get(blacklist).map((entry) => entry.chiral_address.toLowerCase())
      );
      
      const pendingSent = get(transactions)
        .filter((tx) => {
          if (tx.status !== "pending" || tx.type !== "sent") {
            return false;
          }
          // Exclude pending transactions to blacklisted addresses
          if (tx.to && blacklistedAddresses.has(tx.to.toLowerCase())) {
            return false;
          }
          return true;
        })
        .reduce((sum, tx) => sum + tx.amount, 0);

      // Use real balance from Geth (no fallback - if Geth says 0, show 0 unless guarded above)
      const actualBalance = realBalance;
      const availableBalance = Math.max(0, actualBalance - pendingSent);
      wallet.update((current) => ({
        ...current,
        balance: availableBalance,
        actualBalance,
      }));

      // Note: totalRewards and blocksFound are now both set together in refreshTransactions
      // to ensure they stay consistent (totalRewards = blocksFound * blockReward)
    } catch (error) {
      console.error("Failed to refresh balance:", error);
    }
  }

  async loadMoreTransactions(): Promise<void> {
    if (!this.isTauri) {
      return;
    }

    // Check if we're already loading or if there are no more transactions
    const paginationState = get(transactionPagination);
    if (paginationState.isLoading || !paginationState.hasMore) {
      return;
    }

    // Set loading state
    transactionPagination.update((state) => ({ ...state, isLoading: true }));

    try {
      // Check if Geth is running
      const isRunning = await invoke<boolean>("is_geth_running");
      if (!isRunning) {
        transactionPagination.update((state) => ({
          ...state,
          isLoading: false,
        }));
        return;
      }

      // Get account address
      const accountAddress = await invoke<string>("get_active_account_address");

      // Check if pagination state matches current account
      // If not, skip this load and let refreshTransactions() initialize it properly
      if (paginationState.accountAddress !== accountAddress) {
        transactionPagination.update((state) => ({
          ...state,
          isLoading: false,
        }));
        return;
      }

      // If oldestBlockScanned is null, pagination hasn't been initialized yet
      if (paginationState.oldestBlockScanned === null) {
        transactionPagination.update((state) => ({
          ...state,
          isLoading: false,
        }));
        return;
      }

      // Calculate the block range for this batch
      const toBlock = paginationState.oldestBlockScanned;
      const fromBlock = Math.max(0, toBlock - paginationState.batchSize);

      // Fetch transactions for this range
      const txHistory = (await invoke("get_transaction_history_range", {
        address: accountAddress,
        fromBlock: fromBlock,
        toBlock: toBlock,
      })) as Array<{
        hash: string;
        from: string;
        to: string | null;
        value: string;
        block_number: number;
        timestamp: number;
        status: string;
        tx_type: string;
        gas_used: string | null;
        gas_price: string | null;
      }>;

      // Process transactions
      const newTransactions: Transaction[] = [];
      for (const tx of txHistory) {
        if (this.seenHashes.has(tx.hash)) {
          continue;
        }
        this.seenHashes.add(tx.hash);

        const valueInWei = BigInt(tx.value);
        const valueInChiral = Number(valueInWei) / 1e18;

        if (valueInChiral === 0) {
          continue;
        }

        const gasUsed = tx.gas_used
          ? parseInt(tx.gas_used.replace("0x", ""), 16)
          : undefined;

        const gasPrice = tx.gas_price
          ? parseInt(tx.gas_price.replace("0x", ""), 16) / 1e9
          : undefined;

        const feeInWei =
          gasUsed && tx.gas_price
            ? gasUsed * parseInt(tx.gas_price.replace("0x", ""), 16)
            : undefined;
        const feeInChiral = feeInWei ? feeInWei / 1e18 : undefined;

        const transaction: Transaction = {
          id: Date.now() + Math.random(),
          type: tx.tx_type as "sent" | "received",
          amount: valueInChiral,
          from: tx.tx_type === "sent" ? accountAddress : tx.from,
          to: tx.tx_type === "received" ? accountAddress : (tx.to ?? ""),
          date: new Date(tx.timestamp * 1000),
          description:
            tx.tx_type === "sent"
              ? `Sent to ${tx.to?.slice(0, 10)}...`
              : `Received from ${tx.from.slice(0, 10)}...`,
          status: tx.status === "success" ? "success" : "failed",
          hash: tx.hash,
          block_number: tx.block_number,
          timestamp: tx.timestamp,
          gas_used: gasUsed,
          gas_price: gasPrice,
          fee: feeInChiral,
        };

        newTransactions.push(transaction);
      }

      // Add new transactions to store
      if (newTransactions.length > 0) {
        transactions.update((list) => {
          const combined = [...list, ...newTransactions];
          const uniqueMap = new Map();
          for (const tx of combined) {
            const key = tx.hash || tx.txHash || `${tx.id}`;
            if (!uniqueMap.has(key)) {
              uniqueMap.set(key, tx);
            } else {
              const existing = uniqueMap.get(key);
              if (
                (tx.status !== "pending" && existing?.status === "pending") ||
                (tx.block_number && !existing?.block_number)
              ) {
                uniqueMap.set(key, tx);
              }
            }
          }
          return Array.from(uniqueMap.values()).sort(
            (a, b) => b.date.getTime() - a.date.getTime()
          );
        });
      }

      // Update pagination state
      transactionPagination.update((state) => ({
        ...state,
        oldestBlockScanned: fromBlock,
        hasMore: fromBlock > 0,
        isLoading: false,
      }));
    } catch (error) {
      console.error("Failed to load more transactions:", error);
      transactionPagination.update((state) => ({ ...state, isLoading: false }));
    }
  }

  async loadMoreMiningRewards(): Promise<void> {
    if (!this.isTauri) {
      return;
    }

    // Check if we're already loading or if there are no more mining rewards
    const paginationState = get(miningPagination);
    if (paginationState.isLoading || !paginationState.hasMore) {
      return;
    }

    // Set loading state
    miningPagination.update((state) => ({ ...state, isLoading: true }));

    try {
      // Check if Geth is running
      const isRunning = await invoke<boolean>("is_geth_running");
      if (!isRunning) {
        miningPagination.update((state) => ({ ...state, isLoading: false }));
        return;
      }

      // Get account address
      const accountAddress = await invoke<string>("get_active_account_address");

      // Check if pagination state matches current account
      if (paginationState.accountAddress !== accountAddress) {
        miningPagination.update((state) => ({ ...state, isLoading: false }));
        return;
      }

      // If oldestBlockScanned is null, pagination hasn't been initialized yet
      if (paginationState.oldestBlockScanned === null) {
        miningPagination.update((state) => ({ ...state, isLoading: false }));
        return;
      }

      // Calculate the block range for this batch
      const toBlock = paginationState.oldestBlockScanned;
      const fromBlock = Math.max(0, toBlock - paginationState.batchSize);

      // Fetch mining blocks for this range
      const miningBlocks = (await invoke("get_mined_blocks_range", {
        address: accountAddress,
        fromBlock: fromBlock,
        toBlock: toBlock,
      })) as Array<{
        hash: string;
        timestamp: number;
        number: number;
        reward?: number;
      }>;

      // Process mining blocks
      for (const block of miningBlocks) {
        if (this.seenHashes.has(block.hash)) {
          continue;
        }
        this.seenHashes.add(block.hash);
        this.pushRecentBlock({
          hash: block.hash,
          timestamp: new Date((block.timestamp || 0) * 1000),
          reward: block.reward ?? get(blockReward),
          block_number: block.number,
        });
      }

      // Update pagination state
      miningPagination.update((state) => ({
        ...state,
        oldestBlockScanned: fromBlock,
        hasMore: fromBlock > 0,
        isLoading: false,
      }));
    } catch (error) {
      console.error("Failed to load more mining rewards:", error);
      miningPagination.update((state) => ({ ...state, isLoading: false }));
    }
  }

  async startProgressiveLoading(): Promise<void> {
    if (this.isProgressiveLoading) {
      return;
    }

    this.isProgressiveLoading = true;

    const loadNextBatch = async () => {
      if (!this.isProgressiveLoading) {
        return;
      }

      const paginationState = get(transactionPagination);

      // Stop if no more transactions or if we're manually loading
      if (!paginationState.hasMore || paginationState.isLoading) {
        this.isProgressiveLoading = false;
        return;
      }

      // Load next batch
      await this.loadMoreTransactions();

      // Schedule next batch after a short delay (500ms)
      if (this.isProgressiveLoading && get(transactionPagination).hasMore) {
        this.progressiveLoadHandle = setTimeout(loadNextBatch, 500);
      } else {
        this.isProgressiveLoading = false;
      }
    };

    // Start loading
    loadNextBatch();
  }

  stopProgressiveLoading(): void {
    this.isProgressiveLoading = false;
    if (this.progressiveLoadHandle) {
      clearTimeout(this.progressiveLoadHandle);
      this.progressiveLoadHandle = null;
    }
  }

  async ensureGethRunning(): Promise<boolean> {
    if (!this.isTauri) {
      return false;
    }
    try {
      return (await invoke("is_geth_running")) as boolean;
    } catch (error) {
      console.error("Failed to check Geth status:", error);
      return false;
    }
  }

  async createAccount(): Promise<AccountCreationResult> {
    if (this.isTauri) {
      const account = (await invoke(
        "create_chiral_account"
      )) as AccountCreationResult;
      transactions.set([]);
      this.seenHashes.clear();
      this.setActiveAccount(account);
      await this.syncFromBackend();
      return account;
    }

    const account = this.createDemoAccount();
    this.seenHashes.clear();
    this.setActiveAccount(account);
    transactions.set([]);
    return account;
  }

  async importAccount(privateKey: string): Promise<AccountCreationResult> {
    if (!privateKey?.trim()) {
      throw new Error("Private key is required");
    }

    if (this.isTauri) {
      const account = (await invoke("import_chiral_account", {
        privateKey,
      })) as AccountCreationResult;
      transactions.set([]); // clear old account's txs
      this.seenHashes.clear();
      this.setActiveAccount(account);

      // Prime pagination state for the new account so we don't wipe imported tx snapshots
      transactionPagination.update((state) => ({
        ...state,
        accountAddress: account.address,
        oldestBlockScanned: null,
        hasMore: true,
        isLoading: false,
      }));
      miningPagination.update((state) => ({
        ...state,
        accountAddress: account.address,
        oldestBlockScanned: null,
        hasMore: true,
        isLoading: false,
      }));

      await this.syncFromBackend();
      return account;
    }

    const demo = this.createDemoAccount(privateKey);
    this.seenHashes.clear();
    this.setActiveAccount(demo);
    transactions.set([]);
    return demo;
  }

  async sendTransaction(toAddress: string, amount: number): Promise<string> {
    if (!this.isTauri) {
      throw new Error("Transactions are only available in the desktop app");
    }

    // Verify account exists in backend before attempting transaction
    const hasAccount = await invoke<boolean>("has_active_account");
    if (!hasAccount) {
      throw new Error("No active account. Please log in.");
    }

    // Get account address from backend for transaction record
    const accountAddress = await invoke<string>("get_active_account_address");
    console.log(
      `[Transaction] Sending ${amount} CN from ${accountAddress} to ${toAddress}`
    );

    const txHash = (await invoke("send_chiral_transaction", {
      toAddress,
      amount,
    })) as string;

    console.log(`[Transaction] ✅ Broadcast successful! Hash: ${txHash}`);
    console.log(
      `[Transaction] Status: PENDING - monitoring for confirmation...`
    );

    wallet.update((w) => ({
      ...w,
      balance: w.balance - amount,
      pendingTransactions: (w.pendingTransactions ?? 0) + 1,
    }));

    transactions.update((existing) => [
      {
        id: Date.now(),
        type: "sent",
        amount,
        to: toAddress,
        from: accountAddress,
        date: new Date(),
        description: `Sent to ${toAddress.slice(0, 10)}...`,
        status: "pending",
        hash: txHash, // Use 'hash' to match blockchain-scanned transactions
        txHash,
      },
      ...existing,
    ]);

    // Start monitoring this transaction
    this.monitorTransaction(txHash, amount, toAddress);

    return txHash;
  }

  private async monitorTransaction(
    txHash: string,
    amount: number,
    toAddress: string
  ): Promise<void> {
    console.log(`[TX Monitor] 👀 Monitoring ${txHash.substring(0, 10)}...`);

    let attempts = 0;
    const maxAttempts = 60;

    const checkInterval = setInterval(async () => {
      attempts++;

      try {
        const receipt = await invoke<any>("get_transaction_receipt", {
          txHash,
        });

        if (receipt && receipt.block_number !== null) {
          clearInterval(checkInterval);

          const confirmations = receipt.confirmations || 0;
          const status = receipt.status === "success" ? "success" : "failed";

          console.log(
            `[TX Monitor] ✅ ${status.toUpperCase()} in block ${receipt.block_number}`
          );
          console.log(
            `[TX Monitor] Confirmations: ${confirmations}, Gas: ${receipt.gas_used}`
          );

          transactions.update((txs) =>
            txs.map((tx) =>
              tx.txHash === txHash || tx.hash === txHash
                ? {
                    ...tx,
                    status: status as "success" | "failed",
                    confirmations,
                  }
                : tx
            )
          );

          wallet.update((w) => ({
            ...w,
            pendingTransactions: Math.max(0, (w.pendingTransactions ?? 0) - 1),
          }));

          await this.refreshBalance();

          if (status === "success") {
            // showToast(
            //   `Transaction confirmed! ${amount} CN sent to ${toAddress.substring(0, 10)}... (Block ${receipt.block_number})`,
            //   "success"
            // );
            showToast(
              tr("toasts.wallet.transaction.confirmed", {
                values: {
                  amount,
                  address: toAddress.substring(0, 10),
                  block: receipt.block_number,
                },
              }),
              "success"
            );
          } else {
            // showToast(
            //   `Transaction failed in block ${receipt.block_number}. Your funds were not sent.`,
            //   "error"
            // );
            showToast(
              tr("toasts.wallet.transaction.failed", {
                values: { block: receipt.block_number },
              }),
              "error"
            );
          }
        } else if (attempts % 6 === 0) {
          console.log(
            `[TX Monitor] ⏳ Pending... (${attempts * 5}s) - Mining active?`
          );
        }

        if (attempts >= maxAttempts) {
          clearInterval(checkInterval);
          console.warn(
            `[TX Monitor] ⚠️ Timeout after ${maxAttempts * 5}s - check Blockchain page`
          );
        }
      } catch (error) {
        if (attempts % 12 === 0) {
          console.log(`[TX Monitor] ⏳ Still pending (${attempts * 5}s)...`);
        }
      }
    }, 5000);
  }

  async saveToKeystore(password: string, account: ETCAccount): Promise<void> {
    if (!this.isTauri) {
      return;
    }

    if (!account) {
      throw new Error("No active account to save");
    }

    await invoke("save_account_to_keystore", {
      address: account.address,
      privateKey: account.private_key,
      password,
    });
  }

  async listKeystoreAccounts(): Promise<string[]> {
    if (!this.isTauri) {
      return [];
    }
    try {
      return (await invoke("list_keystore_accounts")) as string[];
    } catch (error) {
      console.error("Failed to list keystore accounts:", error);
      return [];
    }
  }

  async loadFromKeystore(
    address: string,
    password: string
  ): Promise<AccountCreationResult> {
    if (!this.isTauri) {
      throw new Error("Keystore access is only available in the desktop app");
    }

    const account = (await invoke("load_account_from_keystore", {
      address,
      password,
    })) as AccountCreationResult;
    this.setActiveAccount(account);
    await this.syncFromBackend();
    return account;
  }

  async deleteKeystoreAccount(address: string): Promise<void> {
    if (!this.isTauri) {
      throw new Error('Keystore deletion is only available in the desktop app');
    }

    try {
      await invoke('remove_account_from_keystore', { address });
    } catch (error) {
      console.error('Failed to delete keystore account:', error);
      throw error;
    }
  }

  async exportSnapshot(options?: {
    includePrivateKey?: boolean;
  }): Promise<WalletExportSnapshot> {
    const walletState = get(wallet);
    const account = get(etcAccount);

    let privateKey: string | undefined = account?.private_key;

    // If private key is not in frontend store and user wants to include it,
    // fetch it from backend
    if (options?.includePrivateKey && !privateKey && this.isTauri) {
      try {
        privateKey = await invoke<string>("get_active_account_private_key");
      } catch (error) {
        console.error("Failed to get private key from backend:", error);
      }
    }

    return {
      address: account?.address,
      balance: walletState.balance,
      totalEarned: walletState.totalEarned,
      totalSpent: walletState.totalSpent,
      pendingTransactions: walletState.pendingTransactions,
      exportDate: new Date().toISOString(),
      version: "1.0",
      privateKey: options?.includePrivateKey ? privateKey : undefined,
    };
  }

  async generateTwoFactorSetup(): Promise<TotpSetupInfo> {
    if (!this.isTauri) {
      throw new Error("2FA is only available in the desktop app");
    }
    const result = (await invoke("generate_totp_secret")) as {
      secret: string;
      otpauth_url: string;
    };
    return {
      secret: result.secret,
      otpauthUrl: result.otpauth_url,
    };
  }

  async verifyAndEnableTwoFactor(
    secret: string,
    code: string,
    password: string
  ): Promise<boolean> {
    if (!this.isTauri) {
      throw new Error("2FA is only available in the desktop app");
    }
    return (await invoke("verify_and_enable_totp", {
      secret,
      code,
      password,
    })) as boolean;
  }

  async verifyTwoFactor(code: string, password: string): Promise<boolean> {
    if (!this.isTauri) {
      throw new Error("2FA is only available in the desktop app");
    }
    return (await invoke("verify_totp_code", {
      code,
      password,
    })) as boolean;
  }

  async disableTwoFactor(password: string): Promise<void> {
    if (!this.isTauri) {
      throw new Error("2FA is only available in the desktop app");
    }
    await invoke("disable_2fa", { password });
  }

  async isTwoFactorEnabled(): Promise<boolean> {
    if (!this.isTauri) {
      return false;
    }
    try {
      return (await invoke("is_2fa_enabled")) as boolean;
    } catch (error) {
      // This is normal for new accounts or accounts without 2FA configured
      return false;
    }
  }

  async calculateAccurateTotals(): Promise<void> {
    if (!this.isTauri) {
      throw new Error(
        "Accurate totals calculation is only available in the desktop app"
      );
    }

    // Get account address from backend
    let accountAddress: string;
    try {
      accountAddress = await invoke<string>("get_active_account_address");
    } catch (error) {
      throw new Error("No active account");
    }

    const {
      isCalculatingAccurateTotals,
      accurateTotals,
      accurateTotalsProgress,
    } = await import("$lib/stores");

    // Set loading state
    isCalculatingAccurateTotals.set(true);
    accurateTotalsProgress.set(null);

    // Capture session counter BEFORE starting the scan
    // This tells us how many blocks were mined before the scan started
    // Any blocks in this counter might already be on the blockchain and included in the scan
    let sessionCounterAtStart = 0;
    try {
      sessionCounterAtStart = await invoke<number>("get_blocks_mined", {
        address: accountAddress,
      });
    } catch (e) {
      // Ignore - assume 0
    }

    // Listen for progress events
    const { listen } = await import("@tauri-apps/api/event");
    const unlisten = await listen<{
      current_block: number;
      total_blocks: number;
      percentage: number;
    }>("accurate-totals-progress", (event) => {
      accurateTotalsProgress.set({
        currentBlock: event.payload.current_block,
        totalBlocks: event.payload.total_blocks,
        percentage: event.payload.percentage,
      });
    });

    try {
      const result = await invoke<{
        blocks_mined: number;
        total_received: number;
        total_sent: number;
      }>("calculate_accurate_totals", {
        address: accountAddress,
      });

      // Store the results
      console.debug("[Accurate Totals] Updating store with:", {
        blocksMined: result.blocks_mined,
        totalReceived: result.total_received,
        totalSent: result.total_sent,
      });
      accurateTotals.set({
        blocksMined: result.blocks_mined,
        totalReceived: result.total_received,
        totalSent: result.total_sent,
      });

      // Initialize the backend counter with accurate count + blocks mined during scan
      // - sessionCounterAtStart: blocks mined before scan (might be included in scan result)
      // - sessionCounterAtEnd: blocks mined before + during scan
      // - blocksDuringCalc = sessionCounterAtEnd - sessionCounterAtStart (blocks mined DURING scan)
      // - These blocks are NOT in scan result (scan captured current_block at start)
      // - Total = scan result + blocks mined during scan
      try {
        // Get current session counter (after scan completed)
        const sessionCounterAtEnd = await invoke<number>("get_blocks_mined", {
          address: accountAddress,
        });

        // Blocks mined DURING the scan are not included in the scan result
        // (because scan only goes up to current_block captured at start)
        const blocksDuringCalc = sessionCounterAtEnd - sessionCounterAtStart;

        // Total = historical (from scan) + blocks mined during scan
        // This avoids double-counting:
        // - Blocks before scan start might be in scan result (we don't add them again)
        // - Blocks during scan are definitely NOT in scan result (we add them)
        const totalCount = result.blocks_mined + blocksDuringCalc;

        await invoke("initialize_mined_blocks_count", {
          address: accountAddress,
          count: totalCount,
        });
        console.debug(
          `[Accurate Totals] Initialized backend blocks count to: ${totalCount} (scan: ${result.blocks_mined}, during calc: ${blocksDuringCalc}, session start: ${sessionCounterAtStart}, session end: ${sessionCounterAtEnd})`
        );

        // Update the mining state store
        const reward = get(blockReward);
        miningState.update((state) => ({
          ...state,
          blocksFound: totalCount,
          totalRewards: totalCount * reward,
        }));
      } catch (initError) {
        console.warn(
          "[Accurate Totals] Failed to initialize backend counter:",
          initError
        );
      }

      console.debug(`[Accurate Totals] Complete!`, result);
    } catch (error) {
      console.error("Failed to calculate accurate totals:", error);
      throw error;
    } finally {
      isCalculatingAccurateTotals.set(false);
      accurateTotalsProgress.set(null);
      unlisten();
    }
  }

  private setActiveAccount(account: AccountCreationResult): void {
    const formatted: ETCAccount = {
      address: account.address,
      private_key: account.private_key,
    };
    etcAccount.set(formatted);

    wallet.update((w: WalletInfo) => ({
      ...w,
      address: formatted.address,
      balance: 0, // Reset balance for new account
      actualBalance: 0,
      pendingTransactions: 0,
    }));
  }

  private pushRecentBlock(block: {
    hash: string;
    reward?: number;
    timestamp?: Date;
    block_number?: number;
  }): void {
    const reward = typeof block.reward === "number" ? block.reward : 0;

    const newBlock = {
      id: `block-${block.hash}-${block.timestamp?.getTime() ?? Date.now()}`,
      hash: block.hash,
      reward,
      timestamp: block.timestamp ?? new Date(),
      difficulty: 0,
      nonce: 0,
    };

    miningState.update((state) => ({
      ...state,
      recentBlocks: [newBlock, ...(state.recentBlocks ?? [])].slice(0, 50),
      // Don't modify blocksFound here - it's set by refreshTransactions from backend
      // This method is only called during refreshTransactions, so blocksFound is already correct
    }));

    if (reward > 0) {
      const last4 = block.hash.slice(-4);
      const tx: Transaction = {
        id: Date.now(),
        type: "mining",
        amount: reward,
        from: "Mining reward",
        date: block.timestamp ?? new Date(),
        description: `Block Reward (…${last4})`,
        status: "success",
        block_number: block.block_number,
        hash: block.hash,
      };
      transactions.update((list) => [tx, ...list]);
    }
  }

  private createDemoAccount(
    overridePrivateKey?: string
  ): AccountCreationResult {
    const address = this.randomHex(40);
    const private_key = overridePrivateKey?.startsWith("0x")
      ? overridePrivateKey
      : `0x${(overridePrivateKey ?? this.randomHex(64)).replace(/^0x/, "")}`;
    return { address: `0x${address}`, private_key };
  }

  private randomHex(length: number): string {
    const chars = "0123456789abcdef";
    let out = "";
    for (let i = 0; i < length; i += 1) {
      out += chars[Math.floor(Math.random() * chars.length)];
    }
    return out;
  }
}

export const walletService = new WalletService();
