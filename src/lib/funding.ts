// Classification of the native pending-funding journal (`pending_accounts`) for the GUI.
// The native consumer and wallet journal still refuse a conflicting payment themselves;
// this only decides when the GUI must stop offering a new one (A.1.7).

export interface PendingFunding {
  contract_id: string;
  provider_wallet?: string;
  model?: string;
  funding_wei?: string;
  opening_pending?: boolean;
  topup_wei: string | null;
  tx_hash?: string | null;
  funding_stage: string;
}

// Stages after which the listed operation can no longer move money or need resuming:
// a cancellation that resolved without the original winning, a cancellation before any
// payment was prepared, and a free opening, which involves no payment. A saved agreement
// stays listed until its account opens, so these must not block a new payment forever.
const SETTLED_OR_UNPAID = new Set(['not_prepared', 'cancelled', 'original_failed', 'cancellation_failed', 'free_open_pending', 'active']);

/** Journaled paid operations that are still unresolved: resume or reconcile them first. */
export function unresolvedPayments(pending: readonly PendingFunding[]): PendingFunding[] {
  return pending.filter(item => !SETTLED_OR_UNPAID.has(item.funding_stage));
}

export const stageLabel = (stage: string) => stage.replaceAll('_', ' ');
