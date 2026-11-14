# APAS progress

## 2026-11-14 — Owner-control acceptance map and usage guidance

- Completed task 3.1's correspondence for all seven owner-control requirements: production entry points, executable acceptance selectors, source-only checks and unresolved evidence. Added a focused acceptance command and native/direct-HTTP/operator usage consequences without changing requirements, runtime, tests or the whitepaper.
- Distinguished existing native saved-account recovery (refresh without reopening) from the unresolved HTTP opening retry that rotates the root credential. The existing ledger test explicitly expects that rotation; it must change alongside the fix, retaining its no-duplicate-credit assertions. Native fixture evidence cannot close the real-router gap.
- Clarified task 2.2.3: the private explicit-time unit case already rejects expiry exactly at now; the adjacent 600/601-second receiver boundary remains unverified. No sleep-based test, exposed private API or weakened bound was introduced. Parent tasks 2.2/2.4 and final archive remain open.
- Re-read the authoritative design and report; neither changed and no author answer was supplied. **Public proposals can exhaust a named payer's quota** remains open and nonblocking. Whitepaper SHA-256: `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed the throwaway documentation check: all seven requirement mappings, 19 local links/anchors, eight qualified test references, issuer case names and the six-case capacity filter resolve against the actual files. No script or new permanent test was added.
- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict validation of all three open changes and the whitepaper checksum.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (331 tests; two existing child-helper entrypoints ignored) and the corresponding headless binary build. No test, runtime or requirement edits; all executed checks passed.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Rate tasks 2.4–2.5 remain blocked by the owner-source boundary: implement the durable non-regressing clock, protected cleanup and conservative initialization together with their regressions when independently committable. Reuse the existing failed-future-minute rollback evidence. Keep enclosing commercial quota work and final integration tasks open; do not archive incomplete changes or use `--skip-specs`.
3. Owner-control 2.4.3 remains blocked in owner-untracked source/tests. Keep native saved-account recovery working while correcting HTTP/ledger reopening, replacing the test expectation that implicitly rotates a root and adding real-router stable-credential recovery evidence. Native exact receiver-expiry coverage (2.2.3) is separately blocked at the private unit-test ownership boundary.
4. Next unblocked step: specify challenge issuance rate budgets/windows and rejection semantics, using the existing shared limiter but not blessing its clock-regression behavior. Keep that work separate from pending-challenge acceptance. Owner-control 3.2 cannot archive until 2.2.3 and 2.4.3 are complete.
5. Then specify owner-authorized opening/funding acknowledgement and exact original-transaction recovery before broader accounting. Reuse archived pending-retention requirements and existing native crash-boundary/chain-journal/router evidence without claiming full financial acceptance. Remaining discovery, credential lifecycle, runtime/performance and consumer/reputation contracts are not fully aligned.

## Checkout discipline

Initial checkout had no staged files and extensive owner-modified/untracked migration work, including the whitepaper and implementation. Commit only the worker's owner-control design/task updates and these notes. Keep usage clarifications in the existing change rather than absorbing owner-modified README/verification/operational documents. Preserve all other work; no push, deployment or installation. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
