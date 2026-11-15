# APAS progress

## 2026-11-15 — Require funding message objects

- Entry was clean at `ddec7e4f`. The complete whitepaper/report retained from the previous read are unchanged by hash; both author Answers are already acted on. The read-only book retains SHA-256 `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`. No new author decision.
- Implemented opening **2.1.2** at the wire-type boundary: `FundingIntentRequest`, `Signed<T>`, `FundingIntent` and `NetworkIdentity` now require objects. A direct map visitor preserves derived field/duplicate checks without buffering or copying through `Value`. Valid serialization/signatures stay unchanged; no array shim, caller bypass or state conversion.
- Extended the real-router request matrix with the previously accepted positional request in both fresh/cached states. Added native refusal/recovery for positional acknowledgement envelope, payload and network, preserving original fields/signature. Existing 33 malformed-member cases and signed identity/domain checks remain. Preconditions are real cached acknowledgements and restored prepared originals, not observed live admission or payment.

## Verification

- **482 passing tests:** 421 Rust across 20 suites (two pre-existing ignored), 15 frontend and 46 provisioning. Headless binary build, Svelte checking (zero errors/warnings), frontend build, CI Python compilation, strict opening-change validation and protected-book checksum passed. Opening **2.1.2/2.1.3** and their schema/binding parent **2.1** are complete; other tasks keep the change open. No check or assertion was weakened.
- Compiler warnings and stale Browserslist data remain. Native desktop/release builds are blocked by missing WebKit and the actual policy authority; external platform/hardware/deployment qualification remains outside this checkout. No installation, invented authority, deployment, live-chain or power-loss claim.

## Next steps, in order

1. Correct remaining committed commercial localhost-alias / embedding price-template mismatches (**2.1.4/2.2/2.3**). [INFERENCE] Other derived protocol structs may also admit positional arrays; compare their documented object contracts and actual decoding before extending this focused fix. Shared envelope/network guards do not imply every signed payload has object-only decoding.
2. Finish challenge-clock non-regression (**2.4–2.5**) and exact private time/fault acceptance: offer/qualification expiry (**commercial 2.1.3.2**), native receiver 600/601-second boundaries (**owner-control 2.2.3**), acknowledgement 30-second edges (**opening 2.7**) and start-state-only persistence failure (**opening 2.5.3**). Source ownership no longer blocks fixes. No sleeps, watcher races or changed accepted timestamps.
3. Complete accepted-paid-intent/elapsed-expiry recovery (**non-reserving parent 3.3/4.3**, opening **2.3.4.3/2.8.2**) and top-up/settlement paths under both exhausted quote budgets. Preserve completed opening **2.4** and existing free/paid reopening and admitted-hold settlement; neither is every native/HTTP recovery path.
4. Finish envelope private byte/item/depth acceptance (**2.5**) and mapping/archive, preserving fixed list/data rejection (**2.6**, `30ad8fd1`). The 14,448-byte profile ceiling precludes an otherwise-valid 16,384-byte fixture; never weaken limits.
5. Continue request-accounting credential expiry during a hold (**2.2.4**), invalid-usage/budget edges (**2.5**), receipt/snapshot/sequence failures (**2.3**), gateway failure (**3.1**), performance binding (**3.2**) and resource/recovery isolation (**3.3**). Preserve completed **2.2.1–2.2.3/2.4**; ledger assertions are not engine or live-chain evidence.
6. Continue safe-credit/reorg, cancellation/top-up/fees, discovery, credential lifecycle, runtime/performance and reputation alignment. Keep incomplete changes open. The report's named-payer quota entry needs remaining acceptance and the proposed author-only factual book update, not another policy answer.

## Checkout discipline

This step owns the four protocol decoders, request/native container regressions, opening-change implementation records, bug documentation and these notes. Preserve unrelated work and the whitepaper; no post-edit git audit, push, stash, reset, restore, installation or deployment. Commit only verified files with per-command `APAS Worker <apas-worker@localhost>`; never archive with `--skip-specs`.
