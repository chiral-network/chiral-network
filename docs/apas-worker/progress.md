# APAS progress

## 2026-11-15 — Prove top-up recovery under paid-intent capacity

- Entry was clean at `f5bb300b`. The previously fully read authoritative book/report remain unchanged by entry hashes; both Answers are already acted on and no new author decision is needed. The read-only book retains SHA-256 `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`. Pending non-reserving deltas remain authoritative over historical wallet quotas in main specs.
- Selected **specify-account-topup-request 2.4.2.2**. Reused the funding-admission fixture and complete signed-intent restoration to isolate 10,000 active paid leases. A fresh owner-authorized intent actually reaches spendability checks and is refused at capacity; an existing paid account then recovers through reopened pending/unsafe/safe/replay phases with fresh admission closed.
- Streaming snapshots preserve all retained originals, leases, terms, credentials, spent/held requests and original funding evidence. Credit/fee occurs once; actual account GETs and consumed-proof replay are checked. One acknowledgement is observed over HTTP; opening credit, supplied usage and 10,000 restored intents are disclosed prerequisites, not historical admissions, live-chain, engine, native or physical-durability evidence. Runtime is unchanged.

## Verification

- Verification after corrections: 445 Rust tests across the full run and exact retry (two existing ignored), 15 frontend tests, 46 provisioning tests, strict change validation, headless/frontend builds, Svelte checking (zero errors/warnings), CI Python compilation and protected-book checksum passed. No second full-suite run.
- Corrected the new test's initial credential-request import error. The full run passed the paid-capacity case but exposed the committed free-capacity fixture's thirteen-challenge demand against the twelve/minute owner limit. Its eligibility probe now uses a different genuinely opened account/owner with matching payment/proof; all assertions and independent capacity bounds remain, without sleeps, counter resets or relaxed limits. The exact corrected owned/global regression passed. **2.4.2.2** and parent **2.4.2** are complete; native **2.4.3**, parent **2.4** and **3.1–3.2** remain open.
- Compiler warnings and stale Browserslist data remain. Native desktop/release builds are blocked by missing WebKit and the actual policy authority; external platform/hardware/deployment qualification remains outside this checkout. No installation, invented authority, deployment, live-chain or power-loss claim.

## Next steps, in order

1. Finish **top-up request 2.4.3**, native same-original recovery under independent quote budgets/applicable opening-capacity pressure and remaining positive-canonical/pending-amount input edges. Preserve provider quote-budget and paid/free-capacity recovery, prior full-storage/journal/lost-response cases and evidence limits. Archive only after every change task/check passes; accounting-response schemas remain the next distinct foundation.
2. Finish challenge-clock non-regression (**2.4–2.5**) and exact private time/fault acceptance: offer/qualification expiry (**commercial 2.1.3.2**), native receiver 600/601-second boundaries (**owner-control 2.2.3**), acknowledgement 30-second edges (**opening 2.7**) and start-state-only persistence failure (**opening 2.5.3**). Source ownership no longer blocks fixes. No sleeps, watcher races or changed accepted timestamps.
3. Complete accepted-paid-intent/elapsed-expiry recovery (**non-reserving parent 3.3/4.3**, opening **2.3.4.3/2.8.2**) and top-up/settlement paths under both exhausted quote budgets. Preserve completed opening **2.4** and existing free/paid reopening and admitted-hold settlement; neither is every native/HTTP recovery path.
4. Finish envelope private byte/item/depth acceptance (**2.5**) and mapping/archive, preserving fixed list/data rejection (**2.6**, `30ad8fd1`). The 14,448-byte profile ceiling precludes an otherwise-valid 16,384-byte fixture; never weaken limits.
5. Continue request-accounting credential expiry during a hold (**2.2.4**), invalid-usage/budget edges (**2.5**), receipt/snapshot/sequence failures (**2.3**), gateway failure (**3.1**), performance binding (**3.2**) and resource/recovery isolation (**3.3**). Preserve completed **2.2.1–2.2.3/2.4**; ledger assertions are not engine or live-chain evidence.
6. Continue safe-credit/reorg, cancellation/top-up/fees, discovery, credential lifecycle, runtime/performance and reputation alignment. Keep incomplete changes open. The report's named-payer quota entry needs remaining acceptance and the proposed author-only factual book update, not another policy answer.

## Checkout discipline

This step owns the funding-admission regression/shared POST-proof helper, the required owner-control fixture/signing-helper fix, top-up request design/tasks and these notes. Runtime code, author report and whitepaper remain untouched; no temporary probes are retained. Preserve unrelated work; no post-edit git audit, push, stash, reset, restore, installation or deployment. Commit only verified files with per-command `APAS Worker <apas-worker@localhost>`.
