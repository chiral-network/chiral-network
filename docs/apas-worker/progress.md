# APAS progress

## 2026-11-14 — Native nonce byte-boundary acceptance

- Completed owner-control task 2.2.2 with two public-Consumer integration tests: native acceptance at one/255/256 ASCII bytes and 255/256 UTF-8 bytes, plus rejection at 257 UTF-8 bytes despite fewer characters. Existing empty/257-byte ASCII rejection tests remain unchanged.
- Accepted receiver cases must transport the exact nonce and a real signature bound to the selected opening request. The production provider must still return HTTP 401 for these substituted, unissued nonces, with no accepted account on either side. Test observations now correlate the actual response status with each control request.
- No runtime, schema, whitepaper or issuer-format change. Parent 2.2 remains open because exact now-relative expiry task 2.2.3 is blocked by the private helper/test module in owner-untracked `consumer.rs`; no sleeps or test hooks.
- Whitepaper/report fingerprints were unchanged at entry; no author answer. **Public proposals can exhaust a named payer's quota** remains open and nonblocking. Whitepaper SHA-256: `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict non-interactive validation of all three open changes and the whitepaper checksum check.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (306 tests, including all six native-consumer cases; two existing child-helper entrypoints ignored) and the corresponding headless `cargo build ... --bins`. No new warnings, failed checks or weakened tests. Parent owner-control task 2.2 and final integration remain open.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Rate tasks 2.4–2.5 remain blocked by the owner-source boundary: implement the durable non-regressing clock, protected cleanup and conservative initialization together with their regressions when independently committable. Reuse the existing failed-future-minute rollback evidence. Keep enclosing commercial quota work and final integration tasks open; do not archive incomplete changes or use `--skip-specs`.
3. Next unblocked work: provider proof replay/conditional consumption and exact ledger expiry (2.3), then the remaining route-authorization and global/transactional pending-capacity cases (2.4–2.5). Keep native clock task 2.2.3 blocked until independently committable explicit-time coverage is available. Preserve issuer/native receiver distinctions; query-bearing provider requests are not a new native pagination feature. Challenge rate-window policy still needs a separate specification/acceptance slice sharing the unresolved helper above.
4. Then specify owner-authorized opening/funding acknowledgement and exact original-transaction recovery before broader accounting. Compare control, consumer, chain-journal and ledger paths with the book and reuse archived pending-retention requirements. Remaining discovery, credential lifecycle, runtime/performance and consumer/reputation contracts are not fully aligned.

## Checkout discipline

Initial checkout: zero staged files, 194 unstaged paths and 50 untracked paths, including the owner's modified whitepaper and extensive native migration. Commit only this worker's nonce-boundary test additions, owner-control design/task acceptance updates and these notes. Do not absorb owner-uncommitted implementation files. Preserve all other work; no push. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
