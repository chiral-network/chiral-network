# APAS progress

## 2026-11-14 — Cover first-opening quota and agreement retention

- Completed pending-quota task 2.4 with two isolated ledger regressions. Missing-payment rejection preserves eight signed proposals and full pending capacity without accounts or fees, live and after reopening storage.
- Successful free opening releases exactly one slot; one replacement is admitted and a further proposal is rejected. At T−1 the remaining pending budget is still full; at T all eight slots are reusable, with a ninth still rejected after storage reopening. Expiry is permitted to exclude or delete old pending records; the test asserts capacity, not an implementation-specific deletion policy.
- Exact original signed account terms, financial state, credential identity and scopes remain recoverable through expiry cleanup and storage reopening using the original bearer. No second account-open command, credential rotation, fabricated payment or live RPC is involved; owner authorization is a ledger-call precondition.
- Production source and existing acknowledged-funding tests are untouched. Their recovery evidence remains mapped in the archived acknowledgement change, whose requirement is already in `inference-proposal-admission`. Pending-quota 2.5 and final integration remain open, as does the commercial-terms change.
- Whitepaper and report fingerprints are unchanged; no author answer. `docs/chiral-book.md` SHA-256 remains `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`. **Public proposals can exhaust a named payer's quota** remains open and nonblocking.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict non-interactive validation of both open changes and the whitepaper checksum check.
- Cancelled the initial headless run before finalizing the expiry test: the specification allows exclusion or deletion, so replaced an overly restrictive expired-record lookup assertion with complete eight-slot reuse and rejection of a ninth.
- The finalized `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` passed (284 tests; two existing child-helper entrypoints ignored), including both new regressions, followed by the corresponding headless `cargo build ... --bins`. No checks failed or were disabled.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Native localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam. Do not absorb owner work, use sleeps or weaken requirements.
2. Next: pending-quota 2.5, finish the implementation/spec correspondence review, then final checks and normal archive. Add its distinct requirements to the existing `inference-proposal-admission` capability without replacing the archived acknowledgement rule. Reuse current acceptance coverage rather than duplicating it.
3. Separately specify/test proposal-rate policy and review commercial saved-agreement/historical recovery and renewal coverage (2.4.2.2.2 and 2.5); keep process-global reader saturation isolated. Never archive with `--skip-specs` or close incomplete tasks.
4. The author owns any public-proposal authentication redesign or whitepaper disclosure. Independent funding acknowledgement/nonce/recovery and exact accounting specifications remain future work beyond the narrow transfer rule, then discovery/authorization, runtime/performance and consumer/reputation. Full three-layer alignment is not established.

## Checkout discipline

Initial checkout: zero staged files, 194 unstaged paths and 50 untracked paths, including the owner's modified whitepaper and extensive native migration. Commit only this worker's quota regression file, pending-quota task update and these notes. Preserve all other work; no push. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
