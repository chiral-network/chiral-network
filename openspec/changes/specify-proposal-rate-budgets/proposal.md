## Why

The author's **Approved non-reserving quotes** decision in `docs/chiral-book.md` forbids public proposals from consuming named-wallet pending or rate allowances. This change now specifies only provider-ledger-wide committed issuance, implementing specification-alignment task 2.1 of `specify-non-reserving-quotes`. The former twelve-per-wallet proposal is superseded, not an acceptable mitigation with better disclosure.

`BillingStore::store_quote` currently uses 12 proposals per named payer and 600 per ledger with `now / 60` buckets. `rate_limit` resets a counter on any differing bucket, and shared cleanup deletes older rate rows. [INFERENCE] An older timestamp can therefore replenish allowance instead of respecting an already observed later minute. Native control samples time before the ledger write transaction, so timestamp order is not inherently commit order even without a wall-clock adjustment. No live exploit or runtime clock-regression result is claimed.

Retain the 600-per-ledger numerical budget and fixed-minute basis, require a durable non-regressing effective minute, and remove named-wallet charging. The independent pre-signing work budget belongs to `specify-non-reserving-quotes`; successful-quote accounting cannot bound signatures attempted before insertion. These are pending implementation requirements, not a claim that the current code complies.

## What Changes

- Define 600 committed proposals per provider ledger per effective minute, shared across all named wallets, models, offers and free/paid terms, with no wallet-specific sub-budget.
- Define fixed Unix-minute selection with a durable ledger-local high-water mark: older samples consume remaining allowance in the latest committed minute, never reopen an earlier budget.
- Require atomic quote/global-counter/window commitment, rollback on failure, persistence through reopening and no committed-rate refund when quote storage is reclaimed. A previously committed pre-signing work permit is separate and is not rolled back with a failed insertion.
- Protect active global proposal-rate evidence from unrelated cleanup. Preserve the retained global counter when introducing its watermark; fail closed on missing or contradictory retained totals rather than summing obsolete wallet counters or inventing allowance.
- Retarget deterministic acceptance tasks and retain old-wallet test observations as historical evidence. Native quota/clock/pruning changes remain blocked while the entirely untracked ledger source is owner work; pair new regressions with the native cutover.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `inference-proposal-admission`: Add provider-wide committed-issuance limits, non-regressing minute selection and transactional charging alongside the non-reserving quote and accepted-agreement retention contract.

## Impact

- Read-only authority: `docs/chiral-book.md`, public proposal/admission boundaries, durable agreements and protected network-scoped state.
- Source: `contracts.rs::control`, `billing_store.rs::{store_quote,rate_limit,cleanup_pending,transaction}` and persisted `rate_limits`/ledger metadata.
- Existing global-capacity, rollback and concurrency tests supply partial evidence. The named-payer cases establish the old exposure, not the absence of a wallet budget; backward-time and adoption safety remain unimplemented.
- No production or test changes in this specification slice. Quote validity, signed wallet identity, challenge policy, owner authentication, financial evidence and paid-retry rules remain intact. The author report stays open until downstream alignment and actual mitigation evidence are complete; no whitepaper edit or deployment is authorized.
