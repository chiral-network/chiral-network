## Context

The whitepaper's authorization matrix deliberately permits public `POST /v1/contracts/propose`; ownership is checked at funding intent, opening and credential operations. Accepted terms and sender/nonce commitments survive quote/admission expiry. It leaves pending-proposal capacities to implementation policy.

Source inspection finds a single provider-ledger transaction that cleans expired pending state, checks quote rate buckets, checks total/payer pending counts and inserts the signed quote. The current ceilings are eight per payer and 10,000 per ledger. Account creation removes its pending quote only after recording the signed agreement and account in the same transaction. Funding-intent acknowledgements retain their own complete signed agreement.

The existing `one_use_challenges_are_expiring_and_pending_state_is_bounded` test covers challenge records, not quotes. The production proposal tests cover issuance, gates, authority separation and body bounds, but not quote-capacity exhaustion or competing insertions.

## Goals and non-goals

Goals: make existing capacity policy normative; define exact reclamation and rollback behavior; preserve recoverable commercial/financial evidence; enable isolated regression tests without importing private production modules.

Non-goals: change authentication, rates, clocks, quote TTL, funding-intent leases, account limits, inference concurrency, metering, provider readiness or public-rollout approval. These caps are not performance classes or reservations of engine capacity.

## Decisions

### One budget across a provider ledger

Keep the existing ceilings: eight pending quotes for one canonical named payer and 10,000 total. Free and paid quotes, different models and refreshed offers consume the same relevant budget. The scope is the provider ledger, with existing network/provider/DHT persistence boundaries; it is not a cross-provider or cross-host global limiter. Sharing a transaction-nonce journal does not merge independent proposal ledgers.

A pending proposal is a still-retained proposal record. An acknowledgement can coexist with that record until quote expiry or successful first account opening. Historical copies retained solely in accounts or funding intents are not pending proposals.

### Rejection must not consume another resource budget

Capacity evaluation and insertion must be atomic across separate connections/processes using the same ledger. A capacity-rejected proposal must not leave an inserted quote or consume quote-rate allowance. Existing live proposals must not be evicted to admit a new caller. Current SQLite immediate transactions already implement this shape; tests must exercise observable admission/preservation, not SQL statement text.

Signing can occur before capacity rejection in the current router. These limits therefore do not promise to reject before cryptographic work or bound all rejected-request CPU cost. Public body-reader limits remain a separate control.

### Expiry reclaims proposal capacity, not agreements or money

At admission time, proposals with expiry at or before the supplied ledger time are eligible for removal before evaluating capacity. No autonomous cleanup schedule is required. Successful first opening transfers retention responsibility to the account and frees that pending slot atomically. Acknowledged funding evidence, accepted balances, fees and original sender/nonce ownership remain governed by their separate durable lifecycle, not by the proposal budget.

### Test the boundary without a live deployment

Use isolated ledgers and the public `BillingStore` API with explicit time. Cover per-payer exhaustion, independence across payers/models, concurrent writers competing for the final slot, rejection rollback, expiry and first opening. Use real signed fixture quotes, not fabricated payment evidence. Free opening can exercise slot transfer without simulating a chain.

The global storage ceiling is defense in depth: the normal 300-second native quote lifetime and existing proposal-rate limits can prevent ordinary monotonic-time traffic from reaching 10,000 simultaneous pending quotes. A global-cap test must explicitly target the storage boundary with valid signed, longer-lived fixture records and controlled time, not change production TTL/rates or claim native issuance of those fixtures. Keep it isolated and bounded; no shared/live database seeding.

## Risks and tradeoffs

- Public callers choose the payer field without proving ownership. They can consume another payer's pending budget and repeatedly obstruct new proposals. Quotas are not Sybil resistance or payer authorization. This source-derived availability exposure is recorded in `docs/apas-worker/report.md`; preserving the current whitepaper does not resolve it or authorize rollout.
- Increasing these limits later is a policy change, not a test-fixture update. This change deliberately avoids freezing unrelated account, challenge or funding-intent limits that happen to share implementation constants.
- Proposal-rate window details, including clock behavior, are not specified here. Their eventual tests need a separate explicit contract; do not silently treat a fixed wall-clock limiter as rolling or monotonic.

## Migration and verification

No schema, data migration or production mutation is required for this specification step. Leave the change open until its regression tasks and checks pass. Preserve the owner's extensive uncommitted implementation, including the entirely untracked ledger source. Any discovered native fix that cannot be independently committed must be marked blocked rather than absorbed.
