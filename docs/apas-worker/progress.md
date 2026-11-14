# APAS progress

## 2026-11-14 — Verify bounded control-reader admission

- Added `src-tauri/tests/inference_control_readers_test.rs`: one isolated production-router scenario for the five-second body deadline, the shared 64-reader non-queuing limit, rejection before polling a surplus body, and permit reclamation after cancellation, transport/JSON/size errors and timeout.
- The scenario uses paused Tokio time and body-poll notifications, keeps 63 readers occupied during recovery probes, checks streamed 96 KiB versus one extra byte without Content-Length, then refills all 64 slots after timeout and cancellation. Successful probes require real signed, persisted quotes without creating accounts.
- Extracted the existing prepared-provider setup into `src-tauri/tests/support/proposal.rs`; the seven HTTP proposal tests retain their assertions and use the same constructor. The new test reserves loopback addresses but calls `contracts::router` in-process, without starting unused servers or simulating chain/engine replies. No production-source edits or qualification claims.
- Completed reader admission task 2.4.2.2.1; remaining quota/historical recovery coverage is in 2.4.2.2.2. The commercial change remains open; no archive.
- The previously read whitepaper is unchanged: `docs/chiral-book.md` SHA-256 `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`. No author-only question/report entry.

## Verification

- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets`, including the new reader scenario and all seven refactored proposal tests, followed by the corresponding headless `cargo build ... --bins`. No retries, disabled checks or production fixes.
- Passed `npm run check` (zero errors/warnings), `npm test` (15 tests), `npm run build` (stale Browserslist warning only), Python unittest discovery (46 tests), CI's seven-script `py_compile`, strict non-interactive OpenSpec validation and the whitepaper checksum check.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost aliases (2.1.4), native embedding output price (2.2) and embedding generation-template admission (2.3) still need fixes in entirely untracked owner source (`offer_registry.rs`, `provider_config.rs`, `engine_runtime.rs`). Do not absorb that work or weaken requirements.
2. Preserve the earlier observed alias gap: public `validate_endpoint` rejects `localhost`/`*.localhost` but accepts `localhost.`/`*.localhost.`. The deleted probe established static metadata admission, not a DNS/transport bypass.
3. Next: review existing tests before completing proposal quota and saved-agreement/historical recovery coverage (2.4.2.2.2), then renewal coverage (2.5). `BillingStore::store_quote` already accepts explicit time. Keep process-global reader tests in their own binary; shared setup is test code, not copied production modules.
4. Exact now-relative offer issuance/expiry/qualification boundaries (2.1.3.2) still need a safe explicit-time seam in owner source. Paused Tokio time does not replace the protocol's system clock. No sleeps or fragile near-now assertions.
5. Final checks and normal archive only after all tasks are complete. If remaining implementation work is blocked, specify independent funding acknowledgement/nonce/recovery and exact accounting, followed by discovery/authorization, runtime/performance and consumer/reputation. Full three-layer alignment is not established.

## Checkout discipline

Extensive pre-existing modified/deleted/untracked implementation and documentation, including the whitepaper, belong to someone else. Commit only this worker's control-reader test, shared fixture, proposal-test refactor, task progress and these notes. Preserve other work; no push. Git identity is per-command `APAS Worker <apas-worker@localhost>`, not persisted configuration.
