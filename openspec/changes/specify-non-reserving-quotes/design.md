## Authority and scope

The author answer and the book's **Approved non-reserving quotes** supersede named-wallet quote quotas. The worker did not edit the book. Native committed admission removes both wallet gates together; this step adds separate pre-signing-work admission. Historical eight-slot and twelve-per-minute observations describe the superseded behavior, not current policy. Remaining paid/expiry acceptance stays separate from this resource-control step.

## Observed implementation

`BillingStore::store_quote` enforces only provider-wide 10,000 retained quotes and 600 committed insertions per effective minute. Its immediate transaction adopts retained global evidence, advances a durable non-regressing minute, retires obsolete wallet rows, cleans pending state and inserts exact signed terms atomically. The public handler now first calls `BillingStore::sign_quote`, whose separate immediate transaction commits a work permit before invoking the real protocol signer. A later insertion failure cannot refund authorized signing or advance committed-issuance time.

The control handler already takes one of 64 process-wide non-queuing permits before buffering at most 96 KiB with a five-second body deadline. These bounds are useful but do not limit repeated signing attempts over time.

`register_funding_intent` retrieves an exact cached original before fresh gates. Fresh registration validates the issued quote, permanent transaction/sender-nonce ownership and execution terms, then counts active un-opened intent leases inside the write transaction. It inserts the complete signed acknowledgement/quote/raw transaction before deleting only the matching standalone proposal. The active-intent limit is 10,000; expiry releases admission capacity without deleting accepted evidence.

`open_account` applies its eight-owned-account / 10,000-total-account limits only to a first free opening. First paid opening requires its retained intent and verified payment; an existing account follows its recovery path without issuing or revoking credentials (`OpenResult.credential` is null). Native recovery after a lost opening reply saves the account and leaves missing-secret recovery to explicit owner credential issuance. Keep these distinctions rather than repeating fresh capacity checks or silently rotating keys after acceptance; owner-control task 2.4.3 carries the regression evidence.

## Decisions

### Three distinct kinds of state

1. **Unaccepted quote retention:** at most 10,000 standalone quotes per provider ledger, without a wallet sub-limit. Expiry reclaims this storage. Retention authenticates the exact issued terms but is not an opening reservation.
2. **Accepted paid admission:** at most 10,000 un-opened, unexpired original intent leases per provider ledger. Admission is payer-authenticated and transactional. The lease ends at the original quote deadline; expiry never revokes the acknowledged agreement or nonce commitment.
3. **Free opening:** the existing finite account limits are checked atomically only at the authenticated first free opening. No payment or funding-intent is fabricated. Existing-account recovery does not repeat fresh-opening capacity admission.

These limits are not model execution slots, prepaid balance guarantees, speed classes or paid-inference reservation limits. A provider may issue otherwise valid quotes while its paid admission leases are full; receipt of a quote does not authorize broadcast.

### Bound work separately from committed issuance

Keep the shared 64-operation, 96-KiB and five-second control boundaries. A provider-ledger-wide budget of 600 quote-signing attempts per fixed effective minute is claimed durably after input validation and before signing. A claimed attempt is not refunded when signing, insertion, persistence or response delivery fails: it limits work already authorized, not successful quotes. Reaching this budget returns 429/`rate_limit` before signing.

Use the existing planned non-regressing-minute approach for this separate work budget: `E = max(floor(now / 60), last_committed_work_minute)`, atomically claim its unit, retain the watermark/count across restart and unrelated cleanup, and refill only for a strictly newer minute. There is one ledger-wide work bucket, not attacker-expandable wallet/source rows. Before enabling the new limiter on a ledger with no work evidence, durably mark the current initialization minute exhausted; wait for its next minute rather than inventing unused signing allowance for the partial cutover minute. Invalid or unreadable work evidence fails closed for fresh quoting only. This bookkeeping does not convert old financial schemas or reconstruct unobserved past attempts.

Persist the work watermark as canonical decimal `metadata.quote_work_minute` and its matching count/window in the sole `rate_limits` bucket `quote-work:global`. They are a coherent pair: missing one, mismatched windows, malformed minutes or counts outside 1–600 refuse fresh signing. The absent-pair initialization returns a non-permitted result from the transaction, commits its count of 600, then returns the outward rate refusal; returning an error inside that transaction would wrongly roll initialization back. Pending cleanup preserves this bucket, and committed-rate legacy retirement only addresses its separate `quote:*` namespace. Neither budget changes signed wall-clock timestamps or accepted terms.

The existing proposal-rate change will retain its separate 600 **committed insertion** limit and its atomic, nonrefundable, non-regressing semantics, but remove the twelve-unit named-wallet bucket. Distinguish the two counters in tests and errors: a failed insertion rolls back committed-issuance charges and quote mutations, not its previously committed work permit. This separation bounds rejected signing without weakening insertion conservation or moving its clock on failure.

Additional source controls are optional; a source key must come from the transport or explicitly trusted proxy configuration, never the proposal's wallet or an untrusted forwarding header. Provider-wide exhaustion can still affect all callers. This decision removes targeted wallet-allowance attribution, not all denial of service.

