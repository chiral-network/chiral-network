## Context

The wire foundation is archived and its main spec is available. The README-designated whitepaper remains `docs/chiral-book.md`, SHA-256 `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`, read-only. This change specifies the next dependency layer: complete commercial declarations and how a consumer accepts a quote.

No current failing committed-code check takes priority: the preceding all-targets Rust run and available frontend/Python/build commands passed. Most runtime sources are nevertheless pre-existing modified/untracked work. Read them for evidence but do not commit or overwrite that work.

## Goals / Non-Goals

Goals:
- Cover the complete typed offer/model/profile/quote vocabulary without duplicating the foundation's signing/money algorithms.
- Distinguish receiver syntax/consistency checks, native issuer policy, native deployment qualification and consumer agreement checks.
- Preserve immutable accepted terms and the narrow evidence-only renewal exception.
- Expose the native embedding output-price gap as work to prove and fix, not silently declare it implemented.

Non-goals:
- Deploy a provider, qualify real hardware, collect funding, change an account or alter the whitepaper.
- Specify DHT pagination/revision reconciliation, DNS pinning transport implementation, full funding/nonce/chain state machines or runtime measurement/waiver accounting in this change.
- Tighten arbitrary remote quote lifetime/nonce syntax simply because the local issuer emits a narrower form; remove existing recovery paths; or reinterpret legacy state.

## Decisions

1. **Layered acceptance, not a universal metadata validator.** `ModelSpec` describes broad signed capabilities; native GGUF tasks, qualified generation profiles and public production admission have stricter rules. For example, base declaration validation alone is not proof that a runtime can jointly execute its advertised APIs. Preserve that boundary rather than turning receiver syntax into a false qualification claim.
2. **Reuse strict wire primitives.** All fields use `inference-wire-foundation` representations. Nullable model options retain the typed codec's canonical null representation; no alternate signing serializer or public schema aliases are introduced.
3. **Preserve issuer/receiver distinctions.** Local offers normally last 120 seconds while receivers accept at most 3,600; local quotes last 300 seconds while current native acceptance checks future skew, ordered timestamps and context-dependent freshness. Native issuance generates random quote IDs/nonces; verification binds the supplied signed nonce rather than proving its randomness. No additional receiver lifetime/nonce rule is invented here.
4. **Record currently implemented profile limits.** The whitepaper names exact fields and delegates detailed consistency to the implementation reference. Preserve existing text/digest limits, platform/backend pairing, memory accounting, device uniqueness, split/cache/batch rules and workload bounds. Declaration validity still is not measured evidence.
5. **Enforce input-only embedding terms at the native boundary.** The whitepaper explicitly requires zero output limit/rate. A provisioning-only check does not protect a directly supplied provider configuration. Add a focused regression and enforce the same rule before native publication/admission; do not merely hide a nonzero rate in the UI or normalize it to zero.
6. **Keep quote formation separate from settlement authority.** Public proposal is intentionally unauthenticated; its result is a durable signed proposal, not owner authorization or credit. New-funding readiness can be closed while accepted balances/recovery remain available. Historical expiry decisions belong to durable funding state, not blanket freshness bypasses for new onboarding.

## Inspected source and coverage

| Boundary | Source inspected | Evidence / remaining work |
|---|---|---|
| Complete schemas and immutable model/profile comparison | `protocol.rs::{InferenceOffer,ModelSpec,ServingProfile,EngineDevice,QualificationIdentity,Quote}`, `ModelSpec::accepts_execution` | Exact field types and unknown-field rejection are present. Foundation tests cover primitive schemas/signatures/commitments; add missing commercial schema cases without duplicating them. |
| Offer/model admission | `offer_registry.rs::{validate_model,OfferRegistry::insert_at}` | Bounds, wallet/network/signature, origin, time, fee, revision and profile concurrency checks are present. Existing tests cover tampering, expiry/network, embedding output limits and revision behavior; add only uncovered boundary cases. |
| Serving declaration | `performance.rs::validate_profile` and its tests | Per-device capacity, unified memory, generation-only policy and term equality have useful coverage. Add missing platform/digest/execution-setting boundary cases, not a synthetic speed-qualification claim. |
| Native configuration | `provider_config.rs::ProviderConfig::validate` | Production generation profiles, evidence presence, shared-device/mixed-profile refusal and unique model IDs are enforced. Rates are checked only for canonical money; no embedding-specific zero-output-price condition is present in the inspected native path. |
| Managed task/artifact | `engine_runtime.rs::{EngineRuntimeConfig::validate,validate_inventory}` | Checks GGUF/tokenizer revision equality, separate task capabilities, selected template and immutable launch/profile identity. It does not inspect output price. The inspected inventory path selects an inventoried template without an embedding-specific prohibition; exercise this native boundary and close the template gap before claiming the embedding contract complete. |
| Reference provisioning | `scripts/provision-provider.py:187–214` | Embedding task explicitly requires zero output tokens and `output_wei_per_1k == '0'`, and excludes generation qualification. This alone does not enforce direct native configuration. |
| Quote formation and bounds | `contracts.rs::control` proposal branch, `accepting`; `billing_store.rs::{store_quote,validate_quote}` | Public proposal copies exact terms, uses native 300-second expiry, persists before return and enforces free/paid funding. Control readers are bounded. Quote storage already has per-minute limits of 600 global/12 per payer and outstanding limits of 10,000 global/8 per payer; retain these in follow-on work. |
| Consumer acceptance and persistence | `consumer.rs::{verify_quote,quote,fund}` | Checks both signatures, selected full terms/payer, returned funding amount, freshness for new onboarding and protected transport before funding. New funding revalidates the offer through the registry; existing saved terms cannot be overwritten. |
| Existing regression coverage | `consumer.rs::tests::provider_signed_quotes_still_must_match_selected_terms_and_payer`, funding acknowledgement/recovery tests, `performance.rs::tests::evidence_renewal_cannot_change_any_agreed_model_or_serving_term` | Preserve these tests. Expand uncovered signed-but-wrong quote dimensions and provider proposal behavior without relying only on signature tampering. |

