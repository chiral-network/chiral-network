# APAS progress

## 2026-11-21 (ninety-fifth round) — Acceptance: opening-intent 2.2 (fresh HTTP admission matrix)

- In step at `de9bccc2`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-opening-funding-intent` parent 2.2), through a workflow: write tests, then an adversarial review. design.md gains "Parent 2.2: fresh HTTP admission matrix". It maps five dimensions (owner authentication, envelope, current nonce and chain evidence, full signed gas cost, qualification and readiness) to requirement text and to passing tests whose assertions were read in source, and names the gap each new test fills. No product code changed and no code difference was found.
- Five new tests in `inference_funding_admission_test`, which now has 45:
  - `fresh_intent_owner_refusals_precede_chain_observation_with_every_gate_open`: proofs that are expired, missing, signed by another key, or bound to another original or quote get 401. Another wallet presenting its own payment gets 403, and the review added a never-issued quote (404 `not_found`). Each refusal makes no RPC call and writes no intent or paid accounting. The owner's fresh proof is then acknowledged, and replaying it gets 401.
  - `nonce_refusals_keep_the_earlier_acknowledgement_and_admit_the_unchanged_next_original`: a second agreement refused at the nonce observation four times leaves the first acknowledgement's row unchanged; once nonce 0 is spent, the same next-nonce bytes are acknowledged.
  - `fresh_intent_requires_identity_checked_healthy_chain_evidence_before_and_after_balance`: another chain ID, a syncing node, a frontier above the head and stale blocks, both before the balance call (no `eth_getBalance`) and at its reply. The review added another genesis, a stale safe frontier behind a fresh head, a future head, a safe timestamp later than the head's, and a same-height safe hash.
  - `fresh_intent_admits_type_two_priority_fee_equal_to_its_signed_maximum`: one wei below full cost is refused, exact cost is acknowledged.
  - `fresh_intent_gates_refuse_before_chain_observation_and_qualification_is_rechecked_after_balance`: draining, readiness and lost qualification are each refused before any RPC; qualification lost at the balance reply is refused after it.
- Mutations: the author ran 9 and the review 9 more; each fails a named new test. Six passed every earlier test of the target: a head-only post-balance recheck, refusing priority equal to its maximum, dropping the safe-frontier age, allowing a future head, and dropping either frontier-order clause. Skipping genesis at admission passed all lib `chain_client` tests. The fixture's chain view gained parameters whose defaults equal the old healthy view; no assertion was weakened.
- Covered by composition, not isolated at the route: contract creation (exact recipient binding), a pre-EIP-155 legacy `v` (core decoder test), the 16 KiB decoded bound (payment-envelope-bounds 2.5 keeps the direct guard evidence), and withdrawal or engine exit (through `qualified_model`, which the gates test pins).
- Observation, not a difference here: a configured-RPC chain-ID or genesis mismatch is refused as 400 `invalid_payment`. This spec requires only refusal; the classification is `specify-funding-settlement` code difference 4, and the health test will need updating when that is fixed. Owner-proof expiry is guarded twice, so only removing both guards fails a test.
- 2.2 ticked. Checks: `--lib` 469 passed, 12 ignored (helpers), 0 failed; bins build and `cargo check --bins --tests` pass; `inference_funding_admission_test` 45/45; desktop `--lib desktop::` 10/10; strict validation passes. Test-only change; round 89's full suite covers the product code. `inference_owner_control_test` cells are cited from round 89 (61/61), unchanged since.

## 2026-11-21 (ninety-fourth round) — Acceptance: opening-intent 2.8 (acknowledgement alone never opens or credits; free opening needs no payment)

- In step at `43294b40`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-opening-funding-intent` parent 2.8), through a workflow: write tests, then an adversarial review. design.md gains "Parent 2.8: acknowledgement alone and free opening without payment". It maps five statements at the ledger, HTTP and native layers to the tests that pass, with a table of every code path that writes accounts, credentials, credit, fee obligations, credit state and fee batches. No product code changed and no code difference was found.
- Four new tests:
  - `billing_store::tests::acknowledgement_alone_never_opens_or_credits_through_lease_release_expiry_and_reopening`: two acknowledged paid originals stay refused, with exact messages, for opening without payment, the bytes check, a different payment, a top-up, an account read, credential issue and rotation, and a funding recheck. This holds with the lease active, after release, after expiry, 30 days later and after reopening, and the full protected snapshot never changes. As a control, each opens with its own payment.
  - `billing_store::tests::free_and_paid_terms_never_cross_at_opening_intent_or_topup`: a transfer or a fabricated zero-value payment on free terms, and paid terms without payment, are refused at opening, the bytes check and registration. Crossed signed terms are refused at storage. A free top-up is forbidden. Free opening adds only its account and root credential.
  - `inference_funding_admission_test::free_terms_refuse_paid_funding_intents_and_open_without_payment_or_rpc`: over HTTP, free terms get 400 for a signed zero-value intent and for a transfer, with nine tables unchanged and nothing submitted. A null-transaction open creates a free account with no RPC.
  - `inference_owner_consumer_test::native_refuses_crossed_free_and_paid_terms_before_any_request_or_payment`: native `fund` refuses crossed quotes with the exact error and no saved agreement, journal entry or request. `Consumer::topup` refuses a free account.
- The review corrected two citations: native quote verification was already tested by `inference_quote_acceptance_test`, and the owner-control native test does not count funding attempts. It added the credential, rotation and recheck refusals to the acknowledgement test. It ran 7 more mutations; one, paid opening creating an uncredited account, passed the existing `paid_open_never_registers_implicitly_even_before_expiry` but fails both new tests. The author ran 6.
- Recorded limits:
  - Over HTTP, the funding-intent route has no free-terms check of its own. A signed zero-value payment for free terms reaches the balance and nonce RPC before the ledger refuses it. Nothing is written or submitted and the spec sets no order, so this is defence in depth, not a code difference.
  - For an acknowledged but unopened agreement, the HTTP `topup` and `credentials` 404s are untested; the ledger refusals are tested.
  - Native recovery of a lost first paid opening reply is still untested.
- 2.8 ticked. Checks: `--lib` 469 passed, 12 ignored (helpers), 0 failed; bins build and `cargo check --bins --tests` pass; `inference_funding_admission_test` 40/40; `inference_owner_consumer_test` 14/14; desktop `--lib desktop::` 10/10; strict validation passes. Test-only change; round 89's full suite covers the product code.

## 2026-11-21 (ninety-third round) — Acceptance: opening-intent 2.8.2 complete (inference settlement at full opening capacity)

- In step at `478aa99c`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-opening-funding-intent` new subtask 2.8.2.6), through a workflow: write tests, then an adversarial review. It fills the two cells that round 92's reconciliation left open. No product code changed and no code difference was found. Four new `billing_store::tests`:
  - `full_paid_opening_capacity_leaves_existing_requests_and_inference_gates_intact` fills 10,000 active paid leases: 9,999 complete signed restored unopened agreements plus one genuine `register_funding_intent`.
  - `full_owned_free_opening_capacity_leaves_existing_requests_and_inference_gates_intact` fills one owner's eight accounts, all genuine.
  - `full_global_free_opening_capacity_leaves_existing_requests_and_inference_gates_intact` fills 10,000 ledger accounts: 9,997 complete signed restored free accounts with roots and distinct owners, the rest genuine.
  - In each, a fresh opening of that kind gets its exact refusal with every row unchanged. That check runs after the fill, after every later step and after reopening, before recovery. Requests held before and during the fill settle at known usage, release unknown and invalid usage with exposure, are released as `restart_unknown_usage` by recovery after reopening, and read back by key, id, list and signed receipt. Fresh inference is admitted and charged, and still refused by its own rules. The capacity state (leases, quotes, accounts, funding, fees, rate limits) never changes.
  - `full_opening_capacity_leaves_gateway_settlement_reads_and_inference_intact` covers all three pressures through the production public and control routers with the controlled engine. Under each it checks known settlement, a cancellation release, a charged fresh request, and refusals with 402, 403 and 409. Public status by key and id, the control request, list and account reads and a signed receipt all return the stored records, and an infer-only bearer gets 403 on reads.
- The review added the refusal check right after reopening and the router list read with the infer-only refusals. Before that, a list refused while capacity was full passed the router test. Mutations: the author ran 11 and the review 12; each fails a named test. Test helpers were refactored without changing behaviour (`check_ledger_reads` and `check_ledger_receipt` split from the full-row asserts).
- 2.8.2.6 ticked, and parent 2.8.2 ticked: all 20 reconciliation cells now cite a test. The statement's remaining sentence is a caveat, and its root-rotation blocker was resolved by owner-control 2.4.3. The cells that cite `inference_owner_control_test` passed in round 89's full suite (61/61); no product code or that target has changed since. request-accounting design.md's matrix and its 3.4 note now point to the new tests.
- Recorded limits: no owner-proof read runs at full opening capacity, and there is no daemon restart at this pressure (recovery is checked on the ledger). The ledger account bound has no admission control because accounts are permanent. Restored leases carry opaque originals. No test has the native client recover a lost first paid opening reply.
- Checks: `--lib` 467 passed, 12 ignored (helpers), 0 failed; bins build and `cargo check --bins --tests` pass; desktop `--lib desktop::` 10/10; strict validation of both changes passes. Test-only change; round 89's full suite covers the product code.

## 2026-11-21 (ninety-second round) — Reconciliation: request-accounting 3.4 with opening-intent 2.8.2

- In step at `3183a2de`; whitepaper unchanged; no new answers.
- Docs-only step (`specify-inference-request-accounting` 3.4), through a workflow: write the reconciliation, then an adversarial review that opened every cited test. request-accounting design.md gains "Reconciliation with opening-intent 2.8.2 (task 3.4)". It is a matrix of 2.8.2's four recovery kinds (accepted paid original, existing account, top-up, inference settlement) against five exhausted resources (full quote storage, signing work, committed issuance, full paid opening capacity, full free opening capacity). Each cell names its test. A second table records which of 2.8.2's preserved properties each test asserts: authorization, safe-credit checks, prices, nonce ownership and exported credential authority.
- Result: 18 of 20 cells are covered. Open: inference settlement at full paid capacity and at full free capacity. Five tests keep a request held under that pressure, but none settles, releases, recovers or reads it. Opening-intent 2.8.2 stays open, and its tasks.md gains a cross-reference note with no checkbox.
- The review corrected three overclaims:
  - The free-bound accepted-original pair recovers from an acknowledgement the client already saved, with no submission.
  - The native top-up recovery attempts replay by POST, not a GET refresh.
  - One authorization cell is "not checked".
  It also notes that 2.5, 3.1 and 3.2 fill no cell.
- Named-wallet position checked in code: the propose route calls only `sign_quote` (provider-wide work) and `store_quote` (provider-wide issuance and the 10,000-record bound). No wallet-keyed quote ceiling or pending counter remains. The removal came from the archived non-reserving and proposal-rate changes, which opening-intent 2.3.2 integrated. Request-accounting evidence neither relied on those gates nor mitigates them. design.md's stale "does not mitigate the public named-wallet quota risk" now says so. Still per wallet: the owner-challenge budget (12 per minute, 8 pending), which a third party can use to delay recovery that needs a fresh owner proof; no cited test exhausts it. Book line 508 still describes the old wallet gates; only the author can change it (report entry 2026-11-14 stays open).
- Blockers: owner-control 2.4.3 (root rotation on an opening replay) was fixed on 2026-11-15, and its change was archived on 2026-11-21. Opening-intent design.md's last limits bullet said it "remains blocked"; I corrected that bullet and kept its evidence gap: no test has the native client recover a lost first paid opening reply. The production quote policy is no longer a blocker. Left for 4.1: request-accounting proposal.md's original Impact text ("owner-untracked", "blocked production cutover") describes the specification step as it was.
- 3.4 ticked. Checks: strict validation of both changes passes. No code or test changed, so no cargo run.

## 2026-11-21 (ninety-first round) — Acceptance: request-accounting 3.3 (quote-budget exhaustion leaves inference intact)

- In step at `54abd7bd`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-inference-request-accounting` 3.3), through a workflow: assess the blocker, write tests, then an adversarial review. No product code changed and no code difference was found.
- Blocker resolved. 3.3 was blocked where the pre-signing work budget or a controlled issuance clock needed owner-untracked source or the non-reserving cutover. Both now exist in tracked source on the propose route's path (contracts.rs:294–295), each with an operation-time argument and its own non-regressing minute: `sign_quote` claims work through `claim_quote_work`, and `store_quote` charges committed issuance through `proposal_rate_limit`. tasks.md replaces the BLOCKED clause with this reason and keeps the ban on sleeps, fabricated accepted timestamps and full stored-quote occupancy.
- design.md gains "Quote budget independence (task 3.3)". Three `billing_store::tests`:
  - `exhausted_quote_signing_work_leaves_existing_requests_and_inference_gates_intact` and `exhausted_committed_quote_issuance_leaves_existing_requests_and_inference_gates_intact` run at explicit ledger times. With either budget exhausted, the following match exact complete rows: known settlement, unknown and invalid-usage releases, restart recovery after reopening, request reads and a signed receipt. A fresh request is still admitted and charged. The account's own checks still refuse new requests with every row unchanged: credit, spend cap, scope, saturation, model revision, a withdrawn report, a duplicate, and both inference budgets. Inference leaves the quote rows unchanged. The budgets are independent of each other: a quote still commits under exhausted work, and an issuance refusal refunds no spent work unit.
  - `exhausted_quote_budgets_refuse_public_proposals_while_gateway_inference_and_reads_continue` runs once per budget through the production control and public routers with a controlled engine. Each budget is exhausted at a minute ten minutes ahead, which its non-regressing clock keeps in effect for real-clock proposals. A refused proposal does not pause admission. Gateway settlement, cancellation, status, control reads and receipts continue. The gateway's checks still refuse: a withdrawn report (503), credit (402), scope (403), a held key (409) and a revoked bearer (401, a review addition).
- Review finding, closed: a quote refusal that commits pending-state cleanup passed every test, because the fixture held nothing that had expired but not yet been removed. The ledger tests now keep a lapsed funding-intent lease and quote pending through every refusal and reopening, so cleanup on refusal fails them; this covers "SHALL NOT preserve partial pending-state mutations". Mutations: the author ran 16 and the review 6; each fails a named test.
- Recorded limits: the router test does not restart the daemon, so restart recovery is covered at the ledger level. The challenge budget and opening capacity are not exhausted here. In-crate tests call the pub(crate) `sign_quote` directly. The controlled engine is not llama.cpp. Reconciliation with opening-intent 2.8.2 is 3.4.
- 3.3 ticked. Checks: `--lib` 463 passed, 12 ignored (helpers), 0 failed; bins build and `cargo check --bins --tests` pass; desktop `--lib desktop::` 10/10; strict validation passes. Test-only change; round 89's full suite covers the product code.

## 2026-11-21 (ninetieth round) — Acceptance: request-accounting 3.2 (admitted evidence stays bound through settlement, waiver, restart and renewal)

