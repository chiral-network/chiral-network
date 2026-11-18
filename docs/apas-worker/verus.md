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

Requirement source: `openspec/changes/specify-profile-path-resolution/specs/profile-path-resolution/spec.md`.

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
