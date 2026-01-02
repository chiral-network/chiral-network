import { invoke } from "@tauri-apps/api/core";
import { get } from "svelte/store";
import {
  accurateTotals,
  etcAccount,
  miningPagination,
  miningState,
  showAuthWizard,
  transactions,
  transactionPagination,
  wallet,
} from "$lib/stores";
import { showToast } from "$lib/toast";
import { t } from "svelte-i18n";

const translate = (key: string, params?: Record<string, unknown>) => {
  const translateFn = get(t) as unknown as (
    k: string,
    p?: Record<string, unknown>
  ) => string;
  return translateFn ? translateFn(key, params) : key;
};

/**
 * Locks the active account (same as Account page) and shows the auth wizard.
 * Clears wallet/mining state, stops mining, logs out of backend, and removes cached session data.
 */
export async function lockAccount(): Promise<void> {
  const isTauri =
    typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

  try {
    // Stop mining if active
    const currentMiningState = get(miningState);
    if (currentMiningState?.isMining) {
      await invoke("stop_miner");
    }

    // Clear active account in backend
    if (isTauri) {
      await invoke("logout");
    }

    // Reset account + wallet state
    etcAccount.set(null);
    wallet.update((w: any) => ({
      ...w,
      address: "",
      balance: 0,
      totalEarned: 0,
      totalSpent: 0,
      totalReceived: 0,
      pendingTransactions: 0,
    }));

    // Clear mining state completely
    miningState.update((state: any) => ({
      ...state,
      isMining: false,
      hashRate: "0 H/s",
      totalRewards: 0,
      blocksFound: 0,
      activeThreads: 0,
      recentBlocks: [],
      sessionStartTime: undefined,
    }));

    accurateTotals.set(null);
    transactions.set([]);

    // Reset pagination states
    transactionPagination.set({
      accountAddress: null,
      oldestBlockScanned: null,
      isLoading: false,
      hasMore: true,
      batchSize: 5000,
    });
    miningPagination.set({
      accountAddress: null,
      oldestBlockScanned: null,
      isLoading: false,
      hasMore: true,
      batchSize: 5000,
    });

    // Clear stored session data
    if (typeof localStorage !== "undefined") {
      const walletKeys = [
        "lastAccount",
        "miningSession",
        "chiral_wallet",
        "chiral_transactions",
        "transactionPagination",
        "miningPagination",
        "chiral_keystore_passwords",
      ];
      walletKeys.forEach((key) => localStorage.removeItem(key));
    }
    if (typeof sessionStorage !== "undefined") {
      sessionStorage.clear();
    }

    showToast(translate("toasts.account.logout.locked"), "success");
    showAuthWizard.set(true);
  } catch (error) {
    console.error("Error during account lock:", error);
    showToast(
      translate("toasts.account.logout.error", {
        values: { error: String(error) },
      }),
      "error"
    );
    throw error;
  }
}
