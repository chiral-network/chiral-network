# APAS progress

## 2026-11-14 — Complete acknowledgement recovery review

- Completed acknowledgement-transfer tasks 2.2/2.3: mapped the existing expiry/reopening, original-transaction, sender/nonce, independent intent-admission, paid-opening and public control tests in the change's design. No additional consumer-visible gap or implementation contradiction was found for this narrow transition; no production or test edits were needed.
- Checked the immediate transaction/commit boundary, registration and exact replay, expiry cleanup and retained funding evidence, plus the public owner's authorization and fresh envelope/spendability/qualification gates. Existing recovery bypasses fresh-admission gates, not ownership or original-transaction identity.
- Evidence limits remain explicit: older ledger tests use constructed post-verification inputs, the HTTP test uses synthetic RPC, and the new quota tests validate offline-signed envelopes with admission preconditions. Reopening a ledger is not a crash/power-loss campaign; none of this is live-chain, public-load or engine qualification.
- Normally archived `specify-acknowledged-proposal-slot-transfer` as `openspec/changes/archive/2026-11-14-specify-acknowledged-proposal-slot-transfer`, integrating its requirement into `openspec/specs/inference-proposal-admission/spec.md` and completing the generated purpose. Pending-proposal quotas and commercial terms remain open; first-opening work was not closed.
- The whitepaper and report remain untouched, with no author answer. Whitepaper SHA-256 remains `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3` (`docs/chiral-book.md`). **Public proposals can exhaust a named payer's quota** remains open and nonblocking.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict non-interactive validation of all three then-open changes and the whitepaper checksum check.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (282 passed; two existing child-helper entrypoints ignored), followed by the corresponding headless `cargo build ... --bins`. Normal archive and strict validation of the integrated `inference-proposal-admission` capability passed. No checks failed or were weakened.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Native localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam. Do not absorb owner work, use sleeps or weaken requirements.
2. Next: pending-quota 2.4, successful free opening and failed unpaid opening must respectively free/preserve pending capacity, with original signed terms/account credentials retained after cleanup and reopening. Reuse T−1/T expiry evidence in the rollback fixture and the mapped acknowledged-funding recovery coverage. Then finish that change's review/checks and normal archive, adding its distinct requirements to the same capability.
3. Separately specify/test proposal-rate policy and review commercial saved-agreement/historical recovery and renewal coverage (2.4.2.2.2 and 2.5); keep process-global reader saturation isolated. Never archive with `--skip-specs` or close incomplete tasks.
4. The author owns any public-proposal authentication redesign or whitepaper disclosure. Independent funding acknowledgement/nonce/recovery and exact accounting specifications remain future work beyond this narrow transfer rule, then discovery/authorization, runtime/performance and consumer/reputation. Full three-layer alignment is not established.

## Checkout discipline

Initial checkout: zero staged files, 194 unstaged paths and 50 untracked paths, including the owner's modified whitepaper and extensive native migration. Commit only this worker's acknowledgement-change evidence/task/archive artifacts, integrated capability and these notes. Preserve all other work; no push. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
