# APAS progress

## 2026-11-14 — Owner-control authorization foundation

- Created `specify-owner-control-authorization` for the missing prerequisite to opening/funding work: public challenge schemas, exact request-bound signing bytes, durable one-use consumption, pending capacity, quote/account ownership, read-bearer boundaries and ambiguous-outcome retry semantics.
- Compared `auth`, `protocol`, control routes, ledger, native consumer and existing tests. Preserved the 120-second issuer lifetime separately from the native receiver's 600-second plausibility guard; neither extends provider-side validity. No runtime or whitepaper edit.
- Added proposal, delta, design/source-evidence map and explicit acceptance tasks. Existing tests cover selected binding/replay/race cases, not the full route/edge matrix; implementation tasks remain unchecked and the change is not archived.
- Whitepaper/report fingerprints were unchanged at entry; no author answer. **Public proposals can exhaust a named payer's quota** remains open and nonblocking. Whitepaper SHA-256: `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict non-interactive validation of all three open changes and the whitepaper checksum check.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (295 tests; two existing child-helper entrypoints ignored), followed by the corresponding headless `cargo build ... --bins`. No check failed or was weakened; these existing tests do not close the new change's unchecked acceptance tasks.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Rate tasks 2.4–2.5 remain blocked by the owner-source boundary: implement the durable non-regressing clock, protected cleanup and conservative initialization together with their regressions when independently committable. Reuse the existing failed-future-minute rollback evidence. Keep enclosing commercial quota work and final integration tasks open; do not archive incomplete changes or use `--skip-specs`.
3. Next unblocked work: owner-control acceptance tasks 2.1–2.5, using existing tests first and adding isolated, consumer-visible regressions only for gaps. Keep issuer/native receiver distinctions and owner-versus-bearer routing exact. Shared challenge rate-window policy still needs its own specification/acceptance slice; it shares the unresolved clock/cleanup helper above.
4. Then specify owner-authorized opening/funding acknowledgement and exact original-transaction recovery before broader accounting. Compare control, consumer, chain-journal and ledger paths with the book and reuse archived pending-retention requirements. Remaining discovery, credential lifecycle, runtime/performance and consumer/reputation contracts are not fully aligned.

## Checkout discipline

Initial checkout: zero staged files, 194 unstaged paths and 50 untracked paths, including the owner's modified whitepaper and extensive native migration. Commit only this worker's new owner-control OpenSpec change and these notes. Do not absorb owner-uncommitted implementation/evidence files. Preserve all other work; no push. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
