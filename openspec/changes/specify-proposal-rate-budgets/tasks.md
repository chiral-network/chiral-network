## 1. Specify policy and evidence limits

- [x] 1.1 Compare the whitepaper, integrated pending-quota requirements, commercial bounded-proposal requirement and actual rate/cleanup/timestamp code; separate occupancy evidence from rate evidence.
- [x] 1.2 Specify 12/600 scoped budgets, atomic nonrefundable charging and durable non-regressing effective minutes; record the existing reset/pruning gap and the source-ownership blocker explicitly.
- [x] 1.3 Strictly validate this change and run available project checks, without claiming the new clock requirement is already implemented.

## 2. Complete implementation and acceptance

- [x] 2.1 Add isolated 12/13 payer-budget, forward minute-boundary, reclamation-without-rate-refund and reopening cases using real signed quotes and explicit storage time. Keep the eight-slot cap from masking the rate edge; do not change native TTL or fabricate payment success.
  Evidence in `src-tauri/tests/inference_proposal_quota_test.rs`: `payer_rate_budget_survives_expiry_reopening_and_fixed_minute_boundaries`, `acknowledgement_neither_refunds_nor_charges_the_payers_proposal_rate` and `free_opening_preserves_rate_allowance_and_credentials_at_exhaustion`. All three passed with the complete headless suite; backward-time safety remains unimplemented under 2.4–2.5.
- [x] 2.2 Cover 600/601 ledger-budget admission, eligible other payers, shared model/free-paid attribution and independent provider ledgers. Preserve separate challenge/inference budgets and reuse deterministic HTTP error mapping evidence.
  Evidence: `ledger_rate_budget_is_shared_across_terms_but_not_providers_or_control_budgets` and `payer_rate_scope_spans_terms_without_blocking_other_payers_or_control_budgets` passed in the quota suite. Reuse `public_pending_quota_follows_the_canonical_named_payer_not_caller_authority` for native canonicalization and HTTP 429/`rate_limit`. Reservations exercise ledger admission only, not engine execution or paid settlement.
- [x] 2.3 Cover both-bucket rollback, refusal without consuming another payer's shared allowance, failed insertion and competing connections at the final rate unit; reuse existing pending-capacity rejection evidence where it already covers a case.
  Acceptance cases in `src-tauri/tests/inference_proposal_quota_test.rs`: `payer_rate_refusals_preserve_the_last_shared_unit_for_another_payer`, `failed_insertion_rolls_back_rate_windows_counters_and_pending_cleanup`, `competing_connections_share_the_final_payer_rate_unit_without_leaking_global_units` and `competing_connections_admit_only_one_quote_at_the_global_rate_boundary`. Reuse `rejected_pending_proposals_preserve_the_payers_rate_allowance` and `rejected_pending_proposals_preserve_other_payers_shared_rate_allowance` for pending-capacity rollback.
- [ ] 2.4 Implement durable non-regressing proposal-minute selection, quote-specific cleanup protection and preservation of retained pre-watermark counters. BLOCKED: `billing_store.rs` is entirely untracked owner source; do not copy it, introduce an unused helper or commit the surrounding migration. Keep financial schema/state and other rate policies unchanged.
- [ ] 2.5 Add regression coverage with the native fix for older/reordered samples, remaining versus exhausted current allowance, reopening, later-time challenge cleanup, failed future-minute admission, initialization from retained counters and fail-closed inconsistent state. BLOCKED with 2.4 where the current native code contradicts these outcomes; do not commit failing tests or weaken the clock requirement.
  Reuse task 2.3's duplicate-ID insertion case for failed-future-minute rollback. It does not establish non-regression after a successful newer-minute admission or protect against unrelated cleanup; those scenarios and retained-state initialization still require the native fix.

## 3. Verify and integrate

- [ ] 3.1 Run acceptance tests and available README/CI checks after implementation, recording unavailable platform/release gates without weakening them.
- [ ] 3.2 Strictly validate and normally archive with `openspec archive specify-proposal-rate-budgets --yes`, never `--skip-specs`, only after all requirements and tasks are complete.

The open public named-payer availability disclosure is nonblocking for this admission policy. No public-authentication redesign, paid retry, live payment, deployment or whitepaper edit is authorized by this change.
