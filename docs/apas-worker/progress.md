# APAS progress

## 2026-11-14 — Pending-challenge capacity acceptance

- Completed owner-control task 2.5 with six acceptance cases: independent wallet quote/challenge allowances, exact-expiry and consumption release, HTTP global refusal, wallet/global final-slot races, nonce collision and transactional insertion failure.
- Verified pending-state preservation and rate-state rollback, including a separately reopened ledger after an injected `AFTER INSERT` failure. Races compare budgets with one successful admission in a control ledger rather than fixing rate-policy constants.
- Global ceilings use explicit restored-state snapshots, not a claim of 10,000 native issuances within the 120-second lifetime/rate windows. Financial preservation starts from genuine free HTTP accounts and a locally signed, acknowledged opening fixture after the spendability boundary; no RPC, broadcast, funds, engine or deployment operations.
- Whitepaper/report fingerprints were unchanged at entry; no author answer. **Public proposals can exhaust a named payer's quota** remains open and nonblocking. Whitepaper SHA-256: `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`. No runtime, schema or whitepaper changes.

## Verification

- Passed frontend type checks (zero errors/warnings), 15 frontend tests, frontend build (stale Browserslist warning only), 46 Python tests, CI's seven-script compilation, strict non-interactive validation of all three open changes and the whitepaper checksum.
- Passed `cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --all-targets` (331 tests, including all 19 owner-control cases; two existing child-helper entrypoints ignored), headless binary build and final owner-control strict validation. The first run caught an incorrect new fixture count: acknowledgement frees its standalone proposal slot. Corrected to eight standalone proposals plus the independent acknowledgement, per `inference-proposal-admission`; no requirement or assertion was weakened.
- Native desktop dependencies, actual release `CHIRAL_POLICY_PUBLIC_KEY`, Windows/macOS and live paid/GPU/public qualification remain unavailable/out-of-checkout gates. No installs or deployment actions; keep Rust invocations sequential.

## Blocked fixes and next steps

1. Prioritize failures attributable to committed code. Localhost-alias, embedding output-price and embedding-template fixes remain blocked in entirely untracked owner source (commercial 2.1.4, 2.2, 2.3). Exact now-relative offer/qualification boundaries (2.1.3.2) require a safe explicit-time seam; no sleeps or weakened requirements.
2. Rate tasks 2.4–2.5 remain blocked by the owner-source boundary: implement the durable non-regressing clock, protected cleanup and conservative initialization together with their regressions when independently committable. Reuse the existing failed-future-minute rollback evidence. Keep enclosing commercial quota work and final integration tasks open; do not archive incomplete changes or use `--skip-specs`.
3. Owner-control 2.4.3 remains blocked: authenticated repeated `/v1/contracts/open` calls reach `BillingStore::open_account`, which revokes existing root credentials and issues another. The book and owner-control retry requirement forbid implicit rotation of exported credentials. Fix ledger/native recovery together without absorbing owner-untracked source; add the stable-root regression with that fix. Parent 2.4 remains open.
4. Next unblocked owner-control work: finish task 3.1's requirement-to-source/test correspondence and any necessary usage documentation. Keep native clock task 2.2.3 blocked until independently committable explicit-time coverage is available. Do not archive while 2.2.3 or 2.4.3 remains open. Provider cursor acceptance does not add native pagination.
5. Then specify challenge rate-window policy (sharing the unresolved rate helper) and owner-authorized opening/funding acknowledgement with exact original-transaction recovery before broader accounting. Compare control, consumer, chain-journal and ledger paths with the book and reuse archived pending-retention requirements. Existing `control_acknowledges_only_spendable_current_nonce_and_recovers_without_rpc_or_broadcast` supplies separate admission/settlement/recovery evidence. Remaining discovery, credential lifecycle, runtime/performance and consumer/reputation contracts are not fully aligned.

## Checkout discipline

Initial checkout: zero staged files, 194 unstaged paths and 50 untracked paths, including the owner's modified whitepaper and extensive native migration. Commit only the worker's extended owner-control integration test, owner-control design/task acceptance updates and these notes after successful verification. Do not absorb owner-uncommitted implementation files. Preserve all other work; no push. Git identity remains per-command `APAS Worker <apas-worker@localhost>`.
