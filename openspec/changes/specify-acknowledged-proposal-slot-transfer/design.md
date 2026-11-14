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

## Integration and non-goals

The correction edits the still-open pending-capability definition and adds one uniquely named requirement here. Normal archives must apply both sets of ADDED requirements. Neither change is complete merely because the draft now matches the source; missing acceptance work stays unticked.

No migration, deployed service, wallet, schema, native TTL, rate policy, account limit, credential policy or public authentication change. The existing author report on unauthenticated proposal exhaustion remains open and nonblocking; this handoff does not resolve it.
