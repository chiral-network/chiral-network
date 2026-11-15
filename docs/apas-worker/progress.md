# APAS progress

## 2026-11-15 — Require serving-profile leaf objects

- Entry was clean at `bc14a7ef`. The complete whitepaper/report retained from the previous read are unchanged by hash; both author Answers are already acted on. The read-only book retains SHA-256 `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`. No new author decision.
- Implemented commercial **2.1.5**, under **Complete serving profile identity**: `EngineDevice` and `QualificationIdentity` now use the shared object-only visitor. Retain exact field types, unknown/missing/raw duplicate-member refusal, serialization and existing model/evidence comparisons; no state conversion or new semantic validation.
- Both new regressions failed before the fix. They now reject positional identities directly and inside raw/preparsed offers, including both device positions, and retain raw duplicate-member refusal for every leaf field. Existing model/configuration, integer-width, signature and renewal coverage remains intact. This does not qualify live admission/hardware or duplicate detection after an upstream `Value` parse.

## Verification

- **492 passing tests:** 431 Rust across 20 suites (two pre-existing ignored), 15 frontend and 46 provisioning. Headless binary build, Svelte checking (zero errors/warnings), frontend build, CI Python compilation, strict commercial-change validation and protected-book checksum passed. No check or assertion was weakened. Remaining schema/time and proposal/recovery tasks keep the change open.
- Compiler warnings and stale Browserslist data remain. Native desktop/release builds are blocked by missing WebKit and the actual policy authority; external platform/hardware/deployment qualification remains outside this checkout. No installation, invented authority, deployment, live-chain or power-loss claim.

## Next steps, in order

1. Complete enclosing model/profile/offer/quote object-versus-sequence acceptance (**commercial 2.1.6**). [INFERENCE] Other derived structs may admit positional arrays; inspect actual decoding before claiming each boundary closed. Device/qualification, price, envelope, network and funding guards do not establish that every signed payload is object-only. Preserve completed commercial origin/embedding corrections **2.1.4/2.2/2.3**.
2. Finish challenge-clock non-regression (**2.4–2.5**) and exact private time/fault acceptance: offer/qualification expiry (**commercial 2.1.3.2**), native receiver 600/601-second boundaries (**owner-control 2.2.3**), acknowledgement 30-second edges (**opening 2.7**) and start-state-only persistence failure (**opening 2.5.3**). Source ownership no longer blocks fixes. No sleeps, watcher races or changed accepted timestamps.
3. Complete accepted-paid-intent/elapsed-expiry recovery (**non-reserving parent 3.3/4.3**, opening **2.3.4.3/2.8.2**) and top-up/settlement paths under both exhausted quote budgets. Preserve completed opening **2.4** and existing free/paid reopening and admitted-hold settlement; neither is every native/HTTP recovery path.
4. Finish envelope private byte/item/depth acceptance (**2.5**) and mapping/archive, preserving fixed list/data rejection (**2.6**, `30ad8fd1`). The 14,448-byte profile ceiling precludes an otherwise-valid 16,384-byte fixture; never weaken limits.
5. Continue request-accounting credential expiry during a hold (**2.2.4**), invalid-usage/budget edges (**2.5**), receipt/snapshot/sequence failures (**2.3**), gateway failure (**3.1**), performance binding (**3.2**) and resource/recovery isolation (**3.3**). Preserve completed **2.2.1–2.2.3/2.4**; ledger assertions are not engine or live-chain evidence.
6. Continue safe-credit/reorg, cancellation/top-up/fees, discovery, credential lifecycle, runtime/performance and reputation alignment. Keep incomplete changes open. The report's named-payer quota entry needs remaining acceptance and the proposed author-only factual book update, not another policy answer.

## Checkout discipline

This step owns the two profile-leaf object decoders, their native wire regressions, commercial-change implementation records, bug documentation and these notes. Preserve unrelated work and the whitepaper; no post-edit git audit, push, stash, reset, restore, installation or deployment. Commit only verified files with per-command `APAS Worker <apas-worker@localhost>`; never archive with `--skip-specs`.
