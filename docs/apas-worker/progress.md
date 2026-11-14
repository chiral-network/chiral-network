# APAS progress

## 2026-11-14 — Historical agreement verification

- Completed commercial task 2.4.2.2.2.2 with the passing `historical_saved_agreements_recheck_authenticity_before_recovery` case and a source/test correspondence table in the change's design.
- The case starts with real zero-funded onboarding, then deliberately ages/re-signs only the native-written consumer fixture envelopes. Neither, offer-only, quote-only and joint expiry exercise historical loading/resume; expired declarations under an unsaved account ID must fail fresh onboarding without HTTP or a new pending operation.
- Twenty invalid signer/domain, identity, term, validity and origin variants per expiry state must fail listing, credential export and recovery after consumer recreation. Refusal retains the invalid bytes and sends no HTTP. Restoring valid fixture records preserves the real provider account and original usable bearer.
- This is controlled persistence verification, not elapsed-time provider retention, paid settlement, OS-crash or hardware evidence. Reused the existing acknowledgement-binding, expired accepted/unaccepted recovery, original-journal and explicit-cancellation tests instead of duplicating financial fixtures. Full funding/cancellation/reorg specifications remain separate work.
- Production source and whitepaper are untouched. No author answer; **Public proposals can exhaust a named payer's quota** remains open and nonblocking. Whitepaper SHA-256: `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict non-interactive validation of both open changes and the whitepaper checksum check.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (295 tests, including the new case; two existing child-helper entrypoints ignored), followed by the corresponding headless `cargo build ... --bins`. No check failed or was weakened.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Rate tasks 2.4–2.5 remain blocked by the owner-source boundary: implement the durable non-regressing clock, protected cleanup and conservative initialization together with their regressions when independently committable. Reuse the existing failed-future-minute rollback evidence. Keep enclosing commercial quota work open; do not archive incomplete changes or use `--skip-specs`.
3. Next unblocked review: unchanged-term evidence-only renewal (commercial 2.5). Inspect existing `performance.rs` compatibility and `billing_store_tests.rs` renewal/request-binding cases before adding missing consumer-visible coverage. Preserve existing financial and isolated process-global reader tests.
4. The author owns public-proposal authentication redesign or whitepaper disclosure. Broader funding acknowledgement/nonce/recovery and exact accounting specifications remain future work, then discovery/authorization, runtime/performance and consumer/reputation. Full three-layer alignment is not established.

## Checkout discipline

Initial checkout: zero staged files, 194 unstaged paths and 50 untracked paths, including the owner's modified whitepaper and extensive native migration. Commit only the worker's proposal-test addition, commercial design/task updates and these notes. Preserve all other work; no push. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
