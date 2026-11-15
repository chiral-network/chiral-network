# APAS progress

## 2026-11-15 — Enforce named-field account opening

- Entry was clean at `a26e86cf`. The retained complete whitepaper/report are unchanged by hash; both author Answers are already acted on, with no new decision needed. The read-only book retains SHA-256 `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`. Pending non-reserving deltas remain authoritative over historical wallet quotas in main specs.
- Implemented the **specify-account-opening-request 2.1–2.2** slice: `OpenRequest` uses the shared direct map visitor and strict derived fields, preserving serialization and omitted/null/string payment semantics. No positional shim, intermediate JSON value, caller migration or financial-state conversion.
- The new production-router regression failed before correction: a genuinely signed complete array returned HTTP 200 and created a free account instead of 400. Its malformed-shape/member matrix covers fresh free opening and existing paid replay, requires all fourteen ledger tables (including the unconsumed proof) unchanged, and retains useful omitted/null free controls and exact-original paid replay. Paid credit is an explicit post-verification fixture prerequisite, not observed chain evidence.

## Verification

- **497 tests passed:** 436 Rust across 20 suites (two pre-existing ignored), 15 frontend and 46 provisioning. Headless build, Svelte checking (zero errors/warnings), frontend build, CI Python compilation, strict change validation and protected-book checksum passed. **2.1–2.2** are complete; **2.3–2.4/3.1–3.2** remain open. Do not archive or infer completion of the broader payment/recovery contract.
- Compiler warnings and stale Browserslist data remain. Native desktop/release builds are blocked by missing WebKit and the actual policy authority; external platform/hardware/deployment qualification remains outside this checkout. No installation, invented authority, deployment, live-chain or power-loss claim.

## Next steps, in order

1. Finish **opening-request 2.3–2.4**: inventory and complete missing free/paid payment-presence, exact-body substitution, business-refusal proof consumption, expiry and existing-account recovery correspondence. Reuse existing safe-credit/ownership/credential conservation evidence; do not infer complete lifecycle coverage from the schema matrix. Then inspect top-up and accounting-response schemas without assuming every derived object has the same optional/unsigned-extension policy.
2. Finish challenge-clock non-regression (**2.4–2.5**) and exact private time/fault acceptance: offer/qualification expiry (**commercial 2.1.3.2**), native receiver 600/601-second boundaries (**owner-control 2.2.3**), acknowledgement 30-second edges (**opening 2.7**) and start-state-only persistence failure (**opening 2.5.3**). Source ownership no longer blocks fixes. No sleeps, watcher races or changed accepted timestamps.
3. Complete accepted-paid-intent/elapsed-expiry recovery (**non-reserving parent 3.3/4.3**, opening **2.3.4.3/2.8.2**) and top-up/settlement paths under both exhausted quote budgets. Preserve completed opening **2.4** and existing free/paid reopening and admitted-hold settlement; neither is every native/HTTP recovery path.
4. Finish envelope private byte/item/depth acceptance (**2.5**) and mapping/archive, preserving fixed list/data rejection (**2.6**, `30ad8fd1`). The 14,448-byte profile ceiling precludes an otherwise-valid 16,384-byte fixture; never weaken limits.
5. Continue request-accounting credential expiry during a hold (**2.2.4**), invalid-usage/budget edges (**2.5**), receipt/snapshot/sequence failures (**2.3**), gateway failure (**3.1**), performance binding (**3.2**) and resource/recovery isolation (**3.3**). Preserve completed **2.2.1–2.2.3/2.4**; ledger assertions are not engine or live-chain evidence.
6. Continue safe-credit/reorg, cancellation/top-up/fees, discovery, credential lifecycle, runtime/performance and reputation alignment. Keep incomplete changes open. The report's named-payer quota entry needs remaining acceptance and the proposed author-only factual book update, not another policy answer.

## Checkout discipline

This step owns `protocol.rs`, the owner-control regression, opening-request proposal/design/tasks, the bug register and these notes. The author report is unchanged. Preserve unrelated work and the whitepaper; no post-edit git audit, push, stash, reset, restore, installation or deployment. Commit only verified files with per-command `APAS Worker <apas-worker@localhost>`; never archive with `--skip-specs`.
