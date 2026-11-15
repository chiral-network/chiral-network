# APAS progress

## 2026-11-15 — Prove opening payment presence and exact-body authority

- Entry was clean at `f188c6d8`. Read the complete whitepaper and author report; both Answers are already acted on and no new author decision is needed. The read-only book retains SHA-256 `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`. Pending non-reserving deltas remain authoritative over historical wallet quotas in main specs.
- Selected **specify-account-opening-request 2.3**. Actual source already enforces free/paid payment presence, exact registered raw strings and raw-body proof binding; this is missing implementation-layer acceptance, not a runtime fix. Preserve the completed object-only decoder and existing ownership/safe-credit/recovery tests.
- Added two production-router regressions: fresh/existing free and unregistered/acknowledged/existing paid payment cases, plus both omission/null substitution directions. They require business-refusal proof consumption, unchanged financial/account authority, no premature observed RPC/engine calls, and useful exact-body/original-payment controls. Correct acknowledged first-payment evidence may reach an unavailable RPC and record its original attempt, but must not credit funds. Paid admission/accepted credit are disclosed fixture prerequisites, not live-chain or broadcast evidence.

## Verification

- **499 tests passed across the full run and targeted correction:** 438 Rust (two pre-existing ignored), 15 frontend and 46 provisioning. Headless/frontend builds, Svelte checking (zero errors/warnings), CI Python compilation, strict change validation and protected-book checksum passed.
- The initial full run exposed one incorrect new-test expectation: owner authentication emits `401 / invalid_api_key`, not the internal `unauthorized` label. Corrected only that expectation; the exact failed-test rerun passed with all proof/state checks intact. **2.3** is complete; **2.4/3.1–3.2** remain open. Do not archive or claim a new runtime defect/fix.
- Compiler warnings and stale Browserslist data remain. Native desktop/release builds are blocked by missing WebKit and the actual policy authority; external platform/hardware/deployment qualification remains outside this checkout. No installation, invented authority, deployment, live-chain or power-loss claim.

## Next steps, in order

1. Finish **opening-request 2.4**: complete deterministic expired-free first-opening versus existing-account recovery correspondence and map paid replay under unavailable readiness/capacity with credential/hold/funding conservation. Reuse existing explicit-time/reopened-provider evidence; do not infer complete lifecycle coverage from payment-presence tests. Complete final checks/archive only when all tasks are done, then inspect top-up and accounting-response schemas without assuming identical optional/unsigned-extension policies.
2. Finish challenge-clock non-regression (**2.4–2.5**) and exact private time/fault acceptance: offer/qualification expiry (**commercial 2.1.3.2**), native receiver 600/601-second boundaries (**owner-control 2.2.3**), acknowledgement 30-second edges (**opening 2.7**) and start-state-only persistence failure (**opening 2.5.3**). Source ownership no longer blocks fixes. No sleeps, watcher races or changed accepted timestamps.
3. Complete accepted-paid-intent/elapsed-expiry recovery (**non-reserving parent 3.3/4.3**, opening **2.3.4.3/2.8.2**) and top-up/settlement paths under both exhausted quote budgets. Preserve completed opening **2.4** and existing free/paid reopening and admitted-hold settlement; neither is every native/HTTP recovery path.
4. Finish envelope private byte/item/depth acceptance (**2.5**) and mapping/archive, preserving fixed list/data rejection (**2.6**, `30ad8fd1`). The 14,448-byte profile ceiling precludes an otherwise-valid 16,384-byte fixture; never weaken limits.
5. Continue request-accounting credential expiry during a hold (**2.2.4**), invalid-usage/budget edges (**2.5**), receipt/snapshot/sequence failures (**2.3**), gateway failure (**3.1**), performance binding (**3.2**) and resource/recovery isolation (**3.3**). Preserve completed **2.2.1–2.2.3/2.4**; ledger assertions are not engine or live-chain evidence.
6. Continue safe-credit/reorg, cancellation/top-up/fees, discovery, credential lifecycle, runtime/performance and reputation alignment. Keep incomplete changes open. The report's named-payer quota entry needs remaining acceptance and the proposed author-only factual book update, not another policy answer.

## Checkout discipline

This step owns only the owner-control regression, opening-request design/tasks and these notes. Runtime source, the author report and the whitepaper are unchanged. Preserve unrelated work; no post-edit git audit, push, stash, reset, restore, installation or deployment. Commit only verified files with per-command `APAS Worker <apas-worker@localhost>`; never archive with `--skip-specs`.