## Historical acceptance correspondence

The commercial recovery review reuses existing financial tests rather than treating this change as the full funding state-machine specification:

| Obligation | Acceptance coverage | Evidence boundary |
|---|---|---|
| Fresh expiry refusal versus historical loading | `inference_proposal_test.rs::historical_saved_agreements_recheck_authenticity_before_recovery` | Starts with native zero-funded onboarding, then deliberately re-signs/ages only the saved fixture envelopes. Offer-only, quote-only and joint expiry remain readable/recoverable, but a distinct unsaved account ID is refused before HTTP. This is controlled consumer persistence coverage, not elapsed-time provider retention or paid settlement evidence. |
| Historical authenticity is not bypassed | The same historical case | For each expiry state, bad offer/quote signer/domain and signed network, payer/provider, account/offer identity, commercial-term, validity and origin substitutions must fail account listing, secret export and resume after consumer recreation. Refusal makes no HTTP request and preserves the invalid bytes for recovery rather than silently repairing/deleting them. Restoring valid fixture records preserves the actual provider account and original bearer. |
| A new funding call cannot overwrite saved terms | `inference_proposal_test.rs::saved_signed_agreements_reject_conflicting_funding_calls_without_remote_side_effects` | Real free onboarding and validly re-signed conflicting input; native-written saved bytes and credentials remain unchanged before/after recreation. |
| Acknowledgement authenticity survives quote expiry | `consumer.rs::tests::funding_acknowledgements_bind_all_terms_and_survive_quote_expiry` | Checks network, parties, quote/hash binding, registration window, provider signer and operation domain. A signed acknowledgement is not canonical payment evidence. |
| Original accepted payment survives local checkpoints | `consumer.rs::tests::funding_crash_boundaries_resume_original_bytes_and_accepted_expired_terms`, `missing_malformed_or_mismatched_ack_never_broadcasts` | Local RPC/control fixtures exercise preparation, acknowledgement persistence, broadcast and repeated recovery. They are not OS-crash or live-chain qualification. |
| Expiry without acknowledgement cannot authorize payment | `consumer.rs::tests::expired_unaccepted_quotes_never_broadcast_or_replace_preparation`, `missing_journal_never_replaces_a_saved_original_transaction`, `expired_unacknowledged_opening_recovers_only_by_explicit_durable_cancellation` | Preserve original bytes and refuse new automatic payment/replacement; cancellation is explicit, durable and distinct from resume. Full cancellation/reorg financial semantics remain separate specification work. |
| Recovering an open account does not rotate its credential | `consumer.rs::tests::resumed_open_accounts_preserve_exported_credentials_and_refresh_funding_status` and both production-router cases above | Existing paid/free control fixtures plus real zero-funded account checks retain the exported bearer. This does not qualify a paid provider deployment. |

`Consumer::load_account` retains signature/network/party/term checks through `verify_quote(..., false)`, then verifies any funding acknowledgement. `resume_inner` separately enforces original journal ownership and acknowledgement-before-broadcast; freshness is not replaced with a blanket recovery bypass. The new fixture does not modify the provider's original signed quote, fabricate acknowledgements/credit or weaken fresh acceptance. Exact now-relative admission boundaries remain under the separate explicit-time-seam task.

## Implementation gap and ownership constraint

Native embedding output-price enforcement is an observed source-level gap: provider configuration parses any canonical output rate, the managed runtime inventory path has no rate check, and only the reference provisioner enforces zero. The next implementation slice must demonstrate rejection/acceptance at the real native configuration boundary and close it there; no live provider or payment is necessary.

`provider_config.rs`, `engine_runtime.rs`, `contracts.rs`, `consumer.rs`, `billing_store.rs`, `offer_registry.rs`, `performance.rs` and `protocol.rs` include pre-existing uncommitted work (several are entirely untracked). A new standalone integration test can be committed independently. A source fix must not absorb the owner's surrounding uncommitted implementation. If the necessary native-boundary edit cannot be committed separately, record that source-ownership blocker, leave the fix task unticked, and continue another unblocked slice rather than claiming the gap closed or publishing the owner's files.

## Verification Plan

- Strictly validate the proposal/delta before implementation.
- Use isolated fixtures and explicit time inputs where available; no real keys, downloads, inference, RPC or paid requests. Do not add flaky clock-edge expectations merely to test numeric limits.
- Test actual acceptance boundaries: valid signed-but-inconsistent offers/quotes, malformed nested declarations, native profile consistency and direct embedding configuration bypassing provisioning.
- Preserve existing tests, especially expired-but-acknowledged funding and evidence-only renewal. Do not reset state, normalize invalid prices or skip a failing path.
- Run the README/CI checks available locally, keeping Rust tests/build sequential. Native platform dependencies, actual release authority and live hardware/deployment gates remain separate unavailable/out-of-checkout checks.

## Migration Plan

This turn changes specifications only. Subsequent tasks add missing contract tests and the native embedding-price correction without migrating money or changing accepted agreement bytes. Archive normally only after implementation tasks and relevant checks pass; do not use `--skip-specs`. Strict declaration rules must not be retroactively used to discard acknowledged historical funding.

## Open Questions

No whitepaper-author decision is needed for this specification. The embedding rate rule is explicit; exact declaration limits and issuer/receiver distinctions are available in the whitepaper's referenced implementation. Existing source ownership is an implementation/commit boundary, not a request to change the whitepaper.
