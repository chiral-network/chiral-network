# APAS progress

## 2026-11-15 — Reject noncanonical request exposure in signed receipts

- Entry was clean at `bacf93f3`. Read the README-designated `docs/chiral-book.md` in full and the current report; both existing Answers were already acted on. The unchanged book SHA-256 is `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`. No new author-only decision or report amendment is needed.
- Comparing accounting readers with `inference-wire-foundation` found a higher-priority implementation violation of **Exact monetary representation and CFX conversion**: `Consumer::receipt` validated request charges, maximum holds and waivers, but omitted `RequestView.provider_loss_wei`. The new native HTTP regression reproduced acceptance of an empty exposure string in a validly signed receipt.
- Added the existing canonical-money validator before receipt-sequence persistence. The regression covers fourteen invalid string forms in the second request, byte-for-byte saved-state and credential preservation, reopening, valid zero/nonzero exposure controls, same-sequence correction and durable replay refusal. Its existing loopback counterparty supplies signed accounting claims and synthetic funding responses; this is not engine-usage, production-ledger, live-chain or qualification evidence. No wire shape, monetary state migration or provider accounting policy changes.

## Verification

- The targeted regression failed before correction with `accepted exposure ""`. After correction, 451 headless Rust tests across 20 suites (two existing ignored tests), the headless binary build, strict foundation-spec validation, Svelte checking (zero errors/warnings), 15 frontend tests, frontend build, 46 provisioning tests, CI Python compilation and the protected-book checksum passed.
- This implementation step uses an existing base requirement, not a new OpenSpec change. The completed top-up request change remains archived; request-accounting and other incomplete changes remain open.
- Compiler warnings and stale Browserslist data remain. Native desktop/release builds are blocked by missing WebKit and the actual policy authority; external platform/hardware/deployment qualification remains outside this checkout. No installation, invented authority, deployment, live-chain or power-loss claim.

## Next steps, in order

1. Continue accounting-response alignment. Inspect evidence-only renewal first: the native receipt reader currently compares performance evidence to the original quote's digest; map the renewed-report writer and retained signed metadata before choosing the next correction. Then specify uncovered account/receipt and other response schemas, preserving existing writers/readers rather than deleting undocumented behavior.
2. Finish challenge-clock non-regression (**2.4–2.5**) and exact private time/fault acceptance: offer/qualification expiry (**commercial 2.1.3.2**), native receiver 600/601-second boundaries (**owner-control 2.2.3**), acknowledgement 30-second edges (**opening 2.7**) and start-state-only persistence failure (**opening 2.5.3**). Source ownership no longer blocks fixes. No sleeps, watcher races or changed accepted timestamps.
3. Complete accepted-paid-intent/elapsed-expiry recovery (**non-reserving parent 3.3/4.3**, opening **2.3.4.3/2.8.2**) and top-up/settlement paths under both exhausted quote budgets. Preserve completed opening **2.4** and existing free/paid reopening and admitted-hold settlement; neither is every native/HTTP recovery path.
4. Finish envelope private byte/item/depth acceptance (**2.5**) and mapping/archive, preserving fixed list/data rejection (**2.6**, `30ad8fd1`). The 14,448-byte profile ceiling precludes an otherwise-valid 16,384-byte fixture; never weaken limits.
5. Continue request-accounting credential expiry during a hold (**2.2.4**), invalid-usage/budget edges (**2.5**), receipt/snapshot/sequence failures (**2.3**), gateway failure (**3.1**), performance binding (**3.2**) and resource/recovery isolation (**3.3**). Preserve completed **2.2.1–2.2.3/2.4**; ledger assertions are not engine or live-chain evidence.
6. Continue safe-credit/reorg, cancellation/top-up/fees, discovery, credential lifecycle, runtime/performance and reputation alignment. Keep incomplete changes open. The report's named-payer quota entry needs remaining acceptance and the proposed author-only factual book update, not another policy answer.

## Checkout discipline

This step owns `src-tauri/src/consumer.rs`, the corresponding fixed-defect entry in `docs/bugs.md`, and these notes. OpenSpec, report and whitepaper are untouched. Commit only those explicit files with per-command APAS identity; do not alter unrelated work, install prerequisites, invent a policy authority or operate outside this checkout.
