# APAS progress

## 2026-11-14 — Transactional proposal-rate acceptance

- Completed proposal-rate task 2.3 with four passing ledger cases in `inference_proposal_quota_test.rs`.
- Payer-rate refusals must preserve the final shared unit for another payer. A duplicate-ID insertion attempted in a later minute must roll back pending cleanup and counter/window changes; after reopening, both payer budgets and the shared budget are compared with an untouched control through their exact exhaustion points.
- Independent connections race for the final payer and global rate units. Each race requires exactly one durable quote and no stored loser; the payer race also requires all 588 remaining global units to remain available after the losing write and another refusal.
- Reused the existing pending-capacity rollback cases instead of duplicating them. The new failed-future-minute case covers transaction rollback only, not backward samples after a newer successful commitment. Thread races/reopening are not crash, public-load, engine or settlement evidence.
- Production source and whitepaper are untouched. Native clock/pruning/bootstrap correction remains blocked in entirely untracked owner `billing_store.rs`; keep the change open.
- Whitepaper/report fingerprints are unchanged, with no author answer. Whitepaper SHA-256: `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`. **Public proposals can exhaust a named payer's quota** remains open and nonblocking.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict non-interactive validation of both open changes and the whitepaper checksum check.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (293 tests, including all four new cases; two existing child-helper entrypoints ignored), followed by the corresponding headless `cargo build ... --bins`. No check failed or was weakened.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Rate tasks 2.4–2.5 remain blocked by the owner-source boundary: implement the durable non-regressing clock, protected cleanup and conservative initialization together with their regressions when independently committable. Reuse task 2.3's failed-future-minute rollback evidence. Do not archive incomplete tasks or use `--skip-specs`.
3. Next unblocked review: commercial saved-agreement/historical recovery and renewal coverage (2.4.2.2.2 and 2.5). Preserve existing acknowledgement/nonce/recovery evidence and isolated process-global reader tests.
4. The author owns public-proposal authentication redesign or whitepaper disclosure. Broader funding acknowledgement/nonce/recovery and exact accounting specifications remain future work, then discovery/authorization, runtime/performance and consumer/reputation. Full three-layer alignment is not established.

## Checkout discipline

Initial checkout: zero staged files, 194 unstaged paths and 50 untracked paths, including the owner's modified whitepaper and extensive native migration. Commit only the worker's quota-test additions, proposal-rate task update and these notes. Preserve all other work; no push. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
