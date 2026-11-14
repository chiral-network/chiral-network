# APAS progress

## 2026-11-14 — Exact structural-gas acceptance

- Authority, README, CI and report hashes match the preceding full reads; no author answer or new contradiction. Compared structural-gas requirements with the shared decoder and admission path. Existing tests had a coarse 21,000-gas refusal and core arithmetic coverage, not the complete signed equality/one-below matrix.
- Added two admission-test groups for task 2.3: nineteen signed refusal/control pairs covering empty access lists on all three envelopes and eight distinct/repeated layouts on both typed envelopes, including simultaneous 64-address/256-key caps. Duplicate candidates cover a deduplicated floor but not the encoded-occurrence floor.
- Expected calldata cost uses an independent zero-byte rebate calculation and literal access charges. Each candidate changes only gas and its real signature; exact-floor controls retain the actual quote commitment. Shared private assertions preserve all earlier local/HTTP refusal, durable quote/acknowledgement, no-submission and no-credit/fee checks.
- No production/schema, wallet/server, limit or whitepaper change; RPC remains synthetic. This is the application structural floor, not an execution estimate or paid-settlement proof. Local calldata-size and defensive parser-budget acceptance remain in 2.4–2.5.
- Whitepaper/report remained unchanged at entry, with no author answer; **Public proposals can exhaust a named payer's quota** remains open and nonblocking. No new author decision or report entry. Whitepaper SHA-256: `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed: all six strict OpenSpec validations, unchanged-whitepaper checksum, frontend type checks (zero errors/warnings), 15 frontend tests/build, 46 Python tests and compilation of CI's seven scripts. The build reports stale Browserslist data; no dependency update was attempted.
- Passed: 362 all-target headless Rust tests (the two existing child-helper tests remain ignored by the ordinary runner) and headless binary build. Total: 423 passing Rust/frontend/Python tests. Task 2.3 is complete; later acceptance tasks and archive stay open.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Proposal-rate 2.4–2.5 and challenge-rate 2.4–2.5 require production clocks, protected cleanup and conservative initialization together with regressions. They remain blocked by owner-untracked ledger source/unit tests. Preserve independent resource watermarks and both resources' retained counters; reuse failed-future-minute rollback evidence.
3. Owner-control 2.4.3 remains blocked in owner-untracked source/tests: correct HTTP/ledger reopening without rotating an existing root, replace the test expecting rotation while retaining no-duplicate-credit assertions, and add real-router recovery evidence. Native saved-account recovery already avoids reopening. Exact native 600/601-second receiver expiry (2.2.3) remains blocked at the private unit-test boundary.
4. Opening-funding-intent 2.1.2 requires object-only request and acknowledgement envelope/payload/network guards; positional decoding is observed, not permitted. Task 2.4 needs the pre-issuance registration guard; 2.7 needs exact native acknowledgement-time boundaries. These fixes/seams remain blocked in owner-untracked parser/protocol/ledger/consumer source. Do not borrow rate clocks for signed time, weaken schemas or commit failing regressions alone.
5. Next unblocked step: envelope-bounds 2.4's local 4,096/4,097-byte calldata boundary with matching expected data and sufficient gas, separate from the opening route's exact 38-byte commitment. Direct private parser-limit work in 2.5 is blocked where owner-untracked native seams are required; the raw 16 KiB equality case is unreachable under the current profile, not permission to weaken it. Then finish opening-funding 2.2's raw/qualification inventory and 2.3, 2.5–2.6, 2.8 recovery work. Parent opening 2.1 and both archives remain open.
6. Challenge-rate 3.1 is complete; its final archive remains blocked by 2.4–2.5. After this bounded pre-credit funding contract, specify safe-credit settlement/reorg accounting, explicit cancellation, top-ups and fees before broader discovery, credential lifecycle, runtime/performance and consumer/reputation alignment. Keep incomplete changes open; never archive with `--skip-specs`.

## Checkout discipline

Initial checkout had no staged files and extensive owner-modified/untracked migration work, including the whitepaper and implementation. Commit only the admission test, envelope-resource design/tasks and these notes; preserve every other edit. No push, deployment or installation. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
