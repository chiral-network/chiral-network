# APAS progress

## 2026-11-14 — Exercise acknowledgement slot transfer

- Added two ledger acceptance regressions for `specify-acknowledged-proposal-slot-transfer` task 2.1. An acknowledged agreement frees exactly one of eight pending slots; a replacement fills it; replay returns the exact original acknowledgement without freeing another slot, both live and after reopening the ledger.
- Draining-provider rejection and acknowledgement-signing identity mismatch preserve all eight signed proposals, leave no acknowledgement or fee batch, and keep replacement admission blocked across reopening. The original transaction can subsequently register successfully, proving rejection did not poison its sender/nonce commitment.
- Fixtures independently encode and sign EIP-155 opening transactions with public test scalars and validate their exact envelopes through the native offline validator. These ledger tests assume prior owner/spendability admission: no RPC, funded wallet, fabricated `VerifiedPayment`, payment settlement or live qualification is involved.
- Existing signed agreements remain recoverable and no accounts or fee obligations appear from acknowledgement alone. Production source, whitepaper and the unanswered report remain unchanged. All three OpenSpec changes remain open; this turn does not complete recovery mapping, implementation review or first-opening coverage.
- The previously read whitepaper fingerprint is unchanged: `docs/chiral-book.md`, SHA-256 `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`. **Public proposals can exhaust a named payer's quota** remains an open, nonblocking author entry.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict non-interactive validation of all three open changes and the whitepaper checksum check.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (282 passed; two existing child-helper entrypoints ignored), including both new acknowledgement regressions, followed by the corresponding headless `cargo build ... --bins`. No failures, retries, disabled checks or production edits.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Native localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam. Do not absorb owner work, use sleeps or weaken requirements.
2. Next: complete acknowledgement-transfer 2.2/2.3 recovery mapping and contradiction review, reusing `preexpiry_funding_intent_preserves_quote_through_settlement_stall_and_restart` and `equivalent_intents_reuse_ack_and_conflicting_sender_nonce_survives_quota_expiry`. Add only genuinely missing consumer-visible cases, then finish checks and normal archive. Keep its distinct ADDED requirement compatible with the pending-quota delta.
3. Complete pending-quota 2.4: successful free opening and failed unpaid opening must respectively free/preserve pending capacity, with original signed terms/account credentials retained after cleanup and reopening. Reuse T−1/T expiry evidence in the rollback fixture and existing acknowledged-funding recovery coverage; then finish that change's review/checks/archive.
4. Separately specify/test proposal-rate policy and review commercial saved-agreement/historical recovery and renewal coverage (2.4.2.2.2 and 2.5); keep process-global reader saturation isolated. Archive changes only after all tasks finish, never with `--skip-specs`.
5. The author owns any public-proposal authentication redesign or whitepaper disclosure. Independent funding acknowledgement/nonce/recovery and exact accounting specifications remain future work beyond this narrow transfer rule, then discovery/authorization, runtime/performance and consumer/reputation. Full three-layer alignment is not established.

## Checkout discipline

Initial checkout: zero staged files, 194 unstaged paths and 50 untracked paths, including the owner's modified whitepaper and extensive native migration. Commit only the worker's quota regression file, acknowledgement-change task update and these notes. Preserve all other work; no push. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
