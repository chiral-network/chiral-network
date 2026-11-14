## Why

The whitepaper's paid-opening order is a financial safety contract, not merely a sequence of HTTP calls: preserve one signed original, obtain and save the provider's acknowledgement, then permit client broadcast. Existing wire, commercial, owner-control and proposal-retention requirements cover its ingredients but leave that pre-credit sequence, fresh-intent spendability and acknowledgement recovery unspecified.

Production has separate consumer/journal and provider/ledger paths, with substantial isolated test evidence. Their guarantees must not be confused with real-chain qualification, an on-chain reservation or escrow. A new lower-time-bound acceptance gap also needs explicit tracking: registration checks quote expiry but not whether its sampled time precedes quote issuance, while the native consumer rejects such an acknowledgement.

## What Changes

- Define durable original-payment preparation and shared signing-wallet journal ownership before disclosure or client broadcast.
- Specify the funding-intent request and signed acknowledgement fields, payment/hash/quote binding, and native registration-time acceptance.
- Preserve fresh admission's signed-envelope, current-nonce, full-gas balance, qualified-execution, expiry and independent 10,000-active-intent gates.
- Require atomic acknowledgement persistence, permanent sender/nonce ownership, unchanged authorized replay and retention independent of admission leases.
- Require acknowledgement verification and durable local storage before a durable broadcast-start marker and submission of the original bytes; preserve exact-operation recovery after ambiguous responses/restart.
- Track executable acceptance, missing fault/time cases and owner-source blockers without claiming full settlement or credential-lifecycle compliance.

## Capabilities

### New Capabilities

- `inference-opening-funding-intent`: The paid-opening pre-credit lifecycle from original preparation through durable provider acknowledgement, client broadcast gating and exact-original recovery.

### Modified Capabilities

None. Reuse the canonical wire foundation and proposal-retention handoff, plus the pending commercial/owner-control contracts. Do not redefine their signing, prices, authorization, proposal quotas or credential rules.

## Impact

- Read-only authority: `docs/chiral-book.md`, especially shared signing-wallet storage, agreements/exact funding order and settlement/recovery risks.
- Production correspondence: `protocol.rs`, `consumer.rs`, `chain_client.rs`, `contracts.rs` and `billing_store.rs`; existing unit and owner-control/proposal acceptance targets.
- Specification-only step: no runtime, test, monetary schema, deployment, wallet/account, network or whitepaper changes. The new pre-issuance registration guard requires a later independently committable native fix and regression, not a failing test committed alone.
- Full safe-credit/reorg accounting, explicit cancellation, top-ups, fee remittance and credential lifecycle remain separate work. The provider can broadcast disclosed signed bytes before acknowledging; this change cannot remove that trust boundary.
- No author-only answer is needed. The existing public named-payer availability report remains open and nonblocking.
