# APAS progress

## 2026-11-15 — Prove native top-up recovery at free-opening limits

- Entry was clean at `d305c725`. Retained the fully read, unchanged README-designated `docs/chiral-book.md` and report: both existing Answers were already acted on. The book SHA-256 remains `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`; pending non-reserving deltas remain authoritative where not yet archived.
- Selected `specify-account-topup-request` 2.4.3.2.1: native recovery at independently full owned-account and ledger-account free-opening bounds. Reused complete signed-account/root restoration and the native lost-reply scenario; a genuine fresh free opening must fail only at capacity while quote budgets, quote storage and active paid-intent capacity remain available.
- Retained the accepted paid offer separately from the newer free publication. Shared missing/unsafe/safe/lost-response/replay assertions preserve exact originals, one broadcast, prior spend/live hold/usage, once-only credit/fee and exported credentials. Added streaming protection of every unrelated financial document plus bearer GETs for restored credential samples. Restored accounts are prerequisites, not observed historical admissions; no challenge-counter reset or runtime policy change.

## Verification

- Strict change validation, the full headless Rust run (449 passed across 20 suites, two existing ignored tests), headless binary build, Svelte checking (zero errors/warnings), 15 frontend tests, frontend build, 46 provisioning tests, CI Python compilation and the protected-book checksum passed. Task 2.4.3.2.1 is complete; no runtime change or author decision was needed.
- Split native opening-capacity acceptance into 2.4.3.2.1 (this free-capacity slice) and 2.4.3.2.2 (paid-intent capacity). Native canonical/different-pending-amount acceptance remains 2.4.3.3; retain existing dropped-broadcast and missing/different-original journal regressions. Parent recovery and integration/archive tasks remain open.
- Compiler warnings and stale Browserslist data remain. Native desktop/release builds are blocked by missing WebKit and the actual policy authority; external platform/hardware/deployment qualification remains outside this checkout. No installation, invented authority, deployment, live-chain or power-loss claim.

## Next steps, in order

1. Finish top-up 2.4.3.2.2 native paid-intent-capacity recovery, then 2.4.3.3 canonical/pending-amount acceptance. Preserve completed provider and native quote-budget/free-capacity matrices and existing journal/lost-response regressions; archive only after every task/check passes. Accounting-response schemas follow this request foundation.
2. Finish challenge-clock non-regression (**2.4–2.5**) and exact private time/fault acceptance: offer/qualification expiry (**commercial 2.1.3.2**), native receiver 600/601-second boundaries (**owner-control 2.2.3**), acknowledgement 30-second edges (**opening 2.7**) and start-state-only persistence failure (**opening 2.5.3**). Source ownership no longer blocks fixes. No sleeps, watcher races or changed accepted timestamps.
3. Complete accepted-paid-intent/elapsed-expiry recovery (**non-reserving parent 3.3/4.3**, opening **2.3.4.3/2.8.2**) and top-up/settlement paths under both exhausted quote budgets. Preserve completed opening **2.4** and existing free/paid reopening and admitted-hold settlement; neither is every native/HTTP recovery path.
4. Finish envelope private byte/item/depth acceptance (**2.5**) and mapping/archive, preserving fixed list/data rejection (**2.6**, `30ad8fd1`). The 14,448-byte profile ceiling precludes an otherwise-valid 16,384-byte fixture; never weaken limits.
5. Continue request-accounting credential expiry during a hold (**2.2.4**), invalid-usage/budget edges (**2.5**), receipt/snapshot/sequence failures (**2.3**), gateway failure (**3.1**), performance binding (**3.2**) and resource/recovery isolation (**3.3**). Preserve completed **2.2.1–2.2.3/2.4**; ledger assertions are not engine or live-chain evidence.
6. Continue safe-credit/reorg, cancellation/top-up/fees, discovery, credential lifecycle, runtime/performance and reputation alignment. Keep incomplete changes open. The report's named-payer quota entry needs remaining acceptance and the proposed author-only factual book update, not another policy answer.

## Checkout discipline

This step owns only the native owner-control acceptance fixture, top-up change design/tasks and these notes. Runtime, report and whitepaper are untouched. Commit only those four paths with per-command APAS identity; do not alter unrelated work, install prerequisites, invent a policy authority or operate outside this checkout.
