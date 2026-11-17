## 1. Specification

- [x] 1.1 Move the finished `inference-proposal-admission` deltas (three MODIFIED, three REMOVED and three ADDED requirements) unchanged from `specify-non-reserving-quotes`, leaving that change's two requirements whose acceptance is still open (*Public quotes do not reserve opening capacity*, *Authenticated paid admission preserves accepted commitments*). Every one of the eleven requirements appears in exactly one change.
- [x] 1.2 Confirm no other pending change or main spec references the removed requirement names (`Pending proposals have ledger-scoped capacity`, `Pending proposal admission is atomic`, `Named-payer quota attribution is not payer authentication`) except the main spec this change replaces.

## 2. Implementation evidence

- [x] 2.1 Confirm the code already meets every moved requirement and that its tests exist and pass: provider-wide retention and atomic storage with no named-wallet gate (`specify-non-reserving-quotes` tasks 3.1, 3.5, 4.1); the pre-signing work bound (`quote_work_*` ledger cases in `src-tauri/src/billing_store_tests.rs`; `public_quote_work_initialization_refuses_durably_without_erasing_opened_terms`, `public_quote_signing_failures_spend_global_work_across_wallets_and_reopened_routers`, `failed_quote_response_body_does_not_refund_work_or_committed_quote` and `public_global_committed_rate_refusal_preserves_quotes_and_an_opened_agreement` in `src-tauri/tests/inference_proposal_test.rs`). No code change is needed.

## 3. Integrate

- [x] 3.1 Strictly validate this change and `specify-non-reserving-quotes`, run the available project checks, then archive this change normally (`openspec archive adopt-provider-wide-quote-retention --yes`, never `--skip-specs`) and correct the main spec's Purpose so it no longer describes named-payer quota attribution.
