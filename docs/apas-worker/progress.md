# APAS progress

## 2026-11-14 — Scoped proposal-rate acceptance coverage

- Completed proposal-rate task 2.2 with two passing ledger cases in `inference_proposal_quota_test.rs`.
- The global case counts 600 signed proposals across models, offer IDs, free/paid prices and funding amounts, then rejects new-payer/new-model proposals across price modes. Exhaustion survives reopening without evicting accepted terms; an independent provider still admits its own proposal.
- The payer case shares twelve units across those serving terms, rejects subsequent proposals despite available pending capacity, and permits another payer and another provider. One-second storage fixtures avoid masking rate limits with pending occupancy; native issuance TTL is unchanged.
- Challenges remain available before/after proposal exhaustion without consuming or restoring proposal allowance. Free-account inference reservations are admitted separately and remain real ledger reservations, not simulated engine results or settlement. Existing HTTP canonical-named-payer/429 evidence is reused.
- Production source and whitepaper are untouched. The non-regressing clock/pruning/bootstrap fix remains blocked in entirely untracked owner `billing_store.rs`; these same/forward-minute cases do not establish backward-time safety. No archive while tasks remain.
- Whitepaper/report fingerprints are unchanged, with no author answer. Whitepaper SHA-256: `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`. **Public proposals can exhaust a named payer's quota** remains open and nonblocking.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict non-interactive validation of both open changes and the whitepaper checksum check.
- Corrected the fixture's initial compile error by using the existing public `auth::AuthPayload` type. Then passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (289 tests, including both new cases; two existing child-helper entrypoints ignored), followed by the corresponding headless `cargo build ... --bins`. No check was weakened.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Next: rate task 2.3, both-bucket rollback, final-unit competition and failed insertion. Reuse existing pending-capacity rollback cases. Clock/pruning/bootstrap implementation and adversarial-time evidence remain blocked under 2.4–2.5; leave them unticked and keep the change open.
3. Review commercial saved-agreement/historical recovery and renewal coverage (2.4.2.2.2 and 2.5). Preserve existing acknowledgement/nonce/recovery evidence and isolated process-global reader tests. Never archive incomplete tasks or use `--skip-specs`.
4. The author owns public-proposal authentication redesign or whitepaper disclosure. Broader funding acknowledgement/nonce/recovery and exact accounting specifications remain future work, then discovery/authorization, runtime/performance and consumer/reputation. Full three-layer alignment is not established.

## Checkout discipline

Initial checkout: zero staged files, 194 unstaged paths and 50 untracked paths, including the owner's modified whitepaper and extensive native migration. Commit only the worker's quota-test additions, proposal-rate task update and these notes. Preserve all other work; no push. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
