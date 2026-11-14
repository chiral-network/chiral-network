# APAS progress

## 2026-11-14 — Specify pending-proposal capacity

- Created `specify-pending-proposal-quotas` with the `inference-proposal-admission` capability: eight pending quotes per canonical named payer, 10,000 per provider ledger, shared across models and free/paid proposals; atomic admission/rejection; exact expiry reclamation; successful first-opening slot transfer without erasing accepted agreements or financial evidence.
- Read actual proposal admission, cleanup and account-opening code. Existing bounded-challenge tests do not establish quote-capacity behavior. This is a specification step, not new runtime enforcement or regression evidence.
- Linked the commercial change's pending-capacity coverage task to the new change. Both changes remain open; quote-rate window policy, regression tasks and historical recovery review are not marked complete.
- Added the open report entry **Public proposals can exhaust a named payer's quota**. It proposes an author-owned whitepaper disclosure of source-derived availability exposure, not an authentication change. It is nonblocking for testing the current public-proposal contract; no live attack or rollout was performed.
- The previously read whitepaper remains unchanged: `docs/chiral-book.md`, SHA-256 `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict non-interactive validation of both open changes and the whitepaper checksum check.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (273 passed; two existing child-helper entrypoints ignored), followed by the corresponding headless `cargo build ... --bins`. No failed checks, disabled checks or production fixes.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Known native fixes remain blocked in entirely untracked owner source: localhost aliases (commercial 2.1.4), embedding output price (2.2) and embedding generation-template admission (2.3). Do not absorb that work or weaken requirements.
2. Next: implement isolated pending-proposal boundary tests under the new change, starting with payer attribution and capacity. Use `BillingStore::store_quote` explicit time; exercise global storage capacity without changing native 300-second TTL/rates or claiming long-lived fixtures came from the native issuer. Then cover competing connections, rejection rollback, expiry and account-opening retention.
3. Separately specify/test proposal-rate policy; review existing saved-agreement/historical recovery tests before adding cases, then review evidence-only renewal (commercial 2.5). Do not confuse challenge, account or funding-intent limits with pending quotes. Keep the process-global reader test isolated.
4. Exact now-relative offer issuance/expiry/qualification boundaries (2.1.3.2) still need a safe explicit-time seam in owner source. Paused Tokio time does not replace the protocol's system clock; no sleeps or fragile near-now assertions. The earlier alias probe established static metadata admission of `localhost.`/`*.localhost.`, not a DNS/transport bypass.
5. Await the author's response/whitepaper update for the report disclosure; any public-proposal authentication redesign is blocked on that design decision. Preserve the existing public route meanwhile.
6. Normal archives only after every change's tasks and checks are complete. Independent funding acknowledgement/nonce/recovery and exact accounting specifications remain future work, followed by discovery/authorization, runtime/performance and consumer/reputation. Full three-layer alignment is not established.

## Checkout discipline

Initial checkout: zero staged files, 194 unstaged paths and 50 untracked paths. Extensive pre-existing implementation/documentation changes, including the whitepaper, belong to someone else. Commit only this worker's new OpenSpec change, commercial task link, report and progress notes. No production/test edits this turn; preserve other work and do not push. Git identity is per-command `APAS Worker <apas-worker@localhost>`, not persisted configuration.
