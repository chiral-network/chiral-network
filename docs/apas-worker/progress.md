# APAS progress

## 2026-11-15 — Enforce the shared account snapshot decoder

- Entry was clean at `9ba4c56f`. The previously fully read README-designated `docs/chiral-book.md` and report are unchanged; both existing Answers were already acted on. Book SHA-256: `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`. No new author-only decision or report entry is needed.
- Completed **`specify-inference-account-snapshot` 2.1–2.2**. `AccountView` and `TokenUsage` now require named objects through the existing direct visitor; account additions and positional aliases are rejected. Closed field checks retain required members and exact integer widths without a `Value` buffer, default, alias, serialization change or monetary-state migration.
- Added seven deterministic decoder regressions: raw/pre-parsed objects, positional/non-object rejection, all required members missing/null/wrong-type, unknown/raw duplicate members, exact counters through `u64::MAX`, invalid numeric forms, unchanged canonical bytes and genuine provider-signature controls. These directly refuse the projections observed before this turn; no repeated pre-fix probe was necessary.
- Updated the change's tasks/evidence map and bug register. Keep the change open: native containing-response/state-preservation, persisted-record and client/export acceptance remain incomplete. Decoder/signature evidence does not establish live funding, inference, hardware performance or full native acceptance.

## Verification and blockers

- Seven focused snapshot regressions and the full 459-test headless run across 21 suites passed (two existing ignored tests). Headless binary build, strict snapshot-change validation, Svelte checking (zero errors/warnings), 15 frontend tests, frontend build, 46 provisioning tests, CI Python compilation and protected book/report checks passed. Two existing Rust warnings and stale Browserslist data remain.
- LSP reference lookup found no Rust language server; source inspection and compiler/test checks cover this slice. Native desktop/release prerequisites remain blocked by missing WebKit and the actual policy authority; external chain/engine/platform/hardware/public-release qualification remains outside this checkout. No installation, invented authority, live payment, deployment or whitepaper edit is authorized.

## Next steps, in order

1. Complete snapshot **2.3–2.5, 3.1–3.4**: full original-term/money/state-preservation matrix, lifecycle reads, exact client/export integers, containing native replies, persisted records and post-commit original-operation recovery. Reuse completed request-accounting **3.2.1** renewal receipt acceptance. Specify remaining accounting-response envelopes before changing their uncovered semantics.
2. Finish challenge-clock non-regression (**2.4–2.5**) and exact private time/fault acceptance: offer/qualification expiry (**commercial 2.1.3.2**), native receiver 600/601 seconds (**owner-control 2.2.3**), acknowledgement 30-second edges (**opening 2.7**) and start-state-only persistence failure (**opening 2.5.3**). Source ownership no longer blocks fixes; no sleeps, watcher races or changed accepted timestamps.
3. Complete accepted-paid-intent/elapsed-expiry recovery (**non-reserving 3.3/4.3**, opening **2.3.4.3/2.8.2**) and top-up/settlement under both exhausted quote budgets. Finish envelope private byte/item/depth acceptance (**2.5**) and mapping/archive; preserve fixed list/data rejection (**2.6**, `30ad8fd1`). The 14,448-byte profile ceiling cannot supply an otherwise-valid 16,384-byte fixture; never weaken limits.
4. Continue request-accounting credential expiry during a hold (**2.2.4**), invalid-usage/budget edges (**2.5**), receipt/snapshot/sequence failures (**2.3**), gateway failure (**3.1**), remaining performance binding (**3.2**) and resource/recovery isolation (**3.3**). Snapshot and ledger tests are not engine, live-chain or power-loss evidence.
5. Continue safe-credit/reorg, cancellation/top-up/fees, discovery, credential lifecycle, runtime/performance and reputation alignment. Keep incomplete changes open. The report's named-payer quota entry needs remaining acceptance and the proposed author-only factual book update, not another policy answer.

## Checkout discipline

This turn owns `src-tauri/src/protocol.rs`, the new `src-tauri/tests/inference_account_snapshot_test.rs`, snapshot change `tasks.md`/`design.md`, `docs/bugs.md` and these notes. Commit those six explicit files with per-command APAS identity; preserve unrelated work. No temporary file, build output, report, base spec or whitepaper belongs in this commit.
