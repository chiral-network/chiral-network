# APAS progress

## 2026-11-14 — Funding-intent request-object acceptance

- Added one real-router test for opening-funding-intent 2.1.1: nine missing/mistyped/unknown/duplicate request-object variants against both new and cached agreements, each with genuine owner proof bound to the exact body.
- Corrected requests with fresh proof reach the new-intent draining gate or return the unchanged signed cached acknowledgement. Reopened ledgers check original terms/acknowledgement, no paid account/funding/fee rows, and unchanged unrelated free-account credentials. No RPC, engine, actual payment or native-client broadcast is claimed.
- A throwaway built-core probe observed `FundingIntentRequest` accepting `["0x00"]` as a positional struct while rejecting an extra object field. This is decoder evidence, not HTTP or payment acceptance. Recorded blocked task 2.1.2 for the required object-only guard in owner-untracked `protocol.rs`/`contracts.rs`; the probe was removed and no permanent test endorses the mismatch.
- Reviewed existing signed-acknowledgement binding and no-broadcast tests; left native response-schema acceptance as 2.1.3 and parent 2.1 open. No runtime, requirement, monetary schema, deployment or whitepaper changes; no new author decision is needed.
- The whitepaper and report were unchanged at entry, with no author answer. **Public proposals can exhaust a named payer's quota** remains open and nonblocking; no duplicate entry was added. Whitepaper SHA-256: `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict validation of all five open changes and the whitepaper checksum.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (340 tests, including the new router case; two existing child-helper entrypoints ignored) and the corresponding headless binary build. All 401 tests and available checks passed without weakening checks or modifying owner runtime source. Task 2.1.1 is complete; container enforcement and native response acceptance remain open.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Proposal-rate 2.4–2.5 and challenge-rate 2.4–2.5 require the production clock, protected cleanup and conservative initialization together with regressions. They remain blocked by owner-untracked ledger source/unit tests. Preserve independent resource watermarks and both resources' retained counters; reuse failed-future-minute rollback evidence, not the old reset behavior.
3. Owner-control 2.4.3 remains blocked in owner-untracked source/tests: correct HTTP/ledger reopening without rotating an existing root, replace the test expecting rotation while retaining no-duplicate-credit assertions, and add real-router recovery evidence. Native saved-account recovery already avoids reopening. The exact native 600/601-second receiver-expiry boundary (2.2.3) remains blocked at the private unit-test ownership boundary.
4. Opening-funding-intent 2.1.2 requires an object-only request guard; current decoder sequence acceptance is observed and must not be blessed. Task 2.4 requires the pre-issuance registration guard; 2.7 needs exact native acknowledgement-time boundaries. These fixes/seams remain blocked in owner-untracked parser/protocol/ledger/consumer source. Do not borrow rate clocks for signed time, weaken schemas or commit failing regressions alone.
5. Next unblocked step: opening-funding-intent 2.1.3 native acknowledgement-response schema/field/type acceptance, reusing existing signed-identity substitutions and proving refusal before client broadcast rather than claiming deserialization is end-to-end acceptance. Then fresh-admission, lease/nonce/rollback and client persistence/original-recovery work (2.2–2.3, 2.5–2.6, 2.8). Parent 2.1 and final archive stay open.
6. Challenge-rate 3.1 is complete; its final archive remains blocked by 2.4–2.5. After the bounded pre-credit funding contract, specify safe-credit settlement/reorg accounting, explicit cancellation, top-ups and fees before broader discovery, credential lifecycle, runtime/performance and consumer/reputation alignment. Keep incomplete changes open; never archive with `--skip-specs`.

## Checkout discipline

Initial checkout had no staged files and extensive owner-modified/untracked migration work, including the whitepaper and implementation. Commit only the worker-owned control acceptance test, opening-funding design/task updates and these notes; preserve every other edit. No push, deployment or installation. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
