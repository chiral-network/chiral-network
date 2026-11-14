## 1. Establish the replacement contract

- [x] 1.1 Read the author's changed whitepaper and complete report answer; compare current proposal, rate, intent and account admission paths without editing the whitepaper.
- [x] 1.2 Write the replacement proposal-admission deltas and design: no named-wallet quote budgets, non-reserving terms, bounded pre-signing work, authenticated atomic paid/free admission and durable accepted-original recovery.
- [x] 1.3 Strictly validate this change and record available project checks; distinguish specification completion from production mitigation. Strict validation passed after preserving unchanged scenario identities and explicitly replacing superseded wallet-cap requirements. The available Rust/frontend/Python checks passed 434 tests; they exercise the existing implementation, not this proposed cutover.

## 2. Reconcile downstream specifications before implementation

- [x] 2.1 Retarget the existing `specify-proposal-rate-budgets` delta, proposal, design and remaining tasks to the single 600/minute committed-ledger bucket. Both changes pass strict validation. Retained non-regressing time, global-counter preservation/fail-closed adoption and insertion rollback; distinguished independently spent pre-signing work. Old wallet tests are explicitly historical and revised acceptance tasks remain open until the native cutover.
- [ ] 2.2 Align `specify-opening-funding-intent`, commercial bounded-proposal wording and scenario/source mappings with quote storage versus authenticated opening capacity. Replace remaining active wallet-slot requirements/tasks, preserve signed acknowledgement/nonce/lease recovery and keep all prior acceptance limitations explicit.

## 3. Implement the cutover with its regressions

- [ ] 3.1 Remove both named-wallet pending-quote and proposal-rate gates from native issuance/storage together. Preserve the provider-wide 10,000 retained-quote and 600 committed-issuance limits and retire obsolete wallet quote-rate bookkeeping without altering challenge policy, signed parties, accounts or financial evidence. BLOCKED while the necessary native modules remain entirely owner-untracked and cannot be committed independently of the owner's migration.
- [ ] 3.2 Implement the durable pre-signing work permit, non-regressing clock, conservative initialization and cleanup protection on the real route; preserve existing control body/concurrency/deadline boundaries. Pair the native fix with deterministic regressions; no unused helper or expected-failing test-only commit. Same source-ownership blocker as 3.1.
- [ ] 3.3 Verify or correct atomic paid-intent capacity and honest refusal-before-acknowledgement/submission; preserve exact cached originals across full capacity, lost replies, restart and expiry. Verify native acknowledgement persistence before exact-original broadcast and no automatic replacement.
- [ ] 3.4 Verify or correct authenticated free first-opening capacity and existing-account recovery without fake payments, new quote gates or credential rotation. Keep top-up and settlement recovery independent of fresh quote work/capacity.
- [ ] 3.5 Migrate every affected old wallet-budget contract test alongside its native cutover; retain useful transaction rollback, provider-limit, expiry and accepted-evidence checks. Do not delete or weaken checks merely to pass the old implementation.

## 4. Prove acceptance and integrate

- [ ] 4.1 Exercise real public proposals beyond both old wallet ceilings, unrelated callers naming the same owner, canonical spellings and free/paid/model variants while provider resources are available. Assert no named-wallet allowance or opening lease is consumed.
- [ ] 4.2 Cover bounded signer invocations, work exhaustion despite failed insertions, control saturation/body boundaries, competing work claims, restart, older samples, initialization and separate committed-counter rollback. Reuse existing tests only when they exercise the changed production path.
- [ ] 4.3 Cover overbooked quotes, final paid/free slot competition, full refusal without acknowledgement/submission and original accepted recovery after post-commit reply loss, reopening and expiry. Preserve authoritative safe-credit prerequisites; do not fabricate chain success or claim live qualification.
- [ ] 4.4 Update the report with actual mitigation evidence and remaining gaps; run available project checks and strict validation, then archive this change normally without `--skip-specs` only after all tasks and dependent specification alignment are complete. Keep external release/deployment/platform tasks unclaimed.
