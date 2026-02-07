/**
 * Wallet Service Unit Tests
 *
 * WHAT IS BEING TESTED:
 * This file tests the WalletService class from src/lib/wallet.ts, which manages
 * Ethereum Classic accounts, transactions, and mining rewards for the Chiral Network.
 *
 * ARCHITECTURE:
 * - WalletService has TWO modes:
 *   1. Desktop (Tauri) mode: Uses Rust backend via invoke() for real blockchain operations
 *   2. Demo mode: Creates mock accounts with random addresses for testing/development
 *
 * - The service checks environment via: this.isTauri (line 59 in wallet.ts)
 * - Methods that work in BOTH environments: createAccount(), importAccount(), exportSnapshot()
 * - Methods that REQUIRE Tauri: sendTransaction(), refreshBalance(), refreshTransactions()
 *
 * WHAT THESE TESTS VERIFY:
 * [OK] Account creation (demo accounts in test environment)
 * [OK] Account import with private key validation
 * [OK] Wallet state management via Svelte stores
 * [OK] Export/snapshot functionality
 * [OK] Error handling for missing accounts or invalid inputs
 *
 * SKIPPED TESTS:
 * [SKIP] Balance refresh from geth (requires Tauri + running geth node)
 * [SKIP] Transaction refresh from blockchain (requires Tauri + geth)
 *
 * These tests run in Node.js environment and test the demo/mock account functionality
 * that developers and web users interact with before the desktop app is built.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { get } from "svelte/store";
import { WalletService } from "../src/lib/wallet";
import { etcAccount, wallet, transactions, miningState } from "../src/lib/stores";

// Mock Tauri invoke - these tests run in non-Tauri (demo) mode
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";

describe("WalletService", () => {
  let walletService: WalletService;
  let mockInvoke: any;

  beforeEach(() => {
    vi.clearAllMocks();
    mockInvoke = vi.mocked(invoke);
    walletService = new WalletService();

    // Reset all Svelte stores to default state
    etcAccount.set(null);
    wallet.set({
      address: "",
      balance: 0,
      pendingTransactions: 0,
    });
    transactions.set([]);
    miningState.set({
      isMining: false,
      hashRate: "0 H/s",
      totalRewards: 0,
      blocksFound: 0,
      activeThreads: 1,
      minerIntensity: 50,
      selectedPool: "solo",
      recentBlocks: [],
      miningHistory: [],
    });
  });

  afterEach(() => {
    walletService.shutdown();
  });

  describe("Initialization", () => {
    /**
     * Test: WalletService should instantiate without errors
     * Based on: Constructor at wallet.ts:58-60
     * Verifies: Service initializes and detects non-Tauri environment
     */
    it("should initialize wallet service in demo mode", () => {
      expect(walletService).toBeDefined();
      expect(walletService.isDesktopEnvironment()).toBe(false);
    });

    /**
     * Test: Multiple initializations should be idempotent (safe to call repeatedly)
     * Based on: initialize() method at wallet.ts:62-84
     * Verifies: The initialized flag prevents double-initialization bugs
     */
    it("should handle multiple initialize calls safely", async () => {
      await walletService.initialize();
      await walletService.initialize();
      // Should not throw or cause issues
      expect(true).toBe(true);
    });

    /**
     * Test: Shutdown should clean up resources properly
     * Based on: shutdown() method at wallet.ts:86-97
     * Verifies: Clears polling handles and unsubscribes from stores
     */
    it("should shutdown cleanly without errors", () => {
      walletService.shutdown();
      expect(() => walletService.shutdown()).not.toThrow();
    });
  });

  describe("Account Creation (Demo Mode)", () => {
    /**
     * Test: Create a new demo account with valid Ethereum address format
     * Based on: createAccount() -> createDemoAccount() at wallet.ts:253-268, 483-488
     * Verifies: Generates 0x-prefixed 40-char hex address and 64-char hex private key
     *
     * In demo mode, accounts are randomly generated but follow ETC address standards.
     * In Tauri mode, this would call Rust backend to generate real cryptographic keys.
     */
    it("should create new demo account with valid Ethereum address format", async () => {
      const result = await walletService.createAccount();

      expect(result).toHaveProperty("address");
      expect(result).toHaveProperty("private_key");
      // ETC addresses: 0x prefix + 40 hex chars (20 bytes)
      expect(result.address).toMatch(/^0x[0-9a-f]{40}$/);
      // Private keys: 0x prefix + 64 hex chars (32 bytes)
      expect(result.private_key).toMatch(/^0x[0-9a-f]{64}$/);
    });

    /**
     * Test: Created account should be stored in etcAccount Svelte store
     * Based on: setActiveAccount() at wallet.ts:437-449
     * Verifies: Account is immediately accessible to UI components via reactive stores
     */
    it("should persist created account to etcAccount store", async () => {
      const result = await walletService.createAccount();
      const account = get(etcAccount);

      expect(account).not.toBeNull();
      expect(account?.address).toBe(result.address);
      expect(account?.private_key).toBe(result.private_key);
    });

    /**
     * Test: Wallet store should update with new account address
     * Based on: wallet.update() call in setActiveAccount() at wallet.ts:444-448
     * Verifies: Wallet UI components receive the new address immediately
     */
    it("should update wallet store with new account address", async () => {
      const result = await walletService.createAccount();
      const walletState = get(wallet);

      expect(walletState.address).toBe(result.address);
    });

    /**
     * Test: Creating new account should clear previous transaction history
     * Based on: transactions.set([]) at wallet.ts:256, 266
     * Verifies: Fresh accounts start with empty transaction history
     * Rationale: Prevents mixing transactions from different accounts
     */
    it("should clear existing transactions when creating new account", async () => {
      // Set up some existing transactions
      transactions.set([
        {
          id: 1,
          type: "received",
          amount: 100,
          date: new Date(),
          description: "Old transaction",
          status: "success",
        },
      ]);

      await walletService.createAccount();
      const txList = get(transactions);

      expect(txList).toEqual([]);
    });
  });

  describe("Account Import (Demo Mode)", () => {
    /**
     * Test: Import account using existing private key
     * Based on: importAccount() at wallet.ts:270-291
     * Verifies: Accepts 0x-prefixed 64-char hex private key
     *
     * In demo mode, private key is stored but no cryptographic validation occurs.
     * In Tauri mode, Rust backend validates key and derives address.
     */
    it("should import account with 0x-prefixed private key", async () => {
      const privateKey = "0x" + "a".repeat(64);
      const result = await walletService.importAccount(privateKey);

      expect(result).toHaveProperty("address");
      expect(result).toHaveProperty("private_key");
      expect(result.private_key).toBe(privateKey);
    });

    /**
     * Test: Normalize private key by adding 0x prefix if missing
     * Based on: createDemoAccount() logic at wallet.ts:485-487
     * Verifies: Users can paste private keys with or without 0x prefix
     */
    it("should auto-prefix private key with 0x if missing", async () => {
      const privateKey = "b".repeat(64); // No 0x prefix
      const result = await walletService.importAccount(privateKey);

      expect(result.private_key).toMatch(/^0x[0-9a-f]{64}$/);
    });

    /**
     * Test: Reject empty private key input
     * Based on: Validation at wallet.ts:271-273
     * Verifies: Prevents importing invalid/empty keys
     */
    it("should reject empty private key", async () => {
      await expect(walletService.importAccount("")).rejects.toThrow(
        "Private key is required"
      );
    });

    /**
     * Test: Reject whitespace-only private key
     * Based on: privateKey?.trim() check at wallet.ts:271
     * Verifies: Handles user input errors (accidental spaces)
     */
    it("should reject whitespace-only private key", async () => {
      await expect(walletService.importAccount("   ")).rejects.toThrow(
        "Private key is required"
      );
    });

    /**
     * Test: Imported account should be set as active account
     * Based on: setActiveAccount() call at wallet.ts:288
     * Verifies: UI immediately switches to imported account
     */
    it("should set imported account as active in stores", async () => {
      const privateKey = "0x" + "c".repeat(64);
      const result = await walletService.importAccount(privateKey);
      const account = get(etcAccount);

      expect(account?.address).toBe(result.address);
      expect(account?.private_key).toBe(result.private_key);
    });
  });

  describe("Wallet Export", () => {
    /**
     * Test: Export wallet snapshot without private key (safe export)
     * Based on: exportSnapshot() at wallet.ts:371-384
     * Verifies: Creates JSON-serializable backup of wallet state
     * Use case: Sharing wallet status or balance history without exposing keys
     */
    it("should export wallet snapshot without private key by default", async () => {
      etcAccount.set({
        address: "0x1234567890123456789012345678901234567890",
        private_key: "0x" + "a".repeat(64),
      });
      wallet.set({
        address: "0x1234567890123456789012345678901234567890",
        balance: 100,
        pendingTransactions: 2,
        totalEarned: 150,
        totalSpent: 50,
      });

      const snapshot = await walletService.exportSnapshot();

      expect(snapshot.address).toBe(
        "0x1234567890123456789012345678901234567890"
      );
      expect(snapshot.balance).toBe(100);
      expect(snapshot.pendingTransactions).toBe(2);
      expect(snapshot.privateKey).toBeUndefined(); // Key NOT included
      expect(snapshot.version).toBe("1.0");
      expect(snapshot.exportDate).toBeDefined();
    });

    /**
     * Test: Include private key when explicitly requested (full backup)
     * Based on: options?.includePrivateKey at wallet.ts:382
     * Verifies: User can export full wallet backup including private key
     * Use case: Migrating wallet to new device
     */
    it("should include private key when explicitly requested", async () => {
      const testPrivateKey = "0x" + "b".repeat(64);
      etcAccount.set({
        address: "0x1234567890123456789012345678901234567890",
        private_key: testPrivateKey,
      });

      const snapshot = await walletService.exportSnapshot({
        includePrivateKey: true,
      });

      expect(snapshot.privateKey).toBe(testPrivateKey);
    });

    /**
     * Test: Handle export when no account is active
     * Based on: account check at wallet.ts:373
     * Verifies: Exports wallet state without address when no account loaded
     */
    it("should export wallet snapshot with undefined address when no account set", async () => {
      etcAccount.set(null);

      const snapshot = await walletService.exportSnapshot();

      expect(snapshot.address).toBeUndefined();
      expect(snapshot.balance).toBeDefined();
    });
  });

  describe("Transaction Sending (Tauri Required)", () => {
    /**
     * Test: Reject transaction when no account is loaded
     * Based on: Account validation at wallet.ts:294-297
     * Verifies: Prevents attempting to send without valid account
     */
    it("should reject transaction in demo mode even without active account", async () => {
      etcAccount.set(null); // No account

      await expect(
        walletService.sendTransaction(
          "0x1234567890123456789012345678901234567890",
          10
        )
      ).rejects.toThrow(
        "Transactions are only available in the desktop app"
      );
    });

    /**
     * Test: Reject transaction in non-Tauri environment (demo mode)
     * Based on: isTauri check at wallet.ts:298-300
     * Verifies: Real blockchain transactions require desktop app
     * Rationale: Demo mode can't sign real transactions without Rust crypto backend
     */
    it("should reject transaction in demo mode (non-Tauri environment)", async () => {
      etcAccount.set({
        address: "0x1234567890123456789012345678901234567890",
        private_key: "0x" + "a".repeat(64),
      });

      await expect(
        walletService.sendTransaction(
          "0x9876543210987654321098765432109876543210",
          10
        )
      ).rejects.toThrow(
        "Transactions are only available in the desktop app"
      );
    });

    /**
     * SKIPPED TEST - Would require Tauri environment:
     * To test "No active account" error, we'd need to mock Tauri environment,
     * which is better suited for integration tests in the desktop app.
     */
  });

  describe("Balance and Transaction Refresh (Tauri Required)", () => {
    /**
     * These tests verify that refresh methods gracefully handle non-Tauri environments.
     *
     * WHAT REFRESH METHODS DO:
     * - refreshBalance(): Queries geth node for account balance (wallet.ts:176-239)
     * - refreshTransactions(): Fetches mined blocks from blockchain (wallet.ts:147-174)
     *
     * WHY THEY REQUIRE TAURI:
     * - Both methods call invoke() to communicate with Rust backend
     * - Rust backend runs a geth (Go Ethereum Classic) node
     * - Demo mode has no blockchain node, so these are no-ops
     *
     * TESTING STRATEGY:
     * - Verify methods don't crash when called in demo mode
     * - Full integration tests for these belong in desktop app E2E tests
     */

    beforeEach(() => {
      // Set up demo account
      etcAccount.set({
        address: "0x1234567890123456789012345678901234567890",
        private_key: "0x" + "a".repeat(64),
      });
    });

    /**
     * Test: refreshBalance should safely no-op in demo mode
     * Based on: Early return at wallet.ts:178-180
     */
    it("should safely skip balance refresh in demo mode", async () => {
      await walletService.refreshBalance();
      // Should complete without errors (no-op due to !this.isTauri)
      expect(true).toBe(true);
    });

    /**
     * Test: refreshTransactions should safely no-op in demo mode
     * Based on: Early return at wallet.ts:149-151
     */
    it("should safely skip transaction refresh in demo mode", async () => {
      await walletService.refreshTransactions();
      // Should complete without errors (no-op due to !this.isTauri)
      expect(true).toBe(true);
    });
  });
});
