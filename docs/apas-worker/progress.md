# APAS progress

## 2026-11-14 — Correct acknowledgement-driven proposal slot transfer

- Recovery review exposed a worker specification error before new account-opening tests were written: `register_funding_intent` durably inserts the acknowledgement/full quote/transaction/sender-nonce state and deletes the standalone quote in the same transaction. The pending-quota draft incorrectly retained that proposal until expiry or opening.
- Created `specify-acknowledged-proposal-slot-transfer` and corrected the still-open pending-quota definition, expiry scenario, first-opening requirement, proposal and design. Acknowledgement frees its own proposal slot; funding-intent admission remains separate; accepted evidence survives expiry/restart. No production code, tests, financial behavior or whitepaper changed.
- Read `preexpiry_funding_intent_preserves_quote_through_settlement_stall_and_restart` and `equivalent_intents_reuse_ack_and_conflicting_sender_nonce_survives_quota_expiry`: existing tests cover quote-row removal at acknowledgement, exact acknowledgement recovery and durable sender/nonce conflicts. Preserve them; add only missing consumer-visible slot-transfer cases.
- All three changes remain open. The new change adds a uniquely named requirement to `inference-proposal-admission`; normal archives must integrate it alongside the corrected pending-quota requirements. First-opening coverage remains pending, not silently completed by this specification correction.
- No author-only question arises from correcting the worker's draft. The report entry **Public proposals can exhaust a named payer's quota** remains unanswered and nonblocking. The previously read whitepaper fingerprint is unchanged: `docs/chiral-book.md`, SHA-256 `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict non-interactive validation of all three open changes and the whitepaper checksum check.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (280 passed; two existing child-helper entrypoints ignored), including the reviewed acknowledgement/recovery tests, followed by the corresponding headless `cargo build ... --bins`. No failures, retries, new tests or disabled checks.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Native localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam. Do not absorb owner work, use sleeps or weaken requirements.
2. Next: acknowledgement-transfer acceptance tasks, proving that a full pending budget gains exactly one slot on acknowledgement, replay cannot free another slot, and rejected registration preserves pending capacity. Keep ledger preconditions distinct from public owner/envelope/spendability validation; no fabricated payment success.
3. Then complete pending-quota 2.4: successful free opening and failed unpaid opening must respectively free/preserve pending capacity, with original signed terms/account credentials retained after cleanup and reopening. Reuse the existing T−1/T expiry evidence in the rollback fixture and existing acknowledged-funding recovery coverage.
4. Finish contradiction reviews/checks and normal archives only after each change's tasks are complete. Separately specify/test proposal-rate policy and review commercial saved-agreement/historical recovery and renewal coverage (2.4.2.2.2 and 2.5); keep process-global reader saturation isolated.
5. The author owns any public-proposal authentication redesign or whitepaper disclosure. Independent funding acknowledgement/nonce/recovery and exact accounting specifications remain future work beyond this narrow transfer rule, then discovery/authorization, runtime/performance and consumer/reputation. Full three-layer alignment is not established.

## Checkout discipline

Initial checkout: zero staged files, 194 unstaged paths and 50 untracked paths, including the owner's modified whitepaper and extensive native migration. Commit only this worker's new correction change, corrected pending-quota artifacts and these notes. Preserve all other work; no push. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
