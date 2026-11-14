# APAS progress

## 2026-11-14 — Verify per-payer pending-proposal capacity

- Completed `specify-pending-proposal-quotas` task 2.1 with two isolated ledger regressions in `src-tauri/tests/inference_proposal_quota_test.rs`: eight accepted proposals versus rejection of a ninth, shared attribution across models/offers/free/paid prices, independent payer/provider-ledger budgets, no persisted rejected quote, and exact signed-term preservation after rejection and reopening.
- Extended the production HTTP proposal tests with eight unauthenticated requests naming one payer using alternating address case. The scenario checks the ninth request's HTTP 429 / `rate_limit`, another payer's remaining capacity, retained agreements and absence of account creation or transaction submission. This is isolated loopback evidence, not a live attack or qualification.
- Ledger fixtures use explicit time and public test scalars through public APIs; they do not simulate a chain, owner authorization or native issuance. No production-source edits. Global capacity, concurrent admission, rate rollback and expiry/opening transitions remain separate tasks; both OpenSpec changes stay open.
- Updated the existing report entry **Public proposals can exhaust a named payer's quota** with the observed isolated HTTP exhaustion evidence, distinguishing it from untested sustained replenishment/live deployment. No author answer; it remains open and nonblocking. No authentication redesign or whitepaper edit.
- The previously read whitepaper fingerprint is unchanged: `docs/chiral-book.md`, SHA-256 `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict non-interactive validation of both open changes and the whitepaper checksum check.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (276 passed; two existing child-helper entrypoints ignored), including all three new regressions, followed by the corresponding headless `cargo build ... --bins`. No failures, retries or disabled checks.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Native localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial tasks 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) likewise need a safe explicit-time seam. Do not absorb owner work, use sleeps or weaken requirements.
2. Next: pending-quota task 2.2, the 10,000-record storage ceiling with bounded explicit-time fixtures; do not change native 300-second TTL/rates or claim long-lived fixtures came from native issuance. Then 2.3 competing connections and rejection rollback, followed by 2.4 expiry and successful/failed first-opening retention. Existing funding-intent/challenge/account bounds are not quote-capacity evidence.
3. Separately specify/test proposal-rate policy; review saved-agreement/historical recovery and evidence-only renewal coverage before adding cases (commercial 2.4.2.2.2 and 2.5). Keep process-global reader saturation isolated.
4. Await the author's response/whitepaper update for the nonblocking availability disclosure. Any public-proposal authentication redesign requires that design decision first. The earlier localhost-alias probe established static metadata admission, not a DNS/transport bypass.
5. Archive normally only after every change's tasks/checks are complete. Independent funding acknowledgement/nonce/recovery and exact accounting specifications remain future work, followed by discovery/authorization, runtime/performance and consumer/reputation. Full three-layer alignment is not established.

## Checkout discipline

Initial checkout: zero staged files, 194 unstaged paths and 50 untracked paths, including the owner's modified whitepaper and extensive native migration. Commit only this worker's new quota test, proposal-test addition, quota task progress, report evidence and these notes. Preserve all other work; no push. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
