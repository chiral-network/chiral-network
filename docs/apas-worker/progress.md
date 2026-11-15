# APAS progress

## 2026-11-15 — Accept signed receipts for renewed qualification evidence

- Entry was clean at `f0545408`. The previously fully read README-designated `docs/chiral-book.md` and report are unchanged; both existing Answers were already acted on. Book SHA-256: `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`. No new author-only decision is required.
- Confirmed a code/spec mismatch under request-accounting **Performance remedies preserve usage and admitted evidence**: reservation/settlement retain the newer report, while `Consumer::receipt` incorrectly required its digest to equal the original quote's. The new production-ledger/native-HTTP regression failed on the renewed reserved receipt with `receipt performance identity or remedy mismatch`.
- Replaced that equality check with canonical lowercase SHA-256 validation, retaining signature, original account/price, policy, remedy, charge/waiver and sequence checks. No wire shape, agreement, financial schema or provider admission rule changed. Receipts attribute the provider's report claim; they do not contain report bytes or measurement timestamps and cannot independently certify renewal qualification or execution.
- Completed request-accounting task **3.2.1**. Its regression passes real ledger/router receipts across original, renewed reserved, known, waived and recovered-unknown states; six hostile responses preserve native account bytes and credentials across reopening, with a valid sequence-gap recovery afterward. Original terms, fees, payment journal and old withdrawal remain intact. Funding, synthetic profiles, supplied usage and logical ledger times are test prerequisites, not chain/engine/hardware evidence.

## Verification

- The new regression reproduced the old native rejection, then passed after correction. All 452 headless Rust tests across 20 suites (two existing ignored tests), headless binary build, strict request-accounting validation, Svelte checking (zero errors/warnings), 15 frontend tests, frontend build, 46 provisioning tests, CI Python compilation and protected book/report checks passed. Two existing Rust warnings and stale Browserslist data remain.
- Keep the request-accounting change open; parent **3.2**, gateway inventory, receipt boundary/failure acceptance and other tasks remain incomplete. Native desktop/release prerequisites remain blocked by missing WebKit and the actual policy authority. External qualification/deployment remains outside this checkout; no installation, invented authority, real payment or deployment is authorized.

## Next steps, in order

1. Continue accounting-response alignment: specify uncovered account/receipt and other response schemas after comparing their actual writers/readers. Preserve original agreements and the distinction between provider-attributed report identity and independently verified qualification; do not delete undocumented behavior.
2. Finish challenge-clock non-regression (**2.4–2.5**) and exact private time/fault acceptance: offer/qualification expiry (**commercial 2.1.3.2**), native receiver 600/601-second boundaries (**owner-control 2.2.3**), acknowledgement 30-second edges (**opening 2.7**) and start-state-only persistence failure (**opening 2.5.3**). Source ownership no longer blocks fixes. No sleeps, watcher races or changed accepted timestamps.
3. Complete accepted-paid-intent/elapsed-expiry recovery (**non-reserving parent 3.3/4.3**, opening **2.3.4.3/2.8.2**) and top-up/settlement paths under both exhausted quote budgets. Preserve completed opening **2.4** and existing free/paid reopening and admitted-hold settlement; neither is every native/HTTP recovery path.
4. Finish envelope private byte/item/depth acceptance (**2.5**) and mapping/archive, preserving fixed list/data rejection (**2.6**, `30ad8fd1`). The 14,448-byte profile ceiling precludes an otherwise-valid 16,384-byte fixture; never weaken limits.
5. Continue request-accounting credential expiry during a hold (**2.2.4**), invalid-usage/budget edges (**2.5**), receipt/snapshot/sequence failures (**2.3**), gateway failure (**3.1**), remaining performance binding (**3.2**) and resource/recovery isolation (**3.3**). Ledger/receipt assertions are not engine or live-chain evidence.
6. Continue safe-credit/reorg, cancellation/top-up/fees, discovery, credential lifecycle, runtime/performance and reputation alignment. Keep incomplete changes open. The report's named-payer quota entry needs remaining acceptance and the proposed author-only factual book update, not another policy answer.

## Checkout discipline

This step owns `src-tauri/src/consumer.rs`, its regression in `src-tauri/tests/inference_owner_control_test.rs`, the request-accounting design/tasks, the corresponding `docs/bugs.md` entry and these notes. Spec requirements, report and whitepaper remain untouched. Commit only those six explicit files with per-command APAS identity; preserve unrelated work and all external-operation restrictions.
