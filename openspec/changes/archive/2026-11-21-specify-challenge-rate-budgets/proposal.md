## Why

The whitepaper requires bounded public challenge issuance and one-use owner authorization, but does not fix issuance-rate numbers or a clock algorithm. The owner-control change specifies pending capacity, signing, consumption and authorization while deliberately leaving challenge rate-window policy separate. Production already charges 12 challenge insertions per named wallet and 600 per provider ledger using minute counters; that behavior needs its own requirements rather than being inferred from proposal-rate tests.

The shared limiter resets on any differing minute, and generic pending cleanup deletes older rate rows. [INFERENCE] An older or reordered provider time sample can therefore replenish challenge allowance, including after another resource's later-time cleanup. This is source evidence, not a live exploit or completed clock-regression test.

Preserve the existing numerical budgets and public-issuance boundary, while requiring the same non-regressing minute-selection principle as the proposal-rate change with independent challenge state. This safety requirement is an explicit implementation gap, not current acceptance evidence.

## What Changes

- Specify 12 durably inserted challenges per canonical named wallet and 600 per provider ledger per effective challenge minute, shared across operation fields and independent of proposal/inference budgets.
- Select a durable effective minute from the maximum of the provider's sampled Unix minute and its last committed challenge minute; an older sample cannot refill allowance.
- Commit minute selection, both charges, pending maintenance and challenge insertion atomically. Failed issuance changes none of them; response loss, consumption or expiry does not refund committed rate units.
- Protect challenge-rate evidence from unrelated maintenance and preserve consistent retained counters during watermark initialization. Do not alter challenge lifetime, proof validity, financial state or proposal-rate policy.
- Record deterministic acceptance tasks and the owner-untracked source blocker. Preserve valid existing proofs and bearer-read/inference authorization without pretending fresh owner proofs are always available under exhaustion.

## Capabilities

### New Capabilities

- `inference-challenge-rate-budgets`: Provider-ledger-local challenge issuance rates, durable minute selection, cross-resource isolation and transactional charging. Complements the pending owner-control capability without duplicating its authentication or pending-capacity requirements.

### Modified Capabilities

None.

## Impact

- Read-only authority: `docs/chiral-book.md`, especially public challenge issuance, one-use owner proofs, protected network/provider state and accepted-agreement recovery.
- Implementation: `contracts.rs::control` and `billing_store.rs::{create_challenge,consume_challenge,rate_limit,cleanup_pending,transaction}` with persisted rate counters/metadata.
- Existing evidence: owner-control capacity/collision/rollback tests and proposal-versus-challenge isolation tests. These do not establish complete challenge 12/600 edges or non-regressing clock behavior.
- No runtime, tests, monetary schema, issuer TTL, signing format, authentication redesign, automatic retry, deployment or whitepaper changes in this step. Numerical and clock choices are delegated admission details; no author-only answer is needed. The existing public named-wallet availability report stays open and nonblocking.
