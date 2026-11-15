## Scope and authority

The read-only book's “Identity, wire types, and signed metadata,” request lifecycle, funding/accounting and performance-accounting chapters require exact accounting, immutable original terms and attributable bounded receipts. Existing `inference-wire-foundation` deliberately leaves individual payload fields to their own schemas. Pending `inference-request-accounting` defines transitions and receipt snapshots, not the complete shared account member schema. This change supplies that dependency, not a new account lifecycle or complete receipt protocol.

The selected independently useful slice is `AccountView` plus the shared accounting `TokenUsage` leaf. Full receipt/request/performance/credential/opening-result envelopes remain separate work. This is a specification step: production and permanent test code are unchanged, and implementation tasks remain unticked.

## Decisions

### One typed accounting object, not a projection-dependent signature

Keep the current producer's 17 account members and two usage counters. Require one closed object schema for each payload, wherever it occurs. Positional arrays and extra account members currently accepted by derived deserialization are not emitted wire formats; retaining them would make acceptance depend on parser projection rather than a portable statement schema. The explicit object visitor already used by network/model/offer/quote/opening/top-up types is the intended implementation convention. Do not introduce a permissive alias, `Value` buffering layer, guessed defaults or a second public serializer.

This closes a detail the book leaves to the implementation; it does not change author-owned commercial or monetary policy. Extra unsigned account members are refused too, so the same type does not have contradictory signed/unsigned decoding modes. Genuine stored schema-3 objects already use the named producer representation. Never rewrite old financial state to force decoding success; unsupported/corrupt records remain protected and fail closed.

### Usage is an exact accounting leaf, not all OpenAI usage

`TokenUsage` has required `u64` prompt/completion counts and currently denies unknown members. Retain that range, explicit zero and known/unknown semantics. A missing field is not the same as internally constructing `TokenUsage::default()` for a separately identified unknown outcome. A valid zero-cost waiver still preserves authoritative known counts.

OpenAI-compatible responses may include `total_tokens` and other endpoint-specific details. Their raw usage schema and validation are not replaced by this closed two-counter accounting object. Counter syntax is not evidence that supplied engine counts are authoritative or within a reservation. Any limited-precision client/export path must preserve signed integers rather than rounding them or shrinking the protocol range; actual boundary acceptance remains a task, not a claim from the Rust probe.

### Original account terms and mutable accounting have different authority

The quote supplies exact account/network/party/model/price/concurrency identity. Account totals and status are mutable provider statements. `account_matches` already checks original terms and all eight canonical money fields, then compares wide `U256` totals. Preserve that design rather than recalculating balances from the bounded recent-request page or treating per-request exposure as new account credit.

Funding reconciliation's current loss compensation is recomputed from retained funding and liabilities, not accumulated as an inference-loss tally. Reversed funding and fee charges can decrease on verified restoration; batched/remitted fee liability has distinct retention rules. These meanings must survive the schema work without importing the separate reorg/remittance transition matrix. `waived_wei` is recorded foregone nominal cost, not a refund or positive term in the available-balance identity.

Approved evidence-only renewal does not rewrite `AccountView.model`. The actual admitted report remains on the request, as covered by request-accounting task 3.2.1 and its completed native regression. The receipt identifies a provider claim; report bytes and qualification timestamps are not present and cannot be independently verified from it.

### Status is not an admission certificate

The inspected writers produce `free`, `active`, `settlement_pending` and `frozen`; do not accidentally omit the pending reconciliation state. Keep the existing string representation and permit an unfamiliar value to remain an opaque readable statement. Operation-specific allowlists still decide whether inference/top-up is permitted; an unknown string is not a successful default.

Even an active snapshot is not proof that the engine is available, paid capacity exists, a fee was remitted, the chain is irreversible or the provider is honest. Free work can occupy concurrency while reserved money is zero. Read authority and credential mutation authority remain separate. No credential secret belongs in the shared account object.

## Observed temporary probe

A disposable `src-tauri/examples/apas_account_snapshot_probe.rs` executable constructed a complete synthetic account and a genuinely provider-signed receipt using a public test scalar. It performed no network, filesystem-money-state or payment operation. `cargo run --locked --no-default-features --manifest-path src-tauri/Cargo.toml --example apas_account_snapshot_probe` succeeded and observed:

