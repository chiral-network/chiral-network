# APAS progress

## 2026-11-14 — Payer-rate acceptance coverage

- Completed proposal-rate task 2.1 with three passing explicit-time ledger cases in `inference_proposal_quota_test.rs`.
- The payer case exercises twelve admissions and rejection of the thirteenth, partial/exhausted-budget reopening, expiry without rate refund, the exact UTC-minute boundary and skipped minutes without accumulated allowance. One-second signed storage fixtures keep the eight-slot pending limit from masking the rate edge; they do not change native issuance TTL.
- The acknowledgement case leaves exactly one rate unit after eleven proposals, transfers the original pending slot without charging/refunding a rate unit, then retains exhaustion and exact acknowledgement recovery through reopening. It uses the existing real offline envelope helper, never settlement or fabricated chain evidence.
- The free-opening case likewise preserves the final rate unit, allows another accepted free quote to open when proposal rates are exhausted, and retains both original signed agreements/accounts/credentials through reopening. Owner authorization remains a ledger-call precondition, not an authentication claim.
- Production source and whitepaper are untouched. The non-regressing clock/pruning/bootstrap fix remains blocked in entirely untracked owner `billing_store.rs`; these forward-time tests do not establish clock-regression safety. No archive while other tasks remain.
- Whitepaper/report fingerprints are unchanged, with no author answer. Whitepaper SHA-256: `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`. **Public proposals can exhaust a named payer's quota** remains open and nonblocking.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict non-interactive validation of both open changes and the whitepaper checksum check.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (287 tests, including all three new cases; two existing child-helper entrypoints ignored), followed by the corresponding headless `cargo build ... --bins`. No check failed or was weakened.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Existing localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Next: proposal-rate task 2.2, 600/601 ledger admission, other eligible payers, shared model/free-paid attribution and independent provider ledgers. Preserve separate challenge/inference budgets; reuse existing HTTP 429/canonical-named-payer evidence.
3. Then rate task 2.3: both-bucket rollback, final-unit competition and failed insertion. Reuse existing pending-capacity rollback cases rather than duplicating them. Clock/pruning/bootstrap implementation and adversarial-time evidence remain blocked under 2.4–2.5; leave them unticked and keep the change open.
4. Review commercial saved-agreement/historical recovery and renewal coverage (2.4.2.2.2 and 2.5). Preserve existing acknowledgement/nonce/recovery evidence and isolated process-global reader tests. Never archive incomplete tasks or use `--skip-specs`.
5. The author owns any public-proposal authentication redesign or whitepaper disclosure. Broader funding acknowledgement/nonce/recovery and exact accounting specifications remain future work beyond these admission/retention contracts, then discovery/authorization, runtime/performance and consumer/reputation. Full three-layer alignment is not established.

## Checkout discipline

Initial checkout: zero staged files, 194 unstaged paths and 50 untracked paths, including the owner's modified whitepaper and extensive native migration. Commit only the worker's quota-test additions, proposal-rate task update and these notes. Preserve all other work; no push. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
