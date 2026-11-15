# APAS progress

## 2026-11-15 — Verify native signed-receipt snapshots

- Entry was clean at `2f9bbb51`. The previously fully read README-designated `docs/chiral-book.md` and report are unchanged; both existing Answers were already acted on. Book SHA-256: `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`. No new author-only decision or report entry is needed.
- Completed snapshot **2.3.2.1 / 3.1.2.1** with `native_signed_snapshots_preserve_terms_balances_and_receipt_watermarks`. The shared native GET mutation plan now also exercises signed receipts: original identity/nested serving terms, all eight canonical money fields, wide balanced sums, one-wei gaps and account/usage field/shape aliases.
- Term/money faults are re-signed by the fixture provider and verified before delivery; shape faults retain the original valid signature and raw duplicates. Production owner authorization and receipt issuance run first. Refusals preserve exact native saved bytes, receipt watermark, exported credential and original payment journal across reopening; provider issuance still increments. Corrected sequence gaps, replay refusal after reopening and later genuine recovery pass.
- Paid/profile/payment records remain restored prerequisites, not observed native opening/top-up, engine execution or live-chain credit. The wide signed claim is not funding proof. Independent batches fit the unchanged twelve-challenge budget. The existing GET and renewed-evidence receipt assertions remain intact; production code, base specs and whitepaper are untouched.

## Verification and blockers

- The focused signed-receipt regression and full headless run passed: 461 Rust tests across 21 suites (two existing ignored tests), including the retained account-refresh and renewed-evidence receipt regressions. Headless binary build, strict snapshot-change validation, Svelte checking (zero errors/warnings), 15 frontend tests, frontend build, 46 provisioning tests, CI Python compilation and protected book/report checks passed. Two existing Rust warnings and stale Browserslist data remain.
- Native desktop/release prerequisites remain blocked by missing WebKit and the actual policy authority; external chain/engine/platform/hardware/public-release qualification remains outside this checkout. No installation, invented authority, live payment, deployment or whitepaper edit is authorized.

## Next steps, in order

1. Finish snapshot **2.3.2.2 / 3.1.2.2** original-term/money/shape/state-preservation matrices for opening/replay and top-up. Then complete **3.2** genuine post-commit invalid-response/original-operation recovery. Reuse completed request-accounting **3.2.1** renewal receipt acceptance; do not substitute restored prerequisites for observed native funding.
2. Complete snapshot **2.4–2.5 / 3.3–3.4** lifecycle reads, exact client/export integers, usage/loss distinctions and persisted-record corruption/refusal evidence. Keep the change open; specify remaining accounting-response envelopes before changing their uncovered semantics.
3. Finish challenge-clock non-regression (**2.4–2.5**) and exact private time/fault acceptance: offer/qualification expiry (**commercial 2.1.3.2**), native receiver 600/601 seconds (**owner-control 2.2.3**), acknowledgement 30-second edges (**opening 2.7**) and start-state-only persistence failure (**opening 2.5.3**). Source ownership no longer blocks fixes; no sleeps, watcher races or changed accepted timestamps.
4. Complete accepted-paid-intent/elapsed-expiry recovery (**non-reserving 3.3/4.3**, opening **2.3.4.3/2.8.2**) and top-up/settlement under both exhausted quote budgets. Finish envelope private byte/item/depth acceptance (**2.5**) and mapping/archive; preserve fixed list/data rejection (**2.6**, `30ad8fd1`). The 14,448-byte profile ceiling cannot supply an otherwise-valid 16,384-byte fixture; never weaken limits.
5. Continue request-accounting credential expiry during a hold (**2.2.4**), invalid-usage/budget edges (**2.5**), receipt/snapshot/sequence failures (**2.3**), gateway failure (**3.1**), remaining performance binding (**3.2**) and resource/recovery isolation (**3.3**). Snapshot and ledger tests are not engine, live-chain or power-loss evidence.
6. Continue safe-credit/reorg, cancellation/top-up/fees, discovery, credential lifecycle, runtime/performance and reputation alignment. Keep incomplete changes open. The report's named-payer quota entry needs remaining acceptance and the proposed author-only factual book update, not another policy answer.

## Checkout discipline

This turn owns only `src-tauri/tests/inference_owner_control_test.rs`, snapshot change `tasks.md`/`design.md`, and these notes. Commit those four explicit files with per-command APAS identity; preserve unrelated work. No temporary file, build output, report, production source, base spec or whitepaper belongs in this commit.
