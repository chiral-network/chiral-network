# APAS progress

## 2026-11-14 — Verify the ledger-wide pending-proposal ceiling

- Completed pending-quota task 2.2 with `ledger_wide_pending_ceiling_rejects_new_payers_without_evicting_signed_terms` in `src-tauri/tests/inference_proposal_quota_test.rs`. It admitted 10,000 signed quotes across 1,250 payers and two free/paid models, then observed `rate_limit` and no persisted excess quote for a new payer/model in two fresh rate windows.
- The identical rejected signed quote succeeded in a separate empty ledger; all 10,000 original signed agreements remained unchanged with no account created. The test uses public `BillingStore` APIs, private in-memory SQLite connections and explicit time, not raw SQL row seeding or altered production limits.
- Fixture issuance advances by 500 admissions per minute with deliberately longer storage-level lifetimes. Native 300-second issuance remains unchanged; this is defensive storage-boundary evidence, not native issuance, load/speed qualification or a deployed attack.
- Extracted unsigned test-payload construction so the large fixture signs each record once and derives the provider address once. Existing per-payer tests retain their assertions. No production-source changes; both OpenSpec changes remain open.
- The author report entry **Public proposals can exhaust a named payer's quota** remains unanswered and nonblocking; no new author-only decision or report edit. The previously read whitepaper fingerprint is unchanged: `docs/chiral-book.md`, SHA-256 `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict non-interactive validation of both open changes and the whitepaper checksum check.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (277 passed; two existing child-helper entrypoints ignored), including all three quota regressions, followed by the corresponding headless `cargo build ... --bins`. The quota binary finished in 24.61 seconds; no failures, retries or disabled checks.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Native localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam. Do not absorb owner work, use sleeps or weaken requirements.
2. Next: pending-quota task 2.3, competing independent connections at the final pending slot and rejection rollback of rate allowance. Then 2.4, expiry and successful/failed first-opening retention, reviewing existing acknowledged-funding recovery evidence before adding cases. Challenge/account/funding-intent limits are not quote-capacity evidence.
3. Separately specify/test proposal-rate policy; review saved-agreement/historical recovery and evidence-only renewal coverage (commercial 2.4.2.2.2 and 2.5). Keep process-global reader saturation isolated.
4. Await the author's response/whitepaper update for the nonblocking availability disclosure. Any public-proposal authentication redesign requires that design decision. The earlier localhost-alias probe established static metadata admission, not a DNS/transport bypass.
5. Normal archives only after all tasks/checks pass. Independent funding acknowledgement/nonce/recovery and exact accounting specifications remain future work, then discovery/authorization, runtime/performance and consumer/reputation. Full three-layer alignment is not established.

## Checkout discipline

Initial checkout: zero staged files, 194 unstaged paths and 50 untracked paths, including the owner's modified whitepaper and extensive native migration. Commit only this worker's quota-test extension, quota task progress and these notes. Preserve other work; no push. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