- In step at `ae20db31`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-inference-request-accounting` 3.2), through a workflow: write tests, then an adversarial review. design.md gains "Admitted evidence through settlement (task 3.2)". It has an inventory of the ten statements against the existing performance, records, receipt and owner-control tests, and a code-sites table of every place admitted evidence is bound, copied, compared or re-read. No product code changed and no code difference was found.
- `billing_store::tests::admitted_reports_stay_bound_through_settlement_waiver_and_restart_across_a_renewal`: requests admitted under renewal R1 stay held while R1 is withdrawn and R2 is admitted. Eight changed-term or not-newer renewals are refused at reserve with every row unchanged: hardware, artifact, engine, concurrency, arrival rate, speed class, an equal measurement time and an older one. Known settlement, a known-usage waiver and an unknown release each refuse outcomes computed against R2 or the agreement's report, then commit against R1. Restart recovery binds each request to its own report. A signed receipt carries each admitted digest. A waiver keeps known usage and waives only inference cost, with the fee unchanged.
- `app::tests::a_restart_under_a_renewed_report_recovers_held_requests_with_their_admitted_report`: a gateway request admitted under R1 is held by a settlement write failure. The daemon restarts with R2, and startup recovery releases the request, still bound to R1, before admission. Under R2 the test then sees a charged request outside the workload envelope and a known-usage time-to-first-token waiver through the production gateway; earlier gateway waivers all had unknown usage. Review addition: a stream admitted under R2 and held while R2 is withdrawn still completes, bound to R2 with the gateway's own timings, and admission stays open. Before this, two `Settlement::finish` regressions that re-read current withdrawal state passed every lib test.
- Mutations: the author ran 13 and the review 15; each fails a named test.
- Recorded limits: a request record and receipt bind the measurement time only through the report digest, and the product keeps no superseded report file. An older request's measurement time therefore needs a copy the operator keeps; this matches the design's earlier trust limit. "Strictly newer" is compared with the agreement's report, not with the latest renewal; neither the spec nor the book names the reference report. The reports are synthetic, a decode-rate miss through the gateway is not exercised, and process-kill and fsync durability are not established.
- 3.2 ticked. Checks: `--lib` 460 passed, 12 ignored (helpers), 0 failed; bins build and `cargo check --bins --tests` pass; desktop `--lib desktop::` 10/10; the cited unchanged targets `inference_account_records_test` 2/2 and `inference_quote_acceptance_test` 17/17 (review run); strict validation passes. Test-only change; round 89's full suite covers the product code.

## 2026-11-21 (eighty-ninth round) — Acceptance and code: request-accounting 3.1 portable part; a committed invalid-usage release no longer closes admission

- In step at `df0c05d7`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-inference-request-accounting` 3.1), through a workflow: write tests, then an adversarial review. design.md gains "Gateway and backend acceptance (task 3.1)". Its inventory maps each gateway/backend behaviour to its statement, the tests that already passed and the gap. Nine new tests drive the production public router and llama.cpp adapter against a controlled loopback engine. That engine is test evidence only; no runtime mock or fallback was added.
  - **Preparation** (`gateway_reserves_and_dispatches_exactly_the_prepared_prompt_and_failed_preparation_holds_nothing`): the ledger is unchanged during tokenization and holds exactly the prepared count at dispatch. This holds for chat, completions (the tokenizer's exact IDs) and embeddings (input only). Seven preparation failures, including the gateway's own tokenization deadline, reserve and dispatch nothing.
  - **Stable keys** (`gateway_duplicates_in_flight_racing_or_after_a_lost_reply_never_dispatch_or_hold_again`): in-flight, racing, lost-reply and completed duplicates all get the 409 naming the original, and seven operations dispatch seven generations.
  - **Cancellation and engine failure** (`gateway_cancellation_charges_known_usage_and_releases_unknown_holds_at_provider_expense`, and the review's `gateway_failed_and_timed_out_responses_charge_known_usage_and_release_unknown_holds`): known usage is charged after cancellation, a truncated stream or the generation deadline. Without it the whole hold is released with equal exposure. Cancelling a settled request only reads it.
  - **Durable settlement failure** (`gateway_settlement_failure_keeps_holds_stops_admission_and_keeps_existing_reads`, `app::tests::a_paid_settlement_failure_keeps_its_hold_until_a_restart_releases_it_before_admission`): under `RAISE(FAIL)`, holds stay exactly as reserved and nothing is acknowledged. Admission and new funding close, including for a request being tokenized. Earlier work settles and reads continue. A real daemon restart releases the exact hold as `restart_unknown_usage` before admitting again.
  - **Usage and credentials** (`gateway_usage_validation_releases_the_whole_hold_without_inventing_tokens`, `credentials_ending_during_generation_settle_against_their_original_accounting`): eight invalid usage shapes release the whole hold. Expiry on the real clock, revocation and rotation during generation settle against the original credential.
- Code difference fixed (`inference_gateway.rs` `Settlement::finish`). The ledger returns `usage_over_reservation` only after it has committed an invalid-usage release; the reachable case is an account's cumulative usage counter overflowing. The gateway treated that as a persistence failure: it recorded `DURABLE_SETTLEMENT_FAILED`, closing admission and new funding for every account, and left the settlement for `Drop` to retry. It now marks the settlement done, records no fault and answers 500 `usage_not_accounted`, both non-streaming and as one stream event before `[DONE]`. `gateway_cumulative_usage_overflow_is_an_invalid_usage_release_not_a_settlement_failure` fails without the fix.
- Mutations: the author ran 25 and the review 11; each fails a named test. Before the review's additions, two passed every lib test: dropping known usage on `failed`/`timed_out`, and removing the gateway's tokenization deadline.
- 3.1 stays open. Remaining: observed evidence at the pinned llama.cpp engine (template and tokenizer counts against its generation usage, final usage before `[DONE]`, and its behaviour when a request is dropped), which needs a real engine on qualified hardware. Also unexercised, because each needs a production seam: a settlement clock failure, a task panic, and a client gone between dispatch and the start of its stream. Some cases are timed (500 ms pauses, 1.2 s preparation delays against a 2 s deadline).
- Recorded limits: public-inference-api decision 4's implementation code list does not name `usage_not_accounted`. It also omits `internal_error`, `duplicate_request` and `request_cancelled`, so it was never exhaustive, and the codes are not normative. The review noted a pre-existing behaviour: when a stream's `[DONE]`-time settlement fails transiently, the worker's retry can charge it as `failed` while the client saw `internal_error`.
- Checks: full suite under caffeinate with `--no-fail-fast`: bins build passes; 32 headless test targets, 774 passed, 12 ignored (helpers), 0 failed (`--lib` 458; `inference_owner_control_test` 61/61); desktop `--lib desktop::` 10/10; `npm test` 124/124 in 19 files; `npm run check` 0 errors, 0 warnings; `python3 -m unittest discover -s scripts` 80 OK; strict validation passes. No `pmset` sleep during the run.

## 2026-11-21 (eighty-eighth round) — Acceptance: request-accounting 2.3 complete (native and SDK clients never retry paid inference)

- In step at `6c49cca5`; whitepaper unchanged; no new answers.
- Acceptance step for 2.3's last statement ("Native workflows and supplied SDK examples SHALL NOT automatically retry paid inference with a new key or another provider after an ambiguous response, cancellation or interrupted stream"), through a workflow: write tests, then an adversarial review. No product code changed and no code difference was found.
- Native: `consumer::tests::lost_interrupted_and_cancelled_paid_inference_is_sent_once_and_reconciled_by_its_key` uses a controlled raw-HTTP loopback provider that reports each dispatched key over a channel before losing it. Drops happen at deterministic points, with no sleeps. Seven calls lose or end their dispatch differently: a lost reply and a truncated JSON body for `infer`; a lost opening and an SSE stream closed after two events for `stream_kind`; a live stream cancelled with `cancel_request`; and in-flight `stream_kind` and `infer` dropped the way the desktop does. After each call:
  - exactly one dispatch is seen for its key;
  - the local journal holds exactly the dispatched keys, also after reopening;
  - `request_status` with the same key reconciles at the same provider;
  - another saved account's provider, the catalog and the RPC accept no connection;
  - the account files are byte-identical.
  Thirteen mutations each fail it, among them a resend under a new or the same key, a self-recursive retry, a stream that keeps reading after cancel, a missing journal write, rerouting, and a spawned background resend.
- SDK examples (static; the openai packages are not installed, so nothing runs): `tests/sdkExamples.test.ts` (TypeScript and svelte/compiler AST) and `scripts/test_sdk_examples.py` (`ast`) check `examples/openai-client.mjs`, `examples/openai_client.py` and both Usage.svelte snippets. Each source must have:
  - one client with retries 0 (the SDKs default to 2, so omitting the setting fails);
  - exactly one completions call, outside loops, re-callable code, except/catch and finally blocks;
  - in the examples, one printed key generated once, a base URL only from the exported credential, and a non-zero exit that names the key to inspect;
  - in the snippets, environment-only settings, with failures left to propagate.
  The review found five holes in the first version, which have been closed: retries or base URL reassigned after construction; a fallback written into the credentials; option spreads; and re-running the script. There are now 71 mutated copies plus 9 real-file edits, and each fails its own finding.
- Recorded limits: the CLI and desktop wrappers call `infer` and `stream_kind` once per action, but this is checked by inspection only. The controlled provider is not the production gateway, whose reservation, cancellation and settlement of lost requests stay in 3.1. The Usage snippets send no Idempotency-Key, which desktop-gui-pages allows.
- 2.3 ticked; section 2 of request-accounting is complete. Checks: `--lib` 449 passed, 12 ignored (helpers), 0 failed; bins build and `cargo check --tests` pass; desktop `--lib desktop::` 10/10; `npm test` 124/124 in 19 files; `npm run check` 0 errors, 0 warnings; `python3 -m unittest discover -s scripts` 80 OK; strict validation passes.

## 2026-11-21 (eighty-seventh round) — Acceptance: request-accounting 2.3 portable part (request identity and attributable recovery reads)

- In step at `6230e83b`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-inference-request-accounting` 2.3), through a workflow: write tests, then an adversarial review. Every statement and scenario of "Request identity prevents automatic paid replay" and "Request and receipt history supports attributable recovery" was mapped to test bodies. The code matches the spec, and no product code changed. Seven new `billing_store::tests`:
  - **Identity** (`request_identity_and_attribution_survive_settlement_reopening_and_duplicates`): opaque request ids distinct from client keys and across accounts. Identity is unchanged through every terminal kind and reopening. Each duplicate, by key or id, changes nothing and names the original.
  - **Pages and receipts** (`request_pages_and_receipts_hold_the_100_newest_account_requests_newest_first`): with 100, 101 and 103 requests, out-of-order times and ties, pages and receipts hold exactly the 100 newest of one account. A receipt has exactly six members, its request records are accounting-only, and only the sequence changes.
  - **Pagination limit** (`timestamp_before_pages_can_omit_requests_sharing_the_boundary_timestamp`): the documented limit, semantics unchanged. Two of six tied requests appear on neither page, and the design now names this test.
  - **Receipt sequences** (`receipt_sequences_are_durable_per_account_and_failed_issuance_commits_nothing`): each account has its own sequence across reopenings. A wrong signer, a malformed key, a sequence-update trigger fault or a commit refusal returns no receipt and changes no row, and the next success takes the next number. The sequence is exhausted at i64::MAX.
  - **Competing settlement** (`a_receipt_waits_for_a_competing_settlement_and_binds_only_committed_state`): a receipt waits for an uncommitted settlement and binds either the committed or the rolled-back state, never a mixture.
  - **Read scope** (`request_and_receipt_reads_are_account_scoped_and_change_no_financial_row`, production routers): read, root and owner reads return exact records, and every table is identical afterwards. Bearers with no credential, infer-only, revoked, expired or from another account are refused.
  - **No stored content** (`completed_inference_retains_no_content_and_duplicates_never_replay_it`, controlled loopback engine): one dispatch per key, and duplicates get the exact 409 with no engine call. Keyless requests get fresh UUIDs, the key alphabet and length are enforced before the engine, and no read or file under the data directory contains the prompt, template or output.
- Review: two of its mutations survived and were then caught: a `+` admitted in keys, and an owner proof deleting its wallet's other pending challenges (the read test had excluded the challenge tables). It strengthened both tests and ran 12 more mutations, which all fail. Seventeen author mutations also fail.
- 2.3 stays open for one statement: native workflows and the supplied SDK examples must not automatically retry paid inference after an ambiguous response, cancellation or interrupted stream. The examples and Usage snippets set `max_retries=0` and `maxRetries: 0`, but no test asserts it. No test interrupts a native transport or stream or cancels natively; only the native redirect test and the mocked GUI Playground test cover part of it. This is the next step.
- Checks (test module only): `--lib` 448 passed, 12 ignored (helpers), 0 failed; bins build and `cargo check --tests` pass; desktop `--lib desktop::` 10/10; strict validation passes. The full suite ran in round 86 on the same product code.

## 2026-11-21 (eighty-sixth round) — Acceptance and code: request-accounting 2.5 ledger edges; the unknown-usage budget counted a wildcard match

- In step at `befc6f64`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-inference-request-accounting` 2.5), through a workflow: write tests, then an adversarial review. Before this step, the only invalid-usage case tested was output above the bound, and no ledger test covered prompt-count mismatch, cumulative-counter overflow or either inference budget. Nine new `billing_store::tests` cases pass logical times only through `reserve`, `settle` and `recover_requests`, and compare complete ledger rows from fixed literals:
  - **Invalid usage:** a prompt-count mismatch and a cumulative-counter overflow each commit one whole `invalid_usage_unknown_usage` release, with exposure, no charge and no counter change. There is no wraparound and no partial update. Exactly u64::MAX is charged, and the first result survives reopening.
  - **Thresholds:** 119 prior requests admit the 120th per credential, and 29 prior unknown-usage releases admit the 30th per account. The `now - 60` and `now - 3600` cutoffs are inclusive and saturate at time zero.
  - **What counts:** each budget is scoped to its credential or account. Windows use creation time, not settlement time, and terminal changes restore no allowance.
  - **Clock regression:** future-dated rows still count after the clock regresses and the ledger reopens, and both windows are anchored at the sample, not the latest record.
  - **Exhaustion:** known, unknown and invalid settlement and restart recovery still succeed with both budgets exhausted.
  - **Refusals:** every refusal leaves every row unchanged.
- Code difference found and fixed (design "Fixed 2026-11-21 by task 2.5"): the account budget counted `status LIKE '%unknown_usage'`, where `_` is a single-character wildcard. A known-charged status such as `completed_unknownxusage` therefore spent unknown-usage allowance. It now counts `GLOB '*unknown_usage'`, the suffix that `settle_request` classifies as unknown. No status the product produces changes classification. The regression fails without the fix.
- Thirty-four mutations each fail a 2.5 test. The review found four that survived the first seven tests: the unknown cutoff falling back to `now` near zero; the credential window measured at settlement time; and either window anchored at the latest record. It added two tests and a late-settled record that catch them. Recorded limits: credentials have no token counters (account counters are the cumulative counters); the code's `*unknown_usage` also matches a bare `unknown_usage`, which no product path produces; live gateway edges stay with 3.1.
- Disk: the data volume was full (1.4 GiB free). `src-tauri/target/debug/deps` held 684,925 unreferenced `.o` files (82.8 GiB) left by earlier rebuilds: macOS debug builds keep debug info in object files, and Cargo never prunes old ones. Only files that no current executable or dylib debug map references were deleted, leaving 80 GiB free; test results are unaffected. The workflow's author had earlier removed 19 incremental-cache directories more than 3 days old (1.77 GiB, regenerable).
- 2.5 ticked. Checks: full suite under caffeinate with `--no-fail-fast`: bins build passes; 32 headless test targets, 757 passed, 12 ignored (helpers), 0 failed (`--lib` 441; `inference_owner_control_test` 61/61); desktop `--lib desktop::` 10/10; `npm test` 89/89 in 18 files; `npm run check` 0 errors, 0 warnings; `python3 -m unittest discover -s scripts` 76 OK; strict validation passes. No `pmset` sleep during the run.

## 2026-11-21 (eighty-fifth round) — Acceptance: request-accounting 2.2.4 (credential expiry during an admitted hold) and parent 2.2

- In step at `79c5dedf`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-inference-request-accounting` 2.2.4), through a workflow: write tests, then adversarial review. The code already matches the spec. `reserve` and `authenticate` refuse from `expires_at` (`expires_at <= now`), so the last admitted second is E - 1. `settle_request`, `recover_requests` and the gateway's settlement never re-check credential liveness. Reads keep live authority: an expired bearer gets 401 on every route, as account-credentials requires, so the tests read through a live successor `read` credential or an owner proof.
- New tests in `billing_store::tests` (test module only; no product code changed):
  - `credential_expiry_admits_through_the_last_second_and_refuses_new_holds_from_expires_at`: admitted at E - 1. Refused (`forbidden`, and `unauthorized` for the bearer) at exactly E, E + 1 and E + 90 days, with all 14 ledger tables unchanged before and after reopening. The live root admits the same key at E.
  - `expired_credential_hold_settles_once_against_its_original_accounting`: 24 cases covering four terminal kinds (known usage, explicit unknown usage, usage over the reservation, restart recovery), three successors (issued; revoked then issued; rotated) and settlement at exactly E or a day later. The hold survives reopening and settles once against the original credential and account, with rows built from literals. The successor gains no hold and gets `duplicate_request` for the held key, and the first terminal result stands.
  - `expired_credential_requests_stay_readable_and_unduplicable_through_the_routers`: on the real control and public routers, with the account opened at a logical time already past expiry (no sleeps, no rewritten timestamps). The expired bearer is refused everywhere. The successor's chat with the held key gets 409 with the original request id before any engine call. The successor and the owner proof read the request and get signed receipts attributed to the original credential. After settlement only the original credential is charged.
- Fifteen mutations fail them: the expiry comparison off by one in either direction; authentication ignoring expiry; settlement requiring a live credential; restart recovery skipping expired credentials; the duplicate check disabled or limited to the caller's credential; revocation or rotation zeroing or moving the hold; settlement skipping the expired credential's spend; and owner-proof receipt reads refused. The last was caught only after the review added the owner receipt read. Remaining gaps, recorded in design: a live gateway request whose credential expires mid-generation needs an operation-time input the gateway does not have (task 3.1). Explicit revocation during a hold is exercised on the expired credential (`revoke_credential` takes no time), and rotation of a live credential during a hold by the existing test.
- 2.2.4 and parent 2.2 ticked; 2.2.1–2.2.3's tests in `inference_owner_control_test` passed in round 81's full suite, and that target's inputs are unchanged since.
- Checks (test module only): `--lib` 432 passed, 12 ignored (helpers), 0 failed; bins build and `cargo check --tests` pass; desktop `--lib desktop::` 10/10; strict validation passes.

## 2026-11-21 (eighty-fourth round) — Acceptance: opening-intent 2.3.2 (non-reserving quote cutover) and parent 2.3

- In step at `4c26d6e2`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-opening-funding-intent` 2.3.2), through a workflow: write tests, then adversarial review. Both public named-wallet gates are gone. `sign_quote` claims one provider-wide `quote-work:global` unit on its own `quote_work_minute` clock. `store_quote` enforces only the 10,000-record storage limit and the `quote:global` 600-per-minute committed-issuance budget, on its own `quote_committed_minute` clock. The handoff fixtures were already migrated to 10,000 provider-wide records by the parent's task 3.5. Three stale design descriptions and two stale test comments (owner-consumer, owner-control) now say so; no assertion changed.
- New tests (no product code changed):
  - `inference_funding_admission_test::unrelated_quotes_beyond_both_former_wallet_ceilings_leave_the_owner_only_its_own_rules`: 14 anonymous quotes naming one owner (5 paid, 9 free, both address spellings) go beyond the former limits of 8 live and 12 per minute. They allocate no lease, account, credential or wallet bucket and spend none of the owner's challenge budget. The owner is then refused only by its own rules: its intent is acknowledged; its reused nonce gets 409; its 9th free opening gets 429; its 13th challenge gets 429. Quoting continues at its full 8-account limit.
  - `inference_proposal_test::quote_work_and_committed_issuance_keep_separate_counters_clocks_and_failure_charges`: two ledgers whose work and committed clocks run in opposite orders. A work-exhausted route refuses before signing and changes neither counter. A committed-exhausted route spends one work unit per refusal and no committed unit, stores nothing, and continues through a reopened ledger and router.
  - Eighteen mutations fail them or the cited tests. Among them: a reintroduced 8-quote wallet gate; quotes charging the owner's challenge budget; free-account limits counting quotes; the 8-account, nonce-conflict and 12-challenge rules removed or raised; the two clocks cross-reading; a work refund after a committed refusal; committed issuance charged outside the durable insertion; quotes refused at full paid, owned or ledger capacity; a charge-only wallet bucket; a 301-second lifetime.
- 2.3.2 and parent 2.3 ticked: each part of 2.3's own text maps to passing tests (capacity and two-writer admission, lease expiry, cached recovery, nonce conflicts, raw-string replay, response loss). HTTP quoting at the ledger-wide 10,000-account limit is `inference_owner_control_test::independent_free_openings_compete_for_the_last_ledger_account_slot`, which passed in round 81's full suite; product code is unchanged since then.
- Checks (test and comment changes only): bins build passes; `cargo check --tests` passes (existing support-helper warnings only); `inference_funding_admission_test` 39/39, `inference_proposal_test` 15/15, `inference_owner_consumer_test` 13/13; the workflow's `--lib` run 429 passed, 12 ignored, plus `inference_proposal_quota_test` 23/23 and `inference_quote_acceptance_test` 17/17; strict validation passes.

## 2026-11-21 (eighty-third round) — Code: callouts wrap long exact values at phone width

- In step at `091f324c`; whitepaper unchanged; no new answers.
- Code-difference step (`specify-desktop-gui-pages` requirement 15, "Presentation may change while required elements stay present and reachable"; book lines 838 and 971). Round 78's observation was rechecked with an overflow probe in headless Chrome at 390 px, using the scratchpad IPC stand-in with the shared exact fixture (never shipped). Usage was 412 px wide: the reorganization warning is the only place the page shows an account's provider loss and reversed funding, and a u128::MAX wei amount, 40 digits with no break point, ran past the window edge. Only error callouts wrapped long words.
- Fix: every `ui/Callout.svelte` body now has `break-words`, and error callouts keep `whitespace-pre-wrap`. Afterwards, Usage, Account, Playground, Marketplace, Wallet, Network, Provider, Diagnostics and Settings are each exactly 390 px wide with no clipped text, the amount wraps inside the warning, and the 1440 px layout is unchanged (screenshots checked). The stat tiles' ellipsis is intended, because the overview repeats each value in full.
- New test `exactStatements.test.ts` › `Usage wraps the exact provider-loss amount, which appears only in its warning, at phone width`: it asserts the amount appears only in the warning, that the warning wraps, and that every callout on Usage wraps. Reverting the class fails it. jsdom has no layout, so the screenshots are the layout evidence. Recorded as design code difference 5, and task 2.5 is ticked. 3.15's automated layout check and the native desktop screenshots (4.1) stay open.
- Checks (frontend only): `npm test` 89/89 in 18 files; `npm run check` 0 errors, 0 warnings; `npm run build` passes; strict validation passes. Rust is unchanged since round 81's full suite.

## 2026-11-21 (eighty-second round) — Archive: `specify-inference-account-snapshot`

- In step at `e7f2cac6`; whitepaper unchanged; no new answers.
- Task 4.2 ticked, then `openspec archive specify-inference-account-snapshot --yes`: the change is now `openspec/changes/archive/2026-11-21-specify-inference-account-snapshot`, and `openspec/specs/inference-account-snapshot/spec.md` is a new normative capability (4 requirements added). The placeholder Purpose was replaced with one sentence. The archived design has no relative links. Two stale path references now point at the archive or the main spec: the `verus.md` requirement source and the `specify-fee-remittance` design's dependency table, which called the change pending.
- Every task had passed: 3.3 and 3.4 in round 81's full suite, 2.5 in round 78's, and the rest earlier. Other pending changes name this change in text only, as an archive-order condition.
- Checks: round 81's full suite at `e7f2cac6` covers the code (no code changed); `openspec validate --all --strict --no-interactive` 34 passed, 0 failed; `openspec validate inference-account-snapshot --type spec --strict` valid.

## 2026-11-21 (eighty-first round) — Acceptance and code: account receipt distinctions; corrupt stored and cached accounts fail closed

