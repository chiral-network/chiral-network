# APAS progress

## 2026-11-14 — Challenge ledger-rate and resource isolation acceptance

- Added two `challenge_*_rate_*` cases to `inference_proposal_quota_test.rs` for task 2.2: ledger 600/601, eligible other wallets, independent provider ledgers, complete remaining proposal allowances and persistence across reopening. Distinct named wallets isolate the global boundary from wallet/pending limits.
- After 600 challenge issuances, all 599 remaining ledger proposal units remain available; after 12 wallet issuances, all eleven remaining wallet proposal units remain available. Existing opposite-direction proposal tests supply reverse isolation evidence. Retained challenges, signed proposals, credentials and reservations survive other-resource exhaustion and reopening; consuming a real retained proof does not refund the exhausted global rate.
- Scoped read-bearer authentication remains usable with exhausted control budgets. A live saturated inference reservation does not spend challenge allowance, and a fresh reservation can be admitted after both control budgets exhaust. These are explicit-time ledger/authentication checks, not executed inference, payment qualification or a timed HTTP exhaustion run; existing HTTP authorization/error tests are reused.
- No runtime, monetary schema, requirement or whitepaper changes. Non-regressing clocks, cross-resource cleanup protection and retained-state adoption remain unimplemented and blocked separately; these same-minute isolation cases do not establish those guarantees.
- The whitepaper and report were unchanged at entry, with no author answer. **Public proposals can exhaust a named payer's quota** remains open and nonblocking; no duplicate entry was added. Whitepaper SHA-256: `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict validation of all four open changes and the whitepaper checksum.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (336 tests, including both new isolation cases; two existing child-helper entrypoints ignored) and the corresponding headless binary build. Task 2.2 is complete; all executed checks passed without weakening checks or modifying owner runtime source.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Proposal-rate 2.4–2.5 and challenge-rate 2.4–2.5 require the production clock, protected cleanup and conservative initialization together with regressions. They remain blocked by owner-untracked ledger source/unit tests. Preserve independent resource watermarks and both resources' retained counters; reuse failed-future-minute rollback evidence, not the old reset behavior.
3. Owner-control 2.4.3 remains blocked in owner-untracked source/tests: correct HTTP/ledger reopening without rotating an existing root, replace the test expecting rotation while retaining no-duplicate-credit assertions, and add real-router recovery evidence. Native saved-account recovery already avoids reopening. The exact native 600/601-second receiver-expiry boundary (2.2.3) remains blocked at the private unit-test ownership boundary.
4. Next unblocked step: challenge-rate 2.3 acceptance for last-wallet/global-rate-unit races, rejection rollback and conservation. Reuse existing pending-slot, collision and insertion-fault evidence while isolating rate limits from occupancy; preserve accepted account/funding/credential state. Keep clock-specific future-minute regressions with the blocked native fix, then complete the correspondence/usage notes in 3.1.
5. Then specify owner-authorized opening/funding acknowledgement and exact original-transaction recovery before broader accounting. Reuse archived pending-retention and existing crash-boundary/journal/router evidence without claiming full financial acceptance. Discovery, credential lifecycle, runtime/performance and consumer/reputation contracts are not fully aligned. Keep all incomplete changes open; never archive with `--skip-specs`.

## Checkout discipline

Initial checkout had no staged files and extensive owner-modified/untracked migration work, including the whitepaper and implementation. Commit only the worker-owned quota acceptance test, challenge-rate task update and these notes; preserve every other edit. No push, deployment or installation. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
