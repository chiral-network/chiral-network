# APAS progress

## 2026-11-14 — Specify signed commercial terms

- Created `specify-inference-commercial-terms` with `openspec new change`; wrote its proposal, design, delta and tasks. It covers complete offer/model/profile/quote declarations, validity/origin and hardware consistency, public proposal, exact free/paid funding terms, consumer verification and immutable evidence-only renewal.
- Completed tasks 1.1–1.3. Strict validation passed. Implementation/coverage tasks remain open; do not archive yet. The existing `inference-wire-foundation` main spec and its archived change are unchanged.
- Inspected the actual validators, proposal/storage/consumer paths and existing tests. Native embedding output price is only checked for canonical money; the provisioner separately requires zero. Managed inventory selects an inventoried template without an embedding-specific prohibition. Tasks 2.2–2.3 require native-boundary regressions and enforcement; this turn did not fix or claim runtime proof of those gaps.
- Runtime sources and the whitepaper were not edited. The complete previously read `docs/chiral-book.md` remains unchanged, verified against SHA-256 `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`. No author-only decision or open report entry.

## Verification

- Passed `openspec validate specify-inference-commercial-terms --strict --no-interactive`.
- Passed sequentially: `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (203 library + 2 discovery + 18 foundation tests; 2 existing child-process entrypoints ignored), then the corresponding `cargo build ... --bins`.
- Passed `npm run check` (zero errors/warnings), `npm test` (15 tests), `npm run build` (only stale Browserslist data warning), Python unittest discovery (46 tests), and CI's seven-script `py_compile`. Both orchestration commands also exited zero.
- Native desktop dependencies, the actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS environments and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs, invented keys or deployment actions. Keep Rust checks sequential; no cache/dependency cleanup.

## Next, in order

1. Prioritize any new failure attributable to committed code. Native embedding enforcement tasks 2.2–2.3 outrank missing coverage but are currently blocked from a source-fix commit: their implementation files are pre-existing untracked owner work, and committing them would absorb that work. Reconsider only if source ownership becomes separable; do not weaken the requirement or mark these tasks complete.
2. Next unblocked slice: task 2.1, standalone deterministic commercial-schema/model/profile acceptance tests. Reuse existing coverage, test real rejection boundaries, and avoid wall-clock-edge flakiness. Then complete missing proposal/quote cases (2.4) and renewal coverage review (2.5) where independently committable.
3. Run final checks and archive normally only when every implementation task is genuinely complete. If native source fixes remain blocked, leave this change open and proceed to another independent capability.
4. Subsequently specify funding acknowledgement/nonce ownership/recovery and exact accounting, then discovery/authorization, runtime/performance and consumer/reputation behavior. Full three-layer alignment is not established.

## Checkout discipline

Extensive pre-existing modified/deleted/untracked implementation and documentation, including the whitepaper, belong to someone else. This step commits only its new OpenSpec change and these notes. Preserve all other work; no push. Git identity is per-command `APAS Worker <apas-worker@localhost>`, not persisted configuration.
