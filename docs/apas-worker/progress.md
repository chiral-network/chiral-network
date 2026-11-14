# APAS progress

## 2026-11-14 — Verify atomic pending admission and rate conservation

- Completed pending-quota task 2.3 with three regressions in `src-tauri/tests/inference_proposal_quota_test.rs`. Two independent disk-backed connections competed for the eighth payer slot behind a shared start barrier; a reopened ledger contained exactly one winner, no rejected proposal, and unchanged original signed agreements.
- Paired in-memory ledgers received the same eight signed proposals. Only one received 1,024 capacity-rejected attempts in a fresh rate window; subsequent same-payer and unrelated-payer admission traces matched the untouched control. This verifies rate-allowance conservation in these scenarios without asserting numerical rate budgets.
- Fixtures use explicit time, public test scalars and storage-only lifetimes; no sleeps, raw SQL seeding, fake chain evidence or production-source edits. The rollback fixture also requires rejection at T−1 followed by successful pending reclamation at T; reuse that evidence when reviewing task 2.4 instead of duplicating it.
- Both OpenSpec changes remain open. The report entry **Public proposals can exhaust a named payer's quota** remains unanswered and nonblocking; no new author-only decision or report edit.
- The previously read whitepaper remains unchanged: `docs/chiral-book.md`, SHA-256 `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict non-interactive validation of both open changes and the whitepaper checksum check.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (280 passed; two existing child-helper entrypoints ignored), including all six quota regressions, followed by the corresponding headless `cargo build ... --bins`. No failures, retries or disabled checks.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Native localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam. Do not absorb owner work, use sleeps or weaken requirements.
2. Next: pending-quota 2.4, review existing expiry/acknowledged-funding recovery evidence and complete successful/failed first-opening slot transfer with retained signed terms. Then finish the implementation-contradiction review and remaining integration tasks before normal archive.
3. Separately specify/test proposal-rate policy. These comparative tests use the current clock-window behavior as fixture setup, not a rolling/monotonic-clock guarantee. Review saved-agreement/historical recovery and evidence-only renewal coverage (commercial 2.4.2.2.2 and 2.5); keep process-global reader saturation isolated.
4. Await the author's response/whitepaper update for the nonblocking availability disclosure. Any public-proposal authentication redesign requires that design decision. The earlier localhost-alias probe established static metadata admission, not a DNS/transport bypass.
5. Independent funding acknowledgement/nonce/recovery and exact accounting specifications remain future work, then discovery/authorization, runtime/performance and consumer/reputation. Full three-layer alignment is not established.

## Checkout discipline

Initial checkout: zero staged files, 194 unstaged paths and 50 untracked paths, including the owner's modified whitepaper and extensive native migration. Commit only this worker's quota-test extension, quota task progress and these notes. Preserve all other work; no push. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
