# APAS progress

## 2026-11-13 — Verify and archive the wire foundation

- Completed task 2.2 with ten additional deterministic tests in `src-tauri/tests/inference_wire_foundation_test.rs` (18 total): canonical UTF-8/escaping/integer bytes, all six framed domains, independent personal-signature verification, malformed signatures, exact Keccak offer/opening/top-up vectors and changed-term binding.
- Derived fixed vectors independently with Python's standard JSON encoder and installed OpenSSL 3.6.3; OpenSSL also verified the normalized signature. No Chiral helper generated expected values. Tests need no OpenSSL process, new dependency, real credential, clock or network. No generator files were created.
- Completed the available checks and all change tasks, then ran `openspec archive specify-inference-wire-foundation --yes` without `--skip-specs`. Archive: `openspec/changes/archive/2026-11-13-specify-inference-wire-foundation/`. Main spec: `openspec/specs/inference-wire-foundation/spec.md` (7 requirements). Strict validation passed both before archive and on the resulting main spec.
- Runtime sources and the whitepaper were not edited. `docs/chiral-book.md` still matches SHA-256 `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`; the complete previously read design remains current. No open report entry or author-only question.

## Verification and blockers

- Passed sequentially: `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (203 library + 2 discovery + 18 foundation tests; 2 existing child-process helper entrypoints ignored), then `cargo build --locked --no-default-features --manifest-path src-tauri/Cargo.toml --bins`.
- All frontend/Python check commands exited zero: `npm run check` (zero errors/warnings), `npm test` (15 tests), `npm run build` (stale Browserslist warning only), Python unittest discovery (46 tests) and CI's seven-script `py_compile`. The outer shell wrapper returned 1 because this worker inverted its final aggregation predicate; no project check failed and none was rerun to conceal that orchestration mistake.
- The earlier Axum/Tokio compilation issue is not a current blocker; its cause was not established. Keep Rust checks sequential; do not delete caches or rewrite others' dependencies.
- Native desktop still lacks WebKit/AppIndicator dependencies; release builds lack the actual `CHIRAL_POLICY_PUBLIC_KEY`. No installs or invented keys. Windows/macOS and live paid/GPU/public qualification require unavailable or out-of-checkout environments. Archiving this primitive contract does not close those release gates.

## Next, in order

1. Prioritize any failure attributable to committed code; preserve unrelated dirty work and pass over recorded blockers without a new safe diagnostic path.
2. Create the next OpenSpec change for complete signed offer/model/quote schema and validation rules: inspect `protocol.rs`, `offer_registry.rs`, `contracts.rs`, `consumer.rs` and their actual tests against the whitepaper. The foundation deliberately covers primitive schemas, not every commercial payload or model/profile constraint.
3. Then specify funding acknowledgement/nonce ownership/recovery and exact accounting; follow with discovery/authorization, runtime/performance and consumer/reputation behavior. Add missing requirements before changing implementation. Full three-layer alignment is not established.

## Checkout discipline

Extensive pre-existing modified/deleted/untracked implementation and documentation, including the whitepaper, belong to someone else. Commit only this worker's test changes, OpenSpec archive/main spec and these notes. Git identity remains per-command `APAS Worker <apas-worker@localhost>`; no identity configuration was persisted.
