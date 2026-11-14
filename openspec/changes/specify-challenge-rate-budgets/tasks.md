## 1. Specify policy and evidence boundaries

- [x] 1.1 Compare the unchanged whitepaper, owner-control pending/authorization requirements, proposal-rate policy and actual challenge admission/consumption/cleanup code.
- [x] 1.2 Specify independent 12/600 budgets, atomic nonrefundable issuance charges and a durable non-regressing challenge minute; distinguish current source behavior from the unimplemented safety contract.
- [x] 1.3 Pass strict non-interactive change validation and available project checks, recording unavailable release/platform gates and leaving the whitepaper unchanged.

## 2. Complete implementation and acceptance

- [ ] 2.1 Add deterministic 12/13 named-wallet rate, operation/alias attribution, normal minute-boundary, skipped-window and reopening acceptance. Keep occupancy from masking the rate boundary by consuming real owner proofs; cover no refund on consumption/expiry and continued usability of an already-issued valid proof after rate exhaustion.
- [ ] 2.2 Cover 600/601 ledger issuance, eligible other wallets, independent provider ledgers and proposal/inference/bearer-read isolation in both exhaustion directions. Reuse native canonicalization and HTTP 429 mapping evidence; do not add live timing races or fabricate financial/engine outcomes.
- [ ] 2.3 Cover both-bucket rollback, rejected-wallet conservation of the last global unit and concurrent issuance at the final wallet/global rate unit. Reuse pending-capacity, nonce-collision and injected-insertion-failure tests; verify accepted account/funding/credential state remains intact and undisclosed committed issuance is still charged.
- [ ] 2.4 Implement durable challenge-minute selection, protected per-resource cleanup and conservative adoption of retained counters in the production ledger. BLOCKED: ledger source and unit tests are entirely owner-untracked; do not copy them, add an unused helper or commit the surrounding migration. Reuse the proposal clock principle with disjoint resource state; preserve financial schemas, lifetimes and authorization.
- [ ] 2.5 With the native fix, add regressions for older/reordered samples, remaining/exhausted budgets, reopening, cross-resource future-time cleanup and independent clocks, failed future-minute rollback including the watermark, retained counters before adoption and fail-closed inconsistent evidence. BLOCKED with 2.4 where current production behavior contradicts the requirement; do not weaken expectations or commit failing tests.

## 3. Verify and integrate

- [ ] 3.1 Record requirement-to-source/test correspondence and operator/client consequences, including the fresh-proof availability cost of named-wallet exhaustion and clock correction; keep payment recovery and credential lifecycle separate.
- [ ] 3.2 Run all available project checks and strict validation after implementation, then archive with `openspec archive specify-challenge-rate-budgets --yes` only when every task is complete; never use `--skip-specs`.

The existing public named-wallet availability report is nonblocking. No new owner-authentication design, automatic retry, real payment, deployment, account/server change or whitepaper edit is authorized by this change.
