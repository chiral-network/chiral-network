# APAS progress

## 2026-11-14 — HTTP payment-authorization acceptance

- Completed owner-control subtask 2.4.2.2 and the mutation matrix 2.4.2 with four control-router tests for first/existing free and paid opening, new/cached funding intent and free/active-paid top-up authorization. Anonymous, bearer-only, incomplete-proof and wrong-owner attempts preserve account/credential/funding state; successful authentication remains distinct from payment success.
- Covered free-opening success, paid opening without an acknowledgement, cached acknowledgement recovery while draining, and top-up commitment refusals. Business refusals durably consume proofs; fresh-proof retries retain the same transaction bytes and cannot imply credit.
- Transactions are locally signed and checked by the public offline envelope validator. Active-paid-account prerequisites follow existing ledger tests AFTER the verification boundary with explicitly synthetic accepted-block inputs, not observed funds or chain qualification. No runtime/signer API, schema or whitepaper changes; no RPC, broadcast or engine use.
- Whitepaper/report fingerprints were unchanged at entry; no author answer. **Public proposals can exhaust a named payer's quota** remains open and nonblocking. Whitepaper SHA-256: `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict non-interactive validation of all three open changes and the whitepaper checksum check.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (325 tests, including all 13 control-route cases; two existing child-helper entrypoints ignored) and the corresponding headless `cargo build ... --bins`. No failed or weakened checks. Owner-control tasks 2.2.3, 2.4.3, 2.5 and final integration remain open.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Rate tasks 2.4–2.5 remain blocked by the owner-source boundary: implement the durable non-regressing clock, protected cleanup and conservative initialization together with their regressions when independently committable. Reuse the existing failed-future-minute rollback evidence. Keep enclosing commercial quota work and final integration tasks open; do not archive incomplete changes or use `--skip-specs`.
3. New blocked mismatch, owner-control 2.4.3: authenticated repeated `/v1/contracts/open` calls reach `BillingStore::open_account`, which revokes existing root credentials and issues another. The book and owner-control retry requirement forbid implicit rotation of exported credentials. Fix ledger/native recovery together without absorbing owner-untracked source; add the stable-root regression with that fix. Current tests cover unauthorized re-entry, not approval of this rotation behavior; parent 2.4 remains open.
4. Next unblocked work after payment acceptance: global/transactional pending-challenge capacity (2.5). Keep native clock task 2.2.3 blocked until independently committable explicit-time coverage is available. Provider cursor acceptance does not add native pagination. Challenge rate-window policy still needs a separate specification/acceptance slice sharing the unresolved rate helper above.
5. Then specify owner-authorized opening/funding acknowledgement and exact original-transaction recovery before broader accounting. Compare control, consumer, chain-journal and ledger paths with the book and reuse archived pending-retention requirements. Existing `control_acknowledges_only_spendable_current_nonce_and_recovers_without_rpc_or_broadcast` supplies separate admission/settlement/recovery evidence. Remaining discovery, credential lifecycle, runtime/performance and consumer/reputation contracts are not fully aligned.

## Checkout discipline

Initial checkout: zero staged files, 194 unstaged paths and 50 untracked paths, including the owner's modified whitepaper and extensive native migration. Commit only the worker's extended HTTP control integration test, owner-control design/task acceptance updates and these notes. Do not absorb owner-uncommitted implementation files. Preserve all other work; no push. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
