## Authority and scope

The author answer and the book's **Approved non-reserving quotes** supersede named-wallet quote quotas. The worker did not edit the book. This is a replacement-contract slice, not evidence that production has cut over. Historical eight-slot and twelve-per-minute acceptance observations remain valid descriptions of the old behavior, not mitigation evidence.

## Observed implementation

`BillingStore::store_quote` currently charges `quote:global` at 600/minute and `quote:<named wallet>` at 12/minute, then enforces 10,000 total pending quotes and eight for the named wallet. The native public handler signs before calling that transaction. Thus removing only the pending-wallet query leaves the rate attack, and a committed-insertion counter alone does not bound signatures for rejected insertions.

The control handler already takes one of 64 process-wide non-queuing permits before buffering at most 96 KiB with a five-second body deadline. These bounds are useful but do not limit repeated signing attempts over time.

`register_funding_intent` retrieves an exact cached original before fresh gates. Fresh registration validates the issued quote, permanent transaction/sender-nonce ownership and execution terms, then counts active un-opened intent leases inside the write transaction. It inserts the complete signed acknowledgement/quote/raw transaction before deleting only the matching standalone proposal. The active-intent limit is 10,000; expiry releases admission capacity without deleting accepted evidence.

`open_account` applies its eight-owned-account / 10,000-total-account limits only to a first free opening. First paid opening requires its retained intent and verified payment; an existing account follows its recovery path. Keep that distinction rather than introducing an account-capacity check after paid acknowledgement.

## Decisions

### Three distinct kinds of state

1. **Unaccepted quote retention:** at most 10,000 standalone quotes per provider ledger, without a wallet sub-limit. Expiry reclaims this storage. Retention authenticates the exact issued terms but is not an opening reservation.
2. **Accepted paid admission:** at most 10,000 un-opened, unexpired original intent leases per provider ledger. Admission is payer-authenticated and transactional. The lease ends at the original quote deadline; expiry never revokes the acknowledged agreement or nonce commitment.
3. **Free opening:** the existing finite account limits are checked atomically only at the authenticated first free opening. No payment or funding-intent is fabricated. Existing-account recovery does not repeat fresh-opening capacity admission.

These limits are not model execution slots, prepaid balance guarantees, speed classes or paid-inference reservation limits. A provider may issue otherwise valid quotes while its paid admission leases are full; receipt of a quote does not authorize broadcast.

### Bound work separately from committed issuance

Keep the shared 64-operation, 96-KiB and five-second control boundaries. Add a provider-ledger-wide budget of 600 quote-signing attempts per fixed effective minute, claimed durably after input validation and before signing. A claimed attempt is not refunded when signing, insertion, persistence or response delivery fails: it limits work already authorized, not successful quotes. Reaching this budget returns 429/`rate_limit` before signing.

Use the existing planned non-regressing-minute approach for this separate work budget: `E = max(floor(now / 60), last_committed_work_minute)`, atomically claim its unit, retain the watermark/count across restart and unrelated cleanup, and refill only for a strictly newer minute. There is one ledger-wide work bucket, not attacker-expandable wallet/source rows. Before enabling the new limiter on a ledger with no work evidence, durably mark the current initialization minute exhausted; wait for its next minute rather than inventing unused signing allowance for the partial cutover minute. Invalid or unreadable work evidence fails closed for fresh quoting only. This bookkeeping does not convert old financial schemas or reconstruct unobserved past attempts.

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

Current native ownership prevents independently committing those source changes. Keep implementation tasks open; do not add unused helpers, weaken the contract, commit expected-failing regressions alone or absorb the surrounding owner migration. Passing the current suite describes the old implementation. No isolated acceptance test is live-chain, public-load, GPU or installer qualification.
