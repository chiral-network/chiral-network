# APAS progress

## 2026-11-14 — Exercise the production proposal router

- Added seven tests in `src-tauri/tests/inference_proposal_test.rs` using the actual `contracts::router`, isolated loopback HTTP and disk-backed `BillingStore`: signed copied terms, fresh identifiers/nonces, persistence across graceful provider shutdown, unauthenticated proposal versus owner authority, strict request/money handling, free/one-sided-paid funding, new-funding gates, exact local-offer selection and the 96 KiB body boundary.
- Fixtures validate explicit development configuration and use protected public-test secret files and distinct provider wallets. The persistence case awaits graceful server shutdown before reopening its ledger; other cases use abort-on-drop cleanup. Offers/readiness are seeded prerequisites, not native engine startup, health-probe or measured-performance evidence. No real chain service, funding, deployment or production-source edits.
- Completed task 2.4.2.1; remaining timed-reader/quota/historical recovery coverage is in 2.4.2.2. The overall commercial change remains open; do not archive it.
- The whitepaper remains untouched: previously read `docs/chiral-book.md` matches SHA-256 `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`. No author-only question/report entry.

## Verification

- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets`, including all seven new proposal tests, followed by the corresponding headless `cargo build ... --bins`. Initial compilation caught new-fixture import/status-type mistakes; corrected them and aligned owner-auth error expectations with the existing API before the passing test execution. No production edits or relaxed assertions. Keep Rust invocations sequential.
- Passed `npm run check` (zero errors/warnings), `npm test` (15 tests), `npm run build` (stale Browserslist warning only), Python unittest discovery (46 tests), CI's seven-script `py_compile`, strict non-interactive OpenSpec validation, and the whitepaper checksum check.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost aliases (2.1.4), native embedding output price (2.2) and embedding generation-template admission (2.3) still need fixes in entirely untracked owner source (`offer_registry.rs`, `provider_config.rs`, `engine_runtime.rs`). Do not absorb that work or weaken requirements.
2. Preserve the earlier observed alias gap: public `validate_endpoint` rejects `localhost`/`*.localhost` but accepts `localhost.`/`*.localhost.`. The deleted probe established static metadata admission, not a DNS/transport bypass.
3. Next: review existing tests before completing control-reader deadline/saturation, proposal quota and saved-agreement/recovery coverage (2.4.2.2), then renewal coverage (2.5). `contracts::router` and `ProviderState` are public; isolated production-route tests do not need to copy/include private modules. Tokio's test-time support is available for reader deadlines; avoid global-semaphore contention with parallel tests. `BillingStore::store_quote` already accepts explicit time for quota checks.
4. Exact now-relative offer issuance/expiry/qualification boundaries (2.1.3.2) still need a safe explicit-time seam in owner source. Keep current fresh-time fixtures away from clock edges; no sleeps or fragile near-now assertions.
5. Final checks and normal archive only after all tasks are complete. If remaining implementation work is blocked, specify independent funding acknowledgement/nonce/recovery and exact accounting, followed by discovery/authorization, runtime/performance and consumer/reputation. Full three-layer alignment is not established.

## Checkout discipline

Extensive pre-existing modified/deleted/untracked implementation and documentation, including the whitepaper, belong to someone else. Commit only this worker's new proposal test, task progress and these notes. Preserve other work; no push. Git identity is per-command `APAS Worker <apas-worker@localhost>`, not persisted configuration.
