## Why

The whitepaper makes public proposal subject to provider admission rules without fixing its rate algorithm. The commercial-terms delta requires per-wallet/global proposal rates, while the integrated pending-quota requirements deliberately leave rate budgets and clock semantics separate. Existing rollback tests establish relative allowance conservation, not the complete numerical rate policy.

`BillingStore::store_quote` currently uses 12 proposals per named payer and 600 per ledger with `now / 60` buckets. `rate_limit` resets a counter on any differing bucket, and shared cleanup deletes older rate rows. [INFERENCE] An older timestamp can therefore replenish allowance instead of respecting an already observed later minute. Native control samples time before the ledger write transaction, so timestamp order is not inherently commit order even without a wall-clock adjustment. No live exploit or runtime clock-regression result is claimed.

Specify the existing numerical budgets and fixed-minute basis, but require a durable non-regressing effective minute rather than making the reset weakness normative. This is a new safety requirement and an explicit implementation gap, not a claim that the current code already satisfies it.

## What Changes

- Define 12 committed proposals per canonical named payer and 600 per provider ledger per effective minute, shared across models, offers and free/paid terms.
- Define fixed Unix-minute selection with a durable ledger-local high-water mark: older samples consume remaining allowance in the latest committed minute, never reopen an earlier budget.
- Require atomic quote/counter/window commitment, rollback on failure, persistence through reopening and no rate refund when pending capacity is reclaimed.
- Protect active proposal-rate evidence from unrelated cleanup; preserve retained counters when introducing the clock watermark without converting financial state or reconstructing discarded history.
- Add deterministic acceptance and implementation tasks. The native clock/pruning fix is blocked while the entirely untracked ledger source remains owner work; do not commit failing regressions or absorb that source.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `inference-proposal-admission`: Add scoped proposal-rate budgets, non-regressing minute selection and transactional charging alongside pending-capacity and acknowledgement-retention requirements.

## Impact

- Read-only authority: `docs/chiral-book.md`, public proposal/admission boundaries, durable agreements and protected network-scoped state.
- Source: `contracts.rs::control`, `billing_store.rs::{store_quote,rate_limit,cleanup_pending,transaction}` and persisted `rate_limits`/ledger metadata.
- Existing evidence: pending-capacity rollback controls and the actual HTTP named-payer quota case. These do not establish 12/600 rate edges or backward-time safety.
- No changes this turn to production, tests, monetary arithmetic, quote validity, challenge-rate policy, owner authentication, paid retries or deployment. No author-only decision is needed for these delegated admission details; the existing public named-payer availability report remains open.
