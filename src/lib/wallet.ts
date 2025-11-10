import { invoke } from "@tauri-apps/api/core";
import { get } from "svelte/store";
import {
  etcAccount,
  miningState,
  transactions,
  wallet,
  type ETCAccount,
  type Transaction,
  type WalletInfo,
} from "$lib/stores";

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
      console.log("[refreshTransactions] Skipping - account is being restored");
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

    try {
      // Get data in parallel
      const [blocks, totalBlockCount] = await Promise.all([
        invoke("get_recent_mined_blocks_pub", {
          address: accountAddress,
          lookback: 2000,
          limit: 50,
        }) as Promise<
          Array<{ hash: string; timestamp: number; reward?: number }>
        >,
        invoke("get_blocks_mined", {
          address: accountAddress,
        }) as Promise<number>,
      ]);

      // Update total count FIRST, before adding blocks
      miningState.update((state) => ({
        ...state,
        blocksFound: totalBlockCount,
      }));

      for (const block of blocks) {
        if (this.seenHashes.has(block.hash)) {
          continue;
        }
        this.seenHashes.add(block.hash);
        this.pushRecentBlock({
          hash: block.hash,
          timestamp: new Date((block.timestamp || 0) * 1000),
          reward: block.reward ?? 2,
        });
      }
    } catch (error) {
      // Expected when Geth is not running - silently skip
    }
  }

  async refreshBalance(): Promise<void> {
    if (!this.isTauri) {
      return;
    }

    // Skip if we're restoring an account
    if (this.isRestoringAccount) {
      console.log("[refreshBalance] Skipping - account is being restored");
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

    try {
      // Get actual total blocks mined from miningState (set by refreshTransactions)
      const currentMiningState = get(miningState);
      const actualBlocksFound = currentMiningState.blocksFound ?? 0;

      // Calculate total rewards based on ACTUAL blocks found, not recentBlocks length
      const totalEarned = actualBlocksFound * 2;

      // Try to get balance from geth
      let realBalance = 0;
      try {
        const balanceStr = (await invoke("get_account_balance", {
          address: accountAddress,
        })) as string;
        realBalance = parseFloat(balanceStr);
      } catch (e) {
        // Expected when Geth is not running
      }

      // Calculate pending sent transactions
      const pendingSent = get(transactions)
        .filter((tx) => tx.status === "pending" && tx.type === "sent")
        .reduce((sum, tx) => sum + tx.amount, 0);

      // Use real balance from Geth, or totalEarned if blocks haven't matured yet
      // In test networks or when blocks are immature, realBalance may be 0 even though we've mined
      const actualBalance = realBalance > 0 ? realBalance : totalEarned;
      const availableBalance = Math.max(0, actualBalance - pendingSent);

      wallet.update((current) => ({
        ...current,
        balance: availableBalance,
        actualBalance,
      }));

      // Update pending transaction status if they've been confirmed
      // If we have pending sent transactions, check if the balance has decreased
      // to mark them as completed
      if (pendingSent > 0 && realBalance > 0) {
        const expectedBalanceAfterPending = availableBalance;
        // If real balance is lower than expected (meaning pending txs were processed),
        // mark pending sent transactions as completed
        if (realBalance < expectedBalanceAfterPending + pendingSent - 0.01) {
          transactions.update((txs) =>
            txs.map((tx) =>
              tx.status === "pending" && tx.type === "sent"
                ? { ...tx, status: "success" as const }
                : tx
            )
          );
        }
      }

      // Update mining state totalRewards (don't override blocksFound - it's set by refreshTransactions)
      miningState.update((state) => ({
        ...state,
        totalRewards: totalEarned,
        // blocksFound is already correctly set by refreshTransactions
      }));
    } catch (error) {
      console.error("Failed to refresh balance:", error);
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
      transactions.set([]);
      this.seenHashes.clear();
      this.setActiveAccount(account);
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

    const txHash = (await invoke("send_chiral_transaction", {
      toAddress,
      amount,
    })) as string;

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
        description: "Manual transfer",
        status: "pending",
        txHash,
      },
      ...existing,
    ]);

    return txHash;
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
        type: "received",
        amount: reward,
        from: "Mining reward",
        date: block.timestamp ?? new Date(),
        description: `Block Reward (…${last4})`,
        status: "success",
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
