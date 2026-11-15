## 1. Establish the missing snapshot contract

- [x] 1.1 Compare the read-only book, existing/pending specifications and actual account/usage producers, persistence and native acceptance paths; retain existing status and reversal semantics rather than inventing a new accounting model.
- [x] 1.2 Specify the complete account object, exact accounting counters, immutable agreement binding, wide monetary conservation and status/attribution limits; separate OpenAI usage and full receipt-envelope work.
- [x] 1.3 The temporary executable observed extra account members, positional accounts and positional usage accepted under the unchanged typed receipt signature; the named control passed and extra usage members were refused. Removed the probe. Strict validation, 452 Rust tests, 15 frontend tests, 46 provisioning tests, available builds/checks and protected-document checks passed. Production/permanent tests remain unchanged; the implementation and native-boundary tasks below remain open.

## 2. Enforce the shared payload without changing serialization

- [x] 2.1 `AccountView` and `TokenUsage` reuse the existing direct object visitor with closed derived field decoders. Account additions and positional aliases are refused without a `Value` buffer or fallback/default. Existing serialization, nested types, integer widths and monetary representation are unchanged; no state migration was added.
- [x] 2.2 Seven deterministic regressions in `inference_account_snapshot_test.rs` pass raw/pre-parsed named objects, reordered fields, non-object/positional refusals, every required member's omission/null/wrong types, unknown and raw duplicate members, exact `u64` boundaries and non-integer refusals. Declared canonical bytes and valid receipt signatures remain intact; account/usage projections carrying the unchanged signature are rejected.
- [ ] 2.3 Map and fill native account acceptance for every immutable identity/term field, all eight money fields, wide balanced sums and one-wei mismatches, preserving account bytes, credentials and original payment evidence on refusal. Reuse existing financial/receipt tests where their assertions suffice.
  - [x] 2.3.1 `native_account_refresh_preserves_original_terms_money_and_recovery_state` passes production-route native account GET acceptance: every original identity/model/profile/price/concurrency member, all eight canonical money fields, exact `2*u128::MAX` balance controls and one-wei mismatches. Refusals preserve saved bytes, a nonzero receipt watermark, exported credentials, original payment journal and provider financial rows across native reopening.
  - [ ] 2.3.2 Complete equivalent original-term/money/state-preservation acceptance for opening/replay, top-up responses and signed receipts; retain renewed per-request evidence without rewriting the original account.
- [ ] 2.4 Exercise actual owner/read-authorized provider snapshots for free, active, settlement-pending and frozen accounts. Retain unfamiliar status strings without treating them as active/free authority; do not create a new chain transition policy or substitute RPC unavailability for an orphan.
- [ ] 2.5 Check actual CLI/native/desktop receipt and account presentation/export boundaries for exact large counters and wei. Repair any lossy signed-statement path without narrowing the wire range; verify each changed surface and preserve native secret isolation.

## 3. Verify containing responses and durable recovery

- [ ] 3.1 Exercise account objects inside native opening/replay, direct account reads, top-up replies and signed receipts, including non-object/missing/extra fields and nested usage aliases. Establish that refusal reaches the intended decoder rather than unrelated auth/rate failure; retain named-object success controls.
  - [x] 3.1.1 Native direct-account GET covers null/scalar/positional accounts, all required account members missing/null/wrong-type/raw-duplicated, extra account members and missing/extra/positional/raw-duplicated usage. Each response follows successful production owner authorization; delivered-response checks exclude unrelated auth/rate failures. Genuine and wide named-object success controls bracket refusals without resetting budgets or timestamps.
  - [ ] 3.1.2 Complete the containing opening/replay, top-up and signed-receipt shape matrices, preserving valid signature controls and genuine post-commit outcomes where applicable.
- [ ] 3.2 Exercise invalid account replies after a genuine committed opening/top-up, then reopen and recover the exact original. Preserve accepted balances, original journal/acknowledgement evidence and exported credentials without a second payment or duplicate credit. Distinguish restored prerequisites, controlled HTTP faults and real operations.
- [ ] 3.3 Retain request-accounting 3.2.1's renewed-report receipt coverage and distinguish original account terms, known waived usage, unknown request exposure and account funding-loss compensation. Reuse its completed regression rather than rewriting its report binding.
- [ ] 3.4 Confirm the stricter decoder reads current genuine persisted schema-3 account/native records without changing their money representation. Corrupt/unsupported records fail closed and remain intact; no automatic legacy migration, reset or conversion.

## 4. Integrate verified behavior

- [ ] 4.1 Update requirement/source/test correspondence and operator/client documentation with observed results and remaining release limits; leave the whitepaper unchanged.
- [ ] 4.2 Pass strict validation and required available project checks after implementation. Archive with `openspec archive specify-inference-account-snapshot --yes` only when every in-scope task is complete; never use `--skip-specs`.

No deployment, live payment, account/server administration, installation, hardware qualification, whitepaper edit or unrelated source adoption is authorized. Full `UsageReceipt`, `RequestView`, `PerformanceOutcome`, `OpenResult` and credential-envelope field schemas remain separate specification work; this change covers their shared account/usage payloads, not replacement wrappers.