- In step at `16c7f73a`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-inference-account-snapshot` 3.3, 3.4 and 4.1) through a workflow: write tests, then an adversarial review. New target `inference_account_records_test`:
  - `signed_receipts_keep_original_terms_waived_usage_unknown_exposure_and_funding_loss_distinct` (3.3): one genuine provider-signed receipt, accepted by the real native `Consumer::receipt`, keeps four quantities apart: the original account terms (renewed requests carry their own report), a known waived usage whose waiver is never a balance term, 40 wei of unknown-usage request exposure at zero counters, and 10 wei of funding-loss compensation on a frozen account. Re-signed receipts that conflate any two are refused, and saved bytes, the watermark, the credential and the journal stay unchanged. Request-accounting 3.2.1's regression is retained unchanged.
  - `persisted_schema3_and_native_records_read_exactly_and_corruption_fails_closed` (3.4): genuine schema-3 ledger rows and native files decode strictly and re-encode to identical bytes through every read path. Corrupt records (extra, positional, missing and duplicate members, a 2^64 counter, non-canonical wei and waiver, unbalanced money and wide shortfalls, positional request usage, cached renewed terms, an unsupported member) and ledger schemas 2 and 4 fail closed for that account only and change no row or file byte. Nothing migrates, repairs or resets.
- Code difference found and fixed (design "Code differences"): semantically corrupt money failed open. The provider served and signed a stored `"spent_wei":"010"` or an unbalanced account, and the native `account` call silently replaced a corrupt cached statement from the provider. Now every stored account read and the operator listing apply the writer's balance identity (`billing_store::balanced`, waiver validated but never counted) and fail with `ledger corruption`. Loading a saved native account applies the existing terms-and-balance check and refuses, before any provider request, with "saved account snapshot is corrupt; it is preserved unchanged".
- The review found six regressions the first tests missed: waiver validation dropped on either side, decoding through `serde_json::Value` (which tolerates duplicate members) on either side, and the receiver's identity in wrapping `u128` or `f64`. It added wide-sum, duplicate-member and non-canonical-waiver cases; all of these, and seven more mutations, now fail. Its documented, deliberate fail-closed effects: one corrupt local file makes both native aggregate listings refuse; one corrupt ledger account makes the operator listing refuse, can stop daemon startup recovery and quarantines its funding recheck (by inspection).
- 3.3, 3.4 and 4.1 ticked; only the archive (4.2) remains. The change's notes that owner-control was not rerun now cite this round's full suite.
- Checks: full suite under caffeinate with `--no-fail-fast`: bins build passes; 32 headless test targets, 743 passed, 12 ignored (helpers), 0 failed (`--lib` 429; `inference_account_records_test` 2/2; `inference_owner_control_test` 61/61; `application_startup_boundaries_test` 14/14); desktop `--lib desktop::` 10/10; `npm test` 88/88 in 18 files; `npm run check` 0 errors, 0 warnings; `python3 -m unittest discover -s scripts` 76 OK; strict validation passes. No `pmset` sleep during the run.

## 2026-11-21 (eightieth round) — Archive: `specify-owner-control-authorization`

- In step at `a8240688`; whitepaper unchanged; no new answers.
- Task 3.2 ticked, then `openspec archive specify-owner-control-authorization --yes`: the change is now `openspec/changes/archive/2026-11-21-specify-owner-control-authorization`, and `openspec/specs/inference-owner-control/spec.md` is a new normative capability (7 requirements added). The placeholder Purpose was replaced with one sentence. The archived design's relative links were moved one level up. The two links from the archived challenge-rate design now point at the archive, and one of them had a stale heading anchor, which is corrected.
- Every task had passed: 2.2.3 in round 79's `--lib` run, and the rest in round 78's full suite and earlier. Two pending changes (`specify-account-credentials`, `specify-opening-funding-intent`) name this capability in text only; nothing links to the old path.
- Checks: `openspec validate --all --strict --no-interactive` 34 passed, 0 failed; `openspec validate inference-owner-control --type spec --strict` valid; every relative link in both archived designs resolves. No code changed.

## 2026-11-21 (seventy-ninth round) — Acceptance: the native owner-challenge expiry window is exact

- In step at `7c64549b`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-owner-control-authorization` 2.2.3, which completes 2.2). The task was small and mechanical, so no workflow was used. The new `consumer::tests::native_challenge_expiry_window_is_exactly_now_exclusive_to_now_plus_600` calls `validate_challenge` at an explicit current time: expiries at or before now and at now + 601 or later are refused, now + 1 through now + 600 are accepted, the window moves with the supplied time, and the exact-bytes check still applies. Three boundary mutations (accept at now; reject at now + 600; accept at now + 601) each fail it, and the source was restored byte for byte. The test uses the existing `now` argument; no clock seam was added.
- No code difference. The design's four "2.2.3 remains open" notes now cite the test. Only the archive (3.2) remains.
- Checks (test-only change inside `consumer.rs`'s `mod tests`): `--lib` 429 passed, 12 ignored (helpers), 0 failed; bins build and `cargo check --tests` pass; desktop `--lib desktop::` 10/10; strict validation passes. Integration targets, npm and Python are unchanged since round 78's full suite.

## 2026-11-21 (seventy-eighth round) — Code: the desktop shows token counters and other integers above 2^53 exactly

- In step at `0a2db426`; whitepaper unchanged; no new answers.
- Code-difference step (`specify-inference-account-snapshot` 2.5; design code difference, fixed). Every surface that presents or exports an account, signed receipt, request record or usage was checked for exact u64 counters and u128 wei:
  - Already exact: the CLI's JSON output, the native protected files (including the `receipt_sequence` watermark), and wei on every page, which uses BigInt.
  - Lossy before the fix: the desktop's command replies and stream channel. The webview parses them as JavaScript numbers, so a counter above 2^53 - 1 was rounded (18,446,744,073,709,551,615 showed as 18,446,744,073,709,552,000). This affected token usage, model limits, receipt sequence, request records and infer usage.
- Fix, presentation only: for the account, receipt, request, usage and infer operations and for stream events, the desktop boundary sends any integer beyond ±(2^53 - 1) as its exact decimal string (`exact_integers`). The frontend types these fields as number-or-string and groups them with a BigInt-based `formatCount`; an already-rounded number shows as "unavailable". Discovered offers and quotes are left unconverted, because the webview hands them back to be signed. The wire, signed bytes, persisted state, CLI output and accepted range are unchanged, and no secret reaches the webview.
- New tests: four `desktop::tests` plus one added in review, the new integration target `inference_account_presentation_test` (CLI stdout, saved account and watermark at 2^53+1 and 2^64-1 through re-signed receipts), and `tests/exactStatements.test.ts` (8 cases, including the real `stream()` path). The review found that reverting the real stream call site passed every test, and that a watermark cast through `f64` passed because the cast saturates at u64::MAX. It moved the stream loop into `forward_stream` with a test of its own and added a 2^53+1 receipt, so both mutants now fail.
- Screenshots of Usage at 1440 and 390 px with mocked exact replies (scratchpad stub, never shipped) show 18,446,744,073,709,551,615 / 9,007,199,254,740,993 and the status `provider_review` verbatim. Observation, older than this change: at 390 px a u128::MAX "Provider loss" amount runs past the edge of the reorganization warning, because the long number does not wrap.
- Recorded limits, outside this task: an offer or quote integer above 2^53 cannot round-trip through the webview (signature verification refuses the rounded copy); the Usage feedback `revision` is a number input; integers above 2^64 - 1 in untyped OpenAI pages are already doubles natively.
- The first full suite caught what the workflow had not run: two `application_startup_boundaries_test` scans read the operation table from `consumer_operation`, which now delegates to `dispatch`. The scans now pin both delegations (`consumer_command` to `consumer_operation`, and `consumer_operation` to `dispatch` plus presentation) and read `dispatch`. A mutant that drops the presentation fails them.
- 2.5 ticked. Checks: full suite under caffeinate (rerun with `--no-fail-fast` after the scan fix): bins build passes; 31 headless test targets, 740 passed, 12 ignored (helpers), 0 failed (`--lib` 428; `inference_account_presentation_test` 1/1; `application_startup_boundaries_test` 14/14; `inference_owner_control_test` 61/61); desktop `--lib desktop::` 10/10; `npm test` 88/88 in 18 files; `npm run check` 0 errors, 0 warnings; `python3 -m unittest discover -s scripts` 76 OK; strict validation passes. No `pmset` sleep during the run.

## 2026-11-21 (seventy-seventh round) — Archive: `specify-non-reserving-quotes` and `specify-proposal-rate-budgets`

- In step at `b3ee6bbf`; whitepaper unchanged; no new answers.
- Non-reserving 4.4: the first report entry ("Public proposals can exhaust a named payer's quota") now lists mitigation evidence for each item of the book's "Implementation gap and required evidence" list. It also lists the remaining gaps: provider-wide exhaustion affects every caller, the 12-per-wallet owner-challenge limit, and no public load, security or live-chain qualification. The proposed factual update of book line 508 is revised to match, and the entry stays open only for that author edit.
- Archived in order, each requirement applied once: `specify-non-reserving-quotes` (2 added) and then `specify-proposal-rate-budgets` (3 added, ledger-only; no wallet-policy requirement) into `openspec/specs/inference-proposal-admission`, which now has 11 requirements. Its Purpose now names the committed-issuance budget and paid admission. The two links from `specify-opening-funding-intent/design.md` now point at the archives.
- Checks: round 76's full suite at `b3ee6bbf` covers the code (no code changed); `openspec validate --all --strict --no-interactive` 34 passed, 0 failed; `openspec validate inference-proposal-admission --type spec --strict` valid.

## 2026-11-21 (seventy-sixth round) — Acceptance: quotes overbook opening capacity; paid admission is atomic and accepted originals survive reply loss, restart and expiry

- In step at `b02e974f`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-non-reserving-quotes` 3.3 and 4.3). The evidence map in the change's design.md ties every statement and scenario of both requirements to a passing test, most of them reused. New tests:
  - `inference_funding_admission_test::overbooked_quotes_change_no_opening_capacity_and_still_need_admission`: with 10,000 active paid leases and the owner's eight free accounts, paid and free quotes for the owner and unrelated wallets are still signed and change no lease, intent, account or credential. The holder still needs an owner proof and a valid payment, then gets 429 after the spendability check; the free opening and the native `fund` get 429, with no RPC, acknowledgement or submission.
  - `inference_funding_admission_test::native_accepted_original_recovers_after_expiry_restart_and_refill_until_safe_credit`: an acknowledged original whose reply was lost survives quote and lease expiry, two restarts, 10,000 refilled leases, full quote storage, exhausted signing work and issuance, and a second genuinely lost reply. Recovery returns the same acknowledgement without a new lease or any replacement quote, payment or journal operation, and credit waits for the safe frontier.
  - `billing_store::tests::paid_lease_bound_is_strict_at_deadlines_and_replay_never_renews_a_lease` and `free_openings_compete_for_the_final_owned_and_ledger_slots_across_connections`.
  - The first review found that splitting the capacity count and the insert into two locked transactions passed every test. A follow-up added a `#[cfg(test)]` pause point between the passed check and the insert, inside the same write transaction, and `paused_paid_registration_holds_the_final_lease_check_until_its_acknowledgement_commits` and `paused_free_opening_holds_the_final_owned_and_ledger_slot_checks_until_its_account_commits`. A second writer must be refused by the capacity check itself; the split and an unlocked count now fail them. The second review fixed a helper that hung, instead of failing, when a mutant let the second writer through. It also confirmed from the binaries' section hashes that the seam compiles to nothing.
- No code difference was found and no production behaviour changed. Recorded limits: the deterministic proof depends on the pause point staying between check and insert. The expired commit uses the ledger's explicit-time clock, because the route samples wall time. Restored prerequisites and scripted RPC are not live-chain evidence.
- 3.3 and 4.3 are ticked; only 4.4 (report update, final checks, archive) remains. `specify-opening-funding-intent` 2.3.4 and 2.3.4.3 are ticked with this evidence. The stale ownership blockers in its 2.3.2 and 2.5.3 now state what is true: 2.3.2 awaits its own review, and 2.5.3 still needs a seam between the acknowledgement write and the start-marker write.
- Checks: full suite under caffeinate: bins build passes; 30 headless test targets, 739 passed, 12 ignored (helpers), 0 failed (`--lib` 428; `inference_funding_admission_test` 38/38; `inference_owner_control_test` 61/61); desktop `--lib desktop::` 5/5; `npm test` 80/80 in 17 files; `npm run check` 0 errors, 0 warnings; `python3 -m unittest discover -s scripts` 76 OK; strict validation of both changes passes. No `pmset` sleep during the run.

## 2026-11-21 (seventy-fifth round) — Archive: `specify-challenge-rate-budgets`

- In step at `759f699d`; whitepaper unchanged; no new answers.
- Task 3.2 ticked, then `openspec archive specify-challenge-rate-budgets --yes`: the change is now `openspec/changes/archive/2026-11-21-specify-challenge-rate-budgets`, and `openspec/specs/inference-challenge-rate-budgets/spec.md` is the seventh normative capability (3 requirements added). The placeholder Purpose was replaced with a one-sentence purpose. The archived design's relative links were moved one level up so they still resolve, and the round-74 report question now points at the spec.
- Every task had passed: 2.4 and 2.5 in round 74's full suite, the rest earlier. The open report question (adopting the old limiter's out-of-order rows) is nonblocking; option (b) would be a new change to this capability.
- Checks: `openspec validate --all --strict --no-interactive` 36 passed, 0 failed; `openspec validate inference-challenge-rate-budgets --type spec --strict` valid; every relative link in the archived design resolves. No code changed.

## 2026-11-20 (seventy-fourth round) — Code: owner challenges get a durable, non-regressing rate minute

- In step at `5d3553cc`; whitepaper unchanged; no new answers.
- Code-difference step (`specify-challenge-rate-budgets` 2.4 and 2.5; design code difference 1, fixed). Before, challenge issuance charged its 600-per-ledger and 12-per-wallet budgets through a plain per-minute window: an older or reordered clock sample restarted an exhausted budget, and any later-time cleanup (for example proposal storage) deleted the challenge counters.
- `billing_store.rs`: `challenge_rate_limit` replaces the generic `rate_limit`, whose only caller was challenge issuance. Inside the issuance transaction it selects `E = max(floor(t / 60), L)` from a `challenge-minute` watermark row and charges both budgets in `E`. Only a committed advance prunes older wallet rows; `cleanup_pending` no longer touches challenge rows. Retained counters without a watermark are adopted, never reset. Contradictory or unreadable evidence refuses fresh issuance with a `ledger` error and changes nothing. A refused or failed issuance rolls back the watermark, the counters and the tentative cleanup. The watermark is a `rate_limits` row, not `metadata`, because two existing tests compare the whole metadata table across wall-clock issuance. Schema 3, the quote clocks, lifetimes, pending capacity, authorization and the 429 mapping are unchanged.
- One existing test relied on the old behaviour: `control_acknowledges_only_spendable_current_nonce_and_recovers_without_rpc_or_broadcast` issued fourteen proofs for one wallet and passed only because a simulated later cleanup erased the challenge counters. Its four later proofs now enter the newer minute; no request or assertion was removed.
- Eight regressions (six `billing_store::tests`, one each in `inference_owner_authentication_test` and `inference_proposal_quota_test`) fail on the old code and pass with the fix: older and reordered samples, two writers racing for the last unit, other operations' later maintenance, failed future-minute issuance, adoption of 599 or 600 retained units, superseded wallet rows, and 30 inconsistent states. The review found three mutants that passed (a superseded row counted as current spend, a watermark accepted with its counters erased, a ledger count above 600 whose wallet rows sum to it) and added a test and two states until each failed.
- Recorded risk, now a report question: the old limiter could itself leave out-of-order rows after two challenges across a minute boundary. A provider stopped in that state and restarted on the new code refuses owner challenges until its `challenge:*` rows are repaired by hand.
- 2.4 and 2.5 are ticked; only the archive (3.2) remains.
- Checks: full suite under caffeinate: bins build passes; 30 headless test targets, 733 passed, 12 ignored (helpers), 0 failed (`--lib` 424; `inference_owner_control_test` 61/61); desktop `--lib desktop::` 5/5; `npm test` 80/80 in 17 files; `npm run check` 0 errors, 0 warnings; `python3 -m unittest discover -s scripts` 76 OK; `openspec validate --all --strict` 36 passed. No `pmset` sleep during the run.

## 2026-11-20 (seventy-third round) — Archive: `specify-profile-path-resolution`

- In step at `cc1e51dc`; whitepaper unchanged; no new answers.
- Task 4.2 ticked, then `openspec archive specify-profile-path-resolution --yes`: the change is now `openspec/changes/archive/2026-11-20-specify-profile-path-resolution`, and `openspec/specs/profile-path-resolution/spec.md` is the sixth normative capability (5 requirements added). The archive's placeholder Purpose was replaced with a one-sentence purpose, as for the other archived specs, and the two links to the change's old path (verus.md and the doc comment of `configuration_path_resolution_test`) now point at the spec.
- Every task had passed: 3.1 and 3.5 in round 72's full suite, the rest earlier. No pending change depends on this capability being pending.
- Checks: `openspec validate --all --strict --no-interactive` 36 passed, 0 failed; `openspec validate profile-path-resolution --type spec --strict` valid; `cargo check --tests` passes after the doc-comment change. No code changed.

## 2026-11-20 (seventy-second round) — Acceptance: provider, relay and gateway paths resolve against their configuration; a refused profile is shown and the earlier one stays loaded

- In step at `8e60d226`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-profile-path-resolution` 3.1 and 3.5). New tests:
  - 3.1: `provider_config::tests::every_relative_provider_path_resolves_against_the_configuration_directory` covers all nine provider path fields over two managed models (a `..` kept, nothing required to exist or created, absolute values unchanged). The new integration target `configuration_path_resolution_test` runs the real `relay_server`, `chiral_rpc_gateway` and `chiral_daemon --check` from a working directory holding decoys under the same relative names, with an absolute and a relative `--config` and with absolute values: the relay creates its identity and data next to the configuration and reports that peer ID, serves the configured policy and sends the configured bearer upstream; the gateway's upstream bearer comes from the configuration's directory while `--journal-dir` and `--bearer-file` follow the working directory; the daemon reads its key, token and bearers next to the configuration and its `--version-policy` from the working directory. `consumer_profile_paths_test` gains a test that every command-line path argument (wallet export/import, `quote --offer`, `infer --request`, `credentials issue/rotate --options`, `fund --quote`, `provider-admin --token-file`) follows the working directory, and one that all 32 `chiral` subcommands report a refused profile with exit 1, nothing on stdout and one line on stderr, creating nothing.
  - 3.5: `desktop::tests::configure_returns_a_refusal_and_the_loaded_profile_stays_loaded` refuses a profile with stranded `data_dir`, RPC bearer and DHT identity in turn while another is loaded with a pending stream: each refusal is returned, nothing is created, the same consumer stays loaded and no stream is cancelled; the absolute-path remedy then loads. `tests/settings.test.ts` shows the refusal verbatim in an alert in Settings, with the session, its epoch and the loaded address unchanged.
- Production change, behaviour-preserving: the body of the `consumer_command` Tauri command moved into `consumer_operation(&DesktopState, …)`, which the command calls, because `tauri::State` has no public constructor and a test must drive the real dispatch (recorded in design.md observations). The review found that this broke two `application_startup_boundaries_test` scans reading the dispatch from `consumer_command`; they now pin `consumer_command` to the one-line delegation and read `consumer_operation`. A test-only lock serializes the desktop tests that set the process-wide effective version policy. `consumer_profile_paths_test` snapshots now hold SHA-256 digests, so a failed comparison never prints a wallet key.
- No code difference was found. The review found five mutants that passed (four `chiral` arguments read next to the profile, and a relay that canonicalizes its identity file, which requires it to exist) and strengthened the tests until each failed. Residual, recorded: a binary that canonicalizes the configuration file's own path before taking its parent is not caught; it differs only with symbolic links, which no scenario covers.
- 3.1 and 3.5 are ticked; every task but the archive (4.2) is done.
- Checks: full suite under caffeinate: bins build passes; 30 headless test targets, 725 passed, 12 ignored (helpers), 0 failed (`--lib` 418); desktop `--lib desktop::` 5/5; `npm test` 80/80 in 17 files; `npm run check` 0 errors, 0 warnings; `python3 -m unittest discover -s scripts` 76 OK; strict validation passes.

## 2026-11-20 (seventy-first round) — Acceptance: no node or miner, nothing at login, legacy data in place, eSpace-only funds, provisioning starts no service

- In step at `4ffe0dc4`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-no-background-start` 3.1–3.5). New integration target `application_startup_boundaries_test` (a Rust lexer that separates code from comments, literals and test code, with self-tests), a macOS process observer (`tests/support/process_observer.rs`: a suspended `posix_spawn`, kqueue fork/exec/exit notes registered before the child runs, libproc child lists), three Vitest files, and audit and AST tests in `scripts/test_provision_provider.py`:
  - 3.1: outside test code the only process construction is `engine_runtime::command()`, private to its module and called only by the engine launch and the fixed device observers; no source names a node or miner and no dependency bundles one or runs programs; no desktop operation, `chiral` subcommand or option (recursive `--help`), operator action or `/admin` route controls a node or mining; on macOS an unmanaged daemon forks nothing through start, serving and `SIGTERM` and sends only `eth_*` to its configured RPC path, and a managed daemon's only child is its stub engine.
  - 3.2: no Tauri plugin or `plugins` member, only `default.json` with its eight permissions, exactly four manifest commands; nothing writes a launch agent, login item, `Run` value, Startup shortcut, scheduled task, XDG autostart entry, systemd unit or cron entry, or runs a service manager; `desktop::run`'s setup only installs the tray; the rendered App on all nine routes with no profile calls only `get_version_status` and the two listeners, makes no network request, and stays so for three fake hours; the earlier `chiral-settings` entry is never rewritten or removed; on macOS `network show` and `wallet create/address/balance` fork nothing.
  - 3.3: legacy `geth`/`ethminer` fakes, Geth data, plain, JSON and keystore wallets, CHI history, Drive, hosting and downloads under the `data_dir` base and the old OS data directory (under a temporary `HOME`) keep their modes and digests through the wallet commands and export, are never executed or loaded, and the new key and journal go only to the namespace; `wallet import` leaves its source's bytes, mode, mtime and inode unchanged.
  - 3.4: no `cfx_` or `pos_` method, CrossSpaceCall address or Core address in native or frontend sources; a balance read, a quote and paid opening and a resume send only `eth_*` to the configured path; the balance shown is `eth_getBalance` only, and the Wallet page shows no CHI or CHR balance.
  - 3.5: native provider, runtime and model provisioning (from an archive or a stubbed download) run no process and write only their destination, under an audit hook; on Windows paths only `whoami`, `icacls` and a `Get-Acl`-only PowerShell run, each returning first; an AST scan that follows aliases pins each script's exact process constructs; the Compose file has exactly the provider, relay, chain RPC and ingress services, each running its Chiral binary or the pinned Caddy image.
