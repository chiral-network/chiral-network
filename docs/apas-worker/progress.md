# APAS progress

## 2026-11-14 — Canonical numeric and signature acceptance

- Extended `inference_funding_admission_test` for opening-funding-intent 2.2.4 with two groups across all three envelope kinds: re-signed noncanonical/over-width integers and high-s signatures recovering the same payer. The matrix has 21 refusals and 21 previously encoded valid-original controls.
- Numeric mutations are signed over their actual RLP fields, with admissible truncated low bits and matching current nonce. High-s variants flip recovery parity and independently recover the original public key before the route assertion, preventing payer mismatch from masking the intended rejection. Existing decoder/wallet tests were inspected first; all earlier acceptance assertions remain.
- Refusals must retain the quote, create no acknowledgement or paid accounting and never submit; the unchanged valid original must still be admitted afterward. Raw-field signing reuses the fixture's existing primitive; no runtime/schema change. RPC remains synthetic and loopback-only, and parent 2.2 stays open for the remaining raw-envelope and qualification inventory.
- Whitepaper/report remained unchanged at entry, with no author answer; **Public proposals can exhaust a named payer's quota** remains open and nonblocking. No new author decision or report entry. Whitepaper SHA-256: `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict validation of all five open changes and the whitepaper checksum.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (355 tests, including both new groups and all retained admission cases; two existing child-helper entrypoints ignored) and the corresponding headless binary build. All 416 tests and available checks passed. Task 2.2.4 is complete; parent 2.2 remains open.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Proposal-rate 2.4–2.5 and challenge-rate 2.4–2.5 require production clocks, protected cleanup and conservative initialization together with regressions. They remain blocked by owner-untracked ledger source/unit tests. Preserve independent resource watermarks and both resources' retained counters; reuse failed-future-minute rollback evidence.
3. Owner-control 2.4.3 remains blocked in owner-untracked source/tests: correct HTTP/ledger reopening without rotating an existing root, replace the test expecting rotation while retaining no-duplicate-credit assertions, and add real-router recovery evidence. Native saved-account recovery already avoids reopening. Exact native 600/601-second receiver expiry (2.2.3) remains blocked at the private unit-test boundary.
4. Opening-funding-intent 2.1.2 requires object-only request and acknowledgement envelope/payload/network guards; positional decoding is observed, not permitted. Task 2.4 needs the pre-issuance registration guard; 2.7 needs exact native acknowledgement-time boundaries. These fixes/seams remain blocked in owner-untracked parser/protocol/ledger/consumer source. Do not borrow rate clocks for signed time, weaken schemas or commit failing regressions alone.
5. Next unblocked step: finish opening-funding-intent 2.2 raw hexadecimal syntax, unsupported envelopes, remaining RLP/access-list structures, exact decoded-byte limits and qualification-gate inventory. The decoded raw cap is 16 KiB and the control body cap is 96 KiB; isolate size checks with otherwise-valid envelopes rather than garbage rejected for another reason. Reuse existing core encoding and deterministic qualification-loss tests. Then lease/nonce/rollback and native persistence/original-recovery work (2.3, 2.5–2.6, 2.8). Parent 2.1 and final archive remain open.
6. Challenge-rate 3.1 is complete; its final archive remains blocked by 2.4–2.5. After this bounded pre-credit funding contract, specify safe-credit settlement/reorg accounting, explicit cancellation, top-ups and fees before broader discovery, credential lifecycle, runtime/performance and consumer/reputation alignment. Keep incomplete changes open; never archive with `--skip-specs`.

## Checkout discipline

Initial checkout had no staged files and extensive owner-modified/untracked migration work, including the whitepaper and implementation. Commit only the admission test, opening-funding design/task updates and these notes; preserve every other edit. No push, deployment or installation. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
