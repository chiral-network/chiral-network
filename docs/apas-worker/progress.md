# APAS progress

## 2026-11-14 — HTTP credential-mutation authorization acceptance

- Completed owner-control subtask 2.4.2.1 with three integration tests for credential issuance/revocation/rotation: anonymous and bearer-only refusal, incomplete/wrong-owner proof refusal without fallback, owner-authorized success, usable new credentials, revoked predecessors and rejection of consumed proof replay.
- Covered same-wallet cross-account credential identifiers and authorized business refusals. Independent ledger connections check consumed authority and unchanged credential records; failed rotation must preserve the previous token. Corrected requests use fresh proofs, with unrelated accounts/credentials preserved.
- Reused the account-read fixture and renamed its target to `inference_owner_control_test.rs`, retaining all six read tests. Split mutation acceptance into credential task 2.4.2.1 and remaining payment task 2.4.2.2; parent 2.4 stays open. No runtime, schema, clock seam, signer API or whitepaper changes.
- Whitepaper/report fingerprints were unchanged at entry; no author answer. **Public proposals can exhaust a named payer's quota** remains open and nonblocking. Whitepaper SHA-256: `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict non-interactive validation of all three open changes and the whitepaper checksum check.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (321 tests, including all nine combined control-route cases; two existing child-helper entrypoints ignored) and the corresponding headless `cargo build ... --bins`. No failed or weakened checks. Owner-control tasks 2.2.3, 2.4.2.2–2.5 and final integration remain open.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Rate tasks 2.4–2.5 remain blocked by the owner-source boundary: implement the durable non-regressing clock, protected cleanup and conservative initialization together with their regressions when independently committable. Reuse the existing failed-future-minute rollback evidence. Keep enclosing commercial quota work and final integration tasks open; do not archive incomplete changes or use `--skip-specs`.
3. Next unblocked work after credential acceptance: payment mutation ownership/bearer matrix (2.4.2.2), then global/transactional pending-challenge capacity (2.5). Existing `control_acknowledges_only_spendable_current_nonce_and_recovers_without_rpc_or_broadcast` covers successful acknowledgement/recovery and paid top-up settlement without duplicate credit; reuse it rather than broadcasting or widening the private transaction signer for tests. Keep native clock task 2.2.3 blocked until independently committable explicit-time coverage is available. Provider cursor acceptance does not add native pagination. Challenge rate-window policy still needs a separate specification/acceptance slice sharing the unresolved helper above.
4. Then specify owner-authorized opening/funding acknowledgement and exact original-transaction recovery before broader accounting. Compare control, consumer, chain-journal and ledger paths with the book and reuse archived pending-retention requirements. Remaining discovery, credential lifecycle, runtime/performance and consumer/reputation contracts are not fully aligned.

## Checkout discipline

Initial checkout: zero staged files, 194 unstaged paths and 50 untracked paths, including the owner's modified whitepaper and extensive native migration. Commit only the worker's renamed/extended HTTP control integration test, owner-control design/task acceptance updates and these notes. Do not absorb owner-uncommitted implementation files. Preserve all other work; no push. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
