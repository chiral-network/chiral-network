# APAS progress

## 2026-11-14 — Complete pending-proposal correspondence review

- Completed pending-quota task 2.5: checked native canonical payer attribution, provider-ledger scope, shared capacity counts, transactional rollback, HTTP 429 mapping, exact expiry and first-opening retention. No contradiction or additional missing consumer-visible case was found within this change's native issuance/authorization boundaries.
- Updated the proposal/design with the implementation correspondence and existing acceptance evidence: ten ledger regressions and the actual HTTP named-payer quota case. Canonicalization occurs before native quote signing; direct ledger fixtures are not an additional public authorization boundary.
- Preserved the evidence limits: controlled storage lifetimes are not native issuance, independent threads/reopened handles are not crash qualification, and offline or synthetic-chain fixtures are not live settlement, engine or public-load evidence. Production source and tests remain unchanged this turn.
- Normally archived `specify-pending-proposal-quotas` as `openspec/changes/archive/2026-11-14-specify-pending-proposal-quotas`, adding its five requirements to `inference-proposal-admission` without removing or modifying the existing acknowledgement requirement. Expanded the capability purpose to cover both. Commercial terms remain open.
- Whitepaper and report fingerprints are unchanged, with no author answer. `docs/chiral-book.md` SHA-256 remains `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`. **Public proposals can exhaust a named payer's quota** remains open and nonblocking.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict non-interactive validation of both then-open changes and the whitepaper checksum check.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (284 tests; two existing child-helper entrypoints ignored), followed by the corresponding headless `cargo build ... --bins`. Normal archive and strict validation of the integrated `inference-proposal-admission` capability passed. No checks failed or were weakened.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Native localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam. Do not absorb owner work, use sleeps or weaken requirements.
2. Next: specify the uncovered proposal-rate policy and clock semantics before adding its acceptance coverage. Current `rate_limit` uses fixed `now / 60` buckets, not rolling windows; a differing bucket resets the counter. Existing quota rollback tests deliberately compare admission against an untouched control without fixing numerical rate budgets.
3. Review commercial saved-agreement/historical recovery and renewal coverage (2.4.2.2.2 and 2.5). Reuse the archived pending-quota/acknowledgement evidence; keep process-global reader saturation isolated. Never archive with `--skip-specs` or close incomplete tasks.
4. The author owns any public-proposal authentication redesign or whitepaper disclosure. Independent funding acknowledgement/nonce/recovery and exact accounting specifications remain future work beyond the narrow retention rules, then discovery/authorization, runtime/performance and consumer/reputation. Full three-layer alignment is not established.

## Checkout discipline

Initial checkout: zero staged files, 194 unstaged paths and 50 untracked paths, including the owner's modified whitepaper and extensive native migration. Commit only this worker's pending-quota review/task/archive artifacts, integrated capability update and these notes. Preserve all other work; no push. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
