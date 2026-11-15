# APAS progress

## 2026-11-15 — Specify the top-up request boundary

- Entry was clean at `6b930986`. Read the whitepaper and author report; both Answers are already acted on and no new author decision is needed. The read-only book retains SHA-256 `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`. Pending non-reserving deltas remain authoritative over historical wallet quotas in main specs.
- Selected the missing **top-up request specification**, before changing its decoder. Created `specify-account-topup-request` with proposal, four requirements, source/acceptance design and bounded implementation tasks. It defines a required string in an object, literal route account selection, current owner-before-decoding ordering, active-paid/safe-credit boundaries and original-preserving recovery without new admission. Accounting-response schemas remain separate.
- A temporary executable using the actual `TopupRequest` confirmed that a complete positional array decodes and serializes as an object. Required-string objects, including empty strings, decode; missing/null/numeric/unknown/duplicate members and a scalar root fail. Removed the probe before checks. This is typed-schema evidence, not an HTTP bypass, payment acceptance or runtime fix; production code and permanent tests are unchanged.

## Verification

- **499 tests passed:** all 438 Rust tests (two pre-existing ignored), 15 frontend and 46 provisioning tests. Strict change validation, headless/frontend builds, Svelte checking (zero errors/warnings), CI Python compilation and protected-book checksum passed. Task **1.4** is complete; implementation/acceptance tasks **2.1–2.4/3.1–3.2** stay open. The current suite does not enforce the newly specified object-only top-up shape; do not archive this specification-only step.
- Compiler warnings and stale Browserslist data remain. Native desktop/release builds are blocked by missing WebKit and the actual policy authority; external platform/hardware/deployment qualification remains outside this checkout. No installation, invented authority, deployment, live-chain or power-loss claim.

## Next steps, in order

1. Implement **top-up request 2.1–2.2** using the shared object visitor and genuine production-router regressions. Preserve its owner-before-decoding ordering, consumed authenticated malformed proofs and unconsumed binding failures; do not copy opening's parse-first behavior. Then finish **2.3–2.4** by reusing existing safe-credit/native lost-response evidence and adding only missing eligibility/resource cases. Archive only after all tasks/checks pass; accounting-response schemas remain the next distinct foundation.
2. Finish challenge-clock non-regression (**2.4–2.5**) and exact private time/fault acceptance: offer/qualification expiry (**commercial 2.1.3.2**), native receiver 600/601-second boundaries (**owner-control 2.2.3**), acknowledgement 30-second edges (**opening 2.7**) and start-state-only persistence failure (**opening 2.5.3**). Source ownership no longer blocks fixes. No sleeps, watcher races or changed accepted timestamps.
3. Complete accepted-paid-intent/elapsed-expiry recovery (**non-reserving parent 3.3/4.3**, opening **2.3.4.3/2.8.2**) and top-up/settlement paths under both exhausted quote budgets. Preserve completed opening **2.4** and existing free/paid reopening and admitted-hold settlement; neither is every native/HTTP recovery path.
4. Finish envelope private byte/item/depth acceptance (**2.5**) and mapping/archive, preserving fixed list/data rejection (**2.6**, `30ad8fd1`). The 14,448-byte profile ceiling precludes an otherwise-valid 16,384-byte fixture; never weaken limits.
5. Continue request-accounting credential expiry during a hold (**2.2.4**), invalid-usage/budget edges (**2.5**), receipt/snapshot/sequence failures (**2.3**), gateway failure (**3.1**), performance binding (**3.2**) and resource/recovery isolation (**3.3**). Preserve completed **2.2.1–2.2.3/2.4**; ledger assertions are not engine or live-chain evidence.
6. Continue safe-credit/reorg, cancellation/top-up/fees, discovery, credential lifecycle, runtime/performance and reputation alignment. Keep incomplete changes open. The report's named-payer quota entry needs remaining acceptance and the proposed author-only factual book update, not another policy answer.

## Checkout discipline

This step owns only `openspec/changes/specify-account-topup-request/` and these notes. Runtime source, permanent tests, the author report and the whitepaper are unchanged; the temporary probe is deleted. Preserve unrelated work; no post-edit git audit, push, stash, reset, restore, installation or deployment. Commit only verified files with per-command `APAS Worker <apas-worker@localhost>`.
