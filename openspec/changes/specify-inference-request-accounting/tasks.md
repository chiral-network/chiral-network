## 1. Establish the request-accounting contract

- [x] 1.1 Compare the read-only book and current/pending specifications with actual ledger, gateway, backend and startup paths; identify that quote-capacity exclusions lack a dedicated request-accounting foundation.
- [x] 1.2 Specify exact reservation/charge arithmetic, account-scoped identity, atomic account/credential transitions, inference-specific bounds, known/unknown usage, restart, performance-remedy binding and attributable recovery reads without changing runtime or whitepaper.
- [x] 1.3 Pass strict noninteractive validation and available README/CI checks; record the specification-only scope and current acceptance/ownership limits.

## 2. Inventory and complete ledger acceptance

- [ ] 2.1 Complete requirement-to-test mapping from actual test bodies. Reuse existing full-width/single-rounding, competing-balance, cap/concurrency/scope/revocation, deduplication, unknown/restart and waiver tests; identify only missing consumer-visible boundaries. Do not mistake injected post-verification funding or supplied usage for observed chain/engine evidence.
- [ ] 2.2 Add missing atomic reservation/terminal-fault and competing-writer coverage, preserving exact account/credential holds and original identities. Include definite rollback versus deliberately committed invalid-usage release, and first-terminal-result-wins after reopening. Pair any necessary native fix with its regression only when that source is independently committable.
- [ ] 2.3 Complete request/receipt identity, scope, bounded snapshot and durable sequence/failure acceptance, including no output replay and no financial mutation from authorized reads. Keep timestamp-only pagination's non-exhaustive limit explicit.
- [ ] 2.4 Prove ledger reservation, known-usage settlement, unknown-usage restart and real authorized request/receipt recovery at full standalone quote storage. Reuse the opening-intent bulk fixture discipline, require a genuine fresh quote refusal, preserve original financial/credential authority and exclude duplicate settlement. Disclose restored opening/history and supplied already-validated usage; leave public dispatch/engine validation to section 3.

## 3. Complete workflow and independent resource boundaries

- [ ] 3.1 Inventory and fill actual gateway/backend acceptance for exact preparation before reserve/dispatch, stable-key duplicates/lost replies, known/unknown cancellation and durable-settlement failure stopping admission. Do not add runtime mock/fallback behavior or claim a controlled backend qualifies a model.
- [ ] 3.2 Verify admitted evidence remains bound through known/unknown settlement, valid waiver, restart and otherwise-identical evidence renewal; reuse existing performance tests. Full qualification, timing eligibility and withdrawal classification remain their own specification/evidence work, not inferred from this accounting interface.
- [ ] 3.3 Exercise independent committed-quote-issuance and signing-work exhaustion with existing-request settlement/reads still available and new-inference gates intact. BLOCKED where the new work budget or controlled issuance clock requires owner-untracked source/cutover; do not substitute wall-clock sleeps, fabricated accepted timestamps or full stored-quote occupancy.
- [ ] 3.4 Reconcile this capability's resource evidence with opening-intent parent 2.8.2. Keep unfinished dimensions and production quote-policy/root-recovery blockers open; do not claim this acceptance mitigates the public named-wallet quotas.

## 4. Integrate verified behavior

- [ ] 4.1 Finalize requirement/source/test correspondence and client/operator accounting consequences without editing the whitepaper. Retain financial schema, network/provider scope, historical evidence and all unrelated owner work.
- [ ] 4.2 Run required available checks and strict validation after implementation/acceptance. Archive with `openspec archive specify-inference-request-accounting --yes` only after all in-scope tasks and checks complete, never with `--skip-specs`.

No deployment, account operation, live payment, installer/hardware qualification, tool installation, automatic monetary migration or whitepaper edit is authorized. External release gates remain outside this checkout and are not closed by these tasks.
