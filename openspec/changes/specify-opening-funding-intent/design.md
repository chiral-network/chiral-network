## Context and scope

Authority is the unchanged [book](../../../docs/chiral-book.md), especially **Shared signing-wallet directory**, **Agreements and exact funding order** and **Settlement and recovery risks**. The new capability specifies the paid opening's pre-credit protocol: one original, durable provider acceptance, client acknowledgement-before-broadcast and original-operation recovery. It does not certify the whole financial lifecycle.

Reuse `inference-wire-foundation` for identity, money, signing and opening calldata; the pending commercial change for quote/execution terms; owner-control for one-use authority; and canonical `inference-proposal-admission` for the atomic standalone-quote retention handoff. Do not duplicate their protocols or change free-opening/top-up semantics. Safe-frontier settlement, reversal/loss accounting, explicit cancellation, fees and credential lifecycle need their own complete acceptance.

## Decisions

### One original, two protected persistence boundaries

The consumer saves selected signed terms, then `ChainClient::prepare_payment_once` owns the original in the shared protected wallet journal. Consumer saved-account state and that journal are not one atomic store: the recoverable gap is journal commit before local raw-byte/hash save. Resume reads the original journal first; a missing consumer copy can be recovered, but an existing copy disagreeing with the journal is not permission to sign again.

Retain the stable `open:<quote_id>` operation and the existing network/wallet storage scope. A provider deployment directory is not a replacement shared nonce journal. Processes with separate journal copies need separate signing wallets; this is not a multi-host SQLite coordinator. Existing terminal/reorg nonce ownership remains in force.

No new database, provider escrow contract, operation identifier, schema conversion or compatibility shim is proposed. Native estimation uses the actual eSpace RPC estimate, bounded by the structural minimum and 1,000,000 gas, rather than a guessed multiplier. Provider admission uses the full signed limit and maximum price, including typed envelopes, not merely the value or minimum intrinsic gas.

### Acknowledgement binds a hash, not a newly invented quote digest field

The existing six-field `FundingIntent` is enough: its original transaction hash binds signed bytes whose calldata commits to the complete quote. The consumer must validate those bytes against its own selected agreement, not treat an opaque hash or provider response as a replacement for that reconstruction. Preserve the existing INTENT domain and strict payload/envelope decoding.

Require registration within the original signed quote interval. Preserve the consumer's 30-second future guard as a receiver plausibility check, not a 30-second acknowledgement lifetime. Recovery after quote expiry remains valid if registration was timely; independent clock-plausibility checks still apply.

Original raw representation is deliberately strict: the provider's retained string must be replayed verbatim. Uppercase hex may decode to the same transaction during initial envelope validation, but changing the string after acceptance is a conflict, not permission to update the stored original. Native recovery already retains that original string.

### Fresh admission is not cached recovery or a chain reservation

The production control handler authenticates and binds the payer before checking for an existing acknowledgement. Fresh admission then checks funding/readiness and compatible qualified execution, obtains current chain evidence, checks qualification again, and enters ledger registration with fresh availability observations. These checks are not a lock spanning an external engine, operator flags and the chain; do not promise commit-time engine liveness or indefinitely reserved chain balance.

Preserve the existing 10,000 active-intent ceiling. An active lease is an unopened agreement whose admission deadline is strictly later than registration time. Its deadline is the quote's original expiry, not a sliding retry timeout. Historical evidence and permanent sender/nonce ownership do not consume that active lease after release. This neither promises inference concurrency nor replaces proposal/challenge quotas.

Cached acknowledgement recovery occurs before new readiness, qualification and RPC gates, but after owner authorization. It returns the original signed acknowledgement without refreshing lease/time or allocating a new pending proposal. Draining, expiry, a now-spent nonce and an unavailable RPC do not revoke earlier acceptance. Exhausted challenge/control admission may still delay fresh authority; durable retention is not an availability guarantee.

### Producer and consumer registration-time consistency

`BillingStore::register_funding_intent` correctly samples its clock after acquiring the immediate transaction and rejects `now >= expires_at`. It currently lacks a `now < issued_at` rejection. `consumer::funding_intent_matches` already refuses registration before issuance. [INFERENCE] A sufficiently regressed provider time can therefore persist an acknowledgement the native consumer will never accept, rather than refusing the new registration. This is a source-derived gap, not an executed regression or live incident.

