## 1. Establish the request-accounting contract

- [x] 1.1 Compare the read-only book and current/pending specifications with actual ledger, gateway, backend and startup paths; identify that quote-capacity exclusions lack a dedicated request-accounting foundation.
- [x] 1.2 Specify exact reservation/charge arithmetic, account-scoped identity, atomic account/credential transitions, inference-specific bounds, known/unknown usage, restart, performance-remedy binding and attributable recovery reads without changing runtime or whitepaper.
- [x] 1.3 Pass strict noninteractive validation and available README/CI checks; record the specification-only scope and current acceptance/ownership limits.

## 2. Inventory and complete ledger acceptance

- [x] 2.1 Complete requirement-to-test mapping from actual test bodies. The design records exact coverage and remaining gaps for money, identity, authority/limits, unknown/restart, performance binding and authorized request/receipt reads. The separate gateway/backend inventory remains 3.1; injected post-verification funding and supplied usage are not observed chain/engine evidence.
- [ ] 2.2 Add missing atomic reservation/terminal-fault and competing-writer coverage, preserving exact account/credential holds and original identities. Include definite rollback versus deliberately committed invalid-usage release, and first-terminal-result-wins after reopening. Pair any necessary native fix with its regression only when that source is independently committable.
- [ ] 2.3 Complete request/receipt identity, scope, bounded snapshot and durable sequence/failure acceptance, including no output replay and no financial mutation from authorized reads. Keep timestamp-only pagination's non-exhaustive limit explicit.
- [x] 2.4 Prove ledger reservation, known-usage settlement, unknown-usage restart and real authorized request/receipt recovery at full standalone quote storage. Evidence: `request_accounting_and_receipts_recover_at_full_quote_storage` passes with a genuine quote refusal, exact account/credential holds and charges, first-terminal-result preservation, unchanged scope/concurrency/cap/deduplication guards and signed owner/read-bearer snapshots across reopening. Complete signed bulk quotes and post-verification opening credit are prerequisites; supplied usage is not engine evidence. All 452 available Rust/frontend/Python tests, available builds/checks and strict validation pass; two existing Rust tests remain ignored. Public dispatch, process-kill durability and independent quote-work/issuance exhaustion remain open.
- [ ] 2.5 Fill the ledger edges identified by 2.1: prompt-count mismatch and cumulative-usage overflow with explicit invalid-usage release; exact inference-budget thresholds/cutoffs, time regression/reopening and completion despite exhausted fresh allowance. Use the existing public operation-time arguments; do not replace signed-time or quote-clock seams or claim current storage-pressure acceptance covers these budgets.

## 3. Complete workflow and independent resource boundaries

- [ ] 3.1 Inventory and fill actual gateway/backend acceptance for exact preparation before reserve/dispatch, stable-key duplicates/lost replies, known/unknown cancellation and durable-settlement failure stopping admission. Do not add runtime mock/fallback behavior or claim a controlled backend qualifies a model.
- [ ] 3.2 Verify admitted evidence remains bound through known/unknown settlement, valid waiver, restart and otherwise-identical evidence renewal; reuse existing performance tests. Full qualification, timing eligibility and withdrawal classification remain their own specification/evidence work, not inferred from this accounting interface.
- [ ] 3.3 Exercise independent committed-quote-issuance and signing-work exhaustion with existing-request settlement/reads still available and new-inference gates intact. BLOCKED where the new work budget or controlled issuance clock requires owner-untracked source/cutover; do not substitute wall-clock sleeps, fabricated accepted timestamps or full stored-quote occupancy.
- [ ] 3.4 Reconcile this capability's resource evidence with opening-intent parent 2.8.2. Keep unfinished dimensions and production quote-policy/root-recovery blockers open; do not claim this acceptance mitigates the public named-wallet quotas.

## 4. Integrate verified behavior

- [ ] 4.1 Finalize requirement/source/test correspondence and client/operator accounting consequences without editing the whitepaper. Retain financial schema, network/provider scope, historical evidence and all unrelated owner work.
- [ ] 4.2 Run required available checks and strict validation after implementation/acceptance. Archive with `openspec archive specify-inference-request-accounting --yes` only after all in-scope tasks and checks complete, never with `--skip-specs`.

No deployment, account operation, live payment, installer/hardware qualification, tool installation, automatic monetary migration or whitepaper edit is authorized. External release gates remain outside this checkout and are not closed by these tasks.
