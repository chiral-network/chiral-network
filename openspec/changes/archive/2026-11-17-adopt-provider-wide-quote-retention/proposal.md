## Why

The archived `inference-proposal-admission` spec still requires the superseded named-wallet quote gates: at most eight pending proposals per named consumer wallet, per-payer quote-rate allowance in atomic admission, and an unauthenticated proposal consuming the named payer's pending budget (*Named-payer quota attribution is not payer authentication*). The whitepaper's **Approved non-reserving quotes** (`docs/chiral-book.md`, Part II) forbids exactly that: "An unauthenticated proposal must not occupy serving capacity or consume the named wallet's pending-quote or proposal-rate allowance", and the decision "supersedes conflicting pending-proposal and proposal-rate requirements".

The replacement deltas were written in `specify-non-reserving-quotes`, and their implementation is complete and tested (that change's tasks 3.1, 3.2, 3.4, 3.5, 4.1 and 4.2). That change cannot be archived yet, because its paid-intent capacity and accepted-recovery acceptance (tasks 3.3 and 4.3) remain open. Leaving the finished deltas inside it keeps a main spec that contradicts both the whitepaper and the shipped code. This change carries those finished deltas alone so they can be integrated now; the open acceptance stays in `specify-non-reserving-quotes`.

## What Changes

- **BREAKING (specification only; the code already behaves this way):** remove the named-wallet eight-proposal ceiling, the per-payer final-slot and quote-rate atomicity, and named-payer quota attribution from `inference-proposal-admission`.
- Add provider-ledger-wide retention of at most 10,000 unaccepted quotes, atomic storage admission, and the durable pre-signing work bound (600 signing attempts per non-regressing effective minute, never refunded).
- Restate acknowledgement handoff, quote-expiry reclamation and first-opening cleanup in terms of provider-wide unaccepted-quote storage instead of a named payer's slot.
- Moved unchanged from `specify-non-reserving-quotes`; the two requirements whose acceptance is still open (*Public quotes do not reserve opening capacity* and *Authenticated paid admission preserves accepted commitments*) stay there.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `inference-proposal-admission`: wallet-independent, provider-wide bounded quote retention and pre-signing work, replacing the named-wallet quota requirements.

## Impact

Specification only. The code already implements these requirements: `BillingStore::store_quote` and `BillingStore::sign_quote` in `src-tauri/src/billing_store.rs`, the public proposal handler in `src-tauri/src/contracts.rs`, with ledger tests in `src-tauri/src/billing_store_tests.rs` and route tests in `src-tauri/tests/inference_proposal_test.rs`. `specify-proposal-rate-budgets` (committed-issuance budget) and `specify-non-reserving-quotes` (paid admission and accepted recovery) remain open. No whitepaper edit, deployment or financial-state migration.
