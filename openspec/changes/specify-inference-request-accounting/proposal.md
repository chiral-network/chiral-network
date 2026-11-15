## Why

The read-only whitepaper requires exact prepaid inference reservations, one ceil-rounded authoritative usage charge, zero-charge unknown-usage recovery and at-most-once settlement. The current OpenSpec wire foundation defines monetary representation, and pending commercial/opening contracts exclude accepted recovery from new quote-capacity gates, but neither specifies these request-accounting transitions. Adding capacity-pressure tests without that foundation would leave their financial expectations implicit.

The implementation already contains these workflows in the ledger, gateway and private engine adapter. This change specifies their consumer-visible contract and plans missing acceptance without removing behavior merely because earlier specifications omitted it.

## What Changes

- Specify account-scoped request identity, non-replaying duplicates and exact maximum-cost reservation before paid dispatch.
- Define atomic account/credential holds, inference-specific admission bounds, authoritative usage validation and first-terminal-result-wins settlement.
- Define cancellation/failure/restart unknown-usage release, reservation-sized request exposure and its distinction from actual loss or funding-reversal compensation.
- Preserve admitted performance-evidence identity and inference-waiver accounting without redefining qualification or withdrawal policy.
- Specify durable request/receipt recovery and separation from quote storage, signing-work, issuance and opening-capacity limits.
- Map current implementation and useful tests, then require isolated financial/state-transition acceptance before broader gateway and capacity-pressure claims.

## Capabilities

### New Capabilities

- `inference-request-accounting`: Exact prepaid request reservation, terminal settlement, unknown-usage recovery, performance-remedy accounting and attributable request/receipt history.

### Modified Capabilities

None. Complement `inference-wire-foundation`, the pending commercial/owner-control contracts and `inference-opening-funding-intent` task 2.8.2. Do not change funding admission, accepted quote terms, funding fees, credential authority or the non-reserving quote decision.

## Impact

- Read-only authority: `docs/chiral-book.md`, **Consumer control, credentials, and request lifecycle**, **Durable accounting and fees**, **Performance accounting and withdrawal**, **API compatibility and model qualification**, and **Provider lifecycle and immutable agreements**.
- Implementation correspondence: `billing_store.rs`, `inference_gateway.rs`, `inference_backend.rs`, `app.rs` and their existing tests. These production/native-unit-test files remain owner-untracked; this step does not adopt or modify them.
- Specification-only step: no runtime/test changes, financial schema migration, wallet operation, installation, server change or whitepaper edit.
- Preserve separate safe-funding/reorg, fee-remittance, full performance qualification, engine/API compatibility and public-release work. Synthetic or post-verification fixtures are not live execution evidence.
- No author-only decision is needed. The answered public named-payer quota report remains open for its blocked production cutover, not for another policy answer.