- The named-object control decoded and verified.
- An extra account member decoded, disappeared from typed reserialization and still verified against the original signature.
- A complete positional account array decoded to the original typed statement and still verified.
- A positional usage array inside that account likewise decoded and verified.
- An extra `total_tokens` member in accounting usage was rejected, confirming the existing leaf member restriction.

In all three accepted noncanonical projections, the reserialized typed receipt exactly equaled the original signed receipt. This demonstrates decoder/signature behavior, not native HTTP acceptance, a forged authorized operation or a financial-state mutation. The probe was removed after observation; no executable or permanent test was part of the initial specification-only commit `9ba4c56f`.

## Implemented decoder acceptance — 2026-11-15

Snapshot tasks 2.1–2.2 now use the shared direct object visitor for both `AccountView` and `TokenUsage`, retaining the derived closed-field checks without buffering through `Value`. `AccountView` no longer silently drops extra members. Both types reject positional containers; `TokenUsage::default()` remains an explicit internal constructor, never a missing-field decoding fallback. The public fields, serialization, monetary representation and nested model semantics are unchanged.

`cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --test inference_account_snapshot_test` passed all seven deterministic regressions. They exercise raw and pre-parsed shapes, all required-member omissions/nulls/wrong types, unknown members, raw duplicates, counter boundaries through `u64::MAX`, fractions/exponents/signs/overflow, reordered named objects and nullable model members. Independent declared account fields and explicit counter bytes retain their canonical representations; unchanged provider signatures verify reordered valid receipts but no longer admit account/usage projections.

This is shared-decoder and signed-statement evidence, not native HTTP installation/recovery, complete persisted-record acceptance, client/export precision or chain/engine qualification. Those remaining tasks stay open.

## Source and retained acceptance map

| Boundary | Inspected implementation or retained evidence | Required work |
|---|---|---|
| Payload and leaf | `protocol.rs::AccountView` and `TokenUsage` now use the direct object visitor with closed members; seven `inference_account_snapshot_test.rs` regressions pass | Tasks 2.1–2.2 complete; actual containing native replies and durable recovery remain below |
| Native account installation | `consumer.rs::account_matches`, `resume_inner`, `topup`, `accept_topup`, `refresh_account`, `receipt`; matching precedes credential/account installation | Fill complete term/money/shape refusal and protected-state assertions for each containing response |
| Producer and local records | `billing_store.rs::open_account`, `account`, `receipt`, `get_account`, `save_account`; genuine account construction and signed snapshots | Verify unchanged serialized/stored schema-3 records through strict decoding; no money migration |
| Funding meanings/status | `credit_funding`, `balance_funding`, `transition_funding` preserve gross funding, current reversals/loss compensation, retained fee liability and pending/frozen status | Preserve these observable meanings; do not claim full reorg/fee lifecycle qualification from snapshot tests |
| Original versus renewed evidence | `native_receipts_preserve_renewed_evidence_through_settlement_and_reopening` passes actual native/HTTP receipts with original account terms and renewed request identities, including waiver/recovery and hostile response refusals | Reuse request-accounting 3.2.1; this is not qualification attestation or complete receipt schema acceptance |
| Request exposure | `signed_receipt_rejects_noncanonical_request_exposure_without_advancing_sequence` preserves native state on malformed request exposure | Keep request exposure distinct from account loss compensation; no inferred full account matrix |
| Client precision/export | Native typed fields and signing use exact integers; presentation/export boundaries need their own inspection and executable acceptance | Preserve large counters and wei end to end without assuming ordinary JavaScript numbers are lossless |

## Integration and release limits

Implement and verify the shared payload before declaring the containing responses fully specified. Every new regression must catch an observable consumer contract violation and preserve an unmodified success control; existing signature/financial/credential tests are retained, not weakened. Invalid replies after a real remote commit remain ambiguous and recoverable with the original operation; local refusal cannot roll back the provider or manufacture a new payment.

Run strict validation and the available README/CI checks. Leave this change open until its listed implementation and acceptance tasks are complete, then archive normally without `--skip-specs`. Native desktop dependencies, actual policy authority and external chain/engine/platform/hardware/public-release qualification remain separate blockers. Do not install prerequisites, use a fake authority, operate on a server/account or edit the book.
