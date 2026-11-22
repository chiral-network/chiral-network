# Rust verification ledger

## Availability and trust — 2026-11-15

`verus --version` failed with exit 127: command not found. No existing Verus/vstd integration or verification command was found. Setup and proof work are blocked by the report entry **2026-11-15 — Verus is unavailable**; nothing was installed or pinned without a working verifier.

No verified proofs, replaced tests, trusted functions, unchecked assumptions or added dependencies. No test deletion is authorized by this ledger. A future proof must follow the OpenSpec requirement and pass the project's pinned verification command, not merely compile a model of the current behavior.

## Judged snapshot, lifecycle and top-up tests

Requirement source: `openspec/specs/inference-account-snapshot/spec.md` (archived from `specify-inference-account-snapshot`, 2026-11-21) (named account schema, exact usage counters, original-term/conservation binding and post-commit recovery). These judgments concern replacing the entire existing test, including its actual calls and asserted results, under the vstd-only/no-interface-change rules. Mathematical subproperties may still merit proofs; they would not replace tests of unmodeled I/O or third-party behavior.

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


## Judged 2026-11-17 — won-original recovery regressions

Requirement source: `openspec/changes/specify-funding-settlement/specs/inference-funding-settlement/spec.md` (requirements 16 and 17).

- `consumer::tests::won_opening_original_is_settled_by_resume_without_cancellation_authority` — **not replaceable**: it drives the async native consumer through restarts against a local HTTP provider and a scripted JSON-RPC chain with reorgs, with protected files and the wallet journal; Verus models none of these.
- `consumer::tests::won_opening_original_without_a_journaled_replacement_is_settled_and_never_rebroadcast` — **not replaceable**, for the same reasons.
- `consumer::tests::won_topup_original_after_cleared_intent_is_credited_once_by_resume` — **not replaceable**, for the same reasons.
- `consumer::tests::won_topup_original_recorded_by_funding_recovery_is_credited_once_by_resume` — **not replaceable**, for the same reasons.

## Judged 2026-11-17 — durable top-up cancellation request regressions

Requirement source: `openspec/changes/specify-funding-settlement/specs/inference-funding-settlement/spec.md` (requirement 16).

- `consumer::tests::failed_topup_cancellation_attempt_keeps_the_request_and_never_rebroadcasts_the_original` — **not replaceable**: it drives the async native consumer through a failed cancellation attempt, restarts and an explicit retry, against a local HTTP provider and a scripted JSON-RPC chain, with protected files and the wallet journal; Verus models none of these.
- `consumer::tests::won_topup_original_after_a_failed_cancellation_attempt_is_credited_once_by_resume` — **not replaceable**, for the same reasons.

## Judged 2026-11-17 — funding disclosure tests

Requirement source: `openspec/changes/specify-funding-settlement/specs/inference-funding-settlement/spec.md` (requirements 2 and 3).

- `src-tauri/tests/inference_cli_fund_disclosure_test.rs::fund_discloses_registration_risk_on_stderr_and_keeps_stdout_machine_readable` — **not replaceable**: it runs the `chiral` binary as a process and checks its stderr and stdout text and an on-disk wallet journal; Verus proves properties of Rust functions, not process output or user-facing wording.
- The seven tests in `tests/fundingDisclosure.test.ts` are TypeScript tests of rendered Svelte pages; Verus does not apply to them.
- `tests/fundingDisclosure.test.ts` › `shows the journaled stage after a failed top-up without Refresh, and clears it after resume` (task 2.9) — TypeScript test of a rendered Svelte page; Verus does not apply.

## Judged 2026-11-17 — client-disconnect classification regressions

Requirement source: `openspec/changes/specify-generation-performance/specs/inference-generation-performance/spec.md` (requirements 11 and 12).

- `inference_gateway::tests::non_streaming_disconnects_before_a_miss_are_client_interruptions_and_never_latch` — **not replaceable**: it drives the async axum/hyper gateway over real TCP sockets with a delaying fixture engine and reads a SQLite ledger; Verus models none of these.
- `inference_gateway::tests::a_measured_miss_survives_a_later_non_streaming_disconnect` — **not replaceable**, for the same reasons.
- `inference_gateway::tests::disconnect_before_the_engine_responds_is_a_client_interruption_in_both_modes` — **not replaceable**, for the same reasons.
- `performance::tests::exclusions_do_not_erase_a_previously_observed_miss` (updated) — **replaceable in principle**: `performance::outcome` is a pure function over status, timings and token counts. A Verus proof could show that a `disconnected` status without a measured miss is never attained or waived, and that an `interrupted` status is always a counted provider-incomplete miss. It waits for the Verus report entry; the test stays.

## Judged 2026-11-17 — zero-TTFT and prompt-boundary regressions

Requirement source: `openspec/changes/specify-generation-performance/specs/inference-generation-performance/spec.md` (requirements 4, 8, 9 and 11).

- `performance::tests::zero_ttft_is_no_evidence_of_attainment` — **replaceable in principle**: `performance::outcome` is pure. A Verus proof could show that `ttft_us == Some(0)` is never attained. It waits for the Verus report entry; the test stays.
- `performance::evidence_tests::zero_ttft_samples_do_not_attain` and `performance::evidence_tests::samples_outside_every_prompt_boundary_are_rejected` — **not replaceable as written**: they read evidence files through `validate_evidence`, which parses JSON from disk and hashes it. The inner `passes` predicate and the boundary predicate are pure and could be proved separately once Verus is available.
- `scripts/test_qualify_provider.py` `ttft_us` 0 case — Python; Verus does not apply.

## Judged 2026-11-17 — fee aggregation and recheck regressions

Requirement source: `openspec/changes/specify-fee-remittance/specs/inference-fee-remittance/spec.md` (requirements 4, 5, 6 and 9).

- `billing_store::tests::amount_threshold_is_evaluated_and_sealed_over_every_pending_obligation`, `explicit_remittance_seals_every_pending_obligation_but_never_a_cancelled_one`, `a_wallet_splits_into_several_batches_only_at_u128_overflow`, `every_confirmed_fee_batch_is_rechecked_whatever_the_pass_timing` and `confirmed_fee_batches_are_rechecked_least_recently_checked_first` — **not replaceable**: their properties live in SQLite queries, transactions and ordering over an on-disk ledger, which Verus does not model. The u128 split rule is pure arithmetic and could be proved separately once Verus is available.
- `billing_store::tests::fee_recheck_time_advances_after_unavailable_and_faulted_outcomes` — **not replaceable**: async maintenance against a scripted JSON-RPC outage fixture and a SQLite ledger.

## Judged 2026-11-18 — fee view regressions

Requirement source: `openspec/changes/specify-fee-remittance/specs/inference-fee-remittance/spec.md` (requirements 3, 11 and 12).

- `billing_store::tests::fee_view_reports_the_four_book_states_from_obligation_and_batch_state` and `fee_view_lists_only_the_newest_100_batches_but_totals_every_obligation` — **not replaceable**: SQLite joins and ordering over an on-disk ledger. The state mapping `reported_fee_obligation` is a pure function of two status strings and could be proved total and exact once Verus is available.
- `app::tests::fee_view_lists_confirmed_remittances_and_the_receipt_statement` — **not replaceable**: async HTTP through the admin router, a provider runtime and signed transactions.

