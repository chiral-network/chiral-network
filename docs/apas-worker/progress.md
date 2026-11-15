# APAS progress

## 2026-11-15 — Require template-free managed embeddings

- Entry was clean at `dd25d6d3`. The complete whitepaper/report retained from the previous read are unchanged by hash; both author Answers are already acted on. The read-only book retains SHA-256 `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`. No new author decision.
- Implemented commercial **2.3** in `validate_inventory`, shared by native preflight/managed startup. Embeddings require explicit null template selection and no selected template file, even empty, matching the provisioner's existing manifest contract. Reuse one inventory-entry lookup; retain generation handling and all identity/byte/digest guards without rewriting artifacts or financial state.
- Added three native regressions for correctly hashed nonempty/empty templates, absent/non-null selectors, valid template-free admission, same-size model-byte corruption and configured/base empty-template-digest binding. Restoring valid bytes/metadata restores admission. Existing synthetic executable-header/model-byte fixtures launch no engine and do not prove executable GGUF content, readiness or qualification.

## Verification

- **488 passing tests:** 427 Rust across 20 suites (two pre-existing ignored), 15 frontend and 46 provisioning. Headless binary build, Svelte checking (zero errors/warnings), frontend build, CI Python compilation, strict commercial-change validation and protected-book checksum passed. Commercial **2.3** is complete; remaining schema/time and proposal/recovery acceptance keep the change open. No check or assertion was weakened.
- Compiler warnings and stale Browserslist data remain. Native desktop/release builds are blocked by missing WebKit and the actual policy authority; external platform/hardware/deployment qualification remains outside this checkout. No installation, invented authority, deployment, live-chain or power-loss claim.

## Next steps, in order

1. Inspect remaining protocol object-schema acceptance. [INFERENCE] Other derived structs may admit positional arrays; compare actual decoding with their documented object contracts before extending the funding fix. Shared envelope/network guards do not imply every signed payload is object-only. Commercial origin/embedding corrections **2.1.4/2.2/2.3** are complete.
2. Finish challenge-clock non-regression (**2.4–2.5**) and exact private time/fault acceptance: offer/qualification expiry (**commercial 2.1.3.2**), native receiver 600/601-second boundaries (**owner-control 2.2.3**), acknowledgement 30-second edges (**opening 2.7**) and start-state-only persistence failure (**opening 2.5.3**). Source ownership no longer blocks fixes. No sleeps, watcher races or changed accepted timestamps.
3. Complete accepted-paid-intent/elapsed-expiry recovery (**non-reserving parent 3.3/4.3**, opening **2.3.4.3/2.8.2**) and top-up/settlement paths under both exhausted quote budgets. Preserve completed opening **2.4** and existing free/paid reopening and admitted-hold settlement; neither is every native/HTTP recovery path.
4. Finish envelope private byte/item/depth acceptance (**2.5**) and mapping/archive, preserving fixed list/data rejection (**2.6**, `30ad8fd1`). The 14,448-byte profile ceiling precludes an otherwise-valid 16,384-byte fixture; never weaken limits.
5. Continue request-accounting credential expiry during a hold (**2.2.4**), invalid-usage/budget edges (**2.5**), receipt/snapshot/sequence failures (**2.3**), gateway failure (**3.1**), performance binding (**3.2**) and resource/recovery isolation (**3.3**). Preserve completed **2.2.1–2.2.3/2.4**; ledger assertions are not engine or live-chain evidence.
6. Continue safe-credit/reorg, cancellation/top-up/fees, discovery, credential lifecycle, runtime/performance and reputation alignment. Keep incomplete changes open. The report's named-payer quota entry needs remaining acceptance and the proposed author-only factual book update, not another policy answer.

## Checkout discipline

This step owns managed embedding-template enforcement, its native inventory regressions, commercial-change implementation records, bug documentation and these notes. Preserve unrelated work and the whitepaper; no post-edit git audit, push, stash, reset, restore, installation or deployment. Commit only verified files with per-command `APAS Worker <apas-worker@localhost>`; never archive with `--skip-specs`.
