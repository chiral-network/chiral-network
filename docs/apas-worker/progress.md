# APAS progress

## 2026-11-15 — Specify the account-opening request boundary

- Entry was clean at `74b43182`. The retained complete whitepaper/report are unchanged by hash; both author Answers are already acted on, with no new decision needed. The read-only book retains SHA-256 `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`. Pending non-reserving deltas remain authoritative over historical wallet quotas in main specs.
- Created **specify-account-opening-request** with the required OpenSpec command. The new request contract specifies named-field objects, omitted/null payment semantics, parse-before-proof ordering, agreement-derived free/paid evidence rules and original-account replay. Proposal/design map inspected source and existing tests; decoder and missing HTTP/payment/recovery acceptance remain explicit unchecked tasks.
- A temporary executable observed actual `OpenRequest` decoding: omitted/null/string object payments decode; `[quote_id, null]` also decodes, but `[quote_id]`, an unknown member and a numeric payment are rejected. This is schema-only evidence, not HTTP opening or valid transaction evidence. The probe was removed. Runtime sources and permanent tests are unchanged; no positional-alias correction is claimed this turn.

## Verification

- **496 existing tests passed:** 435 Rust across 20 suites (two pre-existing ignored), 15 frontend and 46 provisioning. Headless build, Svelte checking (zero errors/warnings), frontend build, CI Python compilation, strict new-change validation and protected-book checksum passed. New-change **1.1–1.3** are complete; **2.1–2.4/3.1–3.2** remain open. Passing current tests does not implement the new contract; do not archive.
- Compiler warnings and stale Browserslist data remain. Native desktop/release builds are blocked by missing WebKit and the actual policy authority; external platform/hardware/deployment qualification remains outside this checkout. No installation, invented authority, deployment, live-chain or power-loss claim.

## Next steps, in order

1. Implement **opening-request 2.1–2.2**: shared object-only decoder and real-router malformed/optional-field regressions. Preserve object omission/null support; only the complete array was observed accepted, not the short array. Finish **2.3–2.4** payment-presence/consumption and recovery correspondence, then inspect top-up and accounting-response schemas without assuming every derived object has the same optional/unsigned-extension policy.
2. Finish challenge-clock non-regression (**2.4–2.5**) and exact private time/fault acceptance: offer/qualification expiry (**commercial 2.1.3.2**), native receiver 600/601-second boundaries (**owner-control 2.2.3**), acknowledgement 30-second edges (**opening 2.7**) and start-state-only persistence failure (**opening 2.5.3**). Source ownership no longer blocks fixes. No sleeps, watcher races or changed accepted timestamps.
3. Complete accepted-paid-intent/elapsed-expiry recovery (**non-reserving parent 3.3/4.3**, opening **2.3.4.3/2.8.2**) and top-up/settlement paths under both exhausted quote budgets. Preserve completed opening **2.4** and existing free/paid reopening and admitted-hold settlement; neither is every native/HTTP recovery path.
4. Finish envelope private byte/item/depth acceptance (**2.5**) and mapping/archive, preserving fixed list/data rejection (**2.6**, `30ad8fd1`). The 14,448-byte profile ceiling precludes an otherwise-valid 16,384-byte fixture; never weaken limits.
5. Continue request-accounting credential expiry during a hold (**2.2.4**), invalid-usage/budget edges (**2.5**), receipt/snapshot/sequence failures (**2.3**), gateway failure (**3.1**), performance binding (**3.2**) and resource/recovery isolation (**3.3**). Preserve completed **2.2.1–2.2.3/2.4**; ledger assertions are not engine or live-chain evidence.
6. Continue safe-credit/reorg, cancellation/top-up/fees, discovery, credential lifecycle, runtime/performance and reputation alignment. Keep incomplete changes open. The report's named-payer quota entry needs remaining acceptance and the proposed author-only factual book update, not another policy answer.

## Checkout discipline

This step owns only the new opening-request change artifacts and these notes; the throwaway executable is deleted and no runtime/test fix is included. Preserve unrelated work and the whitepaper; no post-edit git audit, push, stash, reset, restore, installation or deployment. Commit only verified files with per-command `APAS Worker <apas-worker@localhost>`; never archive with `--skip-specs`.