Choose fail-closed admission before issuance, not fabricated/clamped timestamps or weakened client validation. The native fix and deterministic regression must be committed together when independently committable. It is separate from proposal/challenge rate watermarks: none of those logical clocks may rewrite signed funding time. Lock-wait expiry acceptance also needs a deterministic seam/barrier, not a wall-clock sleep.

### Durable disclosure and ambiguous outcomes

The provider's immediate SQLite transaction owns acknowledgement/quote/raw evidence, nonce uniqueness and lease retention; success returns only after commit. Reuse the archived proposal-slot transfer contract instead of creating a second quota convention. Signing, insertion or commit failure must not produce success or partially release retention.

The client verifies the acknowledgement, durably saves it, durably saves broadcast-started intent, and sends the original journal entry. Broadcast-started describes possible submission, not inclusion or credit. A lost/mismatched RPC response leaves ambiguity; recovery reconciles and, when allowed, resends that original. It does not sign a fee bump or prepare another payment.

The provider receives broadcastable signed bytes before the consumer sees an acknowledgement. Nothing here prevents provider-side broadcast or makes funding refundable. The existing [CLI workflow](../../../README.md#developer-api-quick-start) discloses this and directs `pending`/`resume`, not a new quote after timeout. This change preserves that boundary rather than adding a misleading escrow/atomic-payment claim.

## Source and existing test correspondence

These are inspected source/test assertions, not a claim that the listed tests exhaust the new capability. Unit targets live in owner-untracked files; executable checks for this turn are recorded in worker progress.

|Area|Production path|Existing test evidence|
|---|---|---|
|Saved terms and journal original|`Consumer::fund`, `resume_inner`; `ChainClient::original_payment_raw`, `prepare_payment_once`, `payment_once`|Consumer `funding_crash_boundaries_resume_original_bytes_and_accepted_expired_terms`; chain `prepare_is_durable_never_broadcasts_and_reserves_original_nonce`, `durable_intent_restart_and_nonce_serialization`|
|Fresh preparation prerequisites|`ChainClient::payment_once`, `estimate_gas`, sender journal locking|Chain `preparation_checks_chain_balance_and_nonce_before_persisting`, `cross_process_wallet_lock_is_exclusive`|
|Acknowledgement schema/binding|`protocol::{FundingIntentRequest,FundingIntent}`; consumer `funding_intent_matches`, `verify_funding_intent`, `load_account`|Consumer `funding_acknowledgements_bind_all_terms_and_survive_quote_expiry`; strict message-field/type acceptance still needs an explicit inventory|
|Signed envelope/full cost|`ChainClient::decoded_payment`, `admit_payment_envelope`|Chain `envelope_validation_is_local_and_binds_every_payment_field`, `signed_intent_admission_requires_current_nonce_and_entire_gas_cost`; pending-nonce cases|
|HTTP new versus cached admission|`contracts::control` funding-intent branch; `ProviderState::qualified_model`; ledger `require_execution`|Ledger `control_acknowledges_only_spendable_current_nonce_and_recovers_without_rpc_or_broadcast`; integration `funding_intent_authority_is_required_before_new_admission_and_cached_acknowledgement`|
|Expiry, capacity and permanence|`BillingStore::register_funding_intent`, `cleanup_pending`, `record_funding_attempt`; sender/nonce trigger|Ledger `registration_rejects_expiry_wrong_owner_network_and_replacement`, `pending_capacity_rejects_new_intents_without_losing_accepted_obligations`, `equivalent_intents_reuse_ack_and_conflicting_sender_nonce_survives_quota_expiry`|
|Atomic registration and competition|`BillingStore::transaction`, `register_funding_intent`; acknowledged proposal-slot handoff|Ledger `concurrent_intent_admission_cannot_acknowledge_two_agreements_for_one_nonce`; integration `acknowledgement_frees_exactly_one_slot_and_replay_preserves_its_replacement`, `rejected_acknowledgements_preserve_pending_capacity_and_allow_original_registration`|
|Persistence before client submission|Consumer `save_account`, `verify_funding_intent`, `resume_inner`; chain `send_payment_once`, `broadcast`|Consumer `missing_malformed_or_mismatched_ack_never_broadcasts`, `paid_funding_persists_ack_before_broadcast_and_free_open_skips_intent`; chain `ambiguous_broadcast_retains_original_evidence`|
|Original recovery and saved credentials|Consumer `load_account`, `resume_inner`, `refresh_account`|Consumer `missing_journal_never_replaces_a_saved_original_transaction`, `expired_unaccepted_quotes_never_broadcast_or_replace_preparation`, `resumed_open_accounts_preserve_exported_credentials_and_refresh_funding_status`|
|No implicit paid registration/credit|`contracts::control` open branch; ledger `check_open_funding`, `open_account`|Ledger `paid_open_never_registers_implicitly_even_before_expiry`, `preexpiry_funding_intent_preserves_quote_through_settlement_stall_and_restart`; owner-control opening authorization cases|

Source modules: [`consumer.rs`](../../../src-tauri/src/consumer.rs), [`chain_client.rs`](../../../src-tauri/src/chain_client.rs), [`contracts.rs`](../../../src-tauri/src/contracts.rs), [`billing_store.rs`](../../../src-tauri/src/billing_store.rs) and [`protocol.rs`](../../../src-tauri/src/protocol.rs). Unit evidence is in `consumer.rs`, `chain_client_tests.rs` and `billing_store_tests.rs`; integration evidence is in `inference_owner_control_test.rs` and `inference_proposal_quota_test.rs` under [`src-tauri/tests`](../../../src-tauri/tests).

## Task 2.1.1 request-object field acceptance

`inference_owner_control_test::funding_intent_object_fields_are_strict_before_new_admission_and_cached_recovery` adds real-router coverage for nine request-object variants: absent `raw_transaction`, null, boolean, integer, fraction, array-valued field, object-valued field, an unknown member and duplicate `raw_transaction` members. Each uses the real owner's proof bound to those exact body bytes and is exercised against both a new proposal and a previously acknowledged agreement.

Separate isolated ledgers prevent the eight pending challenges or minute rate budget from masking schema rejection. Every malformed object must return HTTP 400 / `invalid_request`; a subsequent correctly shaped request with fresh authority reaches the draining gate for a new intent or returns the exact provider-signed cached acknowledgement. The fresh branch deliberately stops before RPC: it is not successful spendability/admission evidence.

After both requests an independent ledger connection checks the exact signed quote and optional acknowledgement, absence of a paid account/credited funding/fee obligations, and unchanged unrelated free-account/credential state. Existing root and read bearers remain usable. Cached prerequisites use the existing independently signed, offline-validated envelope and trusted post-spendability ledger boundary, not constructed chain execution. No engine, RPC, actual payment or client broadcast is exercised.

The existing consumer `funding_acknowledgements_bind_all_terms_and_survive_quote_expiry` covers signed network/provider/payer/quote/hash substitutions, wrong signer/domain and invalid registration times; `missing_malformed_or_mismatched_ack_never_broadcasts` covers unavailable/malformed/hash-mismatched/lost replies with original recovery. They do not replace exhaustive native response-schema acceptance, which remains task 2.1.3.

### Request container mismatch

A throwaway executable linked to the built core observed both `{"raw_transaction":"0x00"}` and `["0x00"]` decode successfully as `FundingIntentRequest`, while an extra object member is refused. The signed-envelope validity of `0x00` was not tested or asserted. `contracts::parse` uses `serde_json::from_slice` directly for this request; the derived struct decoder's sequence form does not enforce the specified JSON-object container.

Task 2.1.2 requires a native object-only guard plus regression when the owner-untracked parser/protocol source is independently committable. The probe is decoder evidence, not an HTTP exploit or financial acceptance result. It was removed; no permanent test blesses sequence acceptance, no specification is weakened and the request-object cases above do not claim this missing guarantee. Parent task 2.1 stays open.

## Task 2.1.3.1 native acknowledgement object-member acceptance

`inference_funding_acknowledgement_test` adds a native `Consumer::fund` fixture around the production provider router. It uses a real quote, an independently signed/offline-validated original EIP-155 opening and a real ledger-signed cached acknowledgement. One prepared journal row and prior post-spendability registration are explicit prerequisites; this fixture does not observe initial transaction preparation or fresh chain admission. The provider is draining, so recovery must use the original acknowledgement rather than create a replacement.

Adversarial middleware changes only successful acknowledgement replies after real owner authentication. Four test groups cover 33 malformed documents: unknown members at envelope/payload/network levels; each of the eleven required members missing; each member mistyped plus five additional invalid timestamp types; and identical duplicate members at all three object levels. The timestamp numeric-text case preserves the original value to catch permissive coercion. Duplicate cases preserve otherwise valid signed content, rather than relying on signature mismatch to reject them.

Each refused reply must leave the protected native acknowledgement absent and broadcast-start marker false, preserve the exact quote/raw/hash and one prepared original journal operation, and make no request to the separate consumer RPC endpoint. Reopened provider storage must retain its original quote/acknowledgement without an account, credited funding or fee obligations. The native consumer is then reopened and the genuine reply restored: it must recover the same original, persist the exact acknowledgement and start marker before reaching RPC. The RPC handler snapshots that local state and always returns HTTP 503; no transaction submission, successful chain response, paid account or inference is fabricated.

The isolated loopback test groups are `native_acknowledgement_rejects_unknown_object_members_before_chain_access`, `native_acknowledgement_rejects_missing_object_members_before_chain_access`, `native_acknowledgement_rejects_mistyped_object_members_before_chain_access` and `native_acknowledgement_rejects_duplicate_object_members_before_chain_access`. Existing native signed-identity substitution tests remain relevant; this slice is not exact freshness-boundary, initial spendability, power-loss durability or live-payment qualification.

### Acknowledgement container mismatch

A separate throwaway built-core probe observed that positional sequences for the signed envelope, funding-intent payload and nested network all deserialize and verify the original provider signature after projection into typed objects. These are decoder/signature observations, not native funding or broadcast observations. The production native decoder uses `serde_json::from_slice` directly. Task 2.1.2 therefore includes those response-object guards as well as the known request-object guard; parent 2.1.3 remains open until they are enforced with regressions. The probe was removed; no permanent test endorses the mismatch and no requirement is weakened.


## Acceptance gaps and evidence limits

- Audit existing schema/signature/envelope tests before adding consumer-visible missing cases. Prefer worker-owned integration targets and real fixture signatures, explicit times and isolated disk state. Do not copy owner-untracked modules into the commit or widen production APIs solely to bypass ownership.
- The existing missing-journal test saves synthetic `0x00`; add a valid signed original missing its journal and a different valid journal original. The source's broadcast-start-without-ack guard also needs direct acceptance. No newly prepared replacement or outgoing submission may occur in those failures.
- Add the pre-issuance rejection with its native fix; cover the exact lower/upper registration boundary and lock-wait expiry without sleeps. Exact native 30-second freshness edges require deterministic time control; broad invalid-future checks must not be presented as that boundary proof.
- Reuse retention/nonce/capacity tests, but distinguish their constructed/direct-ledger prerequisites from HTTP admission. Add any missing acknowledgement transaction-failure and post-commit response-loss observations without fabricating credit. An internal ledger call assumes prior owner, signed-envelope and spendability admission; it is not a public bypass.
- Consumer crash tests construct five checkpoints and restart objects; their provider/RPC fixtures are isolated, with provider acknowledgements held in fixture memory. They prove client ordering and byte reuse, not real-provider power-loss durability. The fixture inspects saved acknowledgement/start state at broadcast; this is stronger than an event-name ordering assertion alone.
- Provider router tests use real signatures and synthetic local RPC responses. Lower-level ledger fixtures sometimes construct `VerifiedPayment`; newer quota fixtures independently validate signed envelopes but assume spendability. None is observed live funding, healthy public safe-frontier operation, real execution or a crash/fsync campaign. Unix parent-directory sync is source evidence; non-Unix durability needs platform qualification.
- Existing saved-account native recovery preserves the exported credential. Losing the opening response before local account persistence follows the separate HTTP reopening path, whose root-rotation mismatch remains blocked in owner-control task 2.4.3. Do not turn the narrower passing native test into proof of that missing guarantee.

## Ownership, verification and integration

The change began with a specification and executable acceptance plan; subsequent worker-owned acceptance is recorded above. The production parser/protocol, ledger, consumer, chain client and their native unit tests are owner-untracked migration work. Required request/acknowledgement object containers, the pre-issuance admission fix and any private time/fault seam remain blocked until independently committable; do not commit a failing regression alone, normalize away a flaw or adopt the migration wholesale. Independent worker-owned acceptance can proceed.

Run strict noninteractive validation and all available README/CI checks. Keep platform dependencies, the actual release policy key, real paid-chain/SDK/GPU/public failure/load evidence as separate unavailable/out-of-checkout gates. No deployments, wallet/account changes, installs or whitepaper edits.

After acceptance, record the final requirement-to-source/test map and client/operator consequences. Archive with `openspec archive specify-opening-funding-intent --yes` only after this change's tasks and checks are complete, never `--skip-specs`. Preserve the other changes' blockers; this slice does not close full financial settlement or opening/credential recovery.
