## Context

The whitepaper's authorization matrix deliberately permits public `POST /v1/contracts/propose`; ownership is checked at funding intent, opening and credential operations. Accepted terms and sender/nonce commitments survive quote/admission expiry. It leaves pending-proposal capacities to implementation policy.

Source inspection finds a single provider-ledger transaction that cleans expired pending state, checks quote rate buckets, checks total/payer pending counts and inserts the signed quote. The current ceilings are eight per payer and 10,000 per ledger. First funding-intent acknowledgement durably stores its full signed agreement, acknowledgement, transaction evidence and sender/nonce commitment, then deletes the matching standalone quote in the same transaction. Account creation retains the signed agreement and removes any remaining pending quote atomically.

Acceptance evidence now covers capacity, independent ledgers, concurrent insertion, rejection rollback, exact expiry and first opening. The separately archived acknowledgement-transfer change covers removal of the standalone quote and immutable funding recovery; the correspondence below preserves that distinction.

## Goals and non-goals

Goals: make existing capacity policy normative; define exact reclamation and rollback behavior; preserve recoverable commercial/financial evidence; enable isolated regression tests without importing private production modules.

Non-goals: change authentication, rates, clocks, quote TTL, funding-intent leases, account limits, inference concurrency, metering, provider readiness or public-rollout approval. These caps are not performance classes or reservations of engine capacity.

## Decisions

### One budget across a provider ledger

Keep the existing ceilings: eight pending quotes for one canonical named payer and 10,000 total. Free and paid quotes, different models and refreshed offers consume the same relevant budget. The scope is the provider ledger, with existing network/provider/DHT persistence boundaries; it is not a cross-provider or cross-host global limiter. Sharing a transaction-nonce journal does not merge independent proposal ledgers.

A pending proposal is a still-retained standalone proposal record. Successful first acknowledgement transfers retention responsibility to the funding intent and frees the matching pending slot; first account opening frees any slot that remains. Historical copies retained solely in accounts or funding intents are not pending proposals. The separate funding-intent admission lease does not keep the transferred proposal slot occupied. The earlier version of this design incorrectly described acknowledgement and pending retention as coexisting until expiry/opening; `specify-acknowledged-proposal-slot-transfer` corrects that source-inspection error without changing production behavior.

### Rejection must not consume another resource budget

Capacity evaluation and insertion must be atomic across separate connections/processes using the same ledger. A capacity-rejected proposal must not leave an inserted quote or consume quote-rate allowance. Existing live proposals must not be evicted to admit a new caller. Current SQLite immediate transactions already implement this shape; tests must exercise observable admission/preservation, not SQL statement text.

Signing can occur before capacity rejection in the current router. These limits therefore do not promise to reject before cryptographic work or bound all rejected-request CPU cost. Public body-reader limits remain a separate control.

### Expiry reclaims proposal capacity, not agreements or money

At admission time, proposals with expiry at or before the supplied ledger time are eligible for removal before evaluating capacity. No autonomous cleanup schedule is required. Durable acknowledgement transfers retention to the funding intent; successful first opening retains the agreement in the account and frees any pending slot still present. Neither transition erases the agreement or treats capacity reclamation as payment acceptance. Acknowledged funding evidence, accepted balances, fees and original sender/nonce ownership remain governed by their separate durable lifecycle, not by the proposal budget.

### Test the boundary without a live deployment

Use isolated ledgers and the public `BillingStore` API with explicit time. Cover per-payer exhaustion, independence across payers/models, concurrent writers competing for the final slot, rejection rollback, expiry and first opening. Coordinate acknowledgement-transfer coverage with `specify-acknowledged-proposal-slot-transfer`. Use real signed fixture quotes, not fabricated payment evidence. Free opening can exercise account slot transfer without simulating a chain.

The global storage ceiling is defense in depth: the normal 300-second native quote lifetime and existing proposal-rate limits can prevent ordinary monotonic-time traffic from reaching 10,000 simultaneous pending quotes. A global-cap test must explicitly target the storage boundary with valid signed, longer-lived fixture records and controlled time, not change production TTL/rates or claim native issuance of those fixtures. Keep it isolated and bounded; no shared/live database seeding.

## Reviewed implementation correspondence

