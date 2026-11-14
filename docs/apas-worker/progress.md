# APAS progress

## 2026-11-14 — Saved-agreement conflict acceptance

- Completed commercial task 2.4.2.2.2.1 with a passing saved-term conflict case in `inference_proposal_test.rs`. It uses native `Consumer::fund` and the production control router to open a real zero-funded account on a pre-admitted loopback development offer.
- Validly re-signed replacement declarations alter nonce, validity, offer revision, model, price/funding, fee destination, concurrency or provider under the same account ID. Before and after consumer recreation, rejection must make no HTTP request and preserve the exact native-written protected record and exported credential.
- Original-term resumption must still return the original account, preserve the provider's signed quote and keep the original bearer usable without rotation. No fixture-written consumer state, chain simulation, paid funding, engine or qualification evidence is claimed.
- Reviewed existing native expired-acknowledged recovery, missing-journal refusal and expired-unaccepted refusal tests. They do not directly cover the existing `fund` signed-term conflict guard. Remaining historical recovery correspondence and renewal review stay open; do not duplicate those existing cases.
- Production source and whitepaper are untouched. No author answer; **Public proposals can exhaust a named payer's quota** remains open and nonblocking. Whitepaper SHA-256: `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict non-interactive validation of both open changes and the whitepaper checksum check.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (294 tests, including the new case; two existing child-helper entrypoints ignored), followed by the corresponding headless `cargo build ... --bins`. No check failed or was weakened.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Rate tasks 2.4–2.5 remain blocked by the owner-source boundary: implement the durable non-regressing clock, protected cleanup and conservative initialization together with their regressions when independently committable. Reuse the existing failed-future-minute rollback evidence. Do not archive incomplete tasks or use `--skip-specs`.
3. Next unblocked review: remaining saved-agreement/historical recovery correspondence (commercial 2.4.2.2.2.2), then unchanged-term evidence-only renewal (2.5). Preserve existing acknowledgement/nonce/recovery cases and isolated process-global reader tests. The new free-account case is not paid recovery evidence.
4. The author owns public-proposal authentication redesign or whitepaper disclosure. Broader funding acknowledgement/nonce/recovery and exact accounting specifications remain future work, then discovery/authorization, runtime/performance and consumer/reputation. Full three-layer alignment is not established.

## Checkout discipline

Initial checkout: zero staged files, 194 unstaged paths and 50 untracked paths, including the owner's modified whitepaper and extensive native migration. Commit only the worker's proposal-test additions, commercial task update and these notes. Preserve all other work; no push. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