## Judged 2026-11-18 — unresolved-funding GUI regressions

Requirement source: `openspec/changes/specify-desktop-gui-global-rules` (A.1.7, through `inference-funding-settlement` requirement 2).

- The four new tests in `tests/fundingDisclosure.test.ts` (`a reopened Marketplace offers only Resume…`, `settled, unpaid or free journal entries do not block…`, `an unreadable journal keeps new funding disabled…`, `another account's unresolved payment keeps this account's top-up disabled in Usage`) are TypeScript tests of rendered Svelte pages; Verus does not apply.

## Judged 2026-11-18 — top-up disclosure regressions

Requirement source: `openspec/changes/specify-desktop-gui-global-rules` (requirement 7).

- `consumer::tests::agreement_exposes_only_public_disclosure_terms` — **not replaceable**: it opens an agreement through the async native consumer against a local HTTP provider and scripted JSON-RPC, then reads protected files; Verus models none of these.
- The two new tests in `tests/fundingDisclosure.test.ts` are TypeScript tests of a rendered Svelte page; Verus does not apply.
- `tests/profileGate.test.ts` (task 2.2, requirement 4) — TypeScript tests of rendered Svelte pages; Verus does not apply.
- `tests/requestRecovery.test.ts` (tasks 2.10 and 2.11, requirement 8) — TypeScript tests of rendered Svelte pages; Verus does not apply.
- `tests/native.test.ts` cases added by tasks 2.7 and 2.9 (requirements 3 and 5) — TypeScript; Verus does not apply. The Rust side of exact wei parsing is already covered by `inference-wire-foundation` tests.
- `tests/accessibility.test.ts` (tasks 2.4–2.6, requirement 10) — TypeScript tests of rendered Svelte components; Verus does not apply.
- `tests/noRemoteResources.test.ts` (task 2.8, requirement 1) — TypeScript check of HTML pages; Verus does not apply.
- `tests/credentials.test.ts` (`specify-desktop-gui-pages` tasks 2.1 and 2.2, requirement 6) — TypeScript tests of a rendered Svelte page; Verus does not apply.
- `tests/settings.test.ts` (`specify-desktop-gui-pages` tasks 2.3 and 2.4, requirement 13) — TypeScript tests of rendered Svelte components; Verus does not apply.

## Judged 2026-11-18 — policy signer key-file regressions

Requirement source: `openspec/changes/specify-version-policy/specs/client-version-policy/spec.md` (requirement 8).

- `src-tauri/tests/policy_sign_cli_test.rs` (`keygen_writes_an_owner_only_key_file_and_prints_only_the_public_key`, `sign_reads_the_key_only_from_a_protected_file`) — **not replaceable**: they run the `chiral-policy-sign` binary as a process and check file modes, symlink refusal, stdout and signatures; Verus models none of these.
- `tests/updateGate.test.ts` (`specify-version-policy` tasks 2.2 and 2.3, requirement 13) — TypeScript tests of rendered Svelte components; Verus does not apply.
- `version::tests::reset_forgets_the_effective_policy_and_its_scope` (`specify-version-policy` task 2.4) — **replaceable in principle**: resetting the slot is a pure state assignment, but the slot is a `parking_lot::RwLock` in a `OnceCell`, which Verus would model only through an external specification. It waits for the Verus report entry; the test stays.
- `desktop::tests::logout_drops_the_previous_networks_version_status` — **not replaceable**: async session teardown with Tokio locks and a process-wide policy cell.
- `tests/versionStatus.test.ts` — TypeScript tests of a Svelte store and the rendered shell; Verus does not apply.

## Judged 2026-11-18 — profile path resolution regressions

Requirement source: `openspec/specs/profile-path-resolution/spec.md` (archived 2026-11-20 from `openspec/changes/archive/2026-11-20-specify-profile-path-resolution`).

- The six tests in `src-tauri/tests/consumer_profile_paths_test.rs` — **not replaceable**: they run the `chiral` binary from different working directories and inspect files on disk; Verus models neither processes nor the file system.
- `consumer::tests::every_relative_profile_path_resolves_against_the_profile_directory` — **replaceable in principle** for the join rule (absolute paths unchanged, relative paths joined to the base), which is pure; `std::path` would need an external specification. It waits for the Verus report entry; the test stays.

## Judged 2026-11-18 — managed-runtime requirement for qualified profiles

- `inference_commercial_terms_test::qualified_profiles_require_a_managed_runtime_outside_development` (commercial-terms task 2.6) — **replaceable in principle**: `ProviderConfig::validate`'s rule is a pure predicate over the parsed configuration, but the test exercises JSON parsing and the file-loading path too. It waits for the Verus report entry; the test stays.

## Judged 2026-11-18 — discovery withdrawal, refresh and identity regressions

Requirement source: `openspec/changes/specify-offer-discovery/specs/inference-offer-discovery/spec.md` (requirements 2, 4, 9 and 11).

- `dht::tests::manifest_straddling_a_republish_withdraws_nothing`, `manifest_that_shrank_between_pages_withdraws_nothing`, `complete_snapshot_manifest_withdraws_an_omitted_offer` — **replaceable in principle** for the completeness predicate (`PeerManifest::complete`), which is pure over received revisions and page flags; the tests also drive the registry. They wait for the Verus report entry and stay.
- `dht::tests::republished_namespaces_keep_their_manifest_pages` — **replaceable in principle**: page construction over an ordered map is pure; it waits for Verus.
- `dht::tests::failed_publication_serves_and_advertises_nothing`, `app::tests::failed_refresh_withdraws_the_offers_it_published` — **not replaceable**: a libp2p swarm, a Kademlia store, HTTP fixtures and async refresh.
- `dht::tests::symbolic_link_to_identity_file_is_refused_unchanged`, `exposed_identity_file_is_refused_unchanged`, `windows_identity_file_has_a_current_user_only_access_list` — **not replaceable**: file-system modes, symlinks and Windows access lists.

## 2026-11-18 — chain RPC gateway and origin regressions; Verus companion narrowed

Requirement sources: `openspec/changes/specify-chain-rpc-gateway/specs/chain-rpc-gateway/spec.md` and `specify-inference-commercial-terms` *Offer freshness and advertised origin*.

- `verified/method_allowlist.rs` (an existing Verus companion of `chain_rpc_api::allowed_method`) was narrowed in step with the runtime: `net_version` was removed from `allowed_methods()`. Neither lemma (`unlisted_methods_are_rejected`, `operator_methods_are_rejected`) depends on `net_version`, removing a set member cannot falsify a non-membership `ensures`, and no `assume`, `admit` or `external_body` was added. The proof was **not re-run**, because Verus is unavailable (open report entry); re-run it when Verus is installed.
- `chain_rpc_api::tests::allowlist_is_exactly_ten_eth_methods` — **replaced in principle** by the Verus companion for membership, but the companion's equivalence to the Rust function is manual (see `verified/README.md`), so the test stays.
- `src-tauri/tests/chain_rpc_gateway_test.rs` (3 tests) — **not replaceable**: they spawn the gateway binary and use HTTP and protected files.
- `inference_offer_origin_test::single_label_remote_hostnames_are_rejected_including_root_dot_spellings` — **replaceable in principle** for the label-count predicate, which is pure; URL parsing would need an external specification. It waits for Verus.
- `scripts/test_provider_preflight.py`, `scripts/test_provision_provider.py` — Python; Verus does not apply.

