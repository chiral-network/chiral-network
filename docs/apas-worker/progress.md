# APAS progress

## 2026-11-14 — Specify durable proposal-rate budgets

- Created `specify-proposal-rate-budgets` for the existing `inference-proposal-admission` capability: 12 committed proposals per canonical named payer and 600 per provider ledger per effective minute, shared across models and free/paid terms. Pending occupancy limits remain separate.
- Chose a durable non-regressing minute, `max(floor(admission_now / 60), last_committed_proposal_minute)`, with atomic quote/counter/window commitment, rollback on rejection, no lifecycle refunds, protected cleanup and preservation of retained pre-watermark counters. This is fixed-minute admission, not rolling-window or throughput certification.
- Current source resets counts on any differing minute and lets shared cleanup remove quote-rate rows. [INFERENCE] Older or reordered timestamps can replenish allowance. The handler samples time before the write transaction; no runtime clock exploit was demonstrated. The proposal explicitly requires a native correction rather than describing current code as compliant.
- Native fix/regression tasks 2.4–2.5 are blocked because `billing_store.rs` is entirely untracked owner source. Numerical, forward-boundary, scope and transaction acceptance work can proceed independently. No production/test edits or archive this turn; do not absorb owner work or commit failing clock regressions.
- Whitepaper and report remain untouched, with no author answer. `docs/chiral-book.md` SHA-256 remains `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`. **Public proposals can exhaust a named payer's quota** remains open and nonblocking; these clock/rate details require no author decision.

## Verification

- Passed strict non-interactive validation of the new rate change and commercial-terms change, 46 Python tests, CI's seven-script compilation and the whitepaper checksum check.
- Passed frontend type checks (zero errors/warnings), 15 frontend tests and the frontend build (stale Browserslist warning only).
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (284 tests; two existing child-helper entrypoints ignored), followed by the corresponding headless `cargo build ... --bins`. No checks failed or were weakened. These unchanged-code checks do not prove the new clock requirement.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Existing localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Next: proposal-rate task 2.1, deterministic payer 12/13 admission, forward-minute boundary, reclamation without rate refund and reopening. Use short-lived signed storage fixtures to avoid the eight-slot pending cap masking the rate edge; do not change or claim native issuance TTL.
3. Continue rate tasks 2.2–2.3 for 600/601 ledger admission, scopes and transactional competition/conservation. Reuse archived pending-quota rollback and HTTP evidence. Clock/pruning/bootstrap implementation and adversarial-time evidence remain blocked under 2.4–2.5; leave them unticked and keep the change open.
4. Review commercial saved-agreement/historical recovery and renewal coverage (2.4.2.2.2 and 2.5). Preserve existing acknowledgement/nonce/recovery evidence and isolated process-global reader tests. Never archive incomplete tasks or use `--skip-specs`.
5. The author owns any public-proposal authentication redesign or whitepaper disclosure. Broader funding acknowledgement/nonce/recovery and exact accounting specifications remain future work beyond these admission/retention contracts, then discovery/authorization, runtime/performance and consumer/reputation. Full three-layer alignment is not established.

## Checkout discipline

Initial checkout: zero staged files, 194 unstaged paths and 50 untracked paths, including the owner's modified whitepaper and extensive native migration. Commit only this worker's new proposal-rate change and these notes. Preserve all other work; no push. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
