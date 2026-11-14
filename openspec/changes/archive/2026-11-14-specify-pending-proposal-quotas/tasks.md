## 1. Specify the existing policy

- [x] 1.1 Compare the whitepaper, commercial-terms delta and actual quote admission/opening/cleanup code; distinguish existing challenge tests from missing quote tests.
- [x] 1.2 Specify pending capacities, ledger scope, atomic rejection and agreement-preserving reclamation without changing authentication, rates or native quote lifetime.
- [x] 1.3 Record the public named-payer availability exposure for the whitepaper author without modifying the whitepaper.
- [x] 1.4 Strictly validate this specification and run available project checks.

## 2. Add isolated acceptance evidence

- [x] 2.1 Cover the eight-per-payer pending boundary, payer/model/free-paid attribution, independent ledgers and rejection without storing the new proposal.
- [x] 2.2 Cover the 10,000-record storage ceiling with bounded, explicitly storage-level fixtures and controlled time; do not change native TTL/rates, use live databases or fabricate payment evidence.
- [x] 2.3 Cover competing connections at the final pending slot and rollback of tentative rate increments when pending admission rejects.
- [x] 2.4 Cover exact expiry reclamation and successful/failed first-opening slot transfer with preserved signed agreements. Retain existing acknowledged-funding recovery tests; coordinate acknowledgement transfer with `specify-acknowledged-proposal-slot-transfer` and add only missing consumer-visible cases.
  - `inference_proposal_quota_test::unpaid_first_opening_preserves_pending_capacity_and_signed_terms_after_reopening` proves that a missing-payment rejection retains the full pending budget and signed proposals without creating accounts or fees, both live and after reopening storage.
  - `inference_proposal_quota_test::free_first_opening_frees_one_slot_and_survives_exact_expiry_with_original_credential` proves exactly one replacement slot, rejection at T−1 and reclamation at T, preserved signed account terms and financial state, and continued use of the original credential through cleanup and storage reopening.
  - These are explicit-time ledger tests after the owner-authorization boundary, not public opening or live-payment qualification. Acknowledged-funding recovery coverage remains unchanged and mapped in `archive/2026-11-14-specify-acknowledged-proposal-slot-transfer/design.md`; its requirement is already integrated into `inference-proposal-admission`.
- [x] 2.5 Fix any demonstrated implementation contradiction only where independently committable. Entirely untracked owner source remains blocked; do not absorb it or weaken requirements.
  - Reviewed native canonical attribution, ledger/provider scope, atomic admission and rejection, expiry and first-opening retention, HTTP error mapping and compatibility with the archived acknowledgement requirement. The design maps the existing acceptance evidence and its limits; no contradiction or additional missing consumer-visible case was found for this change. Production source and tests remain untouched by this final review.

## 3. Verify and integrate

- [x] 3.1 Run relevant acceptance tests and available README/CI checks; record unavailable platform/release gates without weakening them.
  - Passed locked headless all-target tests (284 passed; two existing child-helper entrypoints ignored) and binary build, frontend type checks/15 tests/build, 46 Python tests, seven-script compilation and strict validation of both then-open changes.
  - Native desktop dependencies, the actual release policy key, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates, not waived by this archive.
- [x] 3.2 Strictly validate the completed change and archive with `openspec archive specify-pending-proposal-quotas --yes`, never `--skip-specs`, only once all tasks are complete.

Exact proposal-rate policy/clock semantics remain a separate specification step. The whitepaper disclosure request is nonblocking for testing the current public-proposal contract; any authentication redesign requires the author's design update first.
