## 1. Establish the missing snapshot contract

- [x] 1.1 Compare the read-only book, existing/pending specifications and actual account/usage producers, persistence and native acceptance paths; retain existing status and reversal semantics rather than inventing a new accounting model.
- [x] 1.2 Specify the complete account object, exact accounting counters, immutable agreement binding, wide monetary conservation and status/attribution limits; separate OpenAI usage and full receipt-envelope work.
- [x] 1.3 The temporary executable observed extra account members, positional accounts and positional usage accepted under the unchanged typed receipt signature; the named control passed and extra usage members were refused. Removed the probe. Strict validation, 452 Rust tests, 15 frontend tests, 46 provisioning tests, available builds/checks and protected-document checks passed. Production/permanent tests remain unchanged; the implementation and native-boundary tasks below remain open.

## 2. Enforce the shared payload without changing serialization

- [ ] 2.1 Reuse the existing direct object visitor for `AccountView` and `TokenUsage`; reject account additions and positional aliases without a `Value` buffer, fallback/default or second wire convention. Preserve all current serialized fields, nested schemas, exact integer widths and schema-3 monetary state.
- [ ] 2.2 Add deterministic raw and pre-parsed decoder acceptance: named/reordered objects; account and usage arrays; every required member missing/null/wrong-type; unknown and raw duplicate members; `u64` boundaries and non-integer counters. Verify unchanged typed canonical bytes and valid provider signatures, not source-text or wiring assertions.
- [ ] 2.3 Map and fill native account acceptance for every immutable identity/term field, all eight money fields, wide balanced sums and one-wei mismatches, preserving account bytes, credentials and original payment evidence on refusal. Reuse existing financial/receipt tests where their assertions suffice.
- [ ] 2.4 Exercise actual owner/read-authorized provider snapshots for free, active, settlement-pending and frozen accounts. Retain unfamiliar status strings without treating them as active/free authority; do not create a new chain transition policy or substitute RPC unavailability for an orphan.
- [ ] 2.5 Check actual CLI/native/desktop receipt and account presentation/export boundaries for exact large counters and wei. Repair any lossy signed-statement path without narrowing the wire range; verify each changed surface and preserve native secret isolation.

## 3. Verify containing responses and durable recovery

- [ ] 3.1 Exercise account objects inside native opening/replay, direct account reads, top-up replies and signed receipts, including non-object/missing/extra fields and nested usage aliases. Establish that refusal reaches the intended decoder rather than unrelated auth/rate failure; retain named-object success controls.
- [ ] 3.2 Exercise invalid account replies after a genuine committed opening/top-up, then reopen and recover the exact original. Preserve accepted balances, original journal/acknowledgement evidence and exported credentials without a second payment or duplicate credit. Distinguish restored prerequisites, controlled HTTP faults and real operations.
- [ ] 3.3 Retain request-accounting 3.2.1's renewed-report receipt coverage and distinguish original account terms, known waived usage, unknown request exposure and account funding-loss compensation. Reuse its completed regression rather than rewriting its report binding.
- [ ] 3.4 Confirm the stricter decoder reads current genuine persisted schema-3 account/native records without changing their money representation. Corrupt/unsupported records fail closed and remain intact; no automatic legacy migration, reset or conversion.

## 4. Integrate verified behavior

- [ ] 4.1 Update requirement/source/test correspondence and operator/client documentation with observed results and remaining release limits; leave the whitepaper unchanged.
- [ ] 4.2 Pass strict validation and required available project checks after implementation. Archive with `openspec archive specify-inference-account-snapshot --yes` only when every in-scope task is complete; never use `--skip-specs`.

No deployment, live payment, account/server administration, installation, hardware qualification, whitepaper edit or unrelated source adoption is authorized. Full `UsageReceipt`, `RequestView`, `PerformanceOutcome`, `OpenResult` and credential-envelope field schemas remain separate specification work; this change covers their shared account/usage payloads, not replacement wrappers.
