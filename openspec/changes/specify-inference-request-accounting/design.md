## Scope and authority

This is the missing request-accounting foundation identified while inventorying opening-intent task 2.8.2's remaining inference-settlement/resource-pressure acceptance. `docs/chiral-book.md` remains read-only. Its request lifecycle, durable-accounting, performance-accounting and provider-restart chapters require these financial transitions; the author-approved non-reserving decision forbids grafting fresh quote capacity onto existing recovery.

The change covers the ledger transitions and their gateway/usage/receipt boundaries. It does not redefine funding safe/reorg policy, fee remittance, credential issuance/rotation authority, the full model/API compatibility matrix, performance qualification/withdrawal classification, hardware evidence or public-release gates. It supplies no settlement endpoint accepting user-claimed tokens. Pending commercial and owner-control contracts remain dependencies, including their known source-ownership blockers.

## Decisions

### One operation, one durable first terminal result

Persist the account-scoped client key and an opaque ledger request ID with the reservation. Public duplicates report the original ID/status with 409 rather than reconstruct output. Account scope permits the same literal key for distinct accounts. An absent key creates a fresh UUID and cannot identify a lost request. Terminal retention is financially significant: deleting a row to clear a duplicate or restore a rate allowance would permit another chargeable operation.

The terminal accounting transaction owns the only transition out of `reserved`. A valid repeat returns the first committed result even if later supplied usage differs. This preserves known-usage debits, explicit unknown-usage releases and performance remedies equally; late evidence cannot silently re-charge a request already terminally released. Live cancellation signaling is not itself that terminal transition.

### Exact holds are separate from actual charges and funding fees

Use checked integer arithmetic and the immutable quoted price. Compute one ceil-rounded combined token cost for the maximum reservation and one for actual authoritative usage, not separate input/output rounding. The whole maximum hold is removed at settlement; only the actual charge is retained as spend. Zero-price work still occupies a request identity and applicable concurrency slots.

Reserve account available credit and credential cap authority in the same transaction. Cap accounting is `spent + reserved`, not a second spendable wallet. Credential revocation/rotation prevents new unauthorized work but cannot release or transfer a running hold. An independent funding/reversal transition is the only authority for its own fields; request settlement must not rewrite funding fees or create a replacement payment.

### Unknown exposure is not measured provider work or new balance

The whitepaper explicitly distinguishes reservation-sized exposure from measured loss. `RequestView.provider_loss_wei` records the original maximum hold when usage is unknown; zero stored counters carry an unknown status rather than claiming zero-token execution. This is distinct from `AccountView.provider_loss_wei`, which participates in the separate funding-orphan compensation identity. Accumulating request exposure into that account field would manufacture credit and is not permitted.

Invalid counts reaching the ledger deliberately commit a terminal zero-charge release before returning an invalid-usage error. This is not a failed transaction and must not be tested as rollback of the intended release. Genuine persistence failure cannot acknowledge settlement; the gateway stops admission and retained state is recovered rather than masked. Restart recovers only still-reserved requests, before new admission.

### Preserve current bounded inference controls without borrowing quote clocks

The current request-record windows allow at most 120 recent credential requests over the inclusive creation-time cutoff `now - 60`, and refuse after 30 account unknown-usage records over `now - 3600`. Keep these established consumer-visible controls and their provider-selected sample, including retained future rows on time regression. Do not present them as quote/challenge effective-minute watermarks or a measured-loss budget. Exact clock-edge acceptance needs a controlled operation-time input, not sleeps or substituted signed timestamps.

### Performance is a bound accounting input, not a synthetic qualification

Reservations already retain the actual admitted report under otherwise identical serving terms. Terminal outcomes must agree with that report/policy. An eligible waiver changes the charge, not known authoritative token counts, funding fees or report identity. Unknown restart may retain a waiver classification while nominal waived cost remains zero because usage is unavailable. The full classifier, per-token timing and durable withdrawal window remain governed by the whitepaper and separate future specification work; this change must not claim that ledger tests qualify a GPU or speed class.

### Recovery reads remain bounded and attributable

Preserve the current 100-request pages and signed receipt snapshots, persistent per-account receipt sequences, existing account-scoped read authorization and absence of content replay. Timestamp-only `before` pagination can omit other requests sharing the boundary timestamp when a client treats the last item as an exhaustive cursor; the contract therefore makes no exhaustive-history claim. Do not silently introduce a new cursor/response shape in this specification step.

No fresh quote/storage/work/issuance/opening gate may refuse an existing request's settlement or authorized read. Fresh inference still passes its own model/readiness, authority, capacity and balance checks. Resource independence is not permission to dispatch against an unavailable engine or override a proven frozen funding state.

## Source and retained evidence map

