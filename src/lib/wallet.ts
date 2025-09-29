import { invoke } from '@tauri-apps/api/core';
import { etcAccount, miningState, transactions, wallet, type Transaction } from '$lib/stores';
import { get } from 'svelte/store';

// Keep track of transaction hashes we've already processed to avoid duplicates
const seenHashes = new Set<string>();

// This is the helper function that creates the transaction object
function pushRecentBlock(b: { hash: string; reward?: number; timestamp?: Date }) {
  const reward = typeof b.reward === 'number' ? b.reward : 0;

  const newBlock = {
    id: `block-${b.hash}-${b.timestamp?.getTime() ?? Date.now()}`,
    hash: b.hash,
    reward: reward,
    timestamp: b.timestamp ?? new Date(),
    difficulty: 0,
    nonce: 0, 
  };
  miningState.update(state => ({
    ...state,
    recentBlocks: [newBlock, ...(state.recentBlocks ?? [])].slice(0, 50)
  }));

  if (reward > 0) {
    const last4 = b.hash.slice(-4);
    const tx: Transaction = {
      id: Date.now(),
      type: 'received',
      amount: reward,
      from: 'Mining reward',
      date: b.timestamp ?? new Date(),
      description: `Block Reward (…${last4})`,
      status: 'pending'
    };
    transactions.update(list => [tx, ...list]);
  }
}

// This is the main function that fetches new block data from the backend
async function refreshTransactions() {
  const account = get(etcAccount);
  
  if (!account) return;

  try {
    const blocks = await invoke('get_recent_mined_blocks_pub', {
      address: account.address,
      lookback: 2000,
      limit: 50
    }) as Array<{ hash: string; timestamp: number; reward?: number }>;

    for (const b of blocks) {
      if (seenHashes.has(b.hash)) continue;
      seenHashes.add(b.hash);
      pushRecentBlock({
        hash: b.hash,
        timestamp: new Date((b.timestamp || 0) * 1000),
        reward: 2
      });
    }
  } catch (e) {
    console.error('Failed to refresh transactions:', e);
  }
}

async function refreshBalance() {
    const account = get(etcAccount);
    if (!account) return;

    try {
        const balanceStr = await invoke('get_account_balance', { address: account.address }) as string;
        const realBalance = parseFloat(balanceStr);

        wallet.update(w => ({ ...w, balance: realBalance }));

        if (!isNaN(realBalance) && realBalance > (get(miningState).totalRewards ?? 0)) {
            miningState.update(state => ({ ...state, totalRewards: realBalance }));
        }
    } catch (e) {
        console.error('Failed to refresh balance:', e);
    }
}

// Export the functions as a service object
export const walletService = {
  refreshTransactions,
  refreshBalance
};