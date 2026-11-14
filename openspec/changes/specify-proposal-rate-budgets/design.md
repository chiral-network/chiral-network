## Context

The integrated capability limits retained pending proposals to eight per named payer and 10,000 per provider ledger. Those are occupancy budgets, not rates. The native issuer canonicalizes the named payer, signs the quote, then calls `store_quote`; the public proposal is deliberately not owner-authenticated.

Inside one immediate transaction, `store_quote` cleans pending state, tentatively increments the global 600 and payer 12 rate counters, checks pending capacity and inserts the quote. An error rolls the transaction back. The current rate helper uses `window = now / 60`, retains the count only when the stored window equals that sample, and otherwise starts at zero. Shared cleanup deletes every rate row whose window is less than its supplied minute, including when invoked for challenges. The handler samples `protocol::now()` after body buffering but before ledger locking/signing/admission.

## Decisions

### Count committed proposals, not arrivals

Keep the existing 12/600 numerical budgets. Both must permit the same insertion. A committed quote consumes one payer unit and one ledger unit even if its HTTP response is lost. Parsing, readiness, rate, capacity or insertion failures do not consume proposal-rate units. Expiry, acknowledgement and first opening can release pending capacity, but do not refund the already spent rate units.

The scope is the existing provider ledger and its canonical named payer, across all models, offer revisions and funding modes. Other ledgers have independent budgets. Challenge and inference counters remain separate. This policy does not authenticate the caller, stop Sybil identities or bound signing work and rejected-request CPU cost.

### Fixed minutes with non-regressing selection

For a sampled unsigned Unix-second time `t`, compute `W = floor(t / 60)`. Let `L` be the last committed effective proposal minute in this ledger. Select `E = max(W, L)` inside the same write transaction as admission. Only a committed proposal may advance `L` to a newer sample; rejection cannot advance the window or erase its old allowance. All quote rate buckets use the same `E` for that admission.

With normally advancing time this is a fixed UTC-minute policy, not a rolling 60-second window or a token bucket. Crossing to a strictly newer minute gives that minute its normal budget, not accumulated unused budgets from skipped minutes. Adjacent-window bursts remain possible when other admission gates permit them; this is not a measured throughput promise.

Clamping is preferable to rejecting every delayed sample: a delayed request may use the latest minute's remaining units, but gets no refill. A forward clock jump followed by correction can hold new proposals to the already observed minute until a strictly later minute is sampled. That availability cost is intentional; do not delete quota state to conceal it. Existing account/funding recovery and settled inference do not acquire this new proposal-rate gate.

The effective minute is an admission counter clock only. Do not rewrite signed issue/expiry times, owner-proof expiry, funding admission deadlines or chain evidence to match it. Preserve the existing validity and authorization checks.

### Durable state and cleanup ownership

Use the existing SQLite ledger transaction/persistence pattern, not a process-local clock or another service. A proposal-specific watermark in ledger metadata can retain `L`; existing quote rate rows retain current global/per-payer counts. The precise SQL layout is internal, but window selection, both counter increments and quote insertion must commit together across competing connections.

Generic pending/challenge maintenance must not erase the latest committed proposal window or its counters merely because it observes another time. Proposal rows from older effective minutes may be pruned once a committed proposal advance makes them irrelevant. Do not change challenge-rate policy as a side effect of correcting quote rates.

When no watermark exists, use retained quote-rate evidence to establish the last observed proposal minute and preserve the applicable counters; a pristine ledger with no such evidence starts at its first admission sample. Do not silently reset retained budgets when initializing metadata. Unreadable or inconsistent evidence must fail closed for fresh proposals instead of inventing unused allowance. Already discarded legacy rate history cannot be reconstructed or retroactively certified. This is nonfinancial admission bookkeeping within the current ledger, not permission to migrate old schemas, reset a network namespace or alter agreements, money, fees, credentials or nonce ownership.

## Implementation gap and ownership

The present `previous == window` branch resets on an older minute; the generic cleanup can remove quote-rate evidence independently of quote admission. Those source observations are the known gaps against the new clock requirement. Existing tests passing does not close them.

`billing_store.rs` is entirely untracked owner source. A standalone unused helper would not fix its production path, and copying the module or committing its surrounding migration is not acceptable. Leave the clock/pruning/bootstrap fix and its currently failing regression scenarios open until the source edit is independently committable. Numerical-budget, forward-boundary, scope and transaction evidence can proceed independently where the current implementation satisfies the requirement.

## Acceptance strategy

- Use isolated public `BillingStore` interfaces, real signed quotes and explicit time; retain existing pending-quota and acknowledgement tests.
- Isolate 12/13 payer rate admission from the eight-slot occupancy limit with explicit short-lived storage fixtures. Isolate 600/601 ledger admission with distinct eligible payers; do not alter native quote TTL or claim those fixtures are native issuance/load evidence.
- Cover provider-ledger and payer/model/free-paid attribution, normal minute boundaries and persistence across reopening. Existing HTTP 429 mapping may be reused; do not add wall-clock sleeps or a 600-request timing race at the public router.
- Cover rejection conservation for both buckets, last-rate-unit competition across independent connections, no refund after capacity reclamation and no window advance from a failed insertion.
- With the native fix, cover older timestamps after a later committed minute, remaining versus exhausted allowance, reordered writers, reopening, future-time challenge cleanup followed by an older proposal, retained pre-watermark counters and fail-closed inconsistent state.
- Keep request time, logical budget time and quote expiry distinct in fixtures. Reopening storage is not an OS crash/power-loss campaign. No chain, engine, GPU, deployment or real-wallet action is required or claimed.

## Verification and completion

Strictly validate the change and run the available README/CI checks. Keep platform, real release-key and live qualification gates explicit. Do not archive until the numerical, transactional, durable-clock and adoption requirements have actual implementation and acceptance evidence; the owner-source blocker does not permit narrowing the clock contract or treating old passing tests as its proof.
