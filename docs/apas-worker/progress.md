# APAS progress

## 2026-11-14 — Verify advertised origins and record a hostname gap

- Added nine deterministic tests in `src-tauri/tests/inference_offer_origin_test.rs`: public HTTPS/default-port normalization, bounded development exceptions, credential/path/query/fragment rejection, reserved IPv4/IPv6 and mapped-address rejection, and legacy IPv4 spellings of loopback/metadata addresses. They call the real origin validator without clocks, DNS, sockets or reachability claims.
- Completed `specify-inference-commercial-terms` task 2.1.2 for that coverage; remaining signed registry admission moved explicitly to 2.1.3. Parent 2.1 stays open. No production source or whitepaper edits; no archive.
- A separate temporary native probe confirmed an additional gap: `validate_endpoint(..., false)` rejects `https://localhost/` and `https://provider.localhost/` but accepts `https://localhost./` and `https://provider.localhost./`. The terminal DNS dot bypasses the static localhost-name checks. This proves metadata admission only, not DNS/transport access. Task 2.1.4 records the required fix/regression; it is blocked by untracked owner `offer_registry.rs`. The probe source was deleted after observation.
- The previously read `docs/chiral-book.md` remains unchanged, verified against SHA-256 `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`. No author-only decision or report entry.

## Verification

- Passed sequentially: `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (203 library + 20 commercial + 9 origin + 18 foundation + 2 discovery tests; 2 existing child-process helpers ignored), then the corresponding headless `cargo build ... --bins`. The nine new tests passed without retries.
- Passed `npm run check` (zero errors/warnings), `npm test` (15 tests), `npm run build` (stale Browserslist warning only), Python unittest discovery (46 tests), CI's seven-script `py_compile`, and strict non-interactive validation of the open change.
- After the Rust checks finished, ran the temporary `apas-origin-alias-probe` example against the actual library. It performed no requests or DNS lookup. Preserve the observed gap; passing existing tests does not close it.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions. Keep Rust invocations sequential; no cache/dependency cleanup.

## Next, in order

1. Prioritize any failure attributable to committed code. Fix the observed public-origin localhost alias gap (2.1.4), then native embedding output-price/template gaps (2.2–2.3), only once independently committable source edits are possible. All necessary native files remain entirely untracked owner work; do not absorb them into a commit or weaken the contract.
2. Continue independently committable signed offer admission (2.1.3), proposal/quote coverage (2.4), and renewal coverage review (2.5). Exact current-time edges need a safe explicit-time seam; `insert_at` is private in owner code. Do not copy/include production modules or invent fragile near-now/sleep assertions to bypass this constraint.
3. Run final checks and archive normally only when every task is complete. If source fixes/time-seam work remain blocked, leave this change open and specify independent funding acknowledgement/nonce/recovery and exact-accounting requirements.
4. Discovery/authorization, runtime/performance and consumer/reputation specifications follow. Full three-layer alignment is not established.

## Checkout discipline

Extensive pre-existing modified/deleted/untracked implementation and documentation, including the whitepaper, belong to someone else. Commit only this worker's new origin test, task progress and these notes. Preserve other work; no push. Git identity is per-command `APAS Worker <apas-worker@localhost>`, not persisted configuration.
