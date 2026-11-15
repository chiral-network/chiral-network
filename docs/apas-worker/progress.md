# APAS progress

## 2026-11-15 — Enforce the top-up object and authority boundary

- Entry was clean at `6c192e2d`. The fully read whitepaper and author report retain their recorded hashes; both Answers are already acted on and no new author decision is needed. The read-only book retains SHA-256 `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`. Pending non-reserving deltas remain authoritative over historical wallet quotas in main specs.
- Selected **specify-account-topup-request 2.1–2.2**. `TopupRequest` now uses the shared direct object visitor and strict field decoder, preserving its required string, serializer and native callers without a `Value` round-trip or compatibility alias. Route authorization ordering and payment/credit logic are unchanged.
- Added production-router schema and authority regressions: sixteen malformed/root/member cases on free and paid accounts, exact consumed-proof-only ledger changes, a retained unrelated read proof, owner/bearer/missing-account precedence, invalid route syntax, and both body-whitespace substitution directions plus account/query target substitution. Object controls reach free eligibility refusal or actual unavailable paid verification without credit. Post-verification paid setup and the unavailable RPC observer are not live-chain or successful top-up evidence.

## Verification

- All **501 available tests passed**: 440 locked headless Rust all-target tests (two existing tests ignored), 15 frontend and 46 provisioning tests. Locked headless binary build, frontend build, Svelte checking (zero errors/warnings), CI Python compilation, strict non-interactive change validation and protected-book checksum passed. **2.1–2.2** are complete; **2.3–2.4/3.1–3.2** remain open. Do not archive or mistake schema/authority acceptance for complete settlement/resource qualification.
- Compiler warnings and stale Browserslist data remain. Native desktop/release builds are blocked by missing WebKit and the actual policy authority; external platform/hardware/deployment qualification remains outside this checkout. No installation, invented authority, deployment, live-chain or power-loss claim.

## Next steps, in order

1. Finish **top-up request 2.3–2.4**: inventory remaining free/frozen/empty/zero-value and commitment edges, reuse safe-credit/native original/lost-response evidence, and complete independent quote-work/committed-issuance/opening-capacity recovery correspondence. Preserve completed object/owner-order coverage and exact raw binding; add only missing consumer-visible cases. Archive only after all tasks/checks pass; accounting-response schemas remain the next distinct foundation.
2. Finish challenge-clock non-regression (**2.4–2.5**) and exact private time/fault acceptance: offer/qualification expiry (**commercial 2.1.3.2**), native receiver 600/601-second boundaries (**owner-control 2.2.3**), acknowledgement 30-second edges (**opening 2.7**) and start-state-only persistence failure (**opening 2.5.3**). Source ownership no longer blocks fixes. No sleeps, watcher races or changed accepted timestamps.
3. Complete accepted-paid-intent/elapsed-expiry recovery (**non-reserving parent 3.3/4.3**, opening **2.3.4.3/2.8.2**) and top-up/settlement paths under both exhausted quote budgets. Preserve completed opening **2.4** and existing free/paid reopening and admitted-hold settlement; neither is every native/HTTP recovery path.
4. Finish envelope private byte/item/depth acceptance (**2.5**) and mapping/archive, preserving fixed list/data rejection (**2.6**, `30ad8fd1`). The 14,448-byte profile ceiling precludes an otherwise-valid 16,384-byte fixture; never weaken limits.
5. Continue request-accounting credential expiry during a hold (**2.2.4**), invalid-usage/budget edges (**2.5**), receipt/snapshot/sequence failures (**2.3**), gateway failure (**3.1**), performance binding (**3.2**) and resource/recovery isolation (**3.3**). Preserve completed **2.2.1–2.2.3/2.4**; ledger assertions are not engine or live-chain evidence.
6. Continue safe-credit/reorg, cancellation/top-up/fees, discovery, credential lifecycle, runtime/performance and reputation alignment. Keep incomplete changes open. The report's named-payer quota entry needs remaining acceptance and the proposed author-only factual book update, not another policy answer.

## Checkout discipline

This step owns only `protocol.rs`, the owner-control regression, top-up request design/tasks and these notes. The author report and whitepaper remain untouched; no temporary probes are retained. Preserve unrelated work; no post-edit git audit, push, stash, reset, restore, installation or deployment. Commit only verified files with per-command `APAS Worker <apas-worker@localhost>`.
