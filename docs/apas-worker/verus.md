# Rust verification ledger

## Availability and trust — 2026-11-15

`verus --version` failed with exit 127: command not found. No existing Verus/vstd integration or verification command was found. Setup and proof work are blocked by the report entry **2026-11-15 — Verus is unavailable**; nothing was installed or pinned without a working verifier.

No verified proofs, replaced tests, trusted functions, unchecked assumptions or added dependencies. No test deletion is authorized by this ledger. A future proof must follow the OpenSpec requirement and pass the project's pinned verification command, not merely compile a model of the current behavior.

## Judged snapshot, lifecycle and top-up tests

Requirement source: `openspec/changes/specify-inference-account-snapshot/specs/inference-account-snapshot/spec.md` (named account schema, exact usage counters, original-term/conservation binding and post-commit recovery). These judgments concern replacing the entire existing test, including its actual calls and asserted results, under the vstd-only/no-interface-change rules. Mathematical subproperties may still merit proofs; they would not replace tests of unmodeled I/O or third-party behavior.

Top-up recovery also follows `openspec/specs/inference-account-topup-request/spec.md`: exact original ownership, independent safe credit, retained spend/holds/credentials, admission-independent recovery and at-most-once funding/fees. The additional judgments below cover actual HTTP/SQLite/native execution, not a pure model of those operations.

| Test | Judgment | Reason |
|---|---|---|
| `inference_account_snapshot_test::named_account_snapshots_preserve_every_field_and_canonical_bytes` | not replaceable | Executes serde/serde_json decoding and canonical serialization, including reordered objects and nullable nested members. No proof models for those external implementations are present; trusting their results would not authorize deleting this test. |
| `inference_account_snapshot_test::account_snapshots_reject_positional_arrays_and_nonobjects` | not replaceable | Checks actual raw and pre-parsed serde_json rejection, not just an abstract field predicate. |
| `inference_account_snapshot_test::account_snapshots_require_all_named_members_and_refuse_additions` | not replaceable | Exercises external JSON parsers and derived/custom deserializers for missing, extra and wrong-type members. |
| `inference_account_snapshot_test::accounting_usage_rejects_positional_arrays_and_nonobjects` | not replaceable | Requires actual TokenUsage and containing AccountView parser behavior in both raw/value paths. |
| `inference_account_snapshot_test::accounting_usage_requires_two_exact_unsigned_counters_without_defaults` | not replaceable | Asserts actual JSON integer parsing/canonical bytes, including large counters, exponent/floating-point forms and containing account decoding. A proof about u64 bounds alone would not cover these calls/results. |
| `inference_account_snapshot_test::account_and_usage_decoders_reject_raw_duplicate_members` | not replaceable | Tests raw duplicate-member handling in serde_json; converting input to an abstract map would erase the property being checked. |
| `inference_account_snapshot_test::receipts_preserve_named_claims_but_reject_account_and_usage_projections` | not replaceable | Combines actual JSON decoding/canonical bytes with external cryptographic signing/verification. A trusted signature/parser model would not permit test replacement. |
| `inference_owner_control_test::native_account_refresh_preserves_original_terms_money_and_recovery_state` | not replaceable | Actual native HTTP authorization, SQLite, protected files, exported credentials and reopening; no proof models cover those boundaries. |
| `inference_owner_control_test::native_signed_snapshots_preserve_terms_balances_and_receipt_watermarks` | not replaceable | Actual HTTP/signatures, provider issuance, durable watermark/files/journal and replay after reopening; retains external and I/O acceptance beyond pure snapshot predicates. |
| `inference_owner_control_test::native_opening_snapshots_recover_committed_credit_without_installing_invalid_accounts` | not replaceable | Actual router/controlled RPC, SQLite credit, native files, credentials and concurrent isolated runtimes. Post-commit recovery cannot be replaced by a pure balance/identity proof. |
| `inference_owner_control_test::native_free_opening_snapshots_recover_without_payment_or_credential_rotation` | not replaceable | Executes public quote issuance, real native/provider HTTP opening and replay, SQLite/files, credential issuance and reopening, concurrent runtimes and separate RPC tripwires. The absence of financial side effects/RPC cannot be replaced by a pure balance proof; the paid matrix remains intact. |
| `inference_owner_control_test::native_topup_snapshots_preserve_pending_originals_after_committed_credit` | not replaceable | Native signing/submission, independent RPC verification, real provider credit, raw JSON faults, SQLite/files and reopened HTTP recovery. Preserving actual original evidence, exported credentials and prior holds is not covered by an abstract balance proof. |
| `inference_owner_control_test::native_topup_recovers_lost_credit_response_at_full_quote_storage` | not replaceable | Genuine public storage refusal, native payment/journal lifecycle, independent RPC, committed-response disconnection and reopened settlement with complete stored-history comparisons. |
| `inference_owner_control_test::native_topup_rejects_invalid_amounts_and_recovers_at_full_quote_work` | not replaceable | In addition to the shared HTTP/SQLite recovery path, checks actual local amount parsing, preparation failure, protected bytes and absence of RPC/financial side effects; no model covers these calls/results. |
| `inference_owner_control_test::native_topup_recovers_lost_credit_response_at_full_quote_issuance` | not replaceable | Exercises persisted issuance exhaustion, owner authorization, native original preparation and genuine remote credit/recovery without rewriting quota or financial records. |
| `inference_owner_control_test::native_topup_recovers_lost_credit_response_at_owned_free_opening_limit` | not replaceable | Real authenticated opening-capacity refusal and top-up recovery with protected credentials, prior usage/holds, SQLite snapshots and separate native/provider RPC. |
| `inference_owner_control_test::native_topup_recovers_lost_credit_response_at_global_free_opening_limit` | not replaceable | Full restored account population, actual capacity refusal, counted row digests, representative bearer reads and native/HTTP settlement after a lost committed reply; external storage/network behavior is essential. |
| `inference_owner_control_test::native_topup_recovers_lost_credit_response_at_paid_intent_limit` | not replaceable | Independent paid-admission refusal with retained acknowledgement/nonce ownership, genuine native top-up and lost-response recovery under full intent capacity; relies on unmodeled cryptography, HTTP and SQLite. |
| `inference_owner_control_test::topup_inactive_accounts_refuse_before_rpc_and_again_at_credit` | not replaceable | Exercises real owner authorization, HTTP refusals, SQLite funding-state transitions and credit guards, a network tripwire and reopened bearer reads. Supplied chain observations are prerequisites, not a pure verified model of the storage/network effects. |
| `inference_owner_control_test::account_snapshots_preserve_lifecycle_reads_without_granting_spend_authority` | not replaceable | Owner/read-bearer HTTP account and signed-receipt reads, native protected-file retention, SQLite lifecycle/reconciliation, reopened admission refusals and RPC tripwire observations. The unfamiliar-status fixture and exact financial/credential/journal assertions depend on unmodeled I/O and third-party parsing/signatures. |

