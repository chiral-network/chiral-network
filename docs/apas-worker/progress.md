# APAS progress

## 2026-11-14 — Nonce and head admission acceptance

- Extended `inference_funding_admission_test` for opening-funding-intent 2.2.2 with three groups across legacy, EIP-2930 and EIP-1559: current-nonce eligibility, post-balance nonce/head changes and allowed healthy head advancement. Existing full-cost/overflow assertions remain intact through shared outcome helpers.
- Synthetic RPC snapshots transition once at the balance response boundary, without sleeps. The new matrix has 27 refusals and 27 acknowledgement controls across 30 isolated fixtures: future/spent/exhausted/competing/inconsistent nonce, changed latest/pending nonce, regressed or replaced head, and healthy advancement. Recoverable refusals retain the same signed original on restored observations; exhaustion stays refused.
- Checks require the relevant RPC observation, precise HTTP error class, original quote/acknowledgement bindings, reopened storage without paid accounting and no submission. Existing native journal-regression and limited admission tests were inspected first. No live-chain, unrelated historical-retention, wallet replacement or clock-boundary proof is claimed; parent 2.2 stays open for remaining envelope/schema/qualification inventory. No runtime or schema change.
- Whitepaper/report remained unchanged at entry, with no author answer; **Public proposals can exhaust a named payer's quota** remains open and nonblocking. No new author decision or report entry. Whitepaper SHA-256: `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict validation of all five open changes and the whitepaper checksum.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (350 tests, including all three new groups and retained full-cost cases; two existing child-helper entrypoints ignored) and the corresponding headless binary build. All 411 tests and available checks passed. Task 2.2.2 is complete; parent 2.2 remains open.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Proposal-rate 2.4–2.5 and challenge-rate 2.4–2.5 require production clocks, protected cleanup and conservative initialization together with regressions. They remain blocked by owner-untracked ledger source/unit tests. Preserve independent resource watermarks and both resources' retained counters; reuse failed-future-minute rollback evidence.
3. Owner-control 2.4.3 remains blocked in owner-untracked source/tests: correct HTTP/ledger reopening without rotating an existing root, replace the test expecting rotation while retaining no-duplicate-credit assertions, and add real-router recovery evidence. Native saved-account recovery already avoids reopening. Exact native 600/601-second receiver expiry (2.2.3) remains blocked at the private unit-test boundary.
4. Opening-funding-intent 2.1.2 requires object-only request and acknowledgement envelope/payload/network guards; positional decoding is observed, not permitted. Task 2.4 needs the pre-issuance registration guard; 2.7 needs exact native acknowledgement-time boundaries. These fixes/seams remain blocked in owner-untracked parser/protocol/ledger/consumer source. Do not borrow rate clocks for signed time, weaken schemas or commit failing regressions alone.
5. Next unblocked step: finish opening-funding-intent 2.2 signed-envelope/schema and qualification-gate inventory, adding only missing consumer-visible cases. Reuse existing deterministic qualification-loss synchronization rather than duplicating it. Then lease/nonce/rollback and native persistence/original-recovery work (2.3, 2.5–2.6, 2.8). Preserve the distinction between fresh admission and cached historical recovery; parent 2.1 and final archive remain open.
6. Challenge-rate 3.1 is complete; its final archive remains blocked by 2.4–2.5. After this bounded pre-credit funding contract, specify safe-credit settlement/reorg accounting, explicit cancellation, top-ups and fees before broader discovery, credential lifecycle, runtime/performance and consumer/reputation alignment. Keep incomplete changes open; never archive with `--skip-specs`.

## Checkout discipline

Initial checkout had no staged files and extensive owner-modified/untracked migration work, including the whitepaper and implementation. Commit only the admission test, opening-funding design/task updates and these notes; preserve every other edit. No push, deployment or installation. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
