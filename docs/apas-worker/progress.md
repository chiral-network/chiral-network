# APAS progress

## 2026-11-14 — Paid opening funding-intent specification

- Created `specify-opening-funding-intent` with proposal, design, eight requirements, twenty-three scenarios and acceptance tasks. Scope: durable original preparation, strict signed acknowledgement binding, current full-gas spendability, bounded intent leases, permanent nonce ownership, acknowledgement-before-client-broadcast and exact-original recovery.
- Reused wire, commercial, owner-control and canonical proposal-retention contracts rather than redefining their protocols. Kept provider-side early-broadcast risk, fresh-proof availability limits, isolated/mock evidence and the separate HTTP root-recovery mismatch explicit.
- Found a source-derived gap: registration checks expiry after acquiring its write transaction but not whether sampled time predates quote issuance; the consumer already rejects that acknowledgement. Specified refusal, not timestamp clamping or weaker verification. The native fix/regression remains blocked in owner-untracked source.
- No runtime, tests, monetary schema, deployment or whitepaper changes. The new change stays open for acceptance and the blocked fix; drafting and a green existing suite do not establish full financial or platform qualification.
- The whitepaper and report were unchanged at entry, with no author answer. **Public proposals can exhaust a named payer's quota** remains open and nonblocking; no duplicate entry was added. Whitepaper SHA-256: `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed throwaway document verification (eight requirements, twenty-three scenarios, twenty-four named evidence cases and eight links/anchors), strict validation of all five open changes, frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation and the whitepaper checksum.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (339 tests; two existing child-helper entrypoints ignored) and the corresponding headless binary build. All 400 existing tests and available checks passed without modifying runtime source or weakening checks; these runs do not cover the newly specified missing cases.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Proposal-rate 2.4–2.5 and challenge-rate 2.4–2.5 require the production clock, protected cleanup and conservative initialization together with regressions. They remain blocked by owner-untracked ledger source/unit tests. Preserve independent resource watermarks and both resources' retained counters; reuse failed-future-minute rollback evidence, not the old reset behavior.
3. Owner-control 2.4.3 remains blocked in owner-untracked source/tests: correct HTTP/ledger reopening without rotating an existing root, replace the test expecting rotation while retaining no-duplicate-credit assertions, and add real-router recovery evidence. Native saved-account recovery already avoids reopening. The exact native 600/601-second receiver-expiry boundary (2.2.3) remains blocked at the private unit-test ownership boundary.
4. Opening-funding-intent 2.4 requires the pre-issuance registration guard with deterministic interval/lock-wait regressions; 2.7 needs exact native acknowledgement-time boundaries. Both remain blocked where they require owner-untracked ledger/consumer code or private time seams. Do not borrow the rate clock for signed funding time or commit a failing regression alone.
5. Next unblocked step: opening-funding-intent 2.1 strict request/acknowledgement acceptance inventory and missing worker-owned field/type/binding tests, reusing existing signed-identity substitutions. Then fresh-admission, lease/nonce/rollback and client persistence/original-recovery acceptance (2.2–2.3, 2.5–2.6, 2.8), with evidence limits recorded rather than fabricated payment.
6. Challenge-rate 3.1 is complete; its final archive remains blocked by 2.4–2.5. After the bounded pre-credit funding contract, specify safe-credit settlement/reorg accounting, explicit cancellation, top-ups and fees before broader discovery, credential lifecycle, runtime/performance and consumer/reputation alignment. Keep incomplete changes open; never archive with `--skip-specs`.

## Checkout discipline

Initial checkout had no staged files and extensive owner-modified/untracked migration work, including the whitepaper and implementation. Commit only the newly generated opening-funding change metadata/proposal/design/spec/tasks and these notes; preserve every other edit. No push, deployment or installation. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
