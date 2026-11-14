# APAS progress

## 2026-11-14 — Native owner-proof reconstruction acceptance

- Completed owner-control task 2.2.1 with four public-Consumer integration tests. A real local production provider issues challenges; middleware supplies 26 adversarial variants covering bound identities/operation fields, domain, signed-field set, encodings, required response fields and nonce/expiry guards.
- Rejection requires the intended challenge to have been delivered and no subsequent control request or accepted account. Positive free-opening, account-read and credential-revocation paths verify real wallet signatures against transmitted POST/GET/DELETE bytes, including uppercase hex and misleading unsigned extras.
- Split remaining nonce byte-boundary coverage into 2.2.2 and exact now-relative expiry coverage into 2.2.3. The latter is blocked by the private helper/test module in owner-untracked `consumer.rs`; no sleeps, test hooks, runtime, schema or whitepaper changes. Parent 2.2 stays open.
- Whitepaper/report fingerprints were unchanged at entry; no author answer. **Public proposals can exhaust a named payer's quota** remains open and nonblocking. Whitepaper SHA-256: `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict non-interactive validation of all three open changes and the whitepaper checksum check.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (304 tests, including all four new native-consumer cases; two existing child-helper entrypoints ignored) and the corresponding headless `cargo build ... --bins`. No new warnings, failed checks or weakened tests. Parent owner-control task 2.2 and final integration remain open.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Rate tasks 2.4–2.5 remain blocked by the owner-source boundary: implement the durable non-regressing clock, protected cleanup and conservative initialization together with their regressions when independently committable. Reuse the existing failed-future-minute rollback evidence. Keep enclosing commercial quota work and final integration tasks open; do not archive incomplete changes or use `--skip-specs`.
3. Next unblocked work: native nonce byte-boundary acceptance (2.2.2), then provider replay/conditional consumption, route-authorization and global/transactional capacity cases (2.3–2.5). Keep 2.2.3 blocked until an independently committable explicit-time seam is available. Preserve issuer/native receiver distinctions; query-bearing provider requests are not a new native pagination feature. Challenge rate-window policy still needs a separate specification/acceptance slice sharing the unresolved helper above.
4. Then specify owner-authorized opening/funding acknowledgement and exact original-transaction recovery before broader accounting. Compare control, consumer, chain-journal and ledger paths with the book and reuse archived pending-retention requirements. Remaining discovery, credential lifecycle, runtime/performance and consumer/reputation contracts are not fully aligned.

## Checkout discipline

Initial checkout: zero staged files, 194 unstaged paths and 50 untracked paths, including the owner's modified whitepaper and extensive native migration. Commit only the worker's new native consumer integration test, owner-control design/task acceptance updates and these notes. Do not absorb owner-uncommitted implementation files. Preserve all other work; no push. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
