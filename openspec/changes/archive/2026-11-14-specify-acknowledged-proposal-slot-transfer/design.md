## Context

The existing ledger separates standalone proposals, durable funding intents and accounts. A first valid funding-intent registration inserts the signed acknowledgement, complete signed quote, exact raw transaction and hash, sender/nonce commitment and admission deadline, then deletes the matching standalone quote in the same immediate transaction. Repeated registration of the same accepted agreement returns the retained acknowledgement before requiring fresh acceptance conditions. First account opening later retains the quote with the account; it does not need to recreate a standalone proposal.

The pending-quota draft's coexistence description was a source-inspection mistake. Its correction is not a change to the author's whitepaper or a reason to retain redundant pending quota.

## Decisions

- Preserve the existing atomic handoff. A successful acknowledgement frees exactly its matching pending-proposal slot only after durable funding-intent retention succeeds. Failure cannot free the slot or leave a partial acknowledgement.
- Keep quote capacity, active funding-intent admission and financial evidence separate. Releasing a quote slot does not release an active intent lease, authorize a conflicting nonce, credit money, open an account or issue credentials.
- Authorized replay/recovery returns the original acknowledgement and retained terms without reacquiring a proposal slot. Expired admission capacity is not expired evidence or permission to rewrite an agreement.
- Leave exact intent-capacity numbers and the broader funding lifecycle to their own requirements. Preserve existing envelope, owner-proof, spendability and execution-admission checks; the ledger routine's trusted-caller preconditions are not new public bypasses.

## Acceptance strategy

Review existing tests before adding cases. The existing equivalent-intent test already checks that acknowledgement removes the standalone quote and preserves exact terms/acknowledgement and sender/nonce conflicts after expiry/restart. The pre-expiry recovery test retains acknowledgement and permits original funding recovery through settlement unavailability.

Add only missing consumer-visible quota evidence: fill a payer's pending allowance, acknowledge one valid agreement, admit exactly one replacement, and show that acknowledgement replay or rejected registration cannot free additional slots or alter accepted evidence. Use isolated explicit-time storage/control fixtures and real signatures; do not fabricate a successful payment or claim live chain qualification. Keep public envelope/spendability validation distinct from lower-level ledger transition fixtures.

## Reviewed evidence and limits

The two tests in `src-tauri/tests/inference_proposal_quota_test.rs` cover the previously missing pending-budget observations:

- `acknowledgement_frees_exactly_one_slot_and_replay_preserves_its_replacement`: a full payer budget admits exactly one replacement after acknowledgement; live and reopened-ledger replay preserve that replacement and the original signed acknowledgement.
- `rejected_acknowledgements_preserve_pending_capacity_and_allow_original_registration`: draining-provider and acknowledgement-signing identity rejection preserve pending capacity and signed terms across reopening; the original transaction can subsequently register.

Existing tests in `src-tauri/src/billing_store_tests.rs` supply complementary evidence without adding duplicate cases:

- `preexpiry_funding_intent_preserves_quote_through_settlement_stall_and_restart`: acknowledgement immediately before expiry, reopening, delayed unavailable settlement, cleanup, exact acknowledgement replay, original-transaction-only recovery and no fees before payment acceptance.
- `equivalent_intents_reuse_ack_and_conflicting_sender_nonce_survives_quota_expiry`: removal of the standalone proposal, one active intent on replay, exact agreement/acknowledgement recovery and persistent sender/nonce conflicts after cleanup, reopening and opening.
- `pending_capacity_rejects_new_intents_without_losing_accepted_obligations`: independent active-intent capacity blocks fresh registration; expiry restores admission while retaining old recovery evidence. Its capacity population is a direct ledger fixture, not public admission/load evidence.
- `concurrent_intent_admission_cannot_acknowledge_two_agreements_for_one_nonce`: independent ledger connections acknowledge only one competing original operation.
- `paid_open_never_registers_implicitly_even_before_expiry`: a payment-shaped input or recorded pending attempt does not substitute for an acknowledgement, create an account or incur a fee.
- `control_acknowledges_only_spendable_current_nonce_and_recovers_without_rpc_or_broadcast`: the actual HTTP control handler rejects insufficient value-plus-gas balance, future nonce, changed qualification and conflicting/repriced transactions; original acknowledgement recovery still works after lease cleanup with draining/unqualified state and unavailable RPC.

Evidence limits are explicit. The older ledger tests use constructed `VerifiedPayment` values and short raw-byte fixtures after the chain-verification boundary. The HTTP test uses real signatures and a local synthetic RPC; its later settlement observations are not public-chain evidence. The newer quota tests independently sign and validate offline envelopes, but assume prior owner/spendability admission. None qualifies live settlement, funded-wallet operation, public load, a real engine or power-loss/crash durability. Reopening SQLite handles exercises storage recovery, not an operating-system crash campaign. Existing tests remain unchanged.

## Implementation correspondence

- `BillingStore::register_funding_intent` retains the full signed quote, acknowledgement, raw transaction/hash, sender/nonce and admission deadline, then deletes only the matching standalone proposal in one immediate SQLite transaction. `BillingStore::transaction` commits before returning its result; errors propagate without returning a successful acknowledgement. The database uses WAL and FULL synchronization.
- `funding_intent` and the registration replay branch require the original raw transaction/hash and immutable acknowledgement identity, then return the retained acknowledgement without creating a quote or updating admission. `quote` reads retained account/intent agreements as well as standalone proposals.
- `cleanup_pending` deletes expired standalone proposals and clears expired intent admission deadlines; it does not delete funding intents, their signed agreements or sender/nonce commitments. Nonce checks and the persistent uniqueness trigger continue to reject conflicts.
- `contracts::control` consumes owner proof and binds the named payer before acknowledgement recovery. Fresh registration additionally checks accepting/funding readiness, exact model qualification and native signed-envelope/current-spendability admission; it checks qualification again after that asynchronous admission. Recovery bypasses those fresh-admission gates, not owner authorization or exact original-transaction identity.
- The acknowledgement path does not open an account or issue credentials, credit balances or create fee obligations. Paid first opening remains a separate verified-funding path. Its pending-capacity acceptance work stays in the pending-quota change.

No implementation contradiction or additional missing consumer-visible case was found for this narrow acknowledgement-transfer requirement. The reviewed production source belongs to the owner's uncommitted native migration and is not changed or incorporated into this worker's commit. This conclusion does not close the broader funding lifecycle, first-opening, public-release or platform qualification work.

## Integration and non-goals

The correction edits the still-open pending-capability definition and adds one uniquely named requirement here. Normal archives must apply both sets of ADDED requirements. Neither change is complete merely because the draft now matches the source; missing acceptance work stays unticked.

No migration, deployed service, wallet, schema, native TTL, rate policy, account limit, credential policy or public authentication change. The existing author report on unauthenticated proposal exhaustion remains open and nonblocking; this handoff does not resolve it.
