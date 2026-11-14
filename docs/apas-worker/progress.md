# APAS progress

## 2026-11-13 — Specify the inference wire foundation

- Created `specify-inference-wire-foundation` using `openspec new change`: proposal, design, tasks and an `inference-wire-foundation` delta covering full network identity, strict primitive/envelope schemas, exact money, canonical JSON, domains/signatures and payment commitments.
- Compared the whitepaper with actual `protocol.rs`, `network.rs`, `wallet.rs`, offer validation/tests and the control-domain adapter. No implementation changes. The design records existing coverage and missing byte-level tests.
- `openspec validate specify-inference-wire-foundation --strict --no-interactive` passed. Tasks 1.1–1.3 are complete; test and archive tasks remain unticked. Do not archive before completing them.
- Whitepaper: README-designated `docs/chiral-book.md`, read-only, still SHA-256 `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`; its complete contents remain unchanged from the prior full read. No report entry or author-only question is open.

## Verification and blockers

- Passed: `npm run check` (zero errors/warnings), `npm test` (15 tests), `npm run build` (stale Browserslist-data warning only), Python unittest discovery (46 tests) and CI's seven-script `py_compile` command.
- Passed sequentially: `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --lib` (203 passed, 2 existing ignored, no failures), then `cargo build --locked --no-default-features --manifest-path src-tauri/Cargo.toml --bins`.
- The prior Rust `--all-targets` compilation failure remains unresolved and was not rerun merely to confirm. Axum Router/service and Tokio watch mismatches occurred in pre-existing dirty binaries. Investigation found one resolved Axum (`0.7.9`) and Tokio (`1.49.0`), and Windows-only committed compiler overrides. No demonstrated committed-code cause or safe source fix; preserve others' edits.
- Build-artifact involvement is unproven. Build/test share un-hashed `libchiral_network` artifacts; keep Rust checks sequential. No caches were removed. Passing library tests do not establish all-targets success.
- Native desktop build/tests remain blocked by missing WebKit/AppIndicator dependencies; release builds lack the actual `CHIRAL_POLICY_PUBLIC_KEY`. No installs or invented keys. Windows/macOS and live paid/GPU/public-release qualification require unavailable or out-of-checkout environments.

## Next, in order

1. Prioritize a failing check only when attributable to committed code. The unrelated dirty-tree all-targets failure above remains blocked unless new evidence provides a safe diagnostic path.
2. Carry out task 2.1: missing network-isolation, strict-schema and canonical-wei boundary tests. Then task 2.2: independent canonical-byte, framing, signature and commitment vectors/rejection tests. Preserve useful existing tests and unrelated edits; never derive expected values through the helper under test.
3. Run task 2.3 checks, revalidate and archive normally when complete. Main specs remain empty until that archive.
4. Specify funding/nonce recovery and accounting next, then discovery/authorization, inference/runtime/performance and consumer/reputation. Full three-layer alignment has not been established.

## Checkout discipline

Extensive pre-existing modified/deleted/untracked code and documentation, including the whitepaper, are someone else's work. Commit only this change and these notes. Git identity is unset: use per-command `-c user.name='APAS Worker' -c user.email='apas-worker@localhost'`; do not change repository/global identity settings.
