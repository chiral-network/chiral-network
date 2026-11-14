## Why

The whitepaper makes quote proposal public and subject to provider admission rules, while preserving accepted agreements through expiry and recovery. The open commercial-terms change requires bounded pending quotes but does not define the actual capacity policy or its lifecycle. Specify these consumer-visible limits before adding tests that would otherwise merely pin implementation constants.

## What Changes

- Specify eight pending proposals per canonical named payer and 10,000 per provider ledger, shared across models, offers and free/paid proposals.
- Define atomic capacity checking and insertion, rejection without consuming proposal-rate allowance, and isolation between provider ledgers.
- Define reclamation at durable funding-intent acknowledgement, quote expiry and successful first account opening without deleting acknowledged funding terms, account history, credit or sender/nonce commitments.
- Preserve unauthenticated public proposal and explicitly distinguish quota attribution from payer ownership. Record its availability exposure for the whitepaper author.
- Add follow-on regression tasks; this turn changes specifications and APAS notes only.

## Capabilities

### New Capabilities

- `inference-proposal-admission`: Pending-proposal capacity, transactional rejection and lifecycle preservation at the provider ledger boundary.

### Modified Capabilities

None. This capability supplements the bounded-proposal requirement in the open `specify-inference-commercial-terms` change; it does not replace signed schemas, body-reader limits or funding authorization.

## Impact

- Read-only authority: `docs/chiral-book.md`, especially the public proposal authorization matrix, immutable terms and acknowledged-funding recovery contract.
- Existing implementation: `contracts.rs::control`, `BillingStore::store_quote`, `BillingStore::register_funding_intent`, pending cleanup and `BillingStore::open_account`.
- Missing evidence: isolated quote-capacity, concurrent admission, rollback and reclamation tests. Existing bounded challenge tests do not cover quote admission.
- Exact proposal-rate windows and budgets remain a separate specification/coverage step. No rate algorithm, public authentication boundary, native 300-second quote lifetime, financial state or deployment changes here.
