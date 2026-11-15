# APAS progress

## 2026-11-15 — Enforce the funding registration interval

- Entry was clean at `e6d1ac71`. Read the complete README-named whitepaper and report; both author Answers are already acted on. The read-only book retains SHA-256 `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`. No new author decision is needed.
- Selected the committed producer/consumer time contradiction before further missing expiry coverage. Opening task **2.4** now rejects a transaction-sampled registration before quote issuance; the existing expiry check, cached-acknowledgement ordering and native validation remain intact. No fabricated/clamped signed time or logical-rate-clock substitution.
- Added two ledger regressions: exact `[issued_at,expires_at)` endpoints and unavailable time, unchanged durable state on refusal and same-original recovery, precise signed acknowledgement/retention changes, cached recovery without fresh time, and genuine SQLite writer contention with deterministic clock advancement. The writer probe verifies sampling inside the acquired write transaction; channel timeouts only bound deadlocks. Candidate payments are genuinely signed; prior account credit is an explicit post-verification prerequisite.

## Verification

- **481 passing tests:** 420 Rust across 20 suites (two pre-existing ignored), 15 frontend and 46 provisioning. Headless binary build, Svelte checking (zero errors/warnings), frontend build, CI Python compilation, strict `specify-opening-funding-intent` validation and protected-book checksum passed. Opening **2.4** is complete; the change remains open for its other tasks. No check or assertion was weakened.
- Compiler warnings and stale Browserslist data remain. WebKit and the actual release policy authority remain unavailable; native desktop/release builds and external platform/hardware/deployment qualification are blocked. No installation, invented authority, deployment, live-chain execution or power-loss claim.

## Next steps, in order

1. Correct remaining committed contract mismatches before missing acceptance: opening object-only request/envelope/payload/network guards (**2.1.2**) and commercial localhost-alias / embedding price-template rules (**2.1.4/2.2/2.3**). Source ownership no longer blocks native fixes; preserve existing signatures, authorization and monetary boundaries.
2. Finish challenge-clock non-regression (**2.4–2.5**) and exact private time/fault acceptance: offer/qualification expiry (**commercial 2.1.3.2**), native receiver 600/601-second boundaries (**owner-control 2.2.3**), acknowledgement 30-second edges (**opening 2.7**) and start-state-only persistence failure (**opening 2.5.3**). No sleeps, watcher races or changed accepted timestamps.
3. Complete accepted-paid-intent/elapsed-expiry recovery (**non-reserving parent 3.3/4.3**, opening **2.3.4.3/2.8.2**) and top-up/settlement paths under both exhausted quote budgets. This turn's ledger writer-wait proof is not native/HTTP fresh-commit-to-expiry recovery. Existing free/paid reopening and admitted-hold settlement already cover both budgets; broader paths remain.
4. Finish envelope private byte/item/depth acceptance (**2.5**) and mapping/archive, preserving fixed list/data rejection (**2.6**, `30ad8fd1`). The 14,448-byte profile ceiling precludes an otherwise-valid 16,384-byte fixture; never weaken limits.
5. Continue request-accounting credential expiry during a hold (**2.2.4**), invalid-usage/budget edges (**2.5**), receipt/snapshot/sequence failures (**2.3**), gateway failure (**3.1**), performance binding (**3.2**) and broader resource/recovery isolation (**3.3**). Preserve completed **2.2.1–2.2.3/2.4**; ledger assertions are not engine or live-chain evidence.
6. Continue safe-credit/reorg, cancellation/top-up/fees, discovery, credential lifecycle, runtime/performance and reputation alignment. Keep incomplete changes open. The report's named-payer quota entry needs remaining acceptance and the proposed author-only factual book update, not another policy answer.

## Checkout discipline

This step owns only the registration-time guard, its two ledger regressions, opening-change implementation records, bug documentation and these notes. Preserve unrelated work and the whitepaper; no post-edit git audit, push, stash, reset, restore, installation or deployment. Commit only verified files with per-command `APAS Worker <apas-worker@localhost>`; never archive with `--skip-specs`.