### Preserve the pre-broadcast acceptance boundary

Only authenticated funding-intent admission can create a paid commitment. Fresh exact-quote/payment checks and the final capacity decision precede the durable acknowledgement. Competing writers cannot both consume the final lease. Full refusal creates no acknowledgement, accepted intent, account, credit or submission. An honest provider must not submit a refused original; a conforming consumer must obtain and persist a valid acknowledgement before submitting the exact prepared bytes.

The provider still receives signed payment bytes and can technically broadcast them. Preserve this disclosure. A missing response is not a proven refusal: recover the original intent, acknowledgement and transaction instead of obtaining a replacement quote/payment automatically. Cached acknowledged originals remain recoverable without fresh quote/work/rate/lease admission, including after restart or expiry; safe-frontier evidence remains necessary for credit.

### Cutover and integration order

1. This change supplies the replacement canonical proposal-admission deltas. They supersede conflicting old quote-wallet rules during implementation, but are not integrated canonical specs until normal archive.
2. Before implementation, retarget `specify-proposal-rate-budgets` to ledger-only committed issuance and reconcile its design/tasks with the independent work limiter. Update `specify-opening-funding-intent` and any commercial bounded-proposal wording to distinguish retained quote storage from paid/free opening capacity. Do not silently keep an active task requiring the old wallet ceiling.
3. Apply native changes and migrate each affected contract test together once independently committable. Preserve useful rollback, conservation, expiry and nonce/recovery assertions; change the superseded numerical/attribution contract rather than deleting tests to obtain a pass.
4. Retire obsolete named-wallet quote-rate bookkeeping without touching wallet identity in signed terms, challenge policy, account authorization, balances, fees, credentials or accepted commitments. Preserve the retained global committed-rate evidence and its separate clock fixes.
5. Complete acceptance, strict validation and available project checks, then archive without `--skip-specs`. Coordinate the ledger-rate delta so each requirement is integrated once; never archive the old wallet requirement first as an intermediate cutover.

## Acceptance and limits

Required consumer-visible evidence includes public issuance beyond both old wallet ceilings while provider-wide budgets remain; an independently authenticated owner unaffected by another caller naming that owner; no new lease from quote issuance; honest capacity-full refusal with no acknowledgement or broadcast; two-writer final-lease competition; and original acceptance recovery after lost reply, restart and quote/lease expiry. Observe actual signed acknowledgements and durable state, not merely response text.

Signing-work tests must prove no signer invocation after budget/concurrency refusal, finite attempts despite insertion failures, durable exhaustion and conservative initialization, independent committed-counter rollback, and non-regressing clocks. Reuse body-boundary evidence only where it exercises this public route. Free opening must cover authentication, final-capacity competition, no fake payment, failure rollback and idempotent existing-account recovery.

The ledger acceptance cases are `quote_work_bounds_600_real_signatures_when_every_insertion_rolls_back`, `quote_work_failed_signer_spends_last_permit_without_refund`, `quote_work_initialization_restart_and_clock_regression_require_new_minute`, `quote_work_separate_connections_share_the_final_unit`, `quote_work_survives_future_challenge_cleanup_and_legacy_quote_retirement`, `quote_work_persistence_begin_statement_and_commit_failures_return_no_signature` and `quote_work_invalid_evidence_fails_closed_without_blocking_paid_settlement`. They use real signing and explicit-time/persistence boundaries, retaining exact financial and independent committed-counter snapshots.

Actual public-route coverage adds `public_quote_work_initialization_refuses_durably_without_erasing_opened_terms`, `public_quote_signing_failures_spend_global_work_across_wallets_and_reopened_routers` and `failed_quote_response_body_does_not_refund_work_or_committed_quote`; extends `public_global_committed_rate_refusal_preserves_quotes_and_an_opened_agreement`; and observes work conservation through `control_readers_bound_time_and_concurrency_and_recover_every_permit`. Invalid signer keys distinguish admitted real signing failure from refusal before signer invocation. The response-loss case faults the body stream after actual commit, not a TCP transport or disk crash.

`authenticated_free_and_paid_reopening_preserves_credentials_and_funding_without_readiness` now retains exact accounts/credentials/payment evidence through both restored exhausted budgets, explicit issuance/rotation, original held-request settlement and an actual authorized request read. These are initialized-budget and post-verification funding prerequisites, not historical HTTP issuance counts or real chain execution. Wider top-up/gateway resource-pressure acceptance remains separately tracked.

The author authorized incorporating necessary pre-existing source; the migration is committed, so native ownership no longer blocks this work. The committed-admission slice migrated the old wallet cases to public acceptance and provider-global conservation/rollback boundaries. Pre-signing ledger/HTTP/body acceptance and all available checks pass: 418 Rust tests, 61 frontend/provisioning tests, available builds/type/script checks and both strict resource-change validations. Paid/expiry and broader recovery tasks remain open. No isolated acceptance test is live-chain, public-load, GPU or installer qualification.