## Judged 2026-11-18 — managed engine launch regressions

Requirement source: `openspec/changes/specify-managed-engine-launch/specs/managed-engine-runtime/spec.md` (requirements 4, 6 and 11).

- `app::tests::an_engine_key_that_repeats_another_credential_is_refused_before_anything_starts` — **replaceable in principle** for the normalisation predicate (strip `0x`, strip leading zeros, pad to 64, lower-case, compare), which is pure; the test also reads protected files and starts a runtime. It waits for the Verus report entry and stays.
- `engine_runtime_tests::a_listener_on_the_engine_address_or_its_wildcard_is_refused_before_spawn`, `a_port_holding_only_time_wait_connections_passes_the_port_check` — **not replaceable**: kernel socket semantics.
- `tests/daemon_engine_process_test.rs` (4 tests, custom harness) — **not replaceable**: process spawning, signals and a stub engine binary.

## Judged 2026-11-18 — reputation error-mapping regressions

Requirement source: `openspec/changes/specify-payment-gated-reputation/specs/payment-gated-reputation/spec.md` (requirements 2, 6, 7, 11 and 12).

- `rating_api::tests::{malformed_feedback_is_invalid_feedback_without_rpc_or_vote, malformed_feedback_takes_no_verification_slot, oversized_feedback_body_is_413_before_decoding, store_fault_is_a_catalog_failure_and_conflicts_stay_conflicts, out_of_range_score_or_revision_takes_no_verification_slot, unreadable_feedback_body_is_invalid_feedback_json}` — **not replaceable**: axum routing, HTTP bodies, a semaphore and an RPC fixture.
- `rating_storage::tests::{locked_store_is_a_fault_not_a_refusal, store_conflicts_are_typed_refusals_without_database_text}` — **not replaceable**: SQLite locking and transactions.
- `consumer::tests::{feedback_reports_catalog_codes_not_provider_failures, provider_info_reports_catalog_outage_without_blaming_provider}` — **not replaceable**: async HTTP fixtures. `catalog_and_provider_error_tables_stay_separate` — **replaceable in principle** (a pure mapping), but trivial; it stays.

## 2026-11-19 — owner-control mock chain timestamps

- The round-45 change edits only the `TopupRecoveryRpc` and `SafeOpeningRpc` fixtures in `inference_owner_control_test.rs` (block timestamps follow the wall clock). No test was added or removed, so there is no new judgment; the existing owner-control tests stay **not replaceable** (HTTP fixtures, SQLite ledgers and async consumers).

## Judged 2026-11-19 — non-provider error attribution regressions

- `rating_api::tests::snapshot_store_fault_is_the_catalogs_not_the_providers` — **not replaceable**: axum, SQLite locking and an RPC fixture.
- `consumer::tests::offer_catalog_errors_are_the_catalogs_not_a_providers`, `policy_source_errors_name_no_provider_and_still_fail_closed` — **not replaceable**: async HTTP fixtures and protected files.

## Judged 2026-11-19 — policy URL query refusal

- `consumer::tests::policy_url_with_a_query_is_refused_at_profile_load_before_any_request` — **not replaceable** as written: it checks that no request reaches an async HTTP fixture. The refusal predicate itself (`network::validate_endpoint`) is pure URL logic that a future Verus lemma could cover, but no verified URL model exists in `verified/`.

## Judged 2026-11-19 — outbound HTTP answer-set rule and credential export

- `offer_registry::tests::development_localhost_origin_requires_every_answer_to_be_loopback` — **replaceable in principle**: `check_resolved_answers` is a pure function of an address list and two flags, so a Verus lemma over an IP-address model could prove the rule (a refusal when the set is empty, has more than 32 answers, or has any answer outside loopback or outside the public predicate). It is not replaced: Verus is unavailable on this machine (report entry of 2026-11-15) and `verified/` has no IP-address model.
- `consumer::tests::credential_export_refuses_a_saved_origin_that_no_longer_validates`, `credential_export_base_url_is_the_validated_origin_plus_v1` — **not replaceable**: protected files, the credential store and byte-identity of the account file.

## Judged 2026-11-19 — credential options shape and hidden-default check

- `billing_store::tests::credential_options_decode_only_from_one_object_without_repeated_members` — **not replaceable**: it tests serde decoding, which Verus does not model.
- `inference_owner_control_test::credential_mutations_refuse_positional_options_after_consuming_owner_proof`, `consumer::tests::native_credential_options_refuse_a_positional_array_before_any_provider_request` and the five `inference_owner_consumer_test::native_issue_*` tests — **not replaceable**: axum routers, SQLite ledgers, protected files and HTTP fixtures. The probe-answer classification in `default_still_infers` is a small decision over (status, code, body shape) that a lemma could state, but it sits inside async HTTP code with no pure seam.

## Judged 2026-11-19 — public inference API fixes

- `inference_backend::tests::every_advertised_capability_needs_a_probe_on_the_model_endpoints` — **replaceable in principle**: the capability rule (`streaming` needs `chat` or `completions`; `tools` and `structured_output` need `chat`) is a pure predicate over a capability set, which a Verus lemma could state and prove. It is not replaced: Verus is unavailable (report entry of 2026-11-15), and the predicate is inline in async `qualification_probe` with no pure seam.
- `app::tests::a_capability_without_a_probe_on_the_model_endpoints_is_not_ready_or_published`, `readiness_records_the_native_embedding_dimension_only_while_the_model_is_ready`, `inference_gateway::tests::unproducible_embedding_dimensions_are_refused_before_tokenization_and_reservation`, `a_stream_that_fails_while_the_client_is_not_reading_does_not_end_cleanly`, and the two 2.4 tests — **not replaceable**: stub engines, routers, ledgers and stream bodies.

## Judged 2026-11-19 — unprobeable capability rule

- `inference_backend::tests::unprobeable_capabilities_names_each_capability_without_a_probe` — **replaceable**: `unprobeable_capabilities` is now a pure function of a capability list, so a Verus lemma could prove the rule for every set (`streaming` named exactly when neither `chat` nor `completions` is present; `tools` and `structured_output` named exactly when `chat` is absent). Not replaced, because Verus is unavailable on this machine (report entry of 2026-11-15). This supersedes the "no pure seam" note of the round-53 judgment.

## Judged 2026-11-20 — daemon status and settlement-failure stop

