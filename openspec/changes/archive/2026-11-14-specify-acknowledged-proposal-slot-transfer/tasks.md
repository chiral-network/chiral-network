## 1. Correct the draft lifecycle

- [x] 1.1 Compare actual acknowledgement registration, account opening and existing recovery tests with the whitepaper and pending-quota delta; identify the worker's incorrect pending-retention description.
- [x] 1.2 Correct the still-open pending-quota definition/design and add the atomic acknowledgement-transfer requirement without changing implementation or whitepaper.
- [x] 1.3 Strictly validate this change and the corrected pending-quota change; run available project checks.

## 2. Complete acceptance evidence

- [x] 2.1 Add missing consumer-visible acknowledgement quota coverage: freeing exactly one full-budget slot, replay after replacement, and failed registration preserving pending capacity and signed terms.
  - `inference_proposal_quota_test::acknowledgement_frees_exactly_one_slot_and_replay_preserves_its_replacement` covers exact replacement capacity and original-acknowledgement recovery live and after reopening.
  - `inference_proposal_quota_test::rejected_acknowledgements_preserve_pending_capacity_and_allow_original_registration` covers draining-provider and acknowledgement-signing identity rejection, durable absence of a partial acknowledgement, retained full capacity and subsequent success of the original operation.
  - These explicit-time ledger tests use real offline-signed opening envelopes, not RPC/payment-success fixtures. Prior owner/spendability admission is a precondition, not a new public-path qualification claim.
- [x] 2.2 Retain and map existing expiry/restart, original-transaction and sender/nonce recovery tests; add only missing cases, with no fabricated payment success or live qualification claim.
  - The design's reviewed-evidence section maps the existing recovery, nonce-conflict, independent intent-capacity, paid-opening and HTTP admission tests. It distinguishes reopened-ledger recovery and synthetic chain fixtures from live settlement or crash qualification; no duplicate tests were needed.
- [x] 2.3 Resolve any demonstrated implementation contradiction only where independently committable. Preserve the owner's untracked native source and mark inseparable fixes blocked.
  - Reviewed the immediate transaction/commit boundary, registration/replay, retained quote lookup, cleanup, paid opening and public owner/spendability/qualification gates. No contradiction was found in this narrow transition; production source remains untouched.

## 3. Verify and integrate

- [x] 3.1 Run relevant acceptance tests and available project checks, recording unavailable platform/release gates without weakening them.
  - Passed locked headless all-target tests (282 passed; two existing child-helper entrypoints ignored) and headless binary build, frontend type checks/15 tests/build, 46 Python tests, seven-script compilation and strict validation of all three then-open changes.
  - Native desktop dependencies, actual release policy key, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates, not waived or claimed by this narrow change.
- [x] 3.2 Strictly validate and archive with `openspec archive specify-acknowledged-proposal-slot-transfer --yes`, never `--skip-specs`, only when every task is complete. Integrate its distinct ADDED requirement alongside the corrected pending-quota requirements.

First-opening slot transfer remains in pending-quota task 2.4. This correction does not approve public deployment or settle the existing author-owned availability disclosure.
