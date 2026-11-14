# APAS progress

## 2026-11-14 — Payment envelope resource specification

- Created `specify-payment-envelope-resource-bounds` with `openspec new change`, then wrote the proposal, `inference-payment-envelope-bounds` requirements, design/evidence and acceptance tasks. This is a specification-layer step; no runtime or tests changed, and the change remains open for acceptance.
- Comparison found uncovered decoder limits: 64 access-list entries, 256 aggregate key occurrences, 4 KiB calldata, depth four and 1,024 immediate RLP items. The new capability preserves those limits, exact integer/opaque-byte handling, duplicate occurrence costs and financial recovery boundaries without claiming chain-consensus authority.
- Corrected the next-step premise: an otherwise-valid transaction cannot reach the 16 KiB decoded ceiling under the stricter profile. A conservative source-backed calculation gives at most 14,448 bytes generally and 10,388 for an opening; the in-memory calculation passed. This is not isolated runtime proof of the early raw-size guard.
- Reuse the existing numeric/signature/binding/cost/nonce tests. Reachable access-list and gas/calldata boundaries are next; unreachable defensive parser-limit evidence must not be fabricated with inputs rejected for another reason. No limit widening, production test-only API, migration, wallet/server action or author decision.
- Whitepaper/report remained unchanged at entry, with no author answer; **Public proposals can exhaust a named payer's quota** remains open and nonblocking. No new author decision or report entry. Whitepaper SHA-256: `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed strict validation of all six open changes, the unchanged-whitepaper checksum and the conservative size calculation. Frontend type checks had zero errors/warnings; 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests and CI's seven-script compilation passed.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (355 tests; two existing child-helper entrypoints ignored) and the corresponding headless binary build. All 416 existing tests and available checks passed. Envelope-bounds stage 1 is complete; executable acceptance and archive remain open.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Proposal-rate 2.4–2.5 and challenge-rate 2.4–2.5 require production clocks, protected cleanup and conservative initialization together with regressions. They remain blocked by owner-untracked ledger source/unit tests. Preserve independent resource watermarks and both resources' retained counters; reuse failed-future-minute rollback evidence.
3. Owner-control 2.4.3 remains blocked in owner-untracked source/tests: correct HTTP/ledger reopening without rotating an existing root, replace the test expecting rotation while retaining no-duplicate-credit assertions, and add real-router recovery evidence. Native saved-account recovery already avoids reopening. Exact native 600/601-second receiver expiry (2.2.3) remains blocked at the private unit-test boundary.
4. Opening-funding-intent 2.1.2 requires object-only request and acknowledgement envelope/payload/network guards; positional decoding is observed, not permitted. Task 2.4 needs the pre-issuance registration guard; 2.7 needs exact native acknowledgement-time boundaries. These fixes/seams remain blocked in owner-untracked parser/protocol/ledger/consumer source. Do not borrow rate clocks for signed time, weaken schemas or commit failing regressions alone.
5. Next unblocked step: complete envelope-bounds 2.1 mapping, then real-signature access-list 64/65-address and aggregate 256/257-key acceptance (2.2), including distributed/duplicate occurrences and valid controls; follow with occurrence-based gas and local calldata boundaries (2.3–2.4). Direct private parser-limit work in 2.5 is blocked where owner-untracked native seams are required; the raw 16 KiB equality case is unreachable under the current profile, not permission to weaken it. Then finish opening-funding 2.2's raw/qualification inventory and 2.3, 2.5–2.6, 2.8 recovery work. Parent opening 2.1 and both archives remain open.
6. Challenge-rate 3.1 is complete; its final archive remains blocked by 2.4–2.5. After this bounded pre-credit funding contract, specify safe-credit settlement/reorg accounting, explicit cancellation, top-ups and fees before broader discovery, credential lifecycle, runtime/performance and consumer/reputation alignment. Keep incomplete changes open; never archive with `--skip-specs`.

## Checkout discipline

Initial checkout had no staged files and extensive owner-modified/untracked migration work, including the whitepaper and implementation. Commit only the new envelope-resource change's five files and these notes; preserve every other edit. No push, deployment or installation. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
