# APAS progress

## 2026-11-14 — Challenge rate acceptance map and usage

- Completed challenge-rate task 3.1 in the existing change's design: all three requirements and thirteen scenarios now map to production paths, named acceptance cases or explicit blocked tasks.
- Replaced the stale pre-test evidence table with numeric, isolation, race and rollback correspondence. Distinguished native HTTP alias/error evidence from explicit-time ledger boundaries, retained authority from fresh-proof availability, and offline acknowledgement preservation from chain execution or credit.
- Documented nonrefundable issuance units, ambiguous `rate_limit` refusals, safe use of existing authority, original-operation recovery and the availability cost of future non-regressing clock correction. The clock contract remains explicitly unimplemented; no requirement, runtime, test, monetary schema or whitepaper changes.
- Kept challenge-rate 2.4–2.5 and final integration/archive open. No new author decision is needed for this documentation step.
- The whitepaper and report were unchanged at entry, with no author answer. **Public proposals can exhaust a named payer's quota** remains open and nonblocking; no duplicate entry was added. Whitepaper SHA-256: `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed throwaway documentation verification (three requirements, thirteen scenarios, thirteen links/anchors, thirteen named test references and six capacity cases), frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict validation of all four open changes and the whitepaper checksum.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (339 tests; two existing child-helper entrypoints ignored) and the corresponding headless binary build. All 400 executed tests and available checks passed without weakening checks or modifying owner runtime source.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Proposal-rate 2.4–2.5 and challenge-rate 2.4–2.5 require the production clock, protected cleanup and conservative initialization together with regressions. They remain blocked by owner-untracked ledger source/unit tests. Preserve independent resource watermarks and both resources' retained counters; reuse failed-future-minute rollback evidence, not the old reset behavior.
3. Owner-control 2.4.3 remains blocked in owner-untracked source/tests: correct HTTP/ledger reopening without rotating an existing root, replace the test expecting rotation while retaining no-duplicate-credit assertions, and add real-router recovery evidence. Native saved-account recovery already avoids reopening. The exact native 600/601-second receiver-expiry boundary (2.2.3) remains blocked at the private unit-test ownership boundary.
4. Challenge-rate 3.1 correspondence and usage notes are complete. Final integration/archive remains blocked by 2.4–2.5; passing numeric/transactional tests does not satisfy those clock scenarios.
5. Next unblocked step: specify owner-authorized opening/funding acknowledgement and exact original-transaction recovery before broader accounting. Reuse archived pending-retention and existing crash-boundary/journal/router evidence without claiming full financial acceptance. Discovery, credential lifecycle, runtime/performance and consumer/reputation contracts are not fully aligned. Keep all incomplete changes open; never archive with `--skip-specs`.

## Checkout discipline

Initial checkout had no staged files and extensive owner-modified/untracked migration work, including the whitepaper and implementation. Commit only the worker-owned challenge-rate design/task updates and these notes; preserve every other edit. No push, deployment or installation. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