| Contract area | Inspected implementation | Existing tests / limits |
|---|---|---|
| Exact money and single rounding | `billing_store.rs::money`, `usage_cost`, `save_account` | `full_width_money_rounds_once_without_float_or_sqlite_integer` covers full-width arithmetic, one combined rounding and funding fee boundaries; it is not gateway token evidence. |
| Reservation and competing writers | `BillingStore::reserve` | `concurrent_connections_cannot_overspend_one_balance`, `caps_concurrency_revocation_and_scope_are_atomic`, `application_concurrency_rotation_and_revocation_do_not_release_running_holds`; reuse them before adding boundary/fault cases. |
| Identity and terminal result | gateway `request_key`, `duplicate`, `infer`; ledger `request_view`, `settle_request` | `request_ids_are_scoped_and_terminal_usage_is_idempotent` covers duplicate refusal, altered terminal usage and another account's same key at the ledger boundary. Public concurrent/lost-response behavior needs its own inventory. |
| Authoritative usage | backend `parse_usage`, `validate_authorized_usage`, embedding normalization and stream validation; gateway `Settlement::finish` | Existing backend tests cover changed counts, inconsistent totals and structural embedding output zero. Source inspection is not a real-model probe. |
| Unknown/restart | ledger `settle_request`, `recover_requests`; `ProviderState` startup before admission | `restart_releases_unknown_holds_and_preserves_deduplication`, `invalid_usage_cannot_charge_over_hold_or_leave_funds_locked`; no full quote-pressure, transaction-failure or process-kill campaign is claimed. |
| Performance interface | reserve's `PerformanceOutcome` binding and terminal report/policy check | `performance_waiver_preserves_usage_and_releases_reservation_exactly_once`, `restart_performance_waiver_does_not_invent_usage_or_credit` show known/unknown accounting; inspect renewal/withdrawal cases separately rather than reasserting speed qualification. |
| Recovery statements | `BillingStore::requests`, `receipt`; existing contracts owner/read authorization | Signed sequence/restart and owner-control tests exist; inventory exact snapshot/failure boundaries before calling this capability complete. |
| Quote-resource independence | financial transactions do not acquire proposal state; pending opening-intent specification | Earlier 2.8.2 cases cover opening/top-up recovery, not inference reservation/settlement at full quote resources. This remains new acceptance work. |

Several existing ledger tests inject `VerifiedPayment` after verification for setup. They establish ledger behavior, not native CFX execution or healthy public safe-frontier evidence. Existing public/native funding tests have their own explicitly bounded scripted-RPC evidence. Do not count either as engine usage validation.

## Acceptance sequence and ownership

1. Finish the focused source/test inventory against these requirements, distinguishing existing passing cases, observed defects and unexercised edges. Names alone are not acceptance evidence; read the relevant test bodies.
2. First independently useful missing slice: ledger reservation/known-usage/unknown-restart transitions and real authorized request/receipt reads with standalone quote storage full. Use genuine reservation/settlement operations; disclose any restored opening/bulk prerequisites and supplied already-validated usage. Preserve both account and credential accounting, quote/funding/credential authority and at-most-once request evidence through reopening. This is not public dispatch/engine coverage.
3. Cover deterministic transactional faults and competing writers where not already covered. Inject faults only into isolated owned fixtures. Keep successful retry controls and exact monetary identities; do not weaken existing tests or normalize production failures.
4. Complete the actual gateway boundary: preparation before reservation/dispatch, duplicate/ambiguous response, known/unknown cancellation, failed durable settlement and status/receipt recovery. Reuse bounded private test backends only as explicit controlled evidence, never as runtime fallback or model qualification.
5. Extend independently to committed quote issuance and pre-signing work exhaustion when those implemented boundaries and controlled clocks are independently committable. Stored-quote occupancy is not evidence of those budgets. Retain new-inference gates throughout.

Production ledger/gateway/backend/app source and their private unit tests are owner-untracked migration work. Do not absorb them, expose production-only test APIs, copy the modules or commit a knowingly failing regression alone. Worker-owned integration acceptance can proceed when its public interfaces suffice. The missing pre-signing work implementation and existing quote-clock/cutover ownership blockers remain separate; no new author decision is required.

## Verification and completion

Validate this change with `openspec validate specify-inference-request-accounting --strict --no-interactive`, then run the available README/CI checks. The specification-only step neither adds a production behavior nor proves the full acceptance matrix. Keep implementation/acceptance tasks open until their required evidence is recorded, then archive normally without `--skip-specs`.

Native desktop dependencies, actual policy authority, real paid-model/SDK/hardware/platform qualification and public failure/load/deployment gates remain separate unavailable or outside-checkout release work. Do not install tools, deploy, spend funds or edit the whitepaper to make this change appear complete.
