# APAS progress

## 2026-11-14 — HTTP account-read authorization acceptance

- Completed owner-control subtask 2.4.1 with six integration tests across all six production GET routes: owner/read-bearer success; anonymous, inference-only and same-wallet foreign-account refusal; no fallback from incomplete, consumed or wrong-owner proofs; nonempty body rejection.
- Covered cursor syntax/range, actual history filtering and literal signed-query spelling, plus account-scoped global/client request lookup. Read ownership/lookup business refusals consume valid proofs; the foreign-request 404 is checked durably through another ledger connection before fresh-authority recovery.
- Split remaining mutation coverage into 2.4.2; parent 2.4 stays open. Free accounts use actual HTTP opening; credential/request fixtures use existing ledger operations with cancelled unknown usage, not engine output or paid funding. No runtime, schema, clock seam or whitepaper changes.
- Whitepaper/report fingerprints were unchanged at entry; no author answer. **Public proposals can exhaust a named payer's quota** remains open and nonblocking. Whitepaper SHA-256: `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict non-interactive validation of all three open changes and the whitepaper checksum check.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (318 tests, including all six new HTTP read-authorization cases; two existing child-helper entrypoints ignored) and the corresponding headless `cargo build ... --bins`. No new warnings, failed checks or weakened tests. Owner-control tasks 2.2, 2.4.2–2.5 and final integration remain open.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Rate tasks 2.4–2.5 remain blocked by the owner-source boundary: implement the durable non-regressing clock, protected cleanup and conservative initialization together with their regressions when independently committable. Reuse the existing failed-future-minute rollback evidence. Keep enclosing commercial quota work and final integration tasks open; do not archive incomplete changes or use `--skip-specs`.
3. Next unblocked work: owner-control mutation ownership/bearer matrix (2.4.2), then global/transactional pending-challenge capacity (2.5). Reuse successful native free-opening/revocation, read-business refusals and existing payment evidence. Keep native clock task 2.2.3 blocked until independently committable explicit-time coverage is available. Provider cursor acceptance does not add native pagination. Challenge rate-window policy still needs a separate specification/acceptance slice sharing the unresolved helper above.
4. Then specify owner-authorized opening/funding acknowledgement and exact original-transaction recovery before broader accounting. Compare control, consumer, chain-journal and ledger paths with the book and reuse archived pending-retention requirements. Remaining discovery, credential lifecycle, runtime/performance and consumer/reputation contracts are not fully aligned.

## Checkout discipline

Initial checkout: zero staged files, 194 unstaged paths and 50 untracked paths, including the owner's modified whitepaper and extensive native migration. Commit only the worker's new HTTP read-authorization integration test, owner-control design/task acceptance updates and these notes. Do not absorb owner-uncommitted implementation files. Preserve all other work; no push. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