- No product code changed and no code difference was found. The author's mutations and the review's eleven each failed a test. The review closed seven gaps where a plausible regression passed every test: a camel-case mining operation, a public `command` reachable from another module, a bare `fork` imported from `libc`, a program-running crate, a periodic native call from the App, a `cfx_getBalance` in the frontend, and a `Popen` on a branch the audit fixtures do not reach.
- 3.3–3.5 are ticked. 3.1 and 3.2 stay open only for the Linux `/proc` observation of the daemon and the CLI (the observer is macOS-only; no Linux evidence is claimed); the operating-system login itself is task 4.1. Recorded limits: the name and login-mechanism scans are deny lists that string concatenation defeats; the structural process scan and the dependency list are the stronger guard.
- Checks: `--lib` 417 passed, 12 ignored (helpers), 0 failed; `application_startup_boundaries_test` 14/14; `daemon_engine_process_test` 20/20; bins build and `cargo check --tests` pass; `npm test` 78/78 in 17 files; `npm run check` 0 errors, 0 warnings; `python3 -m unittest discover -s scripts` 76 OK; strict validation passes. Other integration targets were not rerun: the only other Rust change is library test code.

## 2026-11-20 (seventieth round) — Acceptance: startup refusals, one daemon per deployment, ledger scoping, no re-terming, shutdown, no content or credentials in logs

