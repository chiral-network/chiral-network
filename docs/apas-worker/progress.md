# APAS progress

## 2026-11-14 — Verify signed offer admission before proposal

- Added four tests to `src-tauri/tests/inference_quote_acceptance_test.rs`, reusing the public `Consumer::quote` fixture: 22 signed invalid metadata cases reject before contacting the provider; five valid namespace/revision/concurrency boundaries remain usable; offer lifetimes of 3,600/3,601 seconds accept/reject without a now-relative edge.
- The byte-limit test accepts a 16,384-byte signed envelope and rejects 16,385 bytes before proposal even though the payload alone fits. Removing the optional signature prefix admits the identical payload at 16,383 bytes, preventing invalid metadata from masking the size boundary.
- Completed `specify-inference-commercial-terms` task 2.1.3.1. Exact now-relative issuance/expiry/qualification cases remain in 2.1.3.2; parent 2.1.3 and the change remain open. No production source edits, external services, funding, native engine launch or qualification claims.
- The whitepaper remains untouched: previously read `docs/chiral-book.md` matches SHA-256 `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`. No author-only question/report entry.

## Verification

- The initial headless all-targets run passed 203 library, 20 commercial, 9 origin and 2 discovery tests before a new size fixture incorrectly assumed the signer emitted `0x`. Fixed the fixture to construct the permitted prefixed representation explicitly, retaining every boundary assertion.
- Then `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --test inference_quote_acceptance_test --test inference_wire_foundation_test` passed all 13 consumer/admission and 18 foundation tests. Together, the runs cover 265 passing Rust tests; 2 existing child-process helpers remain ignored. The subsequent headless `cargo build ... --bins` passed. Keep Rust invocations sequential.
- Passed `npm run check` (zero errors/warnings), `npm test` (15 tests), `npm run build` (stale Browserslist warning only), Python unittest discovery (46 tests), CI's seven-script `py_compile`, strict non-interactive validation of the open OpenSpec change, and the whitepaper checksum check.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions.

## Blocked fixes and next steps

1. Prioritize any failure attributable to committed code. Public-origin localhost aliases (2.1.4), native embedding output price (2.2) and embedding generation-template admission (2.3) still need fixes in entirely untracked owner source (`offer_registry.rs`, `provider_config.rs`, `engine_runtime.rs`). Do not absorb that work or weaken requirements.
2. Preserve the earlier observed alias gap: public `validate_endpoint` rejects `localhost`/`*.localhost` but accepts `localhost.`/`*.localhost.`. The deleted probe established static metadata admission, not a DNS/transport bypass.
3. Next independent slice: actual provider proposal/readiness and remaining saved-agreement/recovery coverage (2.4.2), then renewal coverage review (2.5). Review existing tests before adding any. Fresh native quote verification (2.4.1) is already covered. Exact now-relative offer boundaries still need a safe explicit-time seam in owner source; do not copy/include production modules or add sleeps/near-now assertions.
4. Final checks and normal archive only after every task is complete. If remaining implementation work is blocked, leave this change open and specify independent funding acknowledgement/nonce/recovery and exact accounting, followed by discovery/authorization, runtime/performance and consumer/reputation. Full three-layer alignment is not established.

## Checkout discipline

Extensive pre-existing modified/deleted/untracked implementation and documentation, including the whitepaper, belong to someone else. Commit only this worker's consumer/admission test edits, task progress and these notes. Preserve other work; no push. Git identity is per-command `APAS Worker <apas-worker@localhost>`, not persisted configuration.
