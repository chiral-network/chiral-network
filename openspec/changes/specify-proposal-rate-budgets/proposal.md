## Why

The author's **Approved non-reserving quotes** decision in `docs/chiral-book.md` forbids public proposals from consuming named-wallet pending or rate allowances. This change now specifies only provider-ledger-wide committed issuance, implementing specification-alignment task 2.1 of `specify-non-reserving-quotes`. The former twelve-per-wallet proposal is superseded, not an acceptable mitigation with better disclosure.

The former `BillingStore::store_quote` used 12 proposals per named payer and 600 per ledger with `now / 60` buckets. Its generic limiter reset a counter on any differing bucket and cleanup deleted older evidence. This historical defect permitted older samples to reopen allowance. The implemented provider-only limiter now retains a non-regressing committed minute and global total through restart, reordered writers and unrelated cleanup; no live attack or runtime system-clock manipulation is claimed.

Retain the 600-per-ledger numerical budget and fixed-minute basis, require a durable non-regressing effective minute, and remove named-wallet charging. The independent pre-signing work budget belongs to `specify-non-reserving-quotes`; successful-quote accounting cannot bound signatures attempted before insertion. Native cutover and deterministic acceptance replace the old policy; remaining integration gates are tracked in tasks.

## What Changes

- Define 600 committed proposals per provider ledger per effective minute, shared across all named wallets, models, offers and free/paid terms, with no wallet-specific sub-budget.
- Define fixed Unix-minute selection with a durable ledger-local high-water mark: older samples consume remaining allowance in the latest committed minute, never reopen an earlier budget.
- Require atomic quote/global-counter/window commitment, rollback on failure, persistence through reopening and no committed-rate refund when quote storage is reclaimed. A previously committed pre-signing work permit is separate and is not rolled back with a failed insertion.
- Protect active global proposal-rate evidence from unrelated cleanup. Preserve the retained global counter when introducing its watermark; fail closed on missing or contradictory retained totals rather than summing obsolete wallet counters or inventing allowance.
- Retarget deterministic acceptance tasks and retain old-wallet observations as historical evidence. Source incorporation is authorized and committed; regressions accompany the native cutover rather than preserving obsolete wallet behavior.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `inference-proposal-admission`: Add provider-wide committed-issuance limits, non-regressing minute selection and transactional charging alongside the non-reserving quote and accepted-agreement retention contract.

## Impact

- Read-only authority: `docs/chiral-book.md`, public proposal/admission boundaries, durable agreements and protected network-scoped state.
- Source: `contracts.rs::control`, `billing_store.rs::{store_quote,rate_limit,cleanup_pending,transaction}` and persisted `rate_limits`/ledger metadata.
- Provider-only capacity, rollback, concurrency, backward-time and adoption regressions exercise the implemented committed counter; this step completes its independent signing-work interactions.
- Quote validity, signed wallet identity, challenge policy, owner authentication, financial evidence and paid-retry rules remain intact. The author report stays open until remaining parent acceptance is complete; no whitepaper edit or deployment is authorized.
