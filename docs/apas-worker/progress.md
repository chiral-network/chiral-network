# APAS progress

## 2026-11-13 — Wire foundation boundary tests (task 2.1)

- Added `src-tauri/tests/inference_wire_foundation_test.rs` as a standalone integration target, leaving all pre-existing implementation files untouched.
- Eight deterministic tests cover full-tuple namespace/ledger isolation and preserved provider binding after rejected reopen, deployment/genesis configuration boundaries, required fields and primitive JSON types, unknown fields in signed envelopes/nested network/prices, and canonical wei syntax/full-width range.
- Tests use isolated temporary ledgers, fixed signed terms and a public test scalar; no RPC, model, real credential, wall-clock dependency or server operation. Existing CFX conversion/signature tests remain unchanged.
- `specify-inference-wire-foundation` passes strict OpenSpec validation. Task 2.1 is complete: all eight new integration tests passed. Tasks 2.2–3.1 remain open; do not archive yet.
- Whitepaper `docs/chiral-book.md` remains read-only and unchanged: SHA-256 `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`. The full previously read design remains current. No open report entry or author-only question.

## Verification and blockers

- Passed: `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --test inference_wire_foundation_test` (8 passed, none ignored); `npm run check` (zero errors/warnings), `npm test` (15 tests), `npm run build` (stale Browserslist warning only), Python unittest discovery (46 tests) and CI's seven-script `py_compile`.
- Passed sequentially: `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (203 library + 2 discovery + 8 new contract tests passed; 2 existing child-process helper entrypoints ignored), then `cargo build --locked --no-default-features --manifest-path src-tauri/Cargo.toml --bins`. The successful new integration build provided the new diagnostic path for this full check. The old Axum/Tokio compiler failure is no longer a current blocker; its cause remains unproven. No dependency/configuration edits or cache deletion.
- Native desktop requires missing WebKit/AppIndicator dependencies; release builds lack the actual `CHIRAL_POLICY_PUBLIC_KEY`. No installs or invented keys. Windows/macOS and paid/GPU/public qualification require unavailable or out-of-checkout environments.

## Next, in order

1. Prioritize failures attributable to committed code; preserve unrelated dirty work and pass over recorded blockers without a new safe diagnostic path.
2. Complete task 2.2: independent canonical JSON, UTF-8 framing, personal-signature and Keccak commitment vectors, plus domain/signer/encoding rejection and changed-term cases. Reuse useful existing coverage and never calculate expected values using the helper under test.
3. Finish task 2.3 checks, strictly revalidate and archive normally once every task is complete. Main specs remain empty until archival.
4. Specify funding/nonce recovery and accounting next, then discovery/authorization, inference/runtime/performance and consumer/reputation. Full three-layer alignment is not established.

## Checkout discipline

Extensive pre-existing modified/deleted/untracked code and docs, including the whitepaper, belong to someone else. Commit only the new integration test, this change's task update and these notes. Git identity is unset: use per-command `-c user.name='APAS Worker' -c user.email='apas-worker@localhost'`, not persisted identity settings.