## Remaining inventory

This is a scoped inventory, not a claim that all Rust tests were reviewed. Other Rust test functions remain unclassified; extend the inventory from their implementation and OpenSpec requirements in subsequent steps. Do not rejudge a blocked test unless its code or requirement changes. Missing implementation/specification work retains priority over replacing an existing test with a proof.

## Judged 2026-11-17 — opening-cancellation scope regressions

Requirement source: `openspec/changes/specify-funding-settlement/specs/inference-funding-settlement/spec.md` (requirements 4 and 16).

- `consumer::tests::opening_cancellation_on_a_saved_account_still_refreshes_without_reusing_the_opening` — **not replaceable**: it drives the async native consumer against a local HTTP provider and scripted JSON-RPC, with protected files and the wallet journal; Verus models none of these.
- `consumer::tests::opening_cancellation_on_a_saved_account_keeps_pending_topup_recovery` — **not replaceable**, for the same reasons.

## Judged 2026-11-17 — funding-recheck quarantine regressions

Requirement source: `openspec/changes/specify-funding-settlement/specs/inference-funding-settlement/spec.md` (requirement 9).

- `billing_store::tests::refused_recheck_is_quarantined_without_stalling_other_payments` — **not replaceable**: its property lives in SQLite transactions and an on-disk ledger reopened across connections, which Verus does not model.
- `billing_store::tests::rpc_outage_fee_gas_failure_and_reorg_keep_distinct_admission_states` (extended) — **not replaceable**: async maintenance against a scripted JSON-RPC server, an HTTP gateway and a fixture SQLite trigger.