- `app::tests::a_settlement_failure_keeps_admission_closed_until_the_operator_resumes`, `admin_status_distinguishes_a_drain_from_a_rejected_chain_identity`, `admin_status_names_a_performance_withdrawn_model_and_keeps_its_readiness`, `admin_status_reports_an_unreadable_ledger_and_every_other_fact` and `admin_status_reports_an_engine_exit_with_its_own_reason` — **not replaceable**: a running provider runtime, admin routes, SQLite ledgers, stub engines and a managed helper process. The latch rule (a recorded fault overrides a pass's admission decision until resume) is small, but it is applied under a mutex inside async runtime code.

## Judged 2026-11-20 — daemon shutdown signals and resume ordering

- `app::tests::a_resume_clears_only_the_settlement_failures_recorded_before_it` — **replaceable in principle**: `SettlementFaults` is a small sequence-number state machine (record, `clear_through(n)`), and a Verus lemma could prove that `clear_through(n)` never clears a failure numbered above `n` and always clears those at or below it. Not replaced, because Verus is unavailable (report entry of 2026-11-15).
- `app::tests::a_resume_during_shutdown_does_not_reopen_the_provider`, `a_resume_keeps_a_settlement_failure_recorded_while_it_waits` and the `daemon_engine_process_test` signal tests (`sighup_*`, `sigterm_while_serving_*`, `sighup_stays_ignored_when_the_daemon_starts_ignoring_it`) — **not replaceable**: runtime interleavings, real child processes and POSIX signals.

## Judged 2026-11-20 — daemon startup, deployment lock and runtime liveness

- `app::tests::a_publication_failure_at_start_leaves_the_provider_serving_and_reports_it`, `a_second_provider_for_a_deployment_is_refused_before_it_joins_the_network` and `admin_runtime_reports_engine_liveness_without_calling_it_readiness` — **not replaceable**: provider runtimes, DHT nodes, file locks, a counting TCP listener and a managed helper process.
- `scripts/test_provider_preflight.py` `RuntimeObservationLiveness` and `scripts/test_qualify_provider.py` liveness tests — **not replaceable**: Python operator scripts, outside Verus's scope.

## Judged 2026-11-20 — managed-engine launch acceptance tests

- `engine_runtime::tests::runtime_settings_are_explicit_bounded_and_refused_rather_than_adjusted` — **replaceable in principle** for the bound checks: `EngineRuntimeConfig::validate` is pure over a parsed struct, so the numeric and selector bounds could be stated as Verus lemmas. The half that loads `ProviderConfig` files is not replaceable (serde, files). Not replaced, because Verus is unavailable.
- `launch_command_passes_the_serving_contract_as_explicit_arguments` — **replaceable in principle** if argument construction were split into a pure function returning a vector; today it builds a `tokio::process::Command`. Not replaced.
- `the_engine_child_receives_only_the_allowlisted_environment`, `the_launch_command_sets_only_the_allowlisted_environment`, `engine_key_files_must_hold_one_protected_literal_token` and `app::tests::an_engine_reporting_another_contract_keeps_the_model_unready_and_unpublished` — **not replaceable**: child processes, file permissions, symlinks and HTTP stub engines.

## Judged 2026-11-20 — managed-engine identity, inventories, endpoint and device observation

- `engine_runtime::tests::the_engine_enumeration_gives_each_selected_device_with_inclusive_headroom`, `cuda_selectors_are_the_gpus_in_pci_bus_order_with_the_engines_names_and_free_memory`, `unified_memory_is_apple_silicon_with_available_memory_of_at_least_the_reserve` and `vulkan_devices_are_the_one_discrete_gpu_of_that_name` — **replaceable in part**: the parsers are now pure functions of captured text, and the decisions after parsing (free memory at least the reserve and total above it, a bus ID at most once, the available-memory cap, the page-count overflow) could be Verus lemmas over integers and sequences. The text parsing itself (line splitting, integer and CSV parsing) has no verified model in `vstd`, so the tests stay. Not replaced, because Verus is unavailable (report entry of 2026-11-15).
- `a_managed_endpoint_is_literal_loopback_with_an_explicit_nonzero_port` — **replaceable in principle** for the URL predicate, like the other origin rules; not replaced (no verified URL model in `verified/`).
- `a_serving_profile_must_equal_every_verified_identity_and_launch_setting` and `a_repackaged_runtime_has_another_identity` — **not replaceable**: struct equality over serde-loaded configurations and SHA-256 of canonical manifests.
- The other new `engine_runtime::tests` (identity and entry-point refusals, removed or resized files, altered inventories before the port, nested levels and symbolic links, the wildcard probe, health after exit, observer order, profile mismatch before spawning, post-load observation, observation bounds, host Apple tools) and `daemon_engine_process_test::check_and_start_refuse_an_altered_inventory_before_any_engine_starts` and `check_and_start_verify_every_managed_model` — **not replaceable**: files, symbolic links, sockets, child processes and the daemon binary.

## Judged 2026-11-20 — engine startup failure, exit without restart and shutdown

- `engine_runtime::tests::an_engine_that_exits_before_readiness_fails_the_launch_and_is_not_relaunched`, `the_health_probe_presents_the_engine_key_and_follows_no_redirect`, `the_health_probe_uses_no_proxy`, `stopping_or_dropping_an_engine_kills_its_whole_process_group` and `a_killed_parent_takes_its_engine_with_it_on_linux_and_a_macos_orphan_blocks_the_next_start` — **not replaceable**: child processes, process groups, signals, sockets and HTTP fixtures.
- `app::tests::a_failed_managed_launch_fails_the_start_and_stops_the_engine_already_running` and `a_managed_engine_exit_withdraws_service_and_nothing_restarts_or_replaces_it` — **not replaceable**: provider runtimes, the DHT, routers, ledgers and managed helper processes. The withdrawal rule (a dead engine is never admissible) is one boolean conjunction already inside async runtime code.
- `daemon_engine_process_test::an_engine_that_exits_before_readiness_fails_the_start_without_a_second_launch` and the strengthened `termination_while_serving_stops_the_engine_and_keeps_the_deployment` — **not replaceable**: the daemon binary, a stub engine and POSIX signals.

## Judged 2026-11-20 — origin, development-exception, address-predicate and administration tests

- `offer_registry::tests::public_ip_is_the_exact_documented_predicate` — **replaceable**: `public_ip` is a pure function of an address, and a Verus lemma over `u32` and `u128` could prove it equals the documented predicate (each excluded range by prefix, mapped forms by the IPv4 rule) for every address, not only the sampled edges. Not replaced, because Verus is unavailable (report entry of 2026-11-15) and `verified/` has no IP-address model.
- `offer_registry::tests::safe_http_client_judges_every_resolved_answer_by_the_same_rules` and `development_scenario_origins_reach_resolution_only_when_admitted` — **replaceable in part**: the answer-set decision (`check_resolved_answers`) and the origin predicate are pure, as judged on 2026-11-19; client construction is not.
- `a_development_localhost_client_connects_only_to_its_loopback_answers`, the five new `consumer::tests`, the four `provider_config::tests`, the three new `app::tests` and `outbound_proxy_environment_test::the_administration_client_never_uses_an_environment_proxy` — **not replaceable**: sockets, recording listeners, protected files, provider runtimes, routers and process environment.

## Judged 2026-11-20 — pinned outbound client, engine origin, ingress and preflight

- `offer_registry::tests::answer_sets_that_are_empty_oversized_or_partly_disallowed_build_no_client` — **replaceable in part**, like the answer-set rule judged on 2026-11-19; the client construction is not.
- `provider_config::tests::an_engine_origin_is_a_private_literal_or_exact_localhost` — **replaceable in part**: the private-address predicate (`is_private_engine_ip`) is a pure function of an address and could be a Verus lemma over `u32`/`u128` ranges; the configuration loading, runtime start and `Backend::new` halves are not.
- `resolution_that_has_not_answered_after_five_seconds_is_refused`, `a_pinned_client_abandons_a_stalled_connection_after_ten_seconds`, `a_pinned_https_client_verifies_the_origin_certificate`, the consumer and engine redirect tests, `app::tests::the_engine_origin_is_never_advertised_or_echoed` and the three `outbound_proxy_environment_test` tests — **not replaceable**: timers, sockets, TLS handshakes, HTTP fixtures, provider runtimes and process environment.
- `scripts/test_provision_provider.py` `ReferenceIngress` and `scripts/test_provider_preflight.py` `PrivateRouteProbe` and `ListenerCheck` — **not replaceable**: Python operator scripts, outside Verus's scope.

## Judged 2026-11-20 — provider credential acceptance tests

- `billing_store::tests::credential_options_outside_the_contract_change_no_credential` and `valid_options_issue_and_rotate_credentials_with_exactly_those_limits` — **replaceable in part**: the option rules (every scope known and the list non-empty, concurrency at most the account's, a canonical decimal cap) are pure predicates that Verus lemmas could state; the decoding (serde) and the router, proofs and SQLite rows are not.
- `credential_caps_restrict_authority_and_never_create_or_refund_money` — **replaceable in part**: the reservation decision (`insufficient_funds` before `spend_cap`, checked arithmetic) is integer logic that a lemma over `u128` could prove; the ledger transactions are not.
- `the_ledger_keeps_only_a_hash_of_each_secret_and_an_independent_identifier`, `an_expired_or_revoked_credential_rotates_into_a_working_replacement`, `an_identifier_of_another_account_of_the_same_owner_is_refused`, `credential_management_stays_reachable_while_every_admission_gate_is_closed` and `inference_gateway::tests::bearers_outside_their_scope_or_lifetime_change_nothing_on_any_route` — **not replaceable**: ledger files, SQLite, SHA-256, routers, owner proofs and engine fixtures.

## Judged 2026-11-20 — native credential store and commands

- `consumer::tests::native_credential_secrets_live_only_in_the_owner_only_account_file`, `an_unprotected_or_linked_account_store_fails_closed_without_contacting_the_provider`, `native_credential_management_returns_metadata_and_only_export_reveals_a_secret`, `rotating_the_native_default_keeps_a_usable_default` and `inference_cli_credentials_test::credential_commands_print_metadata_and_only_export_prints_a_secret` — **not replaceable**: file modes, ownership, symbolic links, protected files, provider fixtures, routers and CLI processes. The default-selection rule (a replacement with `infer` becomes the default; a read-only one never does) is a small decision, but it is applied inside the account-store update with no pure seam.

## Judged 2026-11-20 — public inference surface and admission

- `inference_gateway::tests::a_route_the_agreement_model_does_not_advertise_is_refused_before_reservation`, `tools_and_json_output_without_their_capabilities_are_refused_before_reservation`, `engine_only_members_are_refused_on_every_inference_route_and_never_reach_the_engine`, `nested_members_outside_the_subset_are_refused_before_reservation` and `non_text_content_and_adapter_members_are_refused_before_reservation` — **replaceable in part**: the admission decision (endpoint capability, member allowlist, content-part kinds) is a pure predicate over a request's member names and a capability set, which Verus lemmas could state; the JSON parsing and router are not.
- `a_prompt_plus_output_limit_beyond_the_context_is_refused_before_reservation` — **replaceable in part**: the context rule (prompt tokens plus output limit at most the context) is checked integer arithmetic; tokenization through the engine is not.
- `app::tests::the_public_router_serves_no_engine_or_out_of_scope_route`, `each_bearer_lists_only_its_own_agreement_model`, `a_request_naming_another_model_is_refused_as_a_model_mismatch_before_reservation`, the three body-handling tests, `insufficient_prepaid_credit_is_refused_before_any_hold_or_generation`, `a_request_while_the_model_slot_is_busy_is_refused_as_saturation_without_queueing`, the health, stream-failure and no-reroute tests — **not replaceable**: routers, HTTP bodies, timers, semaphores, ledgers and stub engines.

## Judged 2026-11-20 — readiness probes, echo, embeddings and tool output

- `inference_backend::tests::an_embedding_batch_of_unequal_or_unrequested_lengths_is_not_a_success` and `inference_gateway::tests::embedding_vectors_keep_the_native_dimension_and_an_inconsistent_batch_is_a_failure` — **replaceable in part**: the length rule (every vector the same length, equal to the requested or native dimension) is a pure predicate over a list of lengths that a Verus lemma could state; JSON decoding, routers and the stub engine are not.
- `inference_backend::tests::remote_schema_references_are_refused_in_tool_parameters_and_response_schemas` and `inference_gateway::tests::remote_schema_references_and_non_function_tools_are_refused_and_nothing_is_fetched` — **replaceable in part**: the reference rule (only `#`-local `$ref` values, at any depth) is a recursive predicate over a tree; `serde_json::Value` traversal is external to Verus, so it would need a model type.
- `completion_echo_returns_the_exact_prompt_and_only_generated_log_probabilities` and `a_completion_stopping_at_the_full_context_is_accepted_only_with_consistent_usage` — **replaceable in part**: the offset rule (first offset equals the prompt length in scalar values) and the full-context stop rule (prompt plus completion tokens equal to the context) are checked integer arithmetic; the event stream, settlement and timing are not.
- The readiness tests (`readiness_probes_every_advertised_capability_through_the_public_preparation`, `readiness_requires_the_engine_reported_identifier_context_and_template`, `readiness_fails_when_any_advertised_inference_probe_fails`, `app::tests::a_failed_readiness_probe_keeps_a_model_unadmitted_and_unpublished_and_withdraws_a_ready_one`), the stock-client and SDK-shaped tests, `a_tool_call_and_generated_text_are_returned_as_data_and_never_acted_on` and `parallel_tool_call_fragments_are_reassembled_by_their_index` — **not replaceable**: HTTP engines, SSE streams, SHA-256 over served templates, DHT publication and routers.
- `scripts/test_provision_artifacts.py` `ProvisioningCapabilities` and `scripts/test_qualify_provider.py` `test_completion_echo_probe_requires_generated_only_logprobs_after_the_prompt` — **not replaceable**: Python operator scripts, outside Verus's scope.

## Judged 2026-11-20 — provider daemon lifecycle: status, drain, resume and passes

- `app::tests::reading_the_operator_status_changes_no_admission_publication_or_ledger_state`, `a_drain_refuses_new_work_while_admitted_work_settles_and_recovery_continues`, `a_request_admitted_before_a_drain_can_be_cancelled_while_draining`, `admin_runtime_reports_a_live_engine_failing_its_template_probe_as_alive_and_not_ready`, `an_engine_exit_is_reported_while_a_pass_holds_the_lifecycle_lock`, `resume_reopens_only_through_its_pass_and_offer_refresh_does_not_end_a_drain`, `resume_of_a_drained_provider_whose_engine_exited_ends_the_drain_and_reopens_nothing`, `a_latched_report_stays_latched_through_resume_and_offer_refresh`, `a_drain_or_an_engine_exit_during_a_pass_leaves_nothing_admitted_or_published`, `passes_keep_rechecking_credited_funding_while_drained_after_an_engine_exit_and_in_an_outage`, `a_funding_recheck_the_ledger_quarantines_keeps_the_same_pass_from_publishing`, `a_failed_reconciliation_closes_admission_and_publication_until_a_later_pass_succeeds`, `the_periodic_pass_keeps_running_after_a_failed_pass_and_while_drained` and `a_restart_after_a_settlement_failure_recovers_the_request_before_admitting_again` — **not replaceable**: async locks, a pass interleaved with a drain or a process exit, routers, ledgers, a stub chain and stub engines. The admission decision itself (open only when not drained, no settlement fault, every managed engine alive and one model ready; new funding only when also chain-ready and funding-safe) is a pure Boolean function that a Verus lemma could state, but the property tested here is its ordering against concurrent writers, which Verus does not model for tokio.

## Judged 2026-11-20 — provider daemon lifecycle: startup, lock, ledger scope, terms, shutdown, logs

- `app::tests::a_ledger_of_another_schema_network_or_provider_refuses_the_start_and_stays_unchanged` — **replaceable in part**: the open decision (schema equal to 3, recorded network and provider equal to the configured ones) is a pure predicate over three values; SQLite and file comparison are not.
- `app::tests::configuration_changes_reach_new_offers_only_through_a_restart_and_never_reprice_an_agreement` — **replaceable in part**: charging at the agreement's own rate is the checked-arithmetic cost rule already judged replaceable; restarts, offers and ledgers are not.
- `startup_refuses_a_rejected_chain_identity_or_failed_recovery_and_runs_through_an_outage_or_publication_failure`, `a_restart_with_another_model_revision_refuses_old_agreements_before_reservation_and_keeps_their_control`, the strengthened `deployments_share_a_wallet_without_sharing_accounts_or_process_locks` and the seven `daemon_engine_process_test` lifecycle tests — **not replaceable**: processes, signals, file locks, listeners, stub chains and engines, and file scans.

## Judged 2026-11-20 — no background start

- The `application_startup_boundaries_test` source, configuration and enumeration tests (process construction, node and miner names, dependencies, native operations, plugins and permissions, login mechanisms, the desktop start, Core-space requests) — **not replaceable**: they are properties of the repository's text and manifests, not of a function's behaviour; Verus proves code, not the absence of code elsewhere.
- The process-observation tests (`a_cli_session_start_and_wallet_commands_create_no_process`, `legacy_data_under_the_base_and_the_old_data_directory_is_untouched_and_never_loaded`, `an_explicit_import_leaves_its_source_unchanged_and_shows_only_the_espace_balance`, the two macOS daemon cases) and `consumer::tests::balance_quote_fund_and_resume_send_only_espace_requests_to_the_configured_rpc` — **not replaceable**: processes, kqueue, files and HTTP fixtures.
- `tests/startupBoundaries.test.ts`, `tests/legacyPreferences.test.ts`, `tests/legacyWallet.test.ts` and the Python provisioning audit and AST tests — **not replaceable**: TypeScript and Python, outside Verus's scope.

## Judged 2026-11-20 — profile path resolution

- `provider_config::tests::every_relative_provider_path_resolves_against_the_configuration_directory` — **replaceable in part**: the resolution rule (an absolute path unchanged, a relative one joined to the configuration's directory, no normalization) is a pure function over path components that a Verus lemma could state over a component sequence; `std::path` itself is external to Verus.
- `configuration_path_resolution_test` (relay, gateway, daemon), the two new `consumer_profile_paths_test` tests, `desktop::tests::configure_returns_a_refusal_and_the_loaded_profile_stays_loaded` and the Settings Vitest cases — **not replaceable**: processes, working directories, files, HTTP stubs, desktop state and TypeScript.

## Judged 2026-11-20 — owner-challenge rate clock

- `billing_store::tests::inconsistent_challenge_rate_evidence_fails_closed_without_erasing_state`, `retained_challenge_counters_are_adopted_without_fresh_allowance` and `superseded_challenge_wallet_rows_are_not_charges_in_the_adopted_minute` — **replaceable in part**: the admission decision (refuse inconsistent evidence; choose `E = max(floor(t / 60), L)`; charge 600 and 12 in `E`; never grant a unit the retained counters do not leave) is a pure function over a few integers and a list of wallet rows, and a Verus lemma could state that it never reopens a spent unit or moves the minute backwards. SQLite reads, writes and transactions are not.
- `billing_store::tests::challenge_rate_older_and_reordered_samples_spend_only_the_committed_minute`, `challenge_and_quote_clocks_survive_each_others_later_maintenance`, `failed_future_minute_challenge_rolls_back_watermark_counters_and_cleanup`, `inference_owner_authentication_test::wallet_challenge_rate_keeps_older_and_reordered_samples_in_the_committed_minute` and `inference_proposal_quota_test::challenge_ledger_rate_holds_600_601_across_older_samples_and_later_proposal_cleanup` — **not replaceable**: competing SQLite connections, transaction rollback with an injected trigger, reopened ledgers, signed proofs and the other ledger operations' cleanup.

## Judged 2026-11-21 — non-reserving quotes: paid admission and free openings

- `billing_store::tests::paid_lease_bound_is_strict_at_deadlines_and_replay_never_renews_a_lease` — **replaceable in part**: the lease rule (a lease counts while its deadline is later than the transaction time; at most 10,000 count; a replay returns the stored lease and never changes its deadline) is a pure function over integers and a list of deadlines that a Verus lemma could state. SQLite and reopening are not.
- `billing_store::tests::paused_paid_registration_holds_the_final_lease_check_until_its_acknowledgement_commits`, `paused_free_opening_holds_the_final_owned_and_ledger_slot_checks_until_its_account_commits` and `free_openings_compete_for_the_final_owned_and_ledger_slots_across_connections` — **not replaceable**: their property is that the capacity check and the insert share one SQLite write transaction across threads and connections, which Verus does not model.
- `inference_funding_admission_test::overbooked_quotes_change_no_opening_capacity_and_still_need_admission` and `native_accepted_original_recovers_after_expiry_restart_and_refill_until_safe_credit` — **not replaceable**: routers, owner proofs, reply loss, restarts, the native consumer journal and a scripted chain.

## Judged 2026-11-21 — exact presentation of large counters and wei

- `desktop::tests::webview_integers_beyond_double_precision_become_exact_decimal_strings` — **replaceable in part**: the conversion rule (an integer whose magnitude exceeds 2^53 - 1 becomes its decimal string; every other value is unchanged; the transformation is structure-preserving) is a pure recursive function over a JSON tree that a Verus lemma could state over a model type. `serde_json::Value` itself is external to Verus.
- `desktop::tests::account_and_receipt_statements_reach_the_webview_as_the_shared_exact_fixture`, `stream_usage_reaches_the_webview_channel_as_exact_text`, `inference_stream_forwards_provider_usage_to_the_channel_exactly`, `native_accounts_and_request_keys_reach_the_webview_exactly_through_dispatch` and `inference_account_presentation_test::cli_and_native_files_keep_large_counters_wei_and_receipt_watermarks_exact` — **not replaceable**: Tauri channels and IPC text, protected files, processes, routers and signed receipts.
- `tests/exactStatements.test.ts` — **not replaceable**: TypeScript and rendered Svelte pages, outside Verus's scope.

## Judged 2026-11-21 — native owner-challenge expiry window

- `consumer::tests::native_challenge_expiry_window_is_exactly_now_exclusive_to_now_plus_600` — **replaceable**: `validate_challenge`'s lifetime and nonce rule (`now < expires_at <= now + 600`, a nonempty nonce of at most 256 bytes) is pure integer and length arithmetic that a Verus `ensures` clause could state for every `now` and `expires_at`, including the overflow-free upper bound; the hexadecimal decode and byte comparison would need a model of `hex::decode`.

## Judged 2026-11-21 — account receipt distinctions and persisted-record reads

- The balance identity behind `billing_store::balanced` and the native `account_matches` (funded + provider loss = reversed + fee + spent + reserved + available, in wide arithmetic over canonical wei, with the waiver validated but never a term) — **replaceable in part**: the identity, its overflow-free evaluation over U256 and the rule that the waiver is excluded are pure integer arithmetic a Verus `ensures` clause could state; canonical-decimal parsing would need a model of the string decoder.
- `inference_account_records_test::signed_receipts_keep_original_terms_waived_usage_unknown_exposure_and_funding_loss_distinct` and `persisted_schema3_and_native_records_read_exactly_and_corruption_fails_closed` — **not replaceable**: SQLite ledger rows, protected native files, routers, re-signed receipts and byte preservation across reopening are outside Verus's model.

## Judged 2026-11-21 — callouts wrap long exact values

- `tests/exactStatements.test.ts` › `Usage wraps the exact provider-loss amount, which appears only in its warning, at phone width` — **not replaceable**: rendered Svelte markup and CSS layout, outside Verus's scope.

## Judged 2026-11-21 — non-reserving quote cutover (opening-intent 2.3.2)

- `inference_proposal_test::quote_work_and_committed_issuance_keep_separate_counters_clocks_and_failure_charges` — **replaceable in part**: the rule that each budget's effective minute is max(floor(t/60), its own watermark), and that a refusal of one budget never moves the other's counter or watermark, is pure state-transition arithmetic a Verus model of two independent counters could prove; the SQLite transactions, routers and reopening are outside it.
- `inference_funding_admission_test::unrelated_quotes_beyond_both_former_wallet_ceilings_leave_the_owner_only_its_own_rules` — **not replaceable**: HTTP routes, owner proofs, ledger tables and signed quotes.

## Judged 2026-11-21 — credential expiry during an admitted hold (request-accounting 2.2.4)

- `billing_store::tests::credential_expiry_admits_through_the_last_second_and_refuses_new_holds_from_expires_at` — **replaceable in part**: the admission predicate (live iff `now < expires_at`, unrevoked and scoped) is a pure comparison a Verus `ensures` clause could state for every `now`; the unchanged-ledger assertions over SQLite are outside it.
- `billing_store::tests::expired_credential_hold_settles_once_against_its_original_accounting` — **replaceable in part**: that settlement moves exactly the original hold between the original account's and credential's reserved, spent and available amounts, independent of credential liveness, is conserved integer arithmetic over a ledger model; the reopened SQLite state and first-terminal-result persistence are not.
- `billing_store::tests::expired_credential_requests_stay_readable_and_unduplicable_through_the_routers` — **not replaceable**: HTTP routers, bearer and owner-proof authentication and signed receipts.

## Judged 2026-11-21 — invalid usage and inference-budget edges (request-accounting 2.5)

- `prompt_count_mismatch_commits_one_invalid_usage_release_of_the_whole_hold` and `cumulative_usage_overflow_commits_an_invalid_usage_release_without_wrapping` — **replaceable in part**: the settlement classification (charge iff the prompt count equals the prepared count, output is within the bound and both cumulative u64 counters can absorb the usage without overflow; otherwise release the whole hold with no counter change) is pure checked integer arithmetic a Verus `ensures` clause could state; persistence and first-result-wins across reopening are not.
- The five budget tests (`credential_inference_budget_*`, `account_unknown_usage_budget_*`, `inference_budgets_count_future_dated_records_*`, `inference_budget_windows_reach_back_*`) — **replaceable in part**: admission iff fewer than 120 (or 30) records with `created >= now.saturating_sub(60)` (or 3600) is a counting predicate over a modeled record multiset that Verus could prove for every `now`, including saturation and regression; the SQL query (including the `GLOB` suffix match) and SQLite state are outside the model.
- `exhausted_inference_budgets_do_not_strand_admitted_holds` — **not replaceable**: settlement and restart recovery over a reopened SQLite ledger.

## Judged 2026-11-21 — request identity and attributable recovery reads (request-accounting 2.3)

- `request_pages_and_receipts_hold_the_100_newest_account_requests_newest_first` and `timestamp_before_pages_can_omit_requests_sharing_the_boundary_timestamp` — **replaceable in part**: the page selection (the 100 greatest by (created, insertion) strictly before `before`, newest first) and its tie-omission consequence are properties of a sort-and-take function over a modeled list that Verus could prove; the SQL query and signed receipt are outside it.
- `receipt_sequences_are_durable_per_account_and_failed_issuance_commits_nothing` — **replaceable in part**: per-account strict monotonicity and exhaustion at i64::MAX are a counter-map invariant Verus could state; SQLite faults, commit refusal and reopening are not.
- `request_identity_and_attribution_survive_settlement_reopening_and_duplicates`, `a_receipt_waits_for_a_competing_settlement_and_binds_only_committed_state`, `request_and_receipt_reads_are_account_scoped_and_change_no_financial_row` and `completed_inference_retains_no_content_and_duplicates_never_replay_it` — **not replaceable**: SQLite locking and reopening, HTTP routers, bearer and owner-proof authentication, a loopback engine and files on disk.

## Judged 2026-11-21 — no automatic paid retry (request-accounting 2.3, last statement)

- `consumer::tests::lost_interrupted_and_cancelled_paid_inference_is_sent_once_and_reconciled_by_its_key` — **not replaceable**: sockets dropped mid-request and mid-stream, async cancellation, protected files and a reopened consumer.
- `tests/sdkExamples.test.ts` and `scripts/test_sdk_examples.py` — **not replaceable**: static analysis of JavaScript, Python and Svelte sources, outside Verus's scope.

## Judged 2026-11-21 — gateway and backend acceptance (request-accounting 3.1)

- `gateway_cumulative_usage_overflow_is_an_invalid_usage_release_not_a_settlement_failure` — **replaceable in part**: the classification of a ledger result (a committed invalid-usage release is terminal and records no fault; any other error records the fault and closes admission) is a pure function over an error enum that a Verus `ensures` clause could state; the HTTP answers, SSE events and SQLite ledger are outside it.
- `gateway_reserves_and_dispatches_exactly_the_prepared_prompt_and_failed_preparation_holds_nothing`, `gateway_duplicates_in_flight_racing_or_after_a_lost_reply_never_dispatch_or_hold_again`, `gateway_cancellation_charges_known_usage_and_releases_unknown_holds_at_provider_expense`, `gateway_failed_and_timed_out_responses_charge_known_usage_and_release_unknown_holds`, `gateway_settlement_failure_keeps_holds_stops_admission_and_keeps_existing_reads`, `gateway_usage_validation_releases_the_whole_hold_without_inventing_tokens`, `credentials_ending_during_generation_settle_against_their_original_accounting` and `app::tests::a_paid_settlement_failure_keeps_its_hold_until_a_restart_releases_it_before_admission` — **not replaceable**: HTTP routers, a loopback engine over sockets, async cancellation and deadlines, SQLite triggers and reopening, the real clock and a restarted daemon.

## Judged 2026-11-21 — admitted evidence through settlement (request-accounting 3.2)

- `billing_store::tests::admitted_reports_stay_bound_through_settlement_waiver_and_restart_across_a_renewal` — **replaceable in part**: renewal admission (identical serving terms and a strictly later measurement time than the agreement's report), and settlement accepting only an outcome whose digest equals the request's admitted digest, are pure predicates over modeled terms that a Verus `ensures` clause could state; the waiver arithmetic (inference cost waived, usage and fee unchanged) is conserved integer arithmetic over a ledger model. The SQLite rows, reopening, report files and signed receipt are outside it.
- `app::tests::a_restart_under_a_renewed_report_recovers_held_requests_with_their_admitted_report` — **not replaceable**: a restarted daemon, the HTTP gateway, a held loopback engine stream, timed waivers and SQLite triggers.

## Judged 2026-11-21 — quote budget independence (request-accounting 3.3)

- `exhausted_quote_signing_work_leaves_existing_requests_and_inference_gates_intact` and `exhausted_committed_quote_issuance_leaves_existing_requests_and_inference_gates_intact` — **replaceable in part**: that the inference admission predicate and settlement transitions read and write no quote-budget state, and that each quote budget's counter and minute change only under its own claim, is a frame condition over a modeled state that Verus could prove; the SQLite rows, pending-state cleanup, reopening and signed receipts are outside it.
- `exhausted_quote_budgets_refuse_public_proposals_while_gateway_inference_and_reads_continue` — **not replaceable**: HTTP routers, the real clock, bearer authentication and a loopback engine.

## Judged 2026-11-21 — inference settlement at full opening capacity (opening-intent 2.8.2.6)

- `full_paid_opening_capacity_leaves_existing_requests_and_inference_gates_intact`, `full_owned_free_opening_capacity_leaves_existing_requests_and_inference_gates_intact` and `full_global_free_opening_capacity_leaves_existing_requests_and_inference_gates_intact` — **replaceable in part**: that settlement, release and recovery transitions read and write no lease, account-count or quote state (a frame condition), and that the opening predicates (fewer than 10,000 active leases; fewer than 8 owned and 10,000 total accounts) are unchanged by them, could be proved over a modeled ledger state in Verus; the 10,000-row SQLite fills, reopening and signed receipts are outside it.
- `full_opening_capacity_leaves_gateway_settlement_reads_and_inference_intact` — **not replaceable**: HTTP routers, bearer scopes, the real clock and a loopback engine.

## Judged 2026-11-21 — acknowledgement alone and free opening without payment (opening-intent 2.8)

- `billing_store::tests::acknowledgement_alone_never_opens_or_credits_through_lease_release_expiry_and_reopening` and `billing_store::tests::free_and_paid_terms_never_cross_at_opening_intent_or_topup` — **replaceable in part**: that only `credit_funding` creates credit and fee obligations, that it requires verified payment evidence for paid terms and refuses any payment for free terms, and that lease release and expiry transitions never reach it, are reachability and precondition properties of a modeled state machine that Verus could prove; the SQLite snapshot, reopening and exact error strings are outside it.
- `inference_funding_admission_test::free_terms_refuse_paid_funding_intents_and_open_without_payment_or_rpc` and `inference_owner_consumer_test::native_refuses_crossed_free_and_paid_terms_before_any_request_or_payment` — **not replaceable**: HTTP routes, a scripted RPC listener, signed transactions and native consumer files.

## Judged 2026-11-21 — fresh HTTP admission matrix (opening-intent 2.2)

- `fresh_intent_admits_type_two_priority_fee_equal_to_its_signed_maximum` — **replaceable in part**: that the type-2 decoder accepts priority equal to the maximum and that the full-cost check compares value plus gas limit times maximum fee against the balance with checked arithmetic are pure functions Verus could prove; the HTTP route and scripted RPC are outside it.
- `fresh_intent_requires_identity_checked_healthy_chain_evidence_before_and_after_balance` — **replaceable in part**: the settlement-view health predicate (chain ID and genesis equality, not syncing, frontier at or below the head, ordered timestamps, both ages within bounds, same-height hash equality) is a pure function over observed values that Verus could prove; the ordering of RPC calls, the HTTP route and the ledger rows are outside it.
- `fresh_intent_owner_refusals_precede_chain_observation_with_every_gate_open`, `nonce_refusals_keep_the_earlier_acknowledgement_and_admit_the_unchanged_next_original` and `fresh_intent_gates_refuse_before_chain_observation_and_qualification_is_rechecked_after_balance` — **not replaceable**: HTTP routes, owner signatures, a scripted RPC listener and SQLite rows; that refusals precede any RPC is an ordering property of async I/O.

## Judged 2026-11-21 — exact native acknowledgement-time boundaries (opening-intent 2.7)

- The interval and future-guard predicate of `funding_intent_matches` (`issued_at ≤ registered_at < expires_at` and `registered_at ≤ now + 30` with saturating addition) is a pure function of four integers that Verus could prove exactly, including saturation at `u64::MAX`. That would replace the boundary arithmetic exercised by `native_acknowledgement_accepts_exact_registration_time_edges` and `native_acknowledgement_refuses_one_second_past_each_edge`, but not their native checks — **replaceable in part**.
- The native parts of those tests, `native_saved_acknowledgement_survives_quote_expiry_at_exact_receiver_edges` and `native_acknowledgement_future_guard_reads_the_receiver_clock_on_arrival` — **not replaceable**: the HTTP round trip, the point at which the clock is read, the saved file and journal bytes, and reopening.

## Judged 2026-11-21 — client acknowledgement and start-state persistence (opening-intent 2.5)

- `native_broadcast_start_write_failure_after_durable_acknowledgement_never_submits` and `native_transient_acknowledgement_write_failure_ends_the_attempt_before_the_start_write` — **replaceable in part**: that the native opening state machine reaches submission only from a state with a verified durable acknowledgement and a durable start marker, and that any failed write ends the attempt, is a reachability property of a modeled state machine Verus could prove. The real rename failure, the file and journal bytes, reopening and the mock provider and RPC are outside it.

## Judged 2026-11-21 — native restart recovery (opening-intent 2.6)

- `chain_client::tests::reopened_original_survives_fee_and_nonce_changes_and_refuses_other_terms` — **replaceable in part**: that retrieving an existing operation returns the journaled bytes regardless of fee or nonce inputs, and refuses an operation whose recipient, value or commitment differs, is a function of a modeled journal that Verus could prove; the SQLite journal, reopening and signing are outside it.
- `native_unprepared_terms_are_refused_exactly_at_quote_expiry` and `native_expired_terms_create_no_payment_and_keep_an_unaccepted_original` — **replaceable in part**: the guard (refuse preparation when no original exists and `expires_at ≤ now`) is a pure predicate Verus could prove; the native files, mock provider and scripted RPC are outside it.
- `native_journal_commit_before_local_copy_recovers_only_that_original`, `native_saved_account_recovery_refreshes_without_reopening_or_replacing_credentials` and `native_prepared_original_is_never_resigned_after_fee_or_nonce_changes` — **not replaceable**: reopened native files and journals, HTTP requests to the mock provider and credential secrets.