- `contracts::control` is the native production caller of `BillingStore::store_quote`. It canonicalizes the named consumer before signing the quote, without requiring owner proof for proposal. The ledger counts that already-canonical owner string; it is not a separate public wallet-normalization or authorization boundary. The other source-tree callers are test fixtures.
- `store_quote` binds the ledger's network/provider identity and performs pending cleanup, tentative rate increments, total/payer counts and insertion inside `BillingStore::transaction`. The counts span the entire standalone `quotes` table, not a model, offer or funding-mode partition. `MAX_PENDING` is 10,000 and the payer ceiling is eight; independent ledgers have no shared pending counter.
- The shared transaction helper takes an immediate SQLite transaction and commits only after its operation succeeds. Capacity rejection propagates before commit, rolling back rate increments and pending mutations. The public error mapper translates `rate_limit` into HTTP 429. This is not a promise to reject before signing or to bound rejected-request CPU cost.
- `cleanup_pending` removes standalone proposals at `expires <= now`, clears expired funding-intent admission deadlines and leaves account agreements, credentials, funding intents and financial tables intact. The specification also permits exclusion from capacity instead of deletion; acceptance tests do not require expired standalone records to become unqueryable.
- First `open_account` retains the original signed agreement with the account and removes any remaining matching proposal within the same transaction. Paid opening still requires verified funding and its prior acknowledgement; rejected unpaid opening cannot release a slot. Account agreement recovery and credential authentication do not depend on a standalone proposal remaining present.
- Acknowledgement registration/recovery remains governed by the distinct requirement already integrated in `inference-proposal-admission`. Its full signed terms and transaction/nonce evidence are retained separately from pending proposals. The archived acknowledgement design maps the complementary funding-recovery evidence and its limitations.

No implementation contradiction was found for these pending-quota requirements under the native issuance/authorization boundaries. Production source remains part of the owner's uncommitted migration and is neither changed nor incorporated into this worker's commit.

## Reviewed acceptance evidence

Tests in `src-tauri/tests/inference_proposal_quota_test.rs`:

- `pending_capacity_is_shared_across_models_offers_and_free_paid_terms_after_restart` and `one_provider_ledger_cannot_consume_another_providers_pending_allowance`: eight-slot attribution across commercial terms, another payer's capacity, independent ledgers and exact signed-term recovery.
- `ledger_wide_pending_ceiling_rejects_new_payers_without_evicting_signed_terms`: a full 10,000-record ledger rejects a new payer/model in fresh rate windows, retains every signed proposal and permits the same rejected proposal in an independent ledger.
- `competing_connections_admit_only_one_quote_into_the_final_payer_slot`: two independent connections compete for the eighth slot; one succeeds, the rejected quote is absent and the durable result remains full after reopening.
- `rejected_pending_proposals_preserve_the_payers_rate_allowance` and `rejected_pending_proposals_preserve_other_payers_shared_rate_allowance`: rejection-heavy and untouched ledgers retain matching later admission, without freezing numerical rate budgets.
- `acknowledgement_frees_exactly_one_slot_and_replay_preserves_its_replacement` and `rejected_acknowledgements_preserve_pending_capacity_and_allow_original_registration`: acknowledgement handoff, replay and rejection; these also support the already archived acknowledgement requirement.
- `unpaid_first_opening_preserves_pending_capacity_and_signed_terms_after_reopening`: missing-payment rejection retains all pending slots and signed terms without creating accounts or fees.
- `free_first_opening_frees_one_slot_and_survives_exact_expiry_with_original_credential`: exactly one replacement after free opening, rejection at T−1, all eight slots reusable at T, continued ninth-slot rejection, original signed account terms and financial state, and the original credential/scopes surviving cleanup and storage reopening.

`src-tauri/tests/inference_proposal_test.rs::public_pending_quota_follows_the_canonical_named_payer_not_caller_authority` exercises the actual HTTP proposal handler: unauthenticated case aliases share one named payer's quota, excess proposals return 429/`rate_limit`, another payer remains eligible, signed agreements survive and no account or transaction submission is created.

Storage tests use isolated ledgers, real signed quotes and controlled time. Extended/one-second lifetimes isolate capacity and rollback; they are not native issuance evidence. Acknowledgement fixtures validate offline transaction signatures but assume prior admission; free opening avoids invented payment evidence. Threaded independent connections and reopened SQLite handles are not an operating-system crash campaign. The HTTP provider is pre-admitted, not a real engine or public deployment. These tests do not certify paid speed, public load, settlement, stronger authentication or resistance to named-payer quota exhaustion.

## Risks and tradeoffs

- Public callers choose the payer field without proving ownership. They can consume another payer's pending budget and repeatedly obstruct new proposals. Quotas are not Sybil resistance or payer authorization. This source-derived availability exposure is recorded in `docs/apas-worker/report.md`; preserving the current whitepaper does not resolve it or authorize rollout.
- Increasing these limits later is a policy change, not a test-fixture update. This change deliberately avoids freezing unrelated account, challenge or funding-intent limits that happen to share implementation constants.
- Proposal-rate window details, including clock behavior, are not specified here. Their eventual tests need a separate explicit contract; do not silently treat a fixed wall-clock limiter as rolling or monotonic.

## Migration and verification

No schema, data migration or production mutation is required for this specification step. Leave the change open until its regression tasks and checks pass. Preserve the owner's extensive uncommitted implementation, including the entirely untracked ledger source. Any discovered native fix that cannot be independently committed must be marked blocked rather than absorbed.
