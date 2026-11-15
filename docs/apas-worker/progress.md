# APAS progress

## 2026-11-15 — Prove native top-up recovery at paid-intent capacity

- Entry was clean at `dcc6d95c`. Retained the fully read, unchanged README-designated `docs/chiral-book.md` and report: both existing Answers were already acted on. The book SHA-256 remains `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`; pending non-reserving deltas remain authoritative where not yet archived.
- Selected `specify-account-topup-request` 2.4.3.2.2: native recovery with 10,000 independent active paid intents and otherwise available admission resources. A genuine fresh-intent control passes initial/rechecked spendability before capacity refusal, consumes its proof and leaves the pending quote without an acknowledged sender/nonce commitment.
- Moved complete signed-intent restoration into `src-tauri/tests/support/funding.rs`, migrating all three existing funding-admission callers and reusing independently validated native originals without double verification. Native missing/unsafe/safe/lost-reply/replay coverage preserves original bytes/nonce/journal, every unrelated lease/account, prior spend/live hold, one credit/fee and exported credentials. Restored history is a prerequisite; runtime policy is unchanged.

## Verification

- Strict change validation, the full headless Rust run (450 passed across 20 suites, two existing ignored tests), headless binary build, Svelte checking (zero errors/warnings), 15 frontend tests, frontend build, 46 provisioning tests, CI Python compilation and the protected-book checksum passed. Task 2.4.3.2.2 and parent native opening-capacity task 2.4.3.2 are complete.
- Native canonical/different-pending-amount acceptance remains 2.4.3.3. Retain existing dropped-broadcast and missing/different-original journal regressions rather than duplicating them. Parent 2.4.3, 2.4 and integration/archive tasks remain open; no runtime change or author decision was needed.
- Compiler warnings and stale Browserslist data remain. Native desktop/release builds are blocked by missing WebKit and the actual policy authority; external platform/hardware/deployment qualification remains outside this checkout. No installation, invented authority, deployment, live-chain or power-loss claim.

## Next steps, in order

1. Finish top-up 2.4.3.3 native positive-canonical/different-pending-amount acceptance. Preserve completed provider/native quote-budget and paid/free-capacity matrices and existing original-journal/lost-response regressions; archive only after every task/check passes. Accounting-response schemas follow this request foundation.
2. Finish challenge-clock non-regression (**2.4–2.5**) and exact private time/fault acceptance: offer/qualification expiry (**commercial 2.1.3.2**), native receiver 600/601-second boundaries (**owner-control 2.2.3**), acknowledgement 30-second edges (**opening 2.7**) and start-state-only persistence failure (**opening 2.5.3**). Source ownership no longer blocks fixes. No sleeps, watcher races or changed accepted timestamps.
3. Complete accepted-paid-intent/elapsed-expiry recovery (**non-reserving parent 3.3/4.3**, opening **2.3.4.3/2.8.2**) and top-up/settlement paths under both exhausted quote budgets. Preserve completed opening **2.4** and existing free/paid reopening and admitted-hold settlement; neither is every native/HTTP recovery path.
4. Finish envelope private byte/item/depth acceptance (**2.5**) and mapping/archive, preserving fixed list/data rejection (**2.6**, `30ad8fd1`). The 14,448-byte profile ceiling precludes an otherwise-valid 16,384-byte fixture; never weaken limits.
5. Continue request-accounting credential expiry during a hold (**2.2.4**), invalid-usage/budget edges (**2.5**), receipt/snapshot/sequence failures (**2.3**), gateway failure (**3.1**), performance binding (**3.2**) and resource/recovery isolation (**3.3**). Preserve completed **2.2.1–2.2.3/2.4**; ledger assertions are not engine or live-chain evidence.
6. Continue safe-credit/reorg, cancellation/top-up/fees, discovery, credential lifecycle, runtime/performance and reputation alignment. Keep incomplete changes open. The report's named-payer quota entry needs remaining acceptance and the proposed author-only factual book update, not another policy answer.

## Checkout discipline

This step owns native owner-control and funding-admission acceptance, shared funding support, top-up change design/tasks and these notes. Runtime, report and whitepaper are untouched. Commit only those six paths with per-command APAS identity; do not alter unrelated work, install prerequisites, invent a policy authority or operate outside this checkout.
