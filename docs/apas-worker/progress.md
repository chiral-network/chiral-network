# APAS progress

## 2026-11-15 — Complete and archive top-up request acceptance

- Entry was clean at `f2e4cb49`. Retained the fully read, unchanged README-designated `docs/chiral-book.md` and report: both existing Answers were already acted on. The book SHA-256 remains `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`; pending non-reserving deltas remain authoritative where not yet archived.
- Selected `specify-account-topup-request` 2.4.3.3. Extended/renamed the existing native signing-work case instead of duplicating its successful recovery. Fourteen invalid amount forms and four conflicting canonical amounts must leave exact native account bytes, complete original/cancellation journal rows, all fourteen provider tables, exported credentials and HTTP/RPC traffic unchanged.
- Covered no intent, genuinely failed preparation with no signed original, pending/unsafe settlement, lost committed reply and confirmed recovery. Reopening after an isolated unavailable RPC must retain the real persisted UUID/amount when the original is subsequently prepared. Existing positive same-original, one-broadcast/credit/fee, capacity and missing/different-journal acceptance remains; runtime policy is unchanged.

## Verification

- Strict change validation, 450 headless Rust tests across 20 suites (two existing ignored tests), headless binary build, Svelte checking (zero errors/warnings), 15 frontend tests, frontend build, 46 provisioning tests, CI Python compilation and the protected-book checksum passed.
- Completed 2.4.3.3, its recovery parents and integration tasks. `openspec archive specify-account-topup-request --yes` applied four requirements to `openspec/specs/inference-account-topup-request/spec.md` and retained the completed change under `openspec/changes/archive/2026-11-15-specify-account-topup-request/`. Replaced the generated purpose placeholder; strict base-spec validation passed. This closes request-boundary acceptance, not broader reorg/cancellation/remittance or public-release qualification.
- Compiler warnings and stale Browserslist data remain. Native desktop/release builds are blocked by missing WebKit and the actual policy authority; external platform/hardware/deployment qualification remains outside this checkout. No installation, invented authority, deployment, live-chain or power-loss claim.

## Next steps, in order

1. Compare account/receipt and other accounting-response schemas with the book and actual writers/readers. Keep this next foundation separate from the completed request boundary and retained recovery evidence.
2. Finish challenge-clock non-regression (**2.4–2.5**) and exact private time/fault acceptance: offer/qualification expiry (**commercial 2.1.3.2**), native receiver 600/601-second boundaries (**owner-control 2.2.3**), acknowledgement 30-second edges (**opening 2.7**) and start-state-only persistence failure (**opening 2.5.3**). Source ownership no longer blocks fixes. No sleeps, watcher races or changed accepted timestamps.
3. Complete accepted-paid-intent/elapsed-expiry recovery (**non-reserving parent 3.3/4.3**, opening **2.3.4.3/2.8.2**) and top-up/settlement paths under both exhausted quote budgets. Preserve completed opening **2.4** and existing free/paid reopening and admitted-hold settlement; neither is every native/HTTP recovery path.
4. Finish envelope private byte/item/depth acceptance (**2.5**) and mapping/archive, preserving fixed list/data rejection (**2.6**, `30ad8fd1`). The 14,448-byte profile ceiling precludes an otherwise-valid 16,384-byte fixture; never weaken limits.
5. Continue request-accounting credential expiry during a hold (**2.2.4**), invalid-usage/budget edges (**2.5**), receipt/snapshot/sequence failures (**2.3**), gateway failure (**3.1**), performance binding (**3.2**) and resource/recovery isolation (**3.3**). Preserve completed **2.2.1–2.2.3/2.4**; ledger assertions are not engine or live-chain evidence.
6. Continue safe-credit/reorg, cancellation/top-up/fees, discovery, credential lifecycle, runtime/performance and reputation alignment. Keep incomplete changes open. The report's named-payer quota entry needs remaining acceptance and the proposed author-only factual book update, not another policy answer.

## Checkout discipline

This step owns owner-control acceptance, this change's normal archive/spec application and these notes. Runtime, report and whitepaper are untouched. Commit only those explicit paths with per-command APAS identity; do not alter unrelated work, install prerequisites, invent a policy authority or operate outside this checkout.
