# APAS progress

## 2026-11-14 — Verify commercial model/profile declarations

- Added 20 deterministic integration tests in `src-tauri/tests/inference_commercial_terms_test.rs`: complete offer/model/profile schema boundaries, unknown/missing fields, exact unsigned types, nullable metadata, UTF-8/hex/capability/token limits, profile platform/memory/device/capacity constraints and split/cache/batch/admission settings.
- Completed `specify-inference-commercial-terms` task 2.1.1, split from 2.1. Parent task 2.1 remains open for offer-receiver admission cases (2.1.2). These tests exercise public parsers and `validate_model`, not live offer acceptance or measured qualification; no clock, key, network, filesystem or engine fixtures were introduced.
- No production source or whitepaper edits. Previously read `docs/chiral-book.md` still matches SHA-256 `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`. No report entry or author-only question. The change remains open, not archived.

## Verification

- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets`: 203 library + 20 commercial + 18 foundation + 2 discovery tests; 2 existing child-process helper entrypoints ignored. The new 20 tests passed without a retry or production-code changes.
- Then passed `cargo build --locked --no-default-features --manifest-path src-tauri/Cargo.toml --bins`. Keep Rust checks sequential; do not clean caches or rewrite others' dependencies.
- Passed `npm run check` (zero errors/warnings), `npm test` (15 tests), `npm run build` (stale Browserslist warning only), Python unittest discovery (46 tests), CI's seven-script `py_compile`, and strict non-interactive validation of the open OpenSpec change.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs, fabricated keys or deployment actions.

## Next, in order

1. Prioritize any failure attributable to committed code. Native embedding output-price and generation-template enforcement (2.2–2.3) still outrank missing coverage but remain blocked from a source-fix commit: initial status confirms the necessary native files are entirely untracked owner work. Reconsider only if edits become independently committable; do not absorb that implementation or weaken the requirements.
2. Continue remaining offer-receiver admission coverage (2.1.2), then proposal/quote cases (2.4) and renewal coverage review (2.5), where independently committable. Current-time edge tests require a safe explicit-time seam; the existing `insert_at` helper is private in owner code. Do not copy/include production modules or use fragile near-now/sleep assertions to evade that boundary; complete other useful cases first.
3. Final checks and normal archive only after every implementation task is complete. Leave this change open if native fixes/time-seam work stay blocked; move to independent funding acknowledgement/nonce/recovery and exact-accounting specifications.
4. Discovery/authorization, runtime/performance and consumer/reputation specifications follow. Full three-layer alignment is not established.

## Checkout discipline

Extensive pre-existing modified/deleted/untracked implementation and documentation, including the whitepaper, belong to someone else. Commit only this worker's new test, task progress and these notes. Preserve all other work; no push. Git identity is per-command `APAS Worker <apas-worker@localhost>`, not persisted configuration.
