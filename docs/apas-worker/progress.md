# APAS progress

## 2026-11-15 — Enforce native input-only embedding prices

- Entry was clean at `bb0cc078`. The complete whitepaper/report retained from the previous read are unchanged by hash; both author Answers are already acted on. The read-only book retains SHA-256 `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`. No new author decision.
- Implemented commercial **2.2** in `ProviderConfig::validate`, shared by native loading/startup. Reuse the existing canonical integer parser's result, requiring zero output price for every embedding-capable model; no normalization, generation-price restriction or financial-state migration.
- Added two regressions covering direct validation and actual file loading: public/development and managed/unmanaged configurations, free/paid input, one-wei/maximal nonzero output, noncanonical zero spellings, model/capability order and priced generation. Synthetic paths/settings are configuration evidence only, not verified inventories, startup/readiness or qualification.

## Verification

- **485 passing tests:** 424 Rust across 20 suites (two pre-existing ignored), 15 frontend and 46 provisioning. Headless binary build, Svelte checking (zero errors/warnings), frontend build, CI Python compilation, strict commercial-change validation and protected-book checksum passed. Commercial **2.2** is complete; template enforcement and other incomplete tasks keep the change open. No check or assertion was weakened.
- Compiler warnings and stale Browserslist data remain. Native desktop/release builds are blocked by missing WebKit and the actual policy authority; external platform/hardware/deployment qualification remains outside this checkout. No installation, invented authority, deployment, live-chain or power-loss claim.

## Next steps, in order

1. Correct managed embedding inventory/template acceptance (**commercial 2.3**); price task **2.2** and localhost-alias task **2.1.4** are complete. [INFERENCE] Other derived protocol structs may also admit positional arrays; compare their documented object contracts and actual decoding before extending the prior funding fix. Shared envelope/network guards do not imply every signed payload has object-only decoding.
2. Finish challenge-clock non-regression (**2.4–2.5**) and exact private time/fault acceptance: offer/qualification expiry (**commercial 2.1.3.2**), native receiver 600/601-second boundaries (**owner-control 2.2.3**), acknowledgement 30-second edges (**opening 2.7**) and start-state-only persistence failure (**opening 2.5.3**). Source ownership no longer blocks fixes. No sleeps, watcher races or changed accepted timestamps.
3. Complete accepted-paid-intent/elapsed-expiry recovery (**non-reserving parent 3.3/4.3**, opening **2.3.4.3/2.8.2**) and top-up/settlement paths under both exhausted quote budgets. Preserve completed opening **2.4** and existing free/paid reopening and admitted-hold settlement; neither is every native/HTTP recovery path.
4. Finish envelope private byte/item/depth acceptance (**2.5**) and mapping/archive, preserving fixed list/data rejection (**2.6**, `30ad8fd1`). The 14,448-byte profile ceiling precludes an otherwise-valid 16,384-byte fixture; never weaken limits.
5. Continue request-accounting credential expiry during a hold (**2.2.4**), invalid-usage/budget edges (**2.5**), receipt/snapshot/sequence failures (**2.3**), gateway failure (**3.1**), performance binding (**3.2**) and resource/recovery isolation (**3.3**). Preserve completed **2.2.1–2.2.3/2.4**; ledger assertions are not engine or live-chain evidence.
6. Continue safe-credit/reorg, cancellation/top-up/fees, discovery, credential lifecycle, runtime/performance and reputation alignment. Keep incomplete changes open. The report's named-payer quota entry needs remaining acceptance and the proposed author-only factual book update, not another policy answer.

## Checkout discipline

This step owns native embedding-price validation, its configuration/file-loading regressions, commercial-change implementation records, bug documentation and these notes. Preserve unrelated work and the whitepaper; no post-edit git audit, push, stash, reset, restore, installation or deployment. Commit only verified files with per-command `APAS Worker <apas-worker@localhost>`; never archive with `--skip-specs`.
