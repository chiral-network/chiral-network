## Context

The author's approved non-reserving-quote contract replaces public named-wallet pending/rate allowances with provider-wide quote resources and authenticated opening admission. `specify-non-reserving-quotes` supplies the replacement quote-retention and pre-signing-work requirements; this change owns the separate committed-issuance counter. The current implementation still has the old wallet gates, and the public issuer canonicalizes the named payer and signs before calling `store_quote`.

Inside one immediate transaction, `store_quote` cleans pending state, tentatively increments the global 600 and payer 12 rate counters, checks pending capacity and inserts the quote. An error rolls the transaction back. The current rate helper uses `window = now / 60`, retains the count only when the stored window equals that sample, and otherwise starts at zero. Shared cleanup deletes every rate row whose window is less than its supplied minute, including when invoked for challenges. The handler samples `protocol::now()` after body buffering but before ledger locking/signing/admission.

## Decisions

### Count committed proposals, not arrivals

Keep one 600-unit committed-issuance budget per provider ledger. Every durable insertion spends one ledger unit, regardless of the named wallet, even if its response is lost. Parsing, readiness, rate, storage-capacity or insertion failure does not consume this counter. Expiry, acknowledgement and first opening can release standalone quote storage but do not refund a committed unit.

Remove the twelve-per-wallet counter and eight-pending-wallet gate together under the parent cutover; leaving either would contradict the author decision. Changing wallet spelling, model, offer revision or funding mode does not alter the provider-wide budget. Independent ledgers have independent allowances. Canonical wallet identity remains part of signed terms and authenticated account authority, not a public quote-rate key.

The parent change's pre-signing work budget is different: it claims a durable unit before signing and does not refund that unit when insertion fails. Its counter, watermark and conservative first-minute initialization are independent of committed issuance. Do not move this change's committed charge before signing, refund a work permit in an insertion rollback, or use either clock to refill the other. Challenge and inference limits remain separate. Shared provider exhaustion may affect all callers; this is not per-caller authentication, Sybil resistance or a general availability guarantee.

### Fixed minutes with non-regressing selection

For a sampled unsigned Unix-second time `t`, compute `W = floor(t / 60)`. Let `L` be the last committed effective proposal minute in this ledger. Select `E = max(W, L)` inside the same write transaction as quote admission. Only a committed proposal may advance this `L`; rejection cannot advance it or erase its allowance. The single global committed-issuance counter uses `E`, not the independent signing-work watermark.

With normally advancing time this is a fixed UTC-minute policy, not a rolling 60-second window or a token bucket. Crossing to a strictly newer minute gives that minute its normal budget, not accumulated unused budgets from skipped minutes. Adjacent-window bursts remain possible when other admission gates permit them; this is not a measured throughput promise.

Clamping is preferable to rejecting every delayed sample: a delayed request may use the latest minute's remaining units, but gets no refill. A forward clock jump followed by correction can hold new proposals to the already observed minute until a strictly later minute is sampled. That availability cost is intentional; do not delete quota state to conceal it. Existing account/funding recovery and settled inference do not acquire this new proposal-rate gate.

The effective minute is an admission counter clock only. Do not rewrite signed issue/expiry times, owner-proof expiry, funding admission deadlines or chain evidence to match it. Preserve the existing validity and authorization checks.

### Durable state and cleanup ownership

Use the existing SQLite ledger transaction/persistence pattern, not a process-local clock or another service. A proposal-specific watermark in ledger metadata retains `L`; the global `quote:global` row retains its minute and count. Window selection, the global increment and quote insertion commit together across competing connections.

Generic pending/challenge/work maintenance must not erase or advance the committed-issuance watermark or counter merely because it observes another time. Older committed-issuance evidence may be pruned only after a successful proposal advances its minute. Preserve independent challenge, work and inference policies.

For adoption without a committed-issuance watermark, use the retained global row's minute and count, not a sum or copy of legacy wallet counts. Legacy wallet rows are historical consistency evidence only: if they exist without a global row, or prove a newer minute than the retained global row, the retained total is missing or contradictory. Refuse fresh proposal admission without deleting evidence; do not invent a global total for that minute. Unreadable or otherwise inconsistent state likewise fails closed. With neither watermark nor any retained quote-rate evidence, the first successful insertion establishes its initial minute and spends one unit.

Retire only obsolete wallet-keyed quote-rate rows in the same successful adoption transaction that preserves the global counter and installs its watermark. Failed adoption or insertion must not destroy the old evidence. After cutover no wallet-keyed quote bucket may be read as an admission allowance, created or charged. A valid global counter remains authoritative when the old wallet rows are gone; a watermark with missing/inconsistent required global state fails closed rather than reinitializing.

Previously discarded legacy rate history cannot be reconstructed or retroactively certified. This is nonfinancial admission bookkeeping within the current ledger, not permission to convert older schemas, reset a network namespace or alter agreements, money, fees, credentials or nonce ownership.

## Implementation gap and ownership

The old wallet rate/pending gates contradict the author decision. The `previous == window` branch also resets on an older minute, and generic cleanup can remove quote-rate evidence independently of admission. These source observations remain open gaps. Existing tests passing does not close them or establish the parent's pre-signing work limit.

`billing_store.rs` is entirely untracked owner source. A standalone unused helper would not fix the production path; copying the module or committing the surrounding migration is not acceptable. Pair the wallet-gate removal and clock/pruning/bootstrap fixes with their regressions when independently committable. The old wallet-ceiling tests must migrate with that cutover, not be weakened to pass the old implementation.

## Acceptance strategy

- Use isolated public ledger interfaces, real signed quotes and explicit time. Preserve useful provider-bound, retention, rollback and accepted-acknowledgement assertions while migrating superseded wallet-ceiling expectations.
- Require issuance beyond both former wallet ceilings when provider-wide work, storage and committed-rate resources allow it. Exercise unrelated callers naming one wallet, canonical spellings and free/paid/model variants through the real public route; quote issuance must not consume an opening lease.
- Isolate 600/601 committed-ledger admission and the normal minute boundary at the storage boundary so the independent signing-work gate cannot mask it. Verify independent ledgers and challenge/inference state. Such storage tests do not prove native signing-work admission; the parent change owns those route-level cases.
- Cover one-bucket insertion rollback, provider-storage rejection, last-global-unit competition and no refund on response loss, expiry, acknowledgement or first opening. A failed insertion preserves the global committed counter and signed quote state but leaves any prior work permit spent.
- With the native fix, cover older/reordered samples, remaining versus exhausted current allowance, reopening, later challenge/work cleanup, failed future-minute insertion, preserved global counts at initialization, legacy wallet-row retirement and fail-closed missing/newer/invalid retained evidence. Do not sum wallet counters or treat obsolete wallet exhaustion as a live gate.
- Keep request time, logical budget time and quote expiry distinct in fixtures. Reopening storage is not an OS crash/power-loss campaign. No chain, engine, GPU, deployment or real-wallet action is required or claimed.

## Verification and completion

Strictly validate this change and run available README/CI checks. Keep platform, release-authority and live qualification gates explicit. Archive only after the non-reserving quote cutover and this change's numerical, transactional, clock and adoption requirements have actual acceptance evidence. Both changes remain open meanwhile; do not archive the superseded wallet policy as an intermediate step or use `--skip-specs`. Old completed test work is recorded below the task list as historical evidence, not current acceptance.
