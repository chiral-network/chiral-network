# APAS progress

## 2026-11-15 — Require price-schedule JSON objects

- Entry was clean at `eee9a99b`. The complete whitepaper/report retained from the previous read are unchanged by hash; both author Answers are already acted on. The read-only book retains SHA-256 `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`. No new author decision.
- Implemented canonical `inference-wire-foundation` requirement **Strict primitive schemas and canonical wallet identities** for `PriceSchedule`. Reuse the existing object-only visitor and derived field checks; do not buffer through `Value`, change monetary validation, alter serialization or migrate financial state.
- Both new regressions failed before the fix. They now reject positional price arrays from bytes/values and inside signed quotes, retain raw duplicate-member rejection, and preserve independent signature/opening-data vectors for both valid object member orders. This does not establish duplicate rejection after an upstream `Value` parse or qualify live HTTP/payment paths.

## Verification

- **490 passing tests:** 429 Rust across 20 suites (two pre-existing ignored), 15 frontend and 46 provisioning. Headless binary build, Svelte checking (zero errors/warnings), frontend build, CI Python compilation, strict canonical wire-spec validation and protected-book checksum passed. No check or assertion was weakened; no spec change or archive was needed for this existing requirement.
- Compiler warnings and stale Browserslist data remain. Native desktop/release builds are blocked by missing WebKit and the actual policy authority; external platform/hardware/deployment qualification remains outside this checkout. No installation, invented authority, deployment, live-chain or power-loss claim.

## Next steps, in order

1. Inspect remaining profile/model and enclosing payload object schemas. [INFERENCE] Other derived structs may admit positional arrays; compare actual decoding with their documented object contracts before extending the guard. Price, envelope, network and funding guards do not establish that every signed payload is object-only or every raw path rejects duplicates. Commercial origin/embedding corrections **2.1.4/2.2/2.3** remain complete.
2. Finish challenge-clock non-regression (**2.4–2.5**) and exact private time/fault acceptance: offer/qualification expiry (**commercial 2.1.3.2**), native receiver 600/601-second boundaries (**owner-control 2.2.3**), acknowledgement 30-second edges (**opening 2.7**) and start-state-only persistence failure (**opening 2.5.3**). Source ownership no longer blocks fixes. No sleeps, watcher races or changed accepted timestamps.
3. Complete accepted-paid-intent/elapsed-expiry recovery (**non-reserving parent 3.3/4.3**, opening **2.3.4.3/2.8.2**) and top-up/settlement paths under both exhausted quote budgets. Preserve completed opening **2.4** and existing free/paid reopening and admitted-hold settlement; neither is every native/HTTP recovery path.
4. Finish envelope private byte/item/depth acceptance (**2.5**) and mapping/archive, preserving fixed list/data rejection (**2.6**, `30ad8fd1`). The 14,448-byte profile ceiling precludes an otherwise-valid 16,384-byte fixture; never weaken limits.
5. Continue request-accounting credential expiry during a hold (**2.2.4**), invalid-usage/budget edges (**2.5**), receipt/snapshot/sequence failures (**2.3**), gateway failure (**3.1**), performance binding (**3.2**) and resource/recovery isolation (**3.3**). Preserve completed **2.2.1–2.2.3/2.4**; ledger assertions are not engine or live-chain evidence.
6. Continue safe-credit/reorg, cancellation/top-up/fees, discovery, credential lifecycle, runtime/performance and reputation alignment. Keep incomplete changes open. The report's named-payer quota entry needs remaining acceptance and the proposed author-only factual book update, not another policy answer.

## Checkout discipline

This step owns the shared price-object decoder, its two native wire regressions, bug documentation and these notes. Preserve unrelated work and the whitepaper; no post-edit git audit, push, stash, reset, restore, installation or deployment. Commit only verified files with per-command `APAS Worker <apas-worker@localhost>`; never archive with `--skip-specs`.