- In step at `c1409b1f`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-provider-daemon-lifecycle` 3.7–3.12). Four new `app::tests`, one strengthened, and seven new process tests in `daemon_engine_process_test` (in-process stub engines, a gated stub chain RPC and a raw HTTP/1.1 client):
  - 3.7: a restart whose RPC reports another chain ID or genesis is refused with "configured RPC identity was rejected"; at process level the daemon exits non-zero without its start line or an engine, and a request queued on its listener is never answered. With an RPC answering 503 the provider starts, `/readyz` reports `funding_ready` false, nothing is published, a funded account is served at its agreement's rates, and a later pass publishes. An exhausted offer revision is a reported pass failure, not a start failure; a reserved request whose recovery fails refuses the start.
  - 3.8: the second start names the lock error, and the lock is taken before the ledger opens; a second daemon process for the same deployment directory (other ports, its own counting bootstrap listener) exits non-zero with the lock error, starts no engine and makes no DHT connection, while the first keeps serving.
  - 3.9: a ledger of schema 2 or 4, or copied from another signing wallet or network identity, refuses the start (`wrong_provider`, `wrong_network`) with every row and schema object unchanged; legacy wallet, chain, Drive, hosting and file-transfer data under the base `data_dir` keep their digests through start, serving and shutdown.
  - 3.10: a running provider whose configuration file is edited (price, revision, minimum funding, concurrency) changes nothing until restart; after a restart with twice the price, the old account is charged at its agreement's rate and only the new offer carries the new price; a restart with another model revision refuses the old account before reservation and keeps its balance, receipts, funding history and credential control.
  - 3.11: after `SIGTERM` with one admitted stream, new requests are refused at once, the stream completes, the daemon exits 0 and the request is `completed` after restart; a stream that outlasts the real 75 s bound is released as `interrupted_unknown_usage` with zero usage and cost, never `completed`.
  - 3.12: prompts, completions, tool arguments, embedding inputs and vectors carrying a marker, an engine failure echoing the prompt and an owner cancellation leave no marker, account bearer, administration token, engine key, RPC bearer or signing key in standard output or error, any `GET /admin/*` answer, or any file under the data directory (also scanned while a request is still reserved); `/admin/requests` lists each request with identifier, status, usage, cost and times.
- No product code changed and no code difference was found. The author's 17 mutations and the review's 7 each failed a test. The review strengthened four tests where a plausible regression passed: a bound of 0 s passed the admitted-stream test in 2 of 3 runs, schema 4 was untested, re-reading admission terms from the configuration file passed, and so did a request body kept in a file until settlement.
- 3.7–3.10 and 3.12 are ticked. 3.11 stays open: a listener failure taking the shutdown path cannot be caused from outside the process, because axum 0.7.9 retries accept errors and never ends `serve` (design observation 15; a test needs the serve-and-shutdown sequence moved into the library or a test-only hook in the binary); the Windows console events are Windows-only.
- Housekeeping: the build disk filled during the author's run (358 MiB free); with no build running, it deleted macOS split-debuginfo `.o` files older than one day under `src-tauri/target/debug/deps` (regenerable build output, about 23 GB). Failing runs of `daemon_engine_process_test` leave its shared fixture directory in `$TMPDIR`, because its `main` exits with 101 before the directory is dropped; the eighteen left by this round's runs were removed.
- Checks: `--lib` 416 passed, 12 ignored (helpers), 0 failed; `daemon_engine_process_test` 18/18 (the bound case waits the real 75 s); bins build and `cargo check --tests` pass; strict validation passes. Other integration targets were not rerun: the only other change is library test code, which they do not compile. No process was left running.

## 2026-11-20 (sixty-ninth round) — Acceptance: status facts, runtime observation, drain, resume and refresh through a pass, pass ordering, the persistence-failure latch

- In step at `73281ae4`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-provider-daemon-lifecycle` 3.1–3.6; 3.7–3.12 follow). Fourteen new `app::tests` on funded paid accounts, a stub chain that settles signed payments, and a chat engine that can hold one stream open (`HeldStream`):
  - 3.1: reading `/admin/status` changes no admission, publication or ledger state (with a credited funding due for recheck and a pending fee); one admitted held stream counts as one unsettled request and none after it completes; drain, readiness, latch and errors stay separate facts.
  - 3.2: `/admin/runtime` reports a live engine failing its template probe as alive and not ready, and an exited one as neither.
  - 3.3: through `POST /admin/drain`, a new request is refused as `provider_paused` with no record, the held stream settles `completed` with its usage, no offer is published, the owner reads status and receipts, rotates a credential, tops up and cancels while drained, a new quote is refused, passes keep rechecking credited funding, and a second drain changes no durable state.
  - 3.4: resume reopens only through its pass, under a higher revision; offer refresh does not end a drain; resume over an exited engine ends the drain, reopens nothing and starts no process; a latched report stays latched through both.
  - 3.5: `#[cfg(test)]` pause points before and after a pass stores admission show that a racing drain or engine exit leaves nothing admitted or published, closed at once; passes recheck funding while drained, after an exit and in an outage; a quarantining recheck keeps the same pass from publishing; a failed reconciliation closes admission until a later pass succeeds; the periodic loop keeps running after a failed pass (the period is 100 ms only under `cfg(test)`, 30 s otherwise).
  - 3.6: after a settlement failure, a restart settles the reserved request as `restart_unknown_usage` before admitting again.
- Code difference 10 (task 3.1, found by the review), fixed: while a pass held the lifecycle lock, `/admin/status` reported a model whose managed engine had exited as ready with its passed-probe reason, while `/admin/runtime` and `/readyz` reported it not ready. `model_status` now reads engine liveness at read time, as those routes do. Severity low: admission was already closed.
- The author's 13 mutations and the review's 15 each failed a test. Those the review first found passing (both closing stores of a drain and of the engine watcher, a status read that reconciles or remits, a drained model reported not ready, a settlement error reported whenever admission is closed) now fail. The review also fixed a 2-in-11 flake in the periodic test (a request racing a readiness probe for the model's one slot), and replaced status-based waits for the engine watcher with a wait on the watcher's own entry.
- 3.1–3.4 and 3.6 are ticked. 3.5 stays open only for the version-policy admission gate: the effective policy is process-global and its test setter exists only with the desktop feature, so a parallel library test cannot set it (owned by `client-version-policy` 3.12). Recorded, not fixed: a drain beginning between a pass's admission decision and its store can let that pass reopen admission until the drain takes the lock, which happens before the drain answers, so requirement 5 holds.
- Checks: full suite under caffeinate: bins build passes; 28 headless test targets, 691 passed, 12 ignored (helpers), 0 failed (`--lib` 412); desktop `--lib desktop::` 4/4; `npm test` 70/70; strict validation passes. The scripts were not changed this round.

## 2026-11-20 (sixty-eighth round) — Acceptance: stock-client modes, GGUF capability derivation, readiness probes, completion echo, embedding dimensions, tool output as data

- In step at `1a147f92`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-public-inference-api` 3.2 and 3.7–3.11). New tests:
  - 3.2: both generation routes give one JSON object without `stream` and an SSE stream with it, with only a bearer and no Chiral header; SDK-shaped requests over real HTTP/1 under the origin plus `/v1` reach the listing and both generation routes through the merged public router.
  - 3.7: `provision-model.py` runs against an injected `huggingface_hub` stand-in (no install, no network): a generation GGUF derives `chat` only from a selected embedded template, an embedding GGUF with a template derives only `embeddings`, a tool-use template and model-card claims add no capability, and only GGUF files prepare or run a model.
  - 3.8: a new `ProbeEngine` stand-in records each readiness probe; readiness sends the exact probe sequence through the public preparation, requires the engine-reported identifier, alias, context and template digest, fails on any advertised probe fault, keeps a model unadmitted and unpublished until it passes, and withdraws a ready model whose later probe fails (operator-owned and managed).
  - 3.9: echo returns the exact prompt (the literal `Hello` / `' world'` case and a multibyte prompt), `logprobs: null` on the prompt event, generated-only log-probabilities with offsets after the prompt, in both modes and with or without log-probabilities, billed and timed as generated tokens only; a stop at the full context is accepted only with consistent usage.
  - 3.10: vectors keep the native dimension; a batch of unequal or unrequested lengths is a failure, not a success.
  - 3.11: a tool call and generated text are returned as data and never acted on; parallel tool-call fragments are reassembled by their index; remote schema references and non-function tools are refused and nothing is fetched.
- Two code differences were found and fixed, both low severity:
  - Difference 5 (task 3.9): the qualification runner's paid echo probe (`probe_completion_echo`, `scripts/qualify-provider.py`) accepted log-probabilities covering the prompt's tokens or offsets from the start of the prompt. It now requires one entry per generated token in each list and a first offset equal to the prompt's length. The Rust gateway was not affected.
  - Difference 6 (task 3.8, found by the review): readiness accepted a streamed probe that returned none of the log-probabilities it asked for, so such a model was admitted and published. `probe` (inference_backend.rs) now requires at least one generated-token entry before `[DONE]` when the probe asked for them; the public request path is unchanged. Task 4.1 now also asks for pinned-engine evidence that a real chat and completions model still passes.
- The author ran 27 mutations and the review 14 more; each was caught by an assertion. Mutations that first passed (a runner checking only one list's length, an index-blind tool-call collector, a replaced tool name, echo honoured only with log-probabilities, a larger engine context accepted) were caught after cases were added. The only seam is `inference_backend::tests` becoming `pub(crate)` so `app::tests` can share `ProbeEngine`.
- 3.7–3.11 are ticked. 3.2 stays open only for the automated runs of `examples/openai_client.py` and `examples/openai-client.mjs`, blocked because the Python and Node `openai` packages are not installed (shared with account-credentials 3.8). Observation, not a difference: with `stream_options.include_usage`, intermediate chunks' `usage` from an engine is forwarded; the pinned llama.cpp is not known to send one (to check in 4.1).
- Checks: full suite under caffeinate: bins build passes; 28 headless test targets, 677 passed, 12 ignored (helpers), 0 failed (`--lib` 398); desktop `--lib desktop::` 4/4; `npm test` 70/70; `python3 -m unittest discover -s scripts` 68 OK; strict validation passes.

## 2026-11-20 (sixty-seventh round) — Acceptance: public surface, agreement-scoped listing, capability admission, body handling, honest failures, text-only inference

- In step at `08fcbbd3`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-public-inference-api` 3.1, 3.3, 3.4, 3.5, 3.6 and 3.12; 3.2 and 3.7–3.11 follow). Sixteen new tests on a counting, recording stub engine (`CountingEngine`):
  - 3.1: the merged `ProviderRuntime::public_router` answers 404 to 21 engine and arbitrary paths (GET and POST, with no, an invalid and the valid bearer) and to 11 out-of-scope OpenAI surfaces, with no engine request and no record.
  - 3.3: with two ready deployments and one account on each, each bearer lists only its own model, including when only the other deployment is ready.
  - 3.4: another model, a route the agreement does not advertise (with and without `max_tokens`), tools and structured output without their capabilities (also on `/v1/completions`), 35 engine-only members and members nested inside allowed ones are refused before reservation with no engine call.
  - 3.5: requests refused for their content type or scope never have their body polled; an oversized body is refused with or without a declared length, including one that stalls after the excess byte; a slow upload consumes neither the generation deadline nor the performance clock.
  - 3.6: insufficient credit (402, no hold), a busy slot (429 without queueing), a failed health check (before reservation), a context overflow (before reservation) and a stream that fails after output began (exactly one error event, last, and `failed_unknown_usage`) surface honestly.
  - 3.12: image, audio, file and video parts and a `lora` member are refused before reservation; an engine failure fails on the contracted engine after one dispatch, with no retry or reroute.
- The first review stopped when the login expired. I checked that every product file was identical to HEAD (no mutation left applied) and resumed it with a note about its unfinished test edits; the second review completed them. Together the reviews found mutations that passed (a fallback to another ready deployment, an allowlisted `n_predict`, a missing capability check masked by an embedding output limit of zero, an unbounded body read, a dropped per-part member check, an abnormal end accepted instead of an error event) and added cases until each failed. Failure messages check for bearers before showing an answer.
- No product code changed and no code difference was found. Recorded as implementation behaviour: a declared `Content-Length` over the bound is refused only when the bytes arrive, and `/v1/completions` refuses tools and `response_format` even when advertised. Never sending a request to another provider holds by inspection (the gateway has no client for one).
- Checks: `--lib` 384 passed, 12 ignored (helpers), 0 failed; bins build and `cargo check --tests` pass; strict validation passes. The integration suite was not rerun: every change is in library test modules (no non-test hunk, checked), which integration targets and binaries do not compile.

## 2026-11-20 (sixty-sixth round) — Acceptance: native credential store, credential commands, rotating the default

- In step at `2ce79dc5`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-account-credentials` 3.7, 3.8 and 3.9). New tests:
  - 3.7 (Unix): after a native opening, issuance, rotation and one dispatch, the consumer root, account directory and account file are owner-only and network-scoped, and no other file under `data_dir` holds any issued secret in its contents or its name (request-key file, wallet journal, temporary files included); an account file or directory that is group- or other-accessible, or replaced by a symbolic link, makes export, list, issue, rotate, revoke and infer fail closed with no provider request and nothing repaired.
  - 3.8: native issue and rotate return only `{contract_id, expires_at, id}`, revoke only `{revoked}`, and lists no secret member; only `credential_export` returns the secret; eight failure paths hold no secret. New integration target `inference_cli_credentials_test` runs each `chiral credentials` command as a process against the production routers: one JSON document of metadata and an empty stderr on success, empty stdout and one `chiral:` line on failure, and `export` prints exactly the five members with an `api_key` that authenticates as that credential.
  - 3.9: rotating an expired or provider-revoked default with `infer` (alone or with `read`) makes the replacement the default for export and native inference, across a restart, with no fallback to another stored credential; a read-only rotation never becomes the default (the review added the infer-only case).
- The review found three mutations that passed (protection checks ignoring "other" permission bits, a request-key file named after the bearer, activation requiring both scopes) and added cases until each failed.
- No product code changed and no code difference was found. 3.9 is ticked. 3.7 stays open only for the native Windows run; 3.8 only for the automated runs of `examples/openai_client.py` and `examples/openai-client.mjs`, blocked because neither the Python nor the Node `openai` package is installed here (not installed, per the rules). Refusal of a store owned by another user holds by inspection of the uid checks (a test would need a second account).
- Observation, not a code difference: the consumer root is checked only when a `Consumer` is created, so a long-running desktop consumer does not recheck it per operation (the account directory and file are checked on every operation, and the CLI rechecks the root in every process). Replacing the root needs the same user's privileges; no scenario of the spec covers it.
- Checks: `--lib` 367 passed, 12 ignored (helpers), 0 failed; `inference_cli_credentials_test` 1/1; bins build and `cargo check --tests` pass; strict validation passes. Other integration targets were not rerun: the only other change is library test code, which they do not compile.

## 2026-11-20 (sixty-fifth round) — Acceptance: credential options, hashed secrets, bearer scope and lifetime, rotation, limits, management without admission

- In step at `7fdc4436`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-account-credentials` 3.1–3.6; 3.7–3.9 follow). New library tests drive the production control and inference routers over file-backed ledgers with owner proofs:
  - 3.1: issuance and rotation refuse empty, unknown and mixed scopes, concurrency above the account limit, non-canonical caps (`"01"`, `"1.5"`, `"-1"`, `""`, a number), unknown and repeated members and a positional array, with every credential row and the account view unchanged and the rotated credential still working; valid options give a list entry with exactly that expiry, cap, concurrency and scopes, authenticating at `expires_at - 1` and not at `expires_at`.
  - 3.2: no secret (whole, its random hex in either case, or its 32 raw bytes) appears in `ledger.sqlite`, `-wal`, `-shm` or `-journal`, open or closed; each `token_hash` is `hash_hex` of its secret; list entries carry exactly the 11 view members; identifiers are independent of secrets.
  - 3.3: read-only bearers get 403 `credential_scope` on the inference routes and cancel, with nothing reserved, sent or cancelled; infer-only bearers get 403 on request status; revoked bearers and bearers at or past `expires_at` get 401 on all six routes (the exact second is caught by a same-second retry loop, no sleep or seam).
  - 3.4: expired and revoked credentials rotate into replacements with exactly the requested scopes while the predecessor stays refused; another account's identifier of the same owner is refused with 404 for revocation and rotation (added by the review; before, only `inference_owner_control_test` covered it).
  - 3.5: a cap above the balance changes no account, funding or fee state; a reservation above the balance fails `insufficient_funds`, not `spend_cap`; revocation, rotation and expiry move no money.
  - 3.6: with admission, new funding, qualified models, quote budgets and funding-intent capacity all closed and no offer registered, owner list, issuance, rotation and revocation answer 200 and change no money or gate state.
- The review found five mutations that passed (any-known instead of every-known scope, raw secret bytes in the ledger, both cross-account checks removed, issuance gated on opening capacity) and added assertions until each failed. Failure messages of the new tests print only error codes, never a document that could hold a secret.
- No product code changed and no code difference was found. 3.1–3.6 are ticked. Two statements hold by inspection only, as recorded in tasks.md: no secret in provider logs (no logging call in those modules) and no native surface presenting a cap as a balance.
- Checks: `--lib` 363 passed, 12 ignored (helpers), 0 failed; bins build and `cargo check --tests` pass; strict validation passes. The integration suite was not rerun: every change is in library test code (`billing_store_tests.rs` and the `inference_gateway` tests module), which integration targets and binaries do not compile.

## 2026-11-20 (sixty-fourth round) — Acceptance: pinned outbound client, private engine origin, ingress and preflight

- In step at `3140f4ee`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-outbound-http-safeguards` 3.4, 3.7 and 3.8). With round 63, every acceptance task of this change is ticked; 4.1 (deployment evidence), 4.2 and 4.3 remain.
  - 3.4: empty, oversized (33) or partly private answer sets build no client; resolution is refused after exactly 5 s (paused time); a pinned client abandons a stalled connection after exactly 10 s and connects only to its answers; a provider or catalog answering 302 to every consumer request kind (inspection, discovery, quote, signed controls, bearer requests, feedback and the hidden-default check of `credential_issue`) is a failure of that origin, and the redirect target records no connection; provider, catalog, engine and administration clients ignore `HTTP_PROXY`, `HTTPS_PROXY` and `ALL_PROXY` (three tests in `outbound_proxy_environment_test`); an HTTPS client sends the origin's server name and refuses an untrusted certificate before sending anything (added by the review, with a public-only self-signed fixture certificate and no new dependency).
  - 3.7: engine origins accepted and refused in both modes through `validate`, `load`, `ProviderRuntime::start` and `Backend::new`, at every private-range edge (the review added these after narrowing 172.16/12 or fc00::/7 passed); an engine redirect is an engine failure and its target never sees the key or prompt; the engine origin and its error text appear in no signed offer or public answer.
  - 3.8: the generated Caddyfile forwards only public-router paths to `127.0.0.1:9419` and nothing to 9421, 8000 or a file server; the preflight private-route probe, factored into `check_private_routes` with unchanged behaviour, fails on any answer other than a refusal and names the route; `check_listeners` fails private ports on wildcard or LAN addresses in Linux, macOS and Windows formats.
- No product behaviour changed; the Rust changes are test code or `#[cfg(test)]` (resolver delay and silence for the deadline test). No code difference was found.
- Recorded weaknesses: hostname matching after a trusted chain is not executed (no trusted localhost certificate can exist in a test); it holds by construction (rustls default verifier, no custom verifier). The 3.7 advertisement test uses a loopback engine, with `10.0.0.5` only in its error text.
- Checks: `--lib` 355 passed, 12 ignored (helpers), 0 failed; `outbound_proxy_environment_test` 3/3; bins build and `cargo check --tests` pass; Python compile and `unittest discover -s scripts` 63 OK; strict validation passes. The rest of the integration suite was not rerun: every Rust product-file change is `#[cfg(test)]` code, which integration targets and binaries do not compile, and the changed integration target was run.

## 2026-11-20 (sixty-third round) — Acceptance: origins before any request, the development exception, the public address predicate, the administration listener

- In step at `850f8e5b`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-outbound-http-safeguards` 3.1, 3.2, 3.3, 3.5 and 3.6; 3.4, 3.7 and 3.8 follow). New tests:
  - 3.1: non-conforming catalog origins (`:8443`, `10.0.0.5`, a single label, `.localhost`) fail `discover`, `feedback` and the reputation read of `provider_info` with the origin error before any resolution or connection; a validly signed offer at a non-conforming origin is refused by `provider_info`, `quote` and `fund` (no account file saved, current behaviour), and a saved account at such an origin is refused by every consumer operation with the account file unchanged; `ProviderConfig::validate`, `load` and `ProviderRuntime::start` refuse non-conforming public endpoints and create nothing.
  - 3.2: development `public_bind` must be loopback; consumer profiles and provider files without a `development` member are not development; the scenario origins reach resolution only when admitted; a development `localhost` client connects only to the answer it was pinned to (added by the review, after removing the pin passed every test).
  - 3.3: catalog URLs with a path, query, fragment or credentials are refused before any request (load-time refusal pinned as current behaviour).
  - 3.5: `public_ip` judged directly at every range edge and public neighbour, including IPv4-mapped, IPv4-compatible and NAT64 forms; a resolved `::ffff:127.0.0.1` is refused.
  - 3.6: `admin_bind` must be a non-zero loopback port distinct from the public port; the public router serves no `/admin` route; all ten administration routes refuse a missing, wrong, account-API, Origin-carrying, owner-signed or scheme-less credential and nothing drains; the token comes only from a protected file; the native administration client contacts only literal loopback and ignores environment proxies (new integration binary `outbound_proxy_environment_test`).
- Test seam: a `#[cfg(test)]` task-local resolver in `offer_registry.rs`. The system lookup moved into `resolve()`, a behaviour-preserving extraction (the same lookup, still at most 33 answers inspected); it is the only change outside test code. No code difference was found.
- The review found four mutations that passed every test (pin removed, `POST /admin/fees/remit` outside the bearer layer, multicast narrowed to /5, a local credential change before the provider request) and added assertions until each failed. I bounded the pinning test's search for a port free on both loopback families, so a host without IPv6 loopback fails clearly instead of hanging.
- 3.1, 3.2, 3.3, 3.5 and 3.6 are ticked.
- Checks: full headless suite under `caffeinate` 623 passed, 12 ignored (helpers), 0 failed across 27 targets (the new proxy-environment binary included); `cargo test --lib desktop::` (desktop feature) 4 passed; `npm test` 70/70; no daemon or engine process left running; strict validation passes. Two stale `tail -f` watchers left by earlier steps of this session were stopped.

## 2026-11-20 (sixty-second round) — Acceptance: bounded startup, engine exit without restart, shutdown stops owned engines

- In step at `5cd927bc`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-managed-engine-launch` 3.9, 3.10, 3.11 and 3.1's no-fallback scenario). New tests:
  - 3.9: an engine that exits before readiness fails the launch and is not relaunched (unit test and a `chiral_daemon` process test whose stub logs each start); the health probe carries the engine bearer, follows no 307 or 308 redirect and uses no proxy; `ProviderRuntime::start_with` with a running first model and an uninventoried second model fails, makes exactly one attempt for the second model, stops the first engine within 5 s and publishes nothing.
  - 3.10 (with 3.1's no-fallback scenario): within 1 s of a managed engine's death, admission, new funding, `/admin/runtime` readiness and `/readyz` are closed and the model is not admissible; the DHT, `/admin/offers` and consumer discovery hold no offer; refresh, offer refresh and resume reopen and republish nothing and attempt no launch; a request is refused before any record; administration routes answer unchanged and request recovery still works.
  - 3.11: stop and drop kill the engine's whole process group (grandchild gone); on macOS a SIGKILLed parent leaves an orphan that blocks the next start with the occupied-port error (the Linux parent-death branch compiles but did not run here); the shared SIGTERM/SIGHUP serving test now asserts, through `waitid` with `WNOWAIT`, that the engine was gone when the daemon exited.
- The review found that a watcher, resume or refresh that called `ManagedEngine::start` again passed the 3.10 test, because the fixture model has no runtime and fails before spawning. It added a `#[cfg(test)]` launch-attempt record in `start_observed` and assertions that now fail for all three restart mutations and for a retried start.
- No product code changed; every change is test code or `#[cfg(test)]`. No code difference was found.
- 3.9 and 3.10 are ticked. 3.1 now lacks only Linux and Windows runs. 3.11 stays open for the Linux parent-death run (with a `PR_SET_PDEATHSIG` mutation) and a Windows job-object test. Recorded weaknesses: 3.10 compares only zero-valued accounts and fees (a funded fixture needs a chain fixture that answers receipts), and on macOS a daemon without `stop_engines` cannot be told apart from the specified shutdown, because `Drop` still kills the engine before exit.
- Checks: `--lib` 331 passed, 12 ignored (helpers), 0 failed; `daemon_engine_process_test` 11/11; bins build and `cargo check --tests` pass; no engine, daemon or helper process left running; strict validation passes. The rest of the integration suite was not rerun: the only product-file change is `#[cfg(test)]` code, which integration targets and binaries do not compile.

## 2026-11-20 (sixty-first round) — Acceptance: pinned private child, inventories before every launch, loopback listener, device observation

- In step at `7f243cb2`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-managed-engine-launch` 3.1, 3.2, 3.4 and 3.7). Twenty new tests in `engine_runtime::tests` and two process tests in `daemon_engine_process_test`:
  - 3.1: runtime manifests of another release, revision, schema, platform, architecture or backend are refused by `validate_inventory` and `ManagedEngine::start`; only an inventoried native `llama-server` is accepted, and a refused script never runs (marker file).
  - 3.2: removed, resized or uninventoried files, another model identifier, and any serving-profile field that differs are refused; a repackaged runtime has another identity; an altered inventory refuses the launch before the port is bound or any observer or engine runs, including through `chiral_daemon --check` and a daemon start, for every managed model. The review added a test for nested inventory levels, symbolic links (Unix) and the 4 MiB manifest bound after four fail-open mutations passed every test.
  - 3.4: managed endpoints must be literal loopback with an explicit non-zero port; the wildcard port probe never listens; a health answer after the child exited is not readiness, and the review added llama-server's 503 "Loading model" answer, which must not count as health.
  - 3.7: device-output parsing is now pure functions over captured text (`engine_devices`, `cuda_devices`, the Apple parsers and `vulkan_devices`), and `observe_devices`, `start_observed` and `await_loaded` take the observers through a `DeviceObservers` seam (`SystemObservers` in production). Two independent differential runs (21,600 and 30,800 generated outputs) showed the same results and observer sequences as the old code, except for the fix below. Tests cover headroom bounds, PCI bus order, Apple available memory (the review added `hw.memsize` rounding down), Vulkan discrete-GPU identity, observer order, profile mismatches before spawning, post-load loss of headroom stopping the child, and observer size and time bounds.
- Code fix (design code difference 4, requirement 7 "ambiguous output SHALL fail the launch"): a selector listed twice by `--list-devices`, or two `nvidia-smi` rows with one PCI bus ID, are now refused instead of taking the first match or the sorted position. The new tests failed before the fix. Report entry of 2026-11-18 updated.
- 3.2 and 3.4 are ticked (macOS runs; Windows symbolic links, junctions and wildcard listeners stay with task 4.1). 3.1 stays open for the no-fallback scenario (task 3.10) and Linux and Windows runs of the ELF and PE branches. 3.7 stays open: its `--list-devices`, `nvidia-smi` and `vulkaninfo` fixtures follow documented formats, not recordings from the pinned engine on CUDA, Apple silicon and Vulkan hosts.
- 43 author mutations and the review's further mutations each fail the targeted tests; every file was restored and compared with `cmp`.
- Checks: full headless suite under `caffeinate` 598 passed, 6 ignored (helpers), 0 failed across 26 targets (`daemon_engine_process_test` 10/10); `cargo test --lib desktop::` (desktop feature) 4 passed; `npm test` 70/70; no daemon or engine process left running; strict validation passes.

## 2026-11-20 (sixtieth round) — Acceptance: managed-engine settings, launch arguments, environment and contract checks

- In step at `3511e419`; whitepaper unchanged; no new answers.
- Acceptance step (`specify-managed-engine-launch` 3.3, 3.5, 3.6 and 3.8). Six new tests:
  - every runtime setting bound, through both `EngineRuntimeConfig::validate` and a provider file load, refused rather than adjusted;
  - the complete llama-server argument vector for CPU, one- and two-device CUDA, embedding and IPv6 fixtures, with `--fit off` and `--no-context-shift` always present and no key, remote-model or web-UI option;
  - the engine and device-observer children receive only the allowlisted environment, even with a polluted parent (including `CUDA_DEVICE_ORDER` and lower-case proxies), the executable's directory, and `/dev/null` stdio;
  - engine key files must hold one protected literal token;
  - an engine whose `/props` reports another context, slot count, alias or build stays not ready and unpublished.
- Every test was checked against at least one mutation. The review found five mutations that survived and added assertions until each failed.
- No product code changed. 3.3, 3.5 and 3.8 are ticked. 3.6 stays open for the Windows environment branch, Windows key storage, the Unix owner rule and Linux runs. The does-not-fit scenario of 3.8 is assigned to task 4.1 (real engine), as the task states.
- Checks: `--lib` 305 passed, 5 ignored (helpers); bins build; `daemon_engine_process_test` 8/8 (review); strict validation passes. The full integration suite was not rerun: every change is in `#[cfg(test)]` code, which integration targets and binaries do not compile.

## 2026-11-20 (fifty-ninth round) — Spec: the application starts no node, miner or legacy service

- In step at `91970f1f`; whitepaper unchanged; no new answers.
- Specification step (comparison item 16, book line 31 with 17, 777 and 793). New change `specify-no-background-start` (capability `application-startup-boundaries`): 5 requirements and 14 scenarios. They cover:
  - no node or miner in any binary;
  - no node, miner or legacy service at operating-system login, launch or profile load;
  - legacy data left in place and never imported automatically;
  - Core funds usable only through the user's explicit transfer;
  - native provisioning starts no service.
- The review:
  - narrowed rules that went beyond the book (stopping or removing legacy services, placing the policy cache, container-mode provisioning, mining status);
  - corrected the spawn inventory (the engine helper also runs device observers);
  - widened the code search.
- No code difference was found; acceptance tasks 3.1–3.5 name the missing evidence. Report entry with one nonblocking question (start-at-login without a node or miner).
- With this, every missing specification in `comparison-2026-11-18.md` is written.
- Checks: `openspec validate specify-no-background-start --strict` passes; no code changed.

## 2026-11-20 (fifty-eighth round) — Code: non-fatal starting-pass publication, deployment lock before the DHT, runtime liveness without readiness

- In step at `4c41f744`; whitepaper unchanged; no new answers.
- Code step (provider-daemon-lifecycle 2.5, 2.6 and 2.7). All nine original code differences of this change, and the follow-up 2.10, are now done.
  - A failure of the starting maintenance pass no longer stops the daemon. It is reported in `refresh_error` and on stderr; new funding stays closed, and admission is whatever the pass decided. Configuration, secret, inventory, launch, lock, ledger-identity, chain-identity and recovery failures still stop the start.
  - The daemon loads its peer identity (unchanged file rules), takes the deployment lock, and only then starts the DHT node, so a refused second daemon never listens, dials or answers under the deployment's peer ID.
  - `/admin/runtime` rows are now `{model_id, managed, alive, ready, observation}`: `alive` is process liveness, and `ready` is the latest readiness result, never true for a dead engine. `provider-preflight.py --running`, `qualify-provider.py` and the qualification guide now read `alive` and take readiness from `/readyz`. `/readyz` is unchanged.
  - The review found no defect; one stderr wording fix ("running", not "serving").
  - Rust and Python regressions fail without their changes.
- Checks: full headless suite under `caffeinate` 571 passed, 2 ignored, 0 failed across 26 targets; `cargo test --lib desktop::` (desktop feature) 4 passed; Python compile and unit tests ok; `npm test` 70/70; no daemon or engine process left running; strict validation passes.

## 2026-11-20 (fifty-seventh round) — Code: hang-up and console events shut down cleanly; resume cannot reopen during shutdown or clear later faults

- In step at `d337d481`; whitepaper unchanged; no new answers.
- Code step (provider-daemon-lifecycle 2.2, 2.9 and 2.10).
  - `SIGHUP`, and on Windows Ctrl-Break and console close, take the same shutdown path as `SIGTERM`, during engine startup and while serving. A daemon started with `SIGHUP` ignored (`nohup`) keeps ignoring it. The review found the first patch broke this and fixed it with a `sigaction` read before any handler is installed.
  - The drain state is now open, draining or shutting down, and it only rises during shutdown. `POST /admin/resume` answers 409 `shutting_down` once shutdown begins, and no pass admits outside the open state.
  - Settlement failures carry sequence numbers, and a resume clears only those recorded before its request arrived.
  - The Windows paths are verified by reading only; no Windows target exists on this machine.
  - Regressions fail without their changes, including the macOS engine-orphan reproduction for `SIGHUP`.
- Behaviour change: `SIGHUP` now stops the daemon, so it cannot be used to reload. No tracked deployment file does that.
- Checks: full headless suite under `caffeinate` 568 passed, 2 ignored, 0 failed across 26 targets (`daemon_engine_process_test` 8/8); `cargo test --lib desktop::` (desktop feature) 4 passed; no daemon or engine process left running; strict validation passes.

## 2026-11-20 (fifty-sixth round) — Code: settlement-failure stop waits for the operator; status reports drain, latch, ledger fault and engine exit

- In step at `22195828`; whitepaper unchanged; no new answers.
- Code step (provider-daemon-lifecycle 2.1, 2.3, 2.4 and 2.8; 2.2, 2.5–2.7 and 2.9 follow in later rounds).
  - A durable settlement failure, or a clock failure, now records a fault that closes admission and new-funding readiness. No maintenance pass or offer refresh reopens them. `POST /admin/resume` clears the fault and runs a pass. The fault latches at the first refused write even when the drop retry records the request as `interrupted` (design observation 13).
  - `/admin/status` now always answers. Additive members: `draining`, `settlement_error`, `ledger_error` (with `ledger: null` when the summary cannot be read) and each model's `performance_withdrawn`. A latched model's detail names the withdrawal.
  - An engine exit sets the detail "Managed engine exited" in the same write that sets readiness false.
  - The review found new-funding readiness left true during the stop, fixed it and strengthened the regression.
  - Five regressions fail without their changes.
- Recorded follow-up task 2.10: a resume can clear a fault recorded after the resume was requested.
- Checks: full headless suite under `caffeinate` 561 passed, 2 ignored, 0 failed across 26 targets; `cargo test --lib desktop::` (desktop feature) 4 passed; `npm test` 70/70; strict validation passes.

## 2026-11-20 (fifty-fifth round) — Spec: the provider daemon lifecycle

- In step at `35e93e91`; whitepaper unchanged; no new answers.
- Specification step (comparison item 14, book lines 605–680). New change `specify-provider-daemon-lifecycle`: 12 requirements and 36 scenarios. They cover:
  - separate status facts;
  - runtime liveness that is not readiness;
  - operator drain;
  - resume and refresh only through a complete pass;
  - the ordered maintenance pass;
  - an operator-attended stop after a settlement failure;
  - startup refusals;
  - one daemon per deployment;
  - the ledger's own schema, network and provider;
  - no re-terming on restart or configuration change;
  - an orderly shutdown;
  - content-free logs.
  Discovery, engine launch, performance, renewal, request accounting, funding, remittance, version policy, listeners, the RPC gateway and credentials are referenced from their owners. Backup and cutover are recorded as operator procedures.
- The review:
  - fixed an untestable lock scenario and a wrong single-refusal scenario;
  - narrowed one difference;
  - found two more (an engine exit keeps a "Ready" detail; a resume racing shutdown reopens admission);
  - added the book-671 rule that no request content is retained.
- Nine code differences recorded as tasks 2.1–2.9, and a report entry with six nonblocking questions.
- Checks: `openspec validate specify-provider-daemon-lifecycle --strict` passes; no code changed. The review reran 5 library tests and 2 integration suites (40 tests), and all passed.

## 2026-11-20 (fifty-fourth round) — Code: name an unprobeable capability set in the health detail (public-inference-api 2.5)

- In step at `d16a52c0`; whitepaper unchanged; no new answers.
- Code step (public-inference-api 2.5, from the round-53 review). The capability rule is now the pure `unprobeable_capabilities`. `probe_model` reports the named capabilities and why they cannot be probed, in place of the generic probe-failure detail, before taking capacity or contacting the engine. Configuration acceptance is unchanged. A new unit test covers nine sets, and the app regression asserts the detail. It fails when the new branch is neutralised.
- The pure function is a seam a future Verus lemma could cover (verus.md).
- Checks: full headless suite under `caffeinate` 556 passed, 2 ignored, 0 failed across 26 targets; `cargo test --lib desktop::` (desktop feature) 4 passed; strict validation passes.

## 2026-11-19 (fifty-third round) — Code: unprobed capabilities, embedding dimensions, failed streams, empty tokenization

- In step at `3a9611f1`; whitepaper unchanged; no new answers.
- Code step (public-inference-api 2.1–2.4).
  - Readiness fails a model advertising `streaming` without `chat` or `completions`, or `tools`/`structured_output` without `chat`.
  - Readiness records the native embedding dimension from its probes (float and base64 must agree), keeps it on a busy re-check, and clears it when the model stops being ready. Nothing is signed or listed. `infer` refuses an embedding request whose `dimensions` differs from it, or names any while none is recorded, as 400 `unsupported_parameter` before tokenization and reservation. Before, such a request reached the engine, failed as 502 `engine_error` and was settled as unknown usage at the provider's loss.
  - A failed stream whose error event cannot be queued, because the client stopped reading, now ends with a body error instead of a clean chunked end. Successful streams still end with `[DONE]`.
  - An input the engine tokenizes to nothing is 400 `unsupported_parameter` instead of `context_length_exceeded`.
  - The review added two regressions for untested behaviours: a busy re-check keeping the dimension, and qualified streaming models staying eligible.
  - Regressions fail without their changes.
- Changed public answers: (a) empty tokenization and (b) unproducible dimensions now get 400 `unsupported_parameter`; (c) a stream failing behind a stalled client ends abnormally.
- Recorded follow-up task 2.5: the operator sees only a generic health detail for a capability set that can never become ready.
- Checks: full headless suite under `caffeinate` 555 passed, 2 ignored, 0 failed across 26 targets; `cargo test --lib desktop::` (desktop feature) 4 passed; `npm test` 70/70; `npm run check` 0 errors; strict validation passes.

## 2026-11-19 (fifty-second round) — Spec: the public OpenAI-compatible API and per-capability qualification

- In step at `80334be3`; whitepaper unchanged; no new answers.
- Specification step (comparison item 13, book lines 579–601). New change `specify-public-inference-api`: 12 requirements and 37 scenarios. They cover:
  - the endpoint set (no engine or out-of-scope route);
  - both modes for stock clients;
  - credential-scoped listing;
  - capability-bounded admission;
  - body bounds;
  - honest failures;
  - GGUF derivation;
  - readiness probing every advertised capability;
  - exact echo without invented prompt probabilities;
  - matching embedding dimensions;
  - tools and schemas as data that is never executed;
  - whole-model text inference.
  Credentials, request accounting, performance, the engine launch, offer pins and outbound HTTP are referenced from their owners.
- The review:
  - removed a listing rule the book does not state;
  - extended the identity and context checks to operator-owned engines;
  - found a fourth code difference;
  - rewrote task 2.3's regression, which would have passed without the fix.
- Four code differences recorded as tasks 2.1–2.4:
  - unprobed `streaming` on an embedding deployment;
  - embedding dimensions refused only after dispatch;
  - a failed stream that ends cleanly when the client stops reading;
  - an empty tokenization reported as context overflow.
- Report entry with four nonblocking questions.
- Checks: `openspec validate specify-public-inference-api --strict` passes; no code changed. The review reran 42 library tests, 3 integration tests and 46 Python tests behind the coverage table, and all passed.

## 2026-11-19 (fifty-first round) — Code: credential options only as an object; issuance keeps a valid hidden default

- In step at `984513a9`; whitepaper unchanged; no new answers.
- Code step (account-credentials 2.1 and 2.2).
  - `CredentialIssueRequest` now decodes only from one JSON object through `protocol::deserialize_object`. Positional arrays get 400 `invalid_request` on the issuance and rotation routes; natively they get "invalid credential options" before any provider request. The type is never persisted, so no stored data is affected.
  - Native `credential_issue` no longer treats a default missing from the provider's 100-record list as invalid. It probes the stored bearer on its own provider's `GET /v1/models` over the pinned transport. A model list keeps the default; only a definite 401 `invalid_api_key` or 403 `credential_scope` clears it. Any other answer or a transport failure fails the issuance with the account unchanged.
  - `/v1/models` authenticates before any serving gate, creates no request or reservation, and answers an empty list when the provider admits nothing, so requirement 6 holds (tested with every gate closed).
  - The review found that two claimed probe behaviours were untested and added a 403 control and three ambiguous answers.
  - Nine regressions fail without their changes.
- Checks: full headless suite under `caffeinate` 548 passed, 2 ignored, 0 failed across 26 targets; `cargo test --lib desktop::` (desktop feature) 4 passed; `npm test` 70/70; strict validation passes.

## 2026-11-19 (fiftieth round) — Spec: account credential issuance and secret storage

- In step at `572cd32d`; whitepaper unchanged; no new answers.
- Specification step (comparison item 12, book lines 325–360). New change `specify-account-credentials`: 9 requirements and 23 scenarios. They cover:
  - the four-member options object;
  - hash-only provider storage with a separate public identifier;
  - bearer scope and lifetime on the inference routes;
  - revocation and rotation of one named credential;
  - caps as limits, never credit;
  - management reachable without new admission;
  - the protected native store;
  - metadata-only output except explicit export (including the SDK examples' in-process key);
  - default selection on rotation.
  Owner proof, request holds, `base_url` and webview handling are referenced from their owners.
- The review moved non-book details (Unix-seconds encoding, response members, exact modes, export IDs) into design decision 4. It removed restatements of owner-control and request-accounting rules, and checked eight candidate defects against the code, rejecting each with evidence.
- Two code differences recorded as tasks 2.1 (options decode from a positional array) and 2.2 (explicit issuance can replace a valid default outside the 100-record list).
- Report entry with three nonblocking questions: omitted spend cap, rotation restoring a missing default, listing completeness.
- Checks: `openspec validate specify-account-credentials --strict` passes; no code changed. The review reran the 24 cited tests and all passed.

## 2026-11-19 (forty-ninth round) — Code: development `localhost` resolves only to loopback; export validates the origin

- In step at `033168d6`; whitepaper unchanged; no new answers.
- Code step (outbound-http-safeguards 2.1 and 2.2).
  - `safe_http_client`'s answer-set decision is now the pure `offer_registry::check_resolved_answers`. Under the development `localhost` exception, every answer must be loopback (message suggests `127.0.0.1` or `::1`). Other origins keep the empty, more-than-32 and non-public rules and their message.
  - `credential_export` validates the saved offer origin under the current mode before selecting a credential or building `base_url`. On refusal it returns the validator's error, the account file is unchanged, and `load_account` stays lenient for recovery.
  - The review added IPv4-mapped and 32/33-answer cases to the regression and corrected wording about when secrets are read.
  - Three regressions fail without their changes.
  - Not tested: that `safe_http_client` calls the function. There is no resolver seam yet; task 3.4 plans one.
- Checks: full headless suite under `caffeinate` 540 passed, 2 ignored, 0 failed across 26 targets; `cargo test --lib desktop::` (desktop feature) 4 passed; strict validation passes.

## 2026-11-19 (forty-eighth round) — Spec: outbound HTTP safeguards and the private engine origin boundary

- In step at `e39f954a`; whitepaper unchanged; no new answers.
- Specification step (comparison item 11, book lines 262–279). New change `specify-outbound-http-safeguards`: 8 requirements and 29 scenarios. They cover origin validation, the development exception, origins as advertised endpoints, the per-origin DNS-pinned client with no proxies or redirects, the exact public-address predicate, the loopback administration listener, the private engine origin, and TLS ingress. Overlapping rules are referenced, not restated: commercial-terms advertised origin, managed-engine launch, offer discovery, funding-settlement chain RPC, version policy and reputation.
- The review removed statements stronger than the book (funding/listener timing, HTTP 401, literal-IP admin client, double engine check, a misquoted firewall sentence) into design decision 10. It confirmed `safe_http_client` matches line 268 exactly and listed every `reqwest` client.
- Two code differences recorded as tasks 2.1 (a development `localhost` origin accepts non-loopback DNS answers) and 2.2 (credential export derives `base_url` without the origin validator). Acceptance tasks 3.x cite existing tests and name the gaps.
- Report entry with two nonblocking questions: should chain RPC ignore environment proxies, and does the origin rule govern `policy_url`?
- Checks: `openspec validate specify-outbound-http-safeguards --strict` passes. No code changed; the focused tests behind the coverage table passed when run on 2026-11-19 (task 1.3).

## 2026-11-19 (forty-seventh round) — Code: pin the `policy_url` query refusal (version-policy 2.6)

- In step at `113259e8`; whitepaper unchanged; no new answers.
- Code step (version-policy 2.6). No product change was needed. Round 46 recorded that `policy_url` accepts a query, but that was a false positive: `network::validate_endpoint`, which runs for every endpoint when a profile loads, already refuses credentials, queries and fragments, so `Consumer::new` fails before any request. A first draft added a redundant query check to `version_status`; it was removed when the new test showed the load-time refusal. The regression `policy_url_with_a_query_is_refused_at_profile_load_before_any_request` pins the refusal and fails when the query check is removed from `validate_endpoint`.
- Checks: `--lib` 275 passed, 2 ignored; `--bins` builds; `npm run check` 0 errors; `npm test` 70/70; `npm run build` ok; Python compile and unit tests ok. The full integration suite was not re-run: the only change is a `#[cfg(test)]` unit test, which integration targets and binaries do not compile, and round 46's full run (536 passed) covers the same non-test code byte for byte.

## 2026-11-19 (forty-sixth round) — Code: catalog, policy-source and snapshot failures no longer blame the provider

- In step at `bd4b6f80`; whitepaper unchanged; no new answers.
- Code step (reputation 2.5, offer-discovery 2.5, version-policy 2.5; book line 443).
  - The relay's snapshot path now reports store and clock faults with a catalog message, with detail only in operator logs.
  - The consumer decodes `/api/offers` pages with an offer-catalog table, and `policy_url` responses with a policy-source table, sharing one `source_bytes` helper with the reputation reads. Provider endpoints keep the provider table (the review classified every decode in `consumer.rs`).
  - Fail-closed version behaviour and the discovery pagination bounds are unchanged.
  - Three new regressions fail without their changes. Displayed text changes on Marketplace discovery errors and funding, top-up and inference policy errors; no layout changed.
- Checks: full headless suite under `caffeinate` 536 passed, 2 ignored, 0 failed across 26 targets.
- Recorded from the review as unchecked version-policy task 2.6: `policy_url` accepts a query string, which requirement 9 forbids.

## 2026-11-19 (forty-fifth round) — Failing check: frozen mock chain timestamps made owner-control tests fail after host sleep

- In step at `a8ac7774`; whitepaper unchanged; no new answers.
- Failing-check step. `native_topup_snapshots_preserve_pending_originals_after_committed_credit` (rounds 42 and 44) and round 42's opening-matrix failures had one cause. The mock chain servers `TopupRecoveryRpc` and `SafeOpeningRpc` in `inference_owner_control_test.rs` returned the `latest` and `safe` blocks with a timestamp frozen at each case's start. The product correctly refuses a head older than 120 s or a safe block older than 300 s (`chain_client.rs:617–621`). Once enough wall time passed inside a case, both clients got "eSpace head or safe frontier is stale" and the top-up or opening never reached the provider.
  - The trigger was macOS sleep, not CPU load: the wall clock keeps running while test timers stop. `pmset` logs show clamshell sleeps in both failing runs, and the passing rerun had none.
  - Reproduced deterministically: with the frozen fixture and 125 s of elapsed or backdated time, the same panics occur (lines 5689/5530 and 5371/5200); with the fix they pass.
  - Fix (test fixture only, no product code, no assertion loosened): `latest` and `safe` now report `max(now, stored time)` on every read, and fixed historical blocks keep their stored time.
- A sleep of several minutes can still exceed legitimate product limits (120 s owner challenges, 300 s quotes). Long suites now run under `caffeinate -dims`, and a failure that coincides with a `pmset` sleep entry is treated as an invalid run.
- Checks: full headless suite under `caffeinate -dims` on AC power: 533 passed, 2 ignored, 0 failed across all 26 targets (`inference_owner_control_test` 60/60, 2,090 s; wall time 40.5 min, so no sleep). This also covers the targets that did not run after round 44's failure.

## 2026-11-18 (forty-fourth round) — Code: feedback errors are classified and reported as the catalog's

- In step at `f436797a`; whitepaper unchanged; no new answers.
- Code step (`specify-payment-gated-reputation` tasks 2.1–2.3; comparison item 6).
  - 2.1: feedback submission now reads raw bytes and decodes them strictly. Any content-type, syntax, trailing-content, schema, out-of-range score or zero revision, or unreadable-body failure gets 400 `invalid_feedback` JSON before a verification slot is taken. 413 stays. The review added the range check and the unreadable-body case.
  - 2.2: the store distinguishes typed refusals (`feedback_conflict` with fixed text) from faults (500 with a catalog message, detail only in logs). A reused payment is found by lookup, not constraint text.
  - 2.3: the native client decodes reputation-catalog responses with a catalog code table, so a catalog or RPC outage no longer reads as a provider failure (book line 443). Provider responses are unchanged.
  - Twelve new regressions fail without their changes. The displayed text on Marketplace and Usage changes; no layout changed.
- Checks: full headless suite 457 passed, 2 ignored, and 1 failed: `inference_owner_control_test::native_topup_snapshots_preserve_pending_originals_after_committed_credit` panicked at line 5689 ("must reach successful top-up"). It passes alone (2,057 s), and it also failed in round 42's first full run, before this round's `consumer.rs` change, so it is an intermittent failure under full-suite load, not caused by this change. Because cargo stops after a failing target, the targets after `inference_owner_control_test` did not run in this suite; they ran and passed in round 42. The next round investigates it, since a failing check ranks first.
- Follow-ups found in review and recorded as unchecked tasks: reputation 2.5 (snapshot-path faults still blame the provider), offer-discovery 2.5 (catalog page errors decoded as provider errors) and version-policy 2.5 (policy-source errors decoded as provider errors).

## 2026-11-18 (forty-third round) — Payment-gated reputation specified

- In step at `1b514eb5`; whitepaper unchanged; no new answers.
- Spec step (comparison item 10, missing specification; item 6 recorded). New change `specify-payment-gated-reputation` (capability `payment-gated-reputation`, 12 requirements, 60 scenarios) for the current runtime's experimental, catalog-specific reputation (book lines 361–446). It covers:
  - the relay serves no prompts or inference;
  - an exact submission shape, with every malformed body `invalid_feedback`;
  - payer-signed votes bound to their quote, parties and network;
  - no free-session votes;
  - exact canonical opening payments at the eSpace safe frontier;
  - one vote per agreement, with strictly increasing revisions;
  - a durable, bounded store;
  - an exact Elo computation;
  - snapshots that revalidate evidence and never serve a score from unavailable evidence;
  - bounded verification;
  - native submission and inspection.
  M5, funding-settlement and GUI-pages requirements are cited by name. 16 cited tests re-run and pass. Strict validation passes; no code changed.
- Code differences recorded as unchecked tasks:
  - 2.1 (comparison item 6): malformed or trailing-content bodies get axum's plain-text 415/400/422, or are accepted, instead of 400 `invalid_feedback`;
  - 2.2: store faults are reported as `feedback_conflict` and leak SQLite text;
  - 2.3: the native client maps every catalog code to a provider error, blaming the provider for catalog or RPC outages;
  - 2.4: untrustworthy RPC evidence is treated as a rejected payment and can mark votes ineligible (the rating-path counterpart of funding-settlement 2.4).
- Report: can a provider pre-empt its consumer's vote by reusing a quote ID (default: book text, disclosed), and must every snapshot revalidate every vote within 30 s, when the chain client's 300 ms pacing allows about 12 votes (default: as today)?

## 2026-11-18 (forty-second round) — Code: separate engine key, no orphaned engine on a startup signal, wildcard port probe

- In step at `9d77b6cd`; whitepaper unchanged; no new answers.
- Code step (`specify-managed-engine-launch` tasks 2.1–2.3).
  - 2.1 (security, book line 474): a new `ProviderConfig::verify_separate_engine_keys` runs at start and in `--check`, before anything starts. It refuses an engine key file that is, or repeats, the signing key in any spelling (`0x`/`0X`, any case, leading zeros added or removed — the review found the leading-zero bypass), the admin token or the RPC bearer. Comparisons are constant-time, and errors never echo a value.
  - 2.3: the daemon now installs its SIGTERM and SIGINT handlers (Ctrl-C on Windows) before starting the runtime and races startup against them. A signal during engine startup drops the start future, which kills started engines, and exits nonzero instead of dying by default action and orphaning the engine on macOS.
  - 2.2: a non-listening wildcard probe, bound before the endpoint, now refuses a wildcard listener on the engine port while a port holding only `TIME_WAIT` connections stays usable. IPv6 uses `V6ONLY`; Windows skips `SO_REUSEADDR` (left to native task 4.1).
  - New regressions fail without their changes, in `app.rs`, `engine_runtime_tests.rs` and a new process target `tests/daemon_engine_process_test.rs`. That target has its own harness (`harness = false` in `Cargo.toml`) because a copy of the test binary stands in for `llama-server`.
- Behaviour changes for operators: an unreadable `rpc_bearer_file` or external engine key file now refuses start (fail closed); on Linux, a listener on any other specific IPv4 address on the engine port is refused.
- Verified on macOS only; Linux and Windows socket and signal behaviour stay in task 4.1.
- Checks: full headless suite 522 passed, 2 ignored (`inference_owner_control_test` 60/60 in 2,105 s). The first full run had 19 failures in `inference_owner_control_test` after only 88 s (SQLite and fixture `unwrap` panics). The three failed tests I ran individually passed, no stub engine or daemon was left over, and a complete rerun passed. The cause was not identified: watch for a recurrence, especially right after the new `daemon_engine_process_test` target.

## 2026-11-18 (forty-first round) — Managed engine launch specified

- In step at `fae2ea6d`; whitepaper unchanged; no new answers.
- Spec step (comparison item 8, missing specification): new change `specify-managed-engine-launch` (capability `managed-engine-runtime`, 11 requirements, 48 scenarios) for the current provider daemon's managed llama.cpp launch (book lines 69–103 and 605–680). It covers:
  - only the pinned `llama-server` as a supervised child;
  - inventories verified before every launch;
  - explicit, bounded settings that are refused rather than adjusted;
  - a loopback-only engine, with an occupied port refused;
  - the serving contract passed as explicit arguments;
  - a minimal environment, with the API key only as a protected file path;
  - independent device observation and headroom before and after loading;
  - no contract reduction;
  - bounded startup, and an engine exit withdrawing service with no restart;
  - shutdown stopping the children.
  Commercial-terms, generation-performance and offer-discovery requirements are cited by name. Strict validation passes; no code changed.
- Code differences recorded as unchecked tasks:
  - 2.1: the engine API key file is never checked against the signing key, admin token or RPC bearer, so a provider pointed at its wallet key file would hand the wallet key to the engine as its bearer;
  - 2.2: on macOS, the occupied-port probe misses a wildcard listener;
  - 2.3: a SIGTERM or Ctrl-C during engine startup kills the daemon before its handlers exist, orphaning the engine on macOS.
- Missing launch tests are acceptance tasks 3.x.
- Report: what "before startup" requires for fit (default: free memory ≥ reserve, then a post-load check, as today), and what headroom applies to a CPU managed runtime outside development (default: no capacity claim, as today).

## 2026-11-18 (fortieth round) — Spec and code: the chain RPC gateway always requires its bearer; single-label origins refused

- In step at `43a5dff0`; whitepaper unchanged; no new answers.
- Step: comparison items 7 and 5.
  - Item 7 (security). `chiral_rpc_gateway` served every request without authentication when bound to loopback without `--bearer-file`. Behind the generated Caddy ingress, that would expose `eth_sendRawTransaction` with the operator's upstream bearer (book lines 644, 656 and 667).
    - New change `specify-chain-rpc-gateway` (8 requirements, 25 scenarios).
    - The gateway now requires a protected bearer file on every bind address, of at least 32 visible ASCII characters and distinct from the upstream bearer. A 401 carries `no-store`. The allowlist is exactly ten `eth_*` methods (`net_version` removed; the Verus companion was narrowed in step, but its proof was not re-run).
    - Preflight now requires an unauthenticated POST to get 401, and provisioning refuses equal inbound and upstream bearers.
  - Item 5. Origin validation now classifies hostnames by their nonempty labels, so `https://intranet./` and `https://.intranet/` are refused. The review found the leading-dot form. A scenario and task were added to commercial-terms *Offer freshness and advertised origin*.
  - New tests fail without their changes: 3 gateway process tests, an allowlist test, an origin test and 6 script tests.
- Report: should the relay's own loopback RPC route (`relay_server.rs:232`, no bearer) be removed, bearer-protected or kept? And may lines 644 and 667 state the bearer rule and the ten methods (excluding `net_version`)?

## 2026-11-18 (thirty-ninth round) — Code: snapshot-safe manifest withdrawal, clean failed refreshes, a protected DHT identity

- In step at `b416c157`; whitepaper unchanged; no new answers.
- Code step (`specify-offer-discovery` tasks 2.1–2.3; comparison item 4).
  - Manifest pages now follow a stable namespace order, and a receiver withdraws cached offers only for a complete single-snapshot manifest. A refresh between page fetches, or a manifest that shrank between pages, can no longer withdraw a live offer (book line 234).
  - A failed refresh now withdraws the offers it had already published.
  - The review found that a failed DHT advertisement could leave an offer served, and that a partly successful index sync forgot keys it had started. Both are fixed: a failed publication now serves and advertises nothing.
  - The DHT identity file is created and read as a protected secret (symlinks, exposed modes, other owners and Windows inherited access lists are refused, never rewritten).
  - Nine new regressions fail without their changes (the Windows one runs only in CI).
- Recorded as unchecked task 2.4 and a design risk: remote provider records share this node's 128-key Kademlia store, so other peers can fill it and block a provider's new index advertisements until their records expire (600 s). This needs a design choice.
- Behaviour change for operators: identity files with inherited Windows access lists, or owned by another user, are now refused at startup with instructions to protect them in place.

## 2026-11-18 (thirty-eighth round) — Current offer discovery specified

- In step at `ff0d3935`; whitepaper unchanged; no new answers.
- Spec step (comparison items 9, 15 and 17, missing specification; item 4's code fix needs a home). New change `specify-offer-discovery` (capability `inference-offer-discovery`, 16 requirements, 62 scenarios) for the current runtime's discovery (book lines 29, 59 and 211–261). It covers:
  - candidate offers, not a census;
  - network-scoped indexes under a stable, protected peer identity;
  - admission-bounded connections;
  - the paged `/chiral/inference/manifest/1` protocol in stable namespace order;
  - bounded searches;
  - the 8,192-entry registry;
  - monotonic revisions with rollback and equivocation rejected;
  - withdrawal keeping its watermark for 3,630 s;
  - withdrawal only on a complete, single-snapshot manifest;
  - provider publication (30 s refresh, 120 s offers);
  - fresh-registry consumer discovery with DHT precedence;
  - the relay catalog and its client;
  - liveness and readiness;
  - native inspection.
  M3 and commercial-terms requirements are cited by name. The cited tests pass (14 library and 5 integration). Strict validation passes; no code changed.
- Code differences recorded as unchecked tasks:
  - 2.1 (comparison item 4): a manifest built from pages of different snapshots can withdraw a live offer;
  - 2.2: a failed refresh leaves the offers it published earlier in that pass served;
  - 2.3: the DHT identity file is not handled as a protected secret (no owner or symlink check; unchecked on Windows).
- Report: is a provider whose every probe-ready model is performance-withdrawn "ready" (default: yes, provider-wide, as today)?

## 2026-11-18 (thirty-seventh round) — Code: a qualified profile needs a managed runtime

- In step at `eb0b75e9`; whitepaper unchanged; no new answers.
- Step: comparison item 3, a code difference (book lines 73, 609 and 615–617; commercial-terms *Native deployment constraints cannot be bypassed by declaration validity*). A non-development provider could publish a qualified generation profile from an operator-owned external engine. Readiness checked only the evidence file and a few `/props` fields, so evidence measured on one runtime could be reused on other hardware, another backend or another OS. `ProviderConfig::validate` now refuses a serving profile without a managed runtime outside development; development calibration still may use one. Spec: a SHALL and a scenario in that requirement, plus task 2.6. The new regression fails without the check.
- Report: may a priced, unprofiled embedding deployment use an external engine (book 609 versus 656)? Default: yes, as today.

## 2026-11-18 (thirty-sixth round) — Spec and code: consumer profile paths resolve next to the profile

- In step at `43d93e71`; whitepaper unchanged; no new answers.
- Step: comparison item 2, the high-severity one (book lines 449 and 470).
  - Spec: new change `specify-profile-path-resolution` (5 requirements, 15 scenarios).
  - Code: `Consumer::load` used to resolve a relative `data_dir`, RPC bearer file and DHT identity file against the working directory, so one profile run from two folders used two wallets and journals. It now resolves them against the profile's directory, as the provider, relay and gateway already did.
  - A profile whose state exists only at the old working-directory location is refused, naming both paths and both remedies (an absolute `data_dir`, or a deliberate move), and nothing is moved. The review found that an empty scaffold made by a read-only command could hide an old wallet; the check now also compares the wallet key.
  - Six binary-level regressions and a library test fail without the change.
- Also: my round-34 test hook `version::set_effective_policy_for_test` caused a dead-code warning in headless test builds; it is now compiled only with the desktop feature.
- Report: should line 449 name the relay and RPC-gateway configurations and the command-line exception, and should a client warn when both locations hold state (defaults: yes to the wording, no warning).

## 2026-11-18 (thirty-fifth round) — Comparison pass: 19 confirmed findings recorded

- In step at `a5aa2b88`; whitepaper unchanged; no new answers.
- With no known gap left, a new comparison ran over the whole book outside Appendix A and the target-design milestones: five slices, each with one finder and one adversarial verifier. 20 findings, 19 confirmed, recorded in `docs/apas-worker/comparison-2026-11-18.md` in priority order. They comprise one spec-versus-whitepaper wording item, six code differences (one high: consumer profile paths resolve against the working directory) and ten missing specifications.
- This round's step (highest priority, spec contradicts whitepaper): book line 483 says the opening transaction carries "intrinsic gas", while the spec and code correctly use the eSpace estimate within the structural minimum and 1,000,000. The spec is right and the whitepaper is read-only, so a report entry proposes new book wording. No spec or code change.

## 2026-11-18 (thirty-fourth round) — Code: no update status from another network survives a logout or profile change

- In step at `e63b9737`; whitepaper unchanged; no new answers.
- Code step (`specify-version-policy` task 2.4, requirement 13; book A.1.8 and A.2). Logout left the process-wide effective policy in place. `get_version_status` without a profile kept reporting the previous network's status, and the webview kept its last status after a failed refresh, so a banner from the previous network stayed visible.
  - `version::reset_effective_policy` and a desktop `end_session` now clear the effective policy on logout.
  - The webview's `resetVersionStatus()` runs on logout, on quit, and before a profile load's refresh.
  - Regressions fail without the change: a desktop-feature Rust test and two rendered webview tests. Supporting tests cover the reset itself. Screenshot checked.
- Every code difference in `specify-version-policy` is now fixed; its author questions, acceptance tasks 3.x and release evidence 4.x remain.

## 2026-11-18 (thirty-third round) — Code: the required-update modal blocks the keyboard, and only HTTPS download URLs are offered

- In step at `fe9b6098`; whitepaper unchanged; no new answers.
- Code step (`specify-version-policy` tasks 2.2 and 2.3, requirement 13; book A.2 line 846).
  - The required-update modal blocked the pointer only. The navigation, page and Log out stayed reachable by keyboard and assistive technology. One `shellBlocked` flag in `App.svelte` now makes the skip link, navigation and `main` inert, and the modal takes focus and keeps Tab inside it. The tray's Quit dialog stays usable in the top layer.
  - *Copy download URL* and the banner's *Update* were offered for any URL. They now appear only for an HTTPS URL without credentials; otherwise the gate says the policy gives no HTTPS download URL.
  - New `tests/updateGate.test.ts`: four tests fail on the previous components, and two guard the other cases. Headless Chrome confirmed the inert shell and that the navbar still sticks; screenshots checked.
- Checks: svelte-check 0/0, 67 frontend tests, build. Rust is unchanged since the full headless suite passed at `fe9b6098` (493 passed, 2 ignored).

## 2026-11-18 (thirty-second round) — Code: the policy authority's key stays in a protected file

- In step at `7dcb5887`; whitepaper unchanged; no new answers.
- Code step (`specify-version-policy` task 2.1, requirement 8; book lines 474 and 648). `chiral-policy-sign sign` took the authority's private key as `--key <hex>` or from `CHIRAL_POLICY_SECRET`, and `keygen` printed `secret=<hex>`. Now `keygen --out <path>` writes the key to a new owner-only file (an existing path is refused) and prints only the public key as JSON. `sign --key-file <path>` reads it through `wallet::open_secret_file`, which refuses symlinks and group- or world-accessible files. A new process-level test, `policy_sign_cli_test.rs`, fails on the previous binary. The only documentation reference (`docs/provider-qualification.md`) uses `verify`, which is unchanged.

## 2026-11-18 (thirty-first round) — Client version policy specified

- In step at `df2240bf`; whitepaper unchanged; no new answers.
- Spec step: new change `specify-version-policy` (capability `client-version-policy`, 14 requirements, 47 scenarios) for the book's version policy (lines 646–652) and the A.2 update gate (846). It covers:
  - a non-placeholder release authority and a once-per-process override;
  - authentic, current policies and the required, recommended and ok statuses;
  - the durable, protected rollback watermark scoped to authority and network, kept through expiry, with fail-closed caches;
  - the placeholder authority for development only, and an offline signing key;
  - configured policy sources and the provider's client-version header check;
  - what a required update refuses (fund, top-up and new inference at minimum), provider self-health, the desktop gate, and preservation across upgrades.
  The cited tests pass (34 `version::`, 2 `network::`, 61 frontend). Strict validation passes; no code changed.
- Code differences recorded as unchecked tasks:
  - 2.1: `chiral-policy-sign` takes the authority's private key on argv or from the environment and prints it on keygen;
  - 2.2: the required-update modal does not trap focus or make the shell inert;
  - 2.3: *Copy download URL* is offered for non-HTTPS URLs;
  - 2.4: the previous network's update status stays visible after logout or a failed check.
- Report: which operations stay available under a required update, including a corrupt policy cache (default: recovery only), and whether an unreachable `policy_url` fails closed (default: yes, as today).

## 2026-11-18 (thirtieth round) — Code: the Settings profile state is stated in text and a load is announced

- In step at `b5a6a5f4`; whitepaper unchanged; no new answers.
- Code step (`specify-desktop-gui-pages` tasks 2.3 and 2.4, requirement 13).
  - The Settings *Network profile* entry showed the profile state only as a coloured dot with a tooltip. It now carries visually hidden text.
  - A successful profile load set a page-local notice that the session remount discarded, so it was never shown. The confirmation is now an app-level success notification (`role="status"`) that survives the remount.
  - New `tests/settings.test.ts`, including the first test that mounts the real `App` shell (Tauri window and event modules stubbed): both tests fail on the previous page. Screenshot checked.
- Every code difference in `specify-desktop-gui-pages` is now fixed; its acceptance tasks 3.x and native verification 4.1 remain.
- Checks: svelte-check 0/0, 61 frontend tests, build. Rust is unchanged since `2e346ae4`.

## 2026-11-18 (twenty-ninth round) — Code: credential lifetime enforced and the revealed key copyable while masked

- In step at `128b3db6`; whitepaper unchanged; no new answers.
- Code step (`specify-desktop-gui-pages` tasks 2.1 and 2.2, requirement 6, book lines 808 and 930).
  - The lifetime field's bounds were never checked, because the buttons sat outside a form, so 5 s or 60-day credentials reached `credential_issue`. Issue and rotate now refuse anything but a whole number of seconds from 60 to 2,592,000 before any native call.
  - The revealed key sat in a password field that Chromium webviews will not copy. A *Copy key* action now copies it while it stays masked, and reports success or failure in live regions.
  - New `tests/credentials.test.ts`: three tests fail on the previous page, and one guards the default options. Screenshot checked.
- Checks: svelte-check 0/0, 59 frontend tests, build. Rust is unchanged since `2e346ae4`.

## 2026-11-18 (twenty-eighth round) — Desktop GUI page behaviour specified

- In step at `a7d36fa4`; whitepaper unchanged; no new answers.
- Spec step: new change `specify-desktop-gui-pages` (capability `desktop-gui-pages`, 15 requirements, 45 scenarios) for Appendix A.5's required page behaviour. It covers the native operation behind each page action; Marketplace order and gating; offer and serving terms before a quote; Playground limits; Usage accounts, top-up, credentials, request reconciliation, funding recovery and feedback; Wallet file-path key handling with an owner-only backup; Provider loopback administration through a token file; Account, Network, Settings and Diagnostics; and a rule that presentation may change while required elements stay reachable at every width. A.1 rules, funding-settlement, fee-remittance and performance requirements are cited by name. Cited tests pass (55 frontend, 3 Rust). Strict validation passes; no code changed.
- Code differences recorded as unchecked tasks:
  - 2.1: the credential lifetime (60 s–30 days) is not enforced before `credential_issue`;
  - 2.2: the masked API key cannot be copied in Chromium webviews;
  - 2.3: the Settings profile indicator is colour-only;
  - 2.4: a successful profile load is never announced, because the remount discards the notice.
- The review could not reproduce the 390 px Usage overflow in headless Chrome: the tab list scrolls inside its own container. It is recorded as an observation for native task 4.1. No author question.

## 2026-11-18 (twenty-seventh round) — Code: the webview loads nothing from a remote origin

- In step at `cbd3b273`; whitepaper unchanged; no new answers.
- Code step (`specify-desktop-gui-global-rules` task 2.8, requirement 1). `index.html` used to preconnect to, and load a stylesheet from, Google Fonts; the production CSP already blocked the stylesheet. The links are removed. Roboto stays first in the local font stack, which matches the native app's existing rendering; it is not bundled, because this loop may not download a font.
  - With the system font, the browser preview showed that the top bar overflowed by 10 px at exactly 1440 px, as the native app already did. The navbar container now widens to A.3's 1,400 px page frame from 1440 px; there is no overflow at 1280, 1440 or 1536 px.
  - New `tests/noRemoteResources.test.ts` fails on the previous `index.html`. The preview's resource timing shows no request to another origin.
- Every code difference in `specify-desktop-gui-global-rules` is now fixed; its acceptance tasks 3.x and native verification 4.1 remain.
- Checks: svelte-check 0/0, 55 frontend tests, build. Rust is unchanged since `2e346ae4`.

## 2026-11-18 (twenty-sixth round) — Code: accessibility labels, live-region roles and reduced motion

- In step at `a114e665`; whitepaper unchanged; no new answers.
- Code step (`specify-desktop-gui-global-rules` tasks 2.4–2.6, requirement 10, A.1.9).
  - The Usage feedback score field is now labelled.
  - Notifications use `alert` only for errors and warnings, and `status` otherwise. The no-network banner, the Usage funding notices and the Wallet copy confirmation are status messages.
  - A shared `src/lib/motion.ts` combines the system and in-app reduced-motion preferences. The toast transition and the Marketplace scroll respect it; CSS could not reach these script-driven motions.
  - New `tests/accessibility.test.ts`: five behaviour tests fail on the previous components, including one that visits every Usage tab and checks that every field is labelled. A screenshot confirms the feedback form layout is unchanged.
- Checks: svelte-check 0/0, 52 frontend tests, build. Rust is unchanged since `2e346ae4`.

## 2026-11-18 (twenty-fifth round) — Code: canonical wei display and an honest stream-cancel boundary

- In step at `c2392ec7`; whitepaper unchanged; no new answers.
- Code step (`specify-desktop-gui-global-rules` tasks 2.9 and 2.7, requirements 5 and 3), done together because both are `src/lib/native.ts` helpers.
  - `formatCfx` used to display any digit string ("00", "01", 2^128) as an amount. A new `isCanonicalWei` accepts only canonical wei up to 2^128 − 1, and `formatCfx` and the Marketplace lowest-price filter use it.
  - `cancelStream` threw a raw TypeError outside the native runtime; it now refuses with an explanation, like `command` and `stream`.
  - The two extended `tests/native.test.ts` cases fail on the previous helpers.
- Checks: svelte-check 0/0, 46 frontend tests, build. Rust is unchanged since `2e346ae4`.

## 2026-11-18 (twenty-fourth round) — Code: request failures point to recovery and cancellation is labelled possibly billed

- In step at `77914a4b`; whitepaper unchanged; no new answers.
- Code step (`specify-desktop-gui-global-rules` tasks 2.10 and 2.11, requirement 8, book lines 355, 913 and 931).
  - A Playground stream that failed before a request ID was labelled "inspect request status", but there was no request ID to inspect. It is now "Failed — inspect Usage before retrying", with the native error and a note linking to Usage › Requests, where the local request keys are listed. A failure after a request ID keeps *Reconcile*. Nothing is resent.
  - Usage's *Cancel provider request* now states that cancelling does not make generated work free, that the request may still be billed, that cancellation is not a refund, and that *Read status* shows the final accounting.
  - New rendered tests in `tests/requestRecovery.test.ts`: two fail on the previous pages, and one guards the unchanged *Reconcile* path. Screenshots were checked.
- Checks: svelte-check 0/0, 45 frontend tests, build. Rust is unchanged since `2e346ae4`.

## 2026-11-18 (twenty-third round) — Code: profile-dependent actions wait for a loaded profile

- In step at `2e346ae4`; whitepaper unchanged; no new answers.
- Code step (`specify-desktop-gui-global-rules` task 2.2, requirement 4, A.1.3). With no profile loaded, Usage and Playground used to read the native journal on open, and Usage reported "No accounts in this network's local journal". Usage's Refresh and recovery actions, Playground's Refresh accounts and Diagnostics' Collect were enabled; native code refused them. They now read nothing until a profile is loaded, the actions are disabled, and Usage points to Settings. The Diagnostics runtime tile stays. A new rendered test, `tests/profileGate.test.ts`, fails on the previous pages. Screenshots were checked.
- Checks: svelte-check 0/0, 42 frontend tests, build. Rust is unchanged since the full headless suite passed at `2e346ae4` (491 passed, 2 ignored).

## 2026-11-18 (twenty-second round) — Code: the Usage top-up card shows every A.1.6 item

- In step at `c365d384`; whitepaper unchanged; no new answers.
- Code step (`specify-desktop-gui-global-rules` task 2.1, requirement 7). The top-up card used to omit the endpoint, plaintext prompts, the statement that signed metadata does not prove execution, and the fee rate. The webview's account data cannot carry the endpoint or rate, and `AccountView` mirrors the provider wire format. A new read-only native operation `agreement` returns only public terms from the saved signed offer and quote. The card now states every A.1.6 item, with the fee rate exact ("0.5% platform fee (50 basis points)"), and no top-up is offered until those terms are read. The acceptance covers them and resets on account change. A Rust test and two rendered regressions fail without the change. The tests also caught Svelte trimming spaces at `{#if}` boundaries, which is fixed. Screenshots were checked at 1440 px and 390 px.
- Observation, not caused by this change: at 390 px the Usage tab strip makes the page 415 px wide; it does so on the previous page too. This is A.2/A.3 presentation, to consider with the A.5 specification.

## 2026-11-18 (twenty-first round) — Code: no new payment is offered while a journaled payment is unresolved

- In step at `7be73013`; whitepaper unchanged; no new answers.
- Code step (`specify-desktop-gui-global-rules` tasks 2.3 and 2.12; A.1.7 and requirement 1). The Marketplace used to read the funding journal only after its own fund or resume. After a reopen or restart it offered a new quote and payment beside an unresolved one, and a failed journal read replaced the native error with a fixed phrase.
  - The Marketplace now reads `pending_accounts` whenever a profile is loaded. While any paid operation is unresolved, or the journal cannot be read, it lists each one with its stage, transaction hash and *Resume*, and keeps the quote, acceptance and fund controls disabled.
  - Usage disables every account's top-up while another agreement's payment is unresolved and points to it.
  - The new `src/lib/funding.ts` defines unresolved: any stage except `not_prepared`, `cancelled`, `original_failed`, `cancellation_failed` and `free_open_pending`. A saved agreement stays listed until it opens, so terminal or unpaid entries must not block forever.
  - A failed journal read keeps the native text.
  - Three rendered regressions fail on the previous pages, and a fourth test guards against over-blocking. Screenshots were checked at 1440 px and 390 px.
- Checks: svelte-check 0/0, 36 frontend tests, build, 46 script tests. Rust is unchanged since the full headless suite passed at `221212a7` (490 passed, 2 ignored).

## 2026-11-18 (twentieth round) — Desktop GUI global rules specified

- In step at `221212a7`; whitepaper unchanged; no new answers.
- Spec step: new change `specify-desktop-gui-global-rules` (capability `desktop-gui-global-rules`, 10 requirements, 34 scenarios) for Appendix A.1.1–A.1.9: a thin view over the native boundary, secrets staying native except an explicitly revealed key, no simulation, network actions only after an explicit profile load, exact wei, untrusted content as plain text, the remaining A.1.6 disclosures on every opening and top-up, reconcile instead of resend, session scoping, and accessibility. A.1.10 is cited from `desktop-window-lifecycle`, and the reorganization, registration-risk and path disclosures from the funding-settlement and M3 requirements. The cited frontend tests pass (32). Strict validation passes; no code changed.
- Code differences recorded as unchecked tasks 2.1–2.12. The main ones:
  - 2.3: the Marketplace offers a new quote and payment while another journaled payment is unresolved after a reopen or restart;
  - 2.1: the Usage top-up card lacks the endpoint, plaintext-prompt, metadata and fee-rate statements, which needs native data;
  - 2.2: Usage, Playground and Diagnostics act before a profile is loaded;
  - 2.10–2.11: request-recovery guidance and the may-still-be-billed label are missing;
  - 2.9: `formatCfx` accepts non-canonical wei;
  - 2.4–2.6: accessibility gaps (an unlabelled input, alert and status roles, motion driven by script);
  - 2.8: a remote Google Fonts stylesheet.
  GUI fixes need screenshots.

## 2026-11-18 (nineteenth round) — Code: the operator fee view shows the book's states, actual remittances and the receipt disclaimer

- In step at `92ec946a`; whitepaper unchanged; no new answers.
- Code step (`specify-fee-remittance` tasks 2.1 and 2.2, requirements 3, 11 and 12). The fee view used to total obligations by stored status, so a broadcast remittance awaiting safe showed as `batched` and the book's `submitted` and `confirmed` never appeared. No route listed actual remittances. `fee_summary` now derives each obligation's state from its own and its batch's status (design decision 4). `amounts_wei` reports pending, batched, submitted and confirmed; cancelled and orphaned amounts are reported separately. The view lists the newest 100 batches, with transaction hash and inclusion evidence and never signed bytes, read in the same snapshot as the totals. `platform_receipt` states that a recorded obligation is not proof the platform received the fee. Stored statuses are unchanged, and a combination the ledger never produces fails closed. Three regressions fail without the change, two existing assertions moved to the reported names and became stricter, and the review added a 100-batch-limit test.
- `amounts_wei.remitted` is now `amounts_wei.confirmed`. Every reader in the repository (`provider-admin.py`, the CLI and desktop `provider_admin`, the Provider page's raw-JSON panel) prints the view unchanged, so no frontend source changed.
- Every `specify-fee-remittance` code difference is now fixed; its acceptance tasks 3.x and live evidence 4.x remain.

## 2026-11-17 (eighteenth round) — Code: every confirmed remittance is rechecked, and aggregation sees every obligation

- In step at `421e103e`; whitepaper unchanged; no new answers.
- Code step (`specify-fee-remittance` tasks 2.4 and 2.3, requirements 4, 5, 6 and 9), done together because both are bounded fee queries in `billing_store.rs` that skipped rows.
  - 2.4: confirmed batches used to be rechecked on a page chosen from `now / 30`. With more than 100 batches and passes 60 s apart, some were never rechecked, so their reorg went undetected. Rechecks are now least-recently checked first, using a `fee-batch-checked:<id>` time in the ledger's `metadata` table (no schema change, sealed records untouched). `reconcile_fees` advances it after every outcome, including an outage or a ledger fault.
  - 2.3: aggregation used to read only the 1,000 oldest pending obligations across all wallets. It now totals every pending obligation of each wallet with checked arithmetic, and seals a qualifying (or forced) wallet completely in 1,000-row chunks within one transaction, splitting only at u128 overflow.
  - Five regressions, plus an ordering test the review added, fail without the change. The review measured the sealing transaction at 37 ms for 10,000 pending obligations and 0.88 s for 100,000 in a release build. The thresholds bound the pending count in practice.

## 2026-11-17 (seventeenth round) — Fee remittance specified

- In step at `5d97430f`; whitepaper unchanged; no new answers.
- Spec step: new change `specify-fee-remittance` (capability `inference-fee-remittance`, 12 requirements, 44 scenarios) for the book's provider-remitted platform fee (line 528 and related lines). It covers:
  - a funding charge once per accepted payment, never per request, with no obligation for free openings;
  - a durable obligation per fee to the agreement's signed `platform_wallet`;
  - forward-only states pending → batched → submitted → confirmed;
  - inclusive automatic aggregation at 10^15 wei or 86,400 s, per platform wallet;
  - immutable sealed batches;
  - a loopback-only `fees/remit` operator action;
  - one transfer per batch through the shared nonce journal, with the provider paying gas;
  - safe-frontier confirmation, with recheck and reorg handling;
  - restart recovery with no fee bump;
  - an operator view that separates obligations from remittances, with the not-trustless and not-proof-of-receipt statements.
  Orphan reversal and restoration are cited from `specify-funding-settlement`. The cited tests were re-run and pass (21 library, 1 integration, 46 script tests). Strict validation passes; no code changed.
- Code differences recorded as unchecked tasks:
  - 2.1: the operator view never shows `submitted` or `confirmed`;
  - 2.2: no route shows batches, transaction hashes or the not-proof-of-receipt statement;
  - 2.3: aggregation reads only the 1,000 oldest pending obligations across all wallets;
  - 2.4: confirmed-batch rechecks page by clock, so with more than 100 batches and passes 60 s apart, some batches are never rechecked and their reorg goes undetected.
- Report: what evidence makes a remittance confirmed (default: the funding safe-frontier policy), and how a failed or displaced remittance is resolved (default: no automatic replacement, reported every pass).

## 2026-11-17 (sixteenth round) — Code: zero TTFT and out-of-boundary qualification samples

- In step at `904cc43e`; whitepaper unchanged; no new answers.
- Code step (`specify-generation-performance` tasks 2.2 and 2.3, requirements 4, 8, 9 and 11), done together because both are evidence predicates in `performance.rs`.
  - A 0 µs TTFT used to count as attained, in native validation, in the runner and at runtime (book line 558: nonpositive timing is not evidence). It is now no evidence of attainment: a qualification sample with it does not pass, and at runtime it counts as a missing first token.
  - `validate_evidence` used to count samples outside every prompt boundary (book line 566). It now rejects any sample outside 120–128, 504–512, 1,016–1,024 and 2,040–2,048.
  - Three Rust regressions and one runner case fail without the change.
- Checks: full headless suite 481 passed, 2 ignored; svelte-check 0/0, 32 frontend tests, 46 script tests, py_compile.

## 2026-11-17 (fifteenth round) — Code: client disconnects no longer count as provider misses

- In step at `505fb9ed`; whitepaper unchanged; no new answers.
- Code step (`specify-generation-performance` task 2.1, requirements 11 and 12). Before this change, a client that disconnected during `engine.dispatch`, or during non-streaming generation, dropped the HTTP handler. The request then settled as `interrupted`, which was waived and counted as a provider miss, so three cheap disconnects could latch a provider's withdrawal (book line 571). After reservation, the gateway now runs dispatch and collection in a task that owns the `Settlement` and answers the handler through a oneshot channel. A closed reply settles `disconnected` (`client_interrupted`, not counted). An already measured miss stays a waived, counted miss. Only interruptions the gateway cannot attribute (a panic or runtime shutdown) remain `interrupted`, now classified `provider_incomplete`. Three real-socket gateway regressions and the updated `outcome` test fail without the change.
- The adversarial review found no defect. Its probes covered cancellation, a streaming disconnect after headers, deadlines in both phases, an engine crash and runtime shutdown, and the public HTTP responses were byte-identical to HEAD apart from ids and timings. It noted that a provider stall after a timely first token, followed by the client leaving, is now `client_interrupted`. That follows the book: a decode miss needs authoritative usage.

## 2026-11-17 (fourteenth round) — Generation performance specified

- In step at `8ea5a5a4`; whitepaper unchanged; no new answers.
- Spec step: new change `specify-generation-performance` (capability `inference-generation-performance`, 12 requirements) for the book's *Performance accounting and withdrawal* contract. It covers profile-bound speed classes, `warm-generation-v1`, the runtime guarantee envelope (requested output, prepared prompt), TTFT and decode in integer microseconds, the private token clock, overload refusal, evidence v2, the qualification sweep and joint attainment, memory counters, per-request classification for waivers, and the durable withdrawal latch. Renewal and the waiver's money effect are cited from `specify-inference-commercial-terms` and `specify-inference-request-accounting`. The cited tests were re-run and pass (20 library, 2 owner-control, 17 runner tests). Strict validation passes; no code changed.
- Code differences recorded as unchecked tasks:
  - 2.1, serious: an unattributed client disconnect is waived and counted as a provider miss, so three cheap disconnects can latch withdrawal;
  - 2.2: a 0 µs TTFT counts as attained;
  - 2.3: qualification samples outside every prompt bucket are counted.
- Report: new entry on whether request-caused failures (for example, an engine-rejected structured-output schema) count as provider-incomplete misses (default: yes, as the code does).

## 2026-11-17 (thirteenth round) — M3: one process per consumer libp2p identity

- In step at `5449f06f`; whitepaper unchanged; no new answers.
- Spec step in `specify-p2p-provider-transport`: new requirement *A consumer libp2p identity runs in one process at a time* (decision 27, tasks 2.19 and 3.16, plus a coverage row). It closes the gap found last round: neither M3 nor any other change said how the desktop and a CLI gateway on one profile share its libp2p identity and fixed `dht.listen` port. An exclusive advisory lock on the identity file admits one node. A second process fails libp2p operations with a fixed error (503 `transport_unavailable` on HTTP surfaces), never switches port or identity, and keeps HTTPS and catalog discovery. A crashed holder's lock is released by the OS. The CLI gateway's *Shared DHT identity* non-goal now cites the requirement. Both changes pass strict validation; no code changed.
- Report: new entry asking whether two processes on one profile should share libp2p instead (default: no).

## 2026-11-17 (twelfth round) — CLI loopback gateway specified

- In step at `d064cbda`; whitepaper unchanged; no new answers.
- Spec step: new change `specify-cli-loopback-gateway` (capability `cli-loopback-gateway`, 12 requirements, about 55 scenarios), the follow-up that milestone 2 named for the book's "the native desktop and CLI therefore run a local loopback gateway" (line 315). `chiral --config <profile> gateway serve --port <port>` is a foreground, loopback-only gateway for funded network accounts (HTTPS or libp2p). It reuses milestone 2's gate, bounds, routing, route-credential, request-identity and recovery requirements by name.
  - CLI-specific rules: a required port with no fallback; signal shutdown that cancels in-flight requests and reports them as possibly charged; owner-only state scoped to network and wallet; a bearer that no command prints; route commands that issue dedicated capped credentials, always show the spending disclosure and need `--accept-spending-risk`; JSON-line events on stdout with the transport path; and no consumer lock held while serving.
  - M2's proposal and design now point at the change. The review fixed six defects, including an issued credential left unmentioned when its route cannot be saved, and an unrequired DHT start. All three affected changes pass strict validation. No code changed; checks are unchanged since round 11.
- Report: new entry asking whether the CLI gateway should also serve local models (default: no).

## 2026-11-17 (eleventh round) — Code: the desktop re-reads the funding journal after every funding attempt

- In step at `820b6567`; whitepaper unchanged; no new answers.
- Code step (`specify-funding-settlement` task 2.9, requirement 2). `Usage.svelte` used to leave the pending-funding journal stale after a top-up or resume attempt. After a top-up error the *Funding & recovery* tab said nothing was pending until *Refresh*, and a resolved operation could leave a stale notice. `topup()`, `resume()` and the recovery-tab `recover()` now go through an `attempt()` helper that re-reads `pending_accounts` whether the action succeeds or fails. A new frontend regression fails on the previous page. Screenshots show the notice, and the stage and hash, after a failed top-up without *Refresh*. Checks: svelte-check 0/0, 32 frontend tests, build, 46 script tests. Rust is unchanged since the full headless suite passed at `820b6567` (475 passed, 2 ignored).
- `specify-funding-settlement` code differences are now all fixed except 2.4, which waits for the report question. Its remaining open items are acceptance tests (3.x) and live-chain evidence (4.x).

## 2026-11-17 (tenth round) — Code: funding-risk disclosures on the Marketplace, Usage and CLI

- In step at `ea1c7202`; whitepaper unchanged; no new answers.
- Code step (`specify-funding-settlement` tasks 2.1–2.3, requirements 2 and 3). Before this step, the Marketplace funding panel and its pending notice said payment is broadcast only after the provider's acknowledgement, and the CLI `fund` text and the Usage top-up card left out parts of the book's disclosures. All three now follow book line 489 and A.1.6. The Marketplace panel has a *Residual registration risk:* paragraph: the client broadcasts only after a durable acknowledgement, the provider holds broadcastable bytes and could broadcast before or without acknowledging, the ordering is not cryptographic escrow, and an error does not prove no money moved. Its acceptance covers that risk and the reorganization statement. The CLI `fund` stderr has the same elements. The Usage top-up card states that reorganizations can freeze or reverse credit and must be accepted before *Sign and top up*, without registration wording. New tests: seven in `tests/fundingDisclosure.test.ts` and one CLI process test; the four that cover new text fail on the old text. Screenshots of the panel, the pending notice, the top-up card and a frozen account (browser preview with a scratchpad-only IPC stub) were checked. Task 3.3 is complete.
- Spec: the review found that `Usage.svelte` does not re-read the funding journal after a top-up or resume attempt, so the journaled stage is not shown after a top-up error until *Refresh*. This is recorded as design code difference 9 and task 2.9.

## 2026-11-17 (ninth round) — Code: a top-up cancellation request is durable before the attempt

- In step at `3ea39335`; whitepaper unchanged; no new answers.
- Code step (`specify-funding-settlement` task 2.8, requirement 16): `cancel_funding` now saves the top-up's cancellation request together with its validated original bytes before it calls `cancel_payment_once`, as it already did for an opening. Previously, an attempt that failed before journaling a replacement (for example, unaffordable cancellation gas) left the original broadcastable by `resume`, `fund` or `topup` while its nonce was current. From then on those paths never call `send_payment_once` for that operation. They run the read-only reconcile instead: a won original is credited once from its exact bytes, and a resolution without a win clears the intent. An unresolved request still refreshes the account and returns an error that names `funding-recovery` and the explicit retry. `funding-recovery` records a no-replacement win, and the pending view shows `topup_cancellation_requested`. Two regressions fail without the change. The adversarial review found no defect after four extra probes (nonce not current, every settle path after a win, a reorg displacing a recorded win, and a stale win followed by a replacement).
- Known limits, not spec differences: an older binary ignores the new `TopupIntent` field, so a downgrade loses the protection. A no-replacement top-up that resolved `original_failed` at safe is no longer tracked if a deeper reorg later executes it, which matches the existing `cancel_funding` behaviour; explicit `cancel-funding --operation-id` still recovers it.

## 2026-11-17 (eighth round) — Code: a won original is settled by resume without cancellation authority

- In step at `7a8eb7ac`; whitepaper unchanged; no new answers.
- Code step (`specify-funding-settlement` task 2.6, requirements 16 and 17): before this change, an original that won its cancellation race could be recovered only through `cancel-funding --accept-gas-and-race-risk`, a call that can resubmit the replacement. Now `funding-recovery` and `resume` run the read-only reconcile and durably record each cancelled operation's outcome with the saved agreement (`funding_cancellation`, new `won_topups`). `resume` then opens a won opening, or credits a won top-up once, from the exact journaled original. It never signs, broadcasts or calls the cancellation path, and it withdraws a recorded win that a reorg later displaced. The refusal message no longer points to `cancel-funding`. The adversarial review fixed two gaps: an opening whose cancellation journaled no replacement, and a stale win that could reach `send_payment_once`. Four regressions fail without the change.
- Spec: the review also found that a top-up cancellation request is not saved before the cancellation attempt, so a failed attempt leaves the original broadcastable by `resume`. This is recorded as design code difference 8 and unchecked task 2.8.

## 2026-11-17 (seventh round) — Code: one refused funding recheck no longer stalls monitoring

- In step at `de5eb44e`; whitepaper unchanged; no new answers.
- Code step (`specify-funding-settlement` task 2.5, requirement 9): `contracts::reconcile` used to abort the whole pass on a ledger refusal without advancing that row, so one payment blocked every later recheck and maintenance stopped accepting. A refused row is now quarantined durably and its `checked` time advanced. Quarantined rows are revisited first; while any remain, new funding and offers stay closed under a fixed category, while existing credit keeps serving. The quarantine is released when an actual chain observation applies. A ledger regression and the extended reconcile integration test both fail without the change.

## 2026-11-17 (sixth round) — Code: opening cancellation no longer blocks account refresh or top-up recovery

- In step at `aa8342c3`; whitepaper unchanged; no new answers.
- Code step (`specify-funding-settlement` task 2.7, requirements 4 and 16): `Consumer::resume_inner` used to refuse every `resume`/`fund` after an explicit opening cancellation, even on a saved account, which stranded a pending top-up. Now only an unopened cancelled opening is refused. A saved account skips the cancelled opening's payment step, still recovers a pending top-up's exact original bytes and refreshes. Two regressions fail without the change and pass with it.

## 2026-11-17 (fifth round) — Funding settlement specified

- In step at `763ea347`; whitepaper unchanged; no new answers.
- Spec step: new change `specify-funding-settlement` (capability `inference-funding-settlement`, 19 requirements, 65 scenarios). It covers the whitepaper's safe-frontier credit policy, monitoring and reorg/orphan freeze, reversal and loss, restoration, outages, the balance identity, `cancel-funding`, funding recovery, displaced re-inclusion and nonce-journal ownership. Checked tasks cite tests that were re-run and pass; synthetic fixtures are not live-chain evidence.
- Code differences recorded as unchecked tasks 2.1–2.7:
  - desktop and CLI registration-risk disclosure wording;
  - Usage top-up reorganization disclosure;
  - contradictory evidence treated as rejection;
  - one failing recheck stalls monitoring;
  - recovering a won original only through cancellation;
  - a contract-wide opening-cancellation gate.
- Report: three author questions (top-ups to frozen accounts, contradictory evidence vs existing credit, CLI reorganization disclosure).

## 2026-11-17 (fourth round) — M2 session scoping; the comparison's contradictions are all fixed

- In step at `a40d35ea`; whitepaper unchanged; no new answers.
- Spec step:
  - `specify-desktop-local-api-server`: network routes are scoped to network identity and consumer wallet. Another session's routes are hidden on the Local server page (count only, not deleted), and network-route log entries are removed on logout or a profile change (book A.1.8, scoped by A.7). The design now quotes *Use the network* exactly.
  - `specify-desktop-local-models`: its proposal now says five milestones.
  - Both validate strictly; no code changed.
- Every spec-contradicts-whitepaper finding from the 2026-11-17 comparison is now resolved in the specs. What remains is missing specification, then implementation.

## 2026-11-17 (third round) — M4/M5 aligned with the whitepaper

- In step at `0a8e8cf0`; whitepaper unchanged; no new answers.
- Spec step:
  - M4: Auto-price is unavailable for embedding shares, because they cannot hold the valid qualification *Ready to charge* needs; manual pricing stays. This supersedes the earlier embedding-readiness default. The publish disclosure now states the book's prepaid-balance sentence unconditionally.
  - M5: availability sampled at a fixed 300 s period with a random per-identity phase; "24 hours observed" from the exact first counted sample; *Launching — free* only for a generation provider that is free and has never charged; a per-wallet tombstone keeps `sharing_since` after the 180-day retention; no readiness for embedding-only wallets.
  - M1, M2 and M4: stale lines aligned.
  - All five drafts pass strict validation; no code changed.
- Report: new entry on predictable fixed-interval sampling and the *Launching — free* approximation.

## 2026-11-17 (second round) — Durable relay grace evidence and the 30-day deletion bound (M3)

- In step at `e228c76e`; whitepaper unchanged (SHA-256 `eda22ce0…`); no new answers.
- Spec step in `specify-p2p-provider-transport`. Offer evidence is now durable: a protected, owner-only, network-scoped store that fails closed when damaged, with wall-clock caps across restarts and the monotonic cap within a run. A relay restart no longer ends the 24-hour grace. Eviction from the operator-configured evidence maximum (earliest grace end first) is the one disclosed early end, reported in a new loopback-only, bearer-authenticated `relay_server` administration status. Ended free accounts' records, status and receipts now carry the 30-day deletion bound everywhere they were said to stay available. M4's copies of the relayed-grace statement, plus stale lines in M1 and M2, were aligned. All four changes pass strict validation; no code changed.
- Report: new entry proposing whitepaper wording for the evidence-store bound, with the fail-closed and relay-admin-status assumptions.

## 2026-11-17 — Main proposal-admission spec aligned with non-reserving quotes

- Entry clean at `38fc8bea`, in step with `origin/main`; no merge in progress. Whitepaper `docs/chiral-book.md` unchanged (SHA-256 `eda22ce0…`); no new `Answer:` lines.
- Ran a systematic spec-versus-whitepaper comparison (six slices, each finding adversarially verified). Highest confirmed contradiction: the archived main spec `inference-proposal-admission` still required the superseded named-wallet quote gates, which **Approved non-reserving quotes** forbids and the code had already removed.
- Spec step: moved the finished, implemented and tested deltas unchanged from `specify-non-reserving-quotes` into `adopt-provider-wide-quote-retention` and archived it (`+3 ~3 -3`); corrected the main spec Purpose. `specify-non-reserving-quotes` keeps only *Public quotes do not reserve opening capacity* and *Authenticated paid admission preserves accepted commitments* (open tasks 3.3, 4.3, 4.4). No code change. Checks: strict validation of the main spec and of the non-reserving, proposal-rate and opening-intent changes; svelte-check 0/0, 24 frontend tests, frontend build, 46 script tests. Rust code is unchanged since the full headless suite passed (465 tests) at `38fc8bea`.
- Report: worker action under the quota entry (stays open); new entries on the Hugging Face token location, the free-share immutability exception wording, and the assumption that deleting an ended free account frees its wallet's slot.

## Next steps, in order

Code differences, then missing specifications (spec before code):

1. Code differences: none open except reputation 2.4 and funding-settlement 2.4 (author question) and offer-discovery 2.4 (design choice). All missing specifications in `docs/apas-worker/comparison-2026-11-18.md` are written (round 59). `specify-managed-engine-launch` acceptance is done on this host: 3.1, 3.6, 3.7 and 3.11 need only Linux, Windows or real-device runs, and 4.1 is real-host evidence. The shared publication assertions of `inference-offer-discovery` 3.11 and 3.15 can cite round 62's 3.10 test.
2. `docs/apas-worker/comparison-2026-11-18.md` is complete except `specify-offer-discovery` task 2.4 (needs a design choice). Its new changes' acceptance tasks 3.x are open (outbound-http-safeguards 3.x is done; 4.1 needs a deployment): account-credentials (3.x done on this host; 3.7 needs a Windows run, 3.8 the SDK example runs), public-inference-api (3.x done on this host; 3.2 needs the SDK example runs), provider-daemon-lifecycle (3.x done on this host except 3.5, which waits on the version-policy gate, and 3.11's listener-failure path, which needs the daemon's serve-and-shutdown sequence in the library, plus Windows console events) and no-background-start (3.3–3.5 done; 3.1 and 3.2 need the Linux `/proc` observation). `specify-profile-path-resolution` is archived (round 73).

Earlier open work, unchanged:

3. Opening-intent 2.5, 2.6 and 2.7 (2.3 done in round 84; 2.8 in rounds 93–94; 2.2 in round 95), request-accounting 3.1's llama.cpp evidence (its portable part done in round 89) and 4.x (3.2–3.4 done in rounds 90–92; opening-intent 2.8.2 done in round 93) (section 2 done in rounds 85–88), and the other acceptance items listed in the changes' tasks. The acceptance tasks 3.x of `specify-funding-settlement`, `specify-generation-performance` and `specify-fee-remittance` belong here too. Every code difference in those three changes is fixed except funding-settlement 2.4, which waits for the report question.
4. Milestone implementation M1 → M3 → M2 → M5 → M4. Extend `verus.md` judgments; proofs wait for Verus.

## Blocked

- Verus setup and proofs: open report entry (not installed on this Mac).
- The M1 engine-confinement task waits for the report entry on the Hugging Face token location.
- Real chain, engine, GPU, relay deployment, installers and public-release gates are outside this checkout.

## Checkout discipline

The untracked `src-tauri/gen/schemas/macOS-schema.json` is generated build output and is never committed. The whitepaper is read-only.
