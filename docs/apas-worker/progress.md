# APAS progress

## 2026-11-14 — Challenge issuance rate specification

- Created `specify-challenge-rate-budgets` with proposal, design, three requirements and staged acceptance tasks. It preserves 12 committed issuances per canonical named wallet and 600 per provider ledger, independent of pending occupancy, operation fields and proposal/inference budgets.
- Required an independent durable non-regressing challenge minute, conservative retained-counter adoption and atomic, nonrefundable charging. Source-derived clock-reset/shared-cleanup gaps remain explicitly unimplemented; no runtime or test change was made. No author decision is needed for these delegated admission details.
- Kept proof validity and signed lifetimes separate from rate accounting. Consumption/expiry can release a pending slot but cannot refund issuance; already-issued valid proofs and eligible bearer paths retain their own rules. Fresh owner proofs can still be unavailable under exhaustion or clock correction.
- The whitepaper and report were unchanged at entry, with no author answer. **Public proposals can exhaust a named payer's quota** remains open and nonblocking; no duplicate entry was added. Whitepaper SHA-256: `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict validation of all four open changes and the whitepaper checksum.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (331 tests; two existing child-helper entrypoints ignored) and the corresponding headless binary build. All executed checks passed; they do not certify the newly specified, unimplemented clock guarantees.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Proposal-rate 2.4–2.5 and challenge-rate 2.4–2.5 require the production clock, protected cleanup and conservative initialization together with regressions. They remain blocked by owner-untracked ledger source/unit tests. Preserve independent resource watermarks and both resources' retained counters; reuse failed-future-minute rollback evidence, not the old reset behavior.
3. Owner-control 2.4.3 remains blocked in owner-untracked source/tests: correct HTTP/ledger reopening without rotating an existing root, replace the test expecting rotation while retaining no-duplicate-credit assertions, and add real-router recovery evidence. Native saved-account recovery already avoids reopening. The exact native 600/601-second receiver-expiry boundary (2.2.3) remains blocked at the private unit-test ownership boundary.
4. Next unblocked step: challenge-rate 2.1 acceptance for 12/13 issuances, forward boundaries/reopening and no rate refund, using real proof consumption to avoid masking rates with the eight-slot pending ceiling. Then cover ledger 600/601, scope/isolation and final-rate-unit races/rollback (2.2–2.3). Reuse existing HTTP canonicalization/error and pending-capacity evidence; do not commit regressions that need the blocked native fix.
5. Then specify owner-authorized opening/funding acknowledgement and exact original-transaction recovery before broader accounting. Reuse archived pending-retention and existing crash-boundary/journal/router evidence without claiming full financial acceptance. Discovery, credential lifecycle, runtime/performance and consumer/reputation contracts are not fully aligned. Keep all incomplete changes open; never archive with `--skip-specs`.

## Checkout discipline

Initial checkout had no staged files and extensive owner-modified/untracked migration work, including the whitepaper and implementation. Commit only the new challenge-rate change and these notes; preserve every other edit. No push, deployment or installation. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
