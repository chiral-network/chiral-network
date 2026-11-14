# APAS progress

## 2026-11-14 — Evidence-only renewal correspondence

- Completed commercial task 2.5 by comparing the renewal requirement with `protocol`, `performance`, ledger, provider readiness and gateway code and the existing regressions. Added the source/test correspondence and fixture limitations to the commercial design; no duplicate tests or runtime edits were needed.
- Existing cases cover unchanged terms, strictly newer evidence, preserved account prices/fees/credit, admitted-report settlement/restart binding, old withdrawal latches, public model visibility and refusal to substitute renewal into a fresh quote. Historical funding recovery coverage remains intact.
- Compatibility is not qualification: readiness probes validate report identity and feed the qualified-model gate, which the gateway rechecks before reservation. Synthetic ledger/visibility/parser fixtures do not qualify GPUs, real payments, runtime failure recovery or public rollout.
- Whitepaper and report fingerprints remain unchanged; no author answer. **Public proposals can exhaust a named payer's quota** remains open and nonblocking. Whitepaper SHA-256: `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict non-interactive validation of both open changes and the whitepaper checksum check.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (295 tests; two existing child-helper entrypoints ignored), followed by the corresponding headless `cargo build ... --bins`. Existing renewal and historical recovery cases remain passing; no check failed or was weakened.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Rate tasks 2.4–2.5 remain blocked by the owner-source boundary: implement the durable non-regressing clock, protected cleanup and conservative initialization together with their regressions when independently committable. Reuse the existing failed-future-minute rollback evidence. Keep enclosing commercial quota work and final integration tasks open; do not archive incomplete changes or use `--skip-specs`.
3. Next unblocked specification work: owner-authorized opening/funding acknowledgement and exact original-transaction recovery, before broader financial accounting. Compare the actual control, consumer, chain-journal and ledger paths with the whitepaper; reuse the archived pending-retention rules rather than duplicating them. Add specifications before uncovered implementation work.
4. Then cover remaining exact accounting, discovery/authorization, runtime/performance and consumer/reputation contracts. The author owns public-proposal authentication redesign or whitepaper disclosure. Full three-layer alignment is not established.

## Checkout discipline

Initial checkout: zero staged files, 194 unstaged paths and 50 untracked paths, including the owner's modified whitepaper and extensive native migration. Commit only the worker's commercial design/task updates and these notes. Existing native renewal tests are owner-uncommitted evidence, not files to absorb. Preserve all other work; no push. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
