## ADDED Requirements

### Requirement: Pending proposals have ledger-scoped capacity

A provider SHALL admit at most eight simultaneously pending proposals for one canonical named consumer wallet and at most 10,000 simultaneously pending proposals in one provider ledger. The ceilings SHALL be shared across models, offer revisions, funding amounts and free/paid proposals within that ledger. Different provider ledgers SHALL retain independent budgets under their existing persistence scopes; this policy SHALL NOT be represented as a network-wide or cross-host global limiter.

A pending proposal SHALL mean a retained proposal record awaiting removal by quote-expiry maintenance or successful first account opening. A copy retained only as an account agreement or funding-intent acknowledgement SHALL NOT count as a pending proposal merely because that historical copy exists. An acknowledgement that still coexists with a pending proposal record SHALL NOT cause double counting or independently free that pending slot.

#### Scenario: One payer reaches its pending ceiling

- **GIVEN** eight unexpired pending proposals for one named payer in a provider ledger
- **WHEN** another otherwise valid proposal names that payer
- **THEN** the provider rejects it with HTTP 429 and the `rate_limit` error code
- **AND** selecting another model, offer revision or free/paid funding mode does not bypass the payer ceiling

#### Scenario: Different payer with available ledger capacity

- **GIVEN** one payer has exhausted its eight pending slots but the ledger has fewer than 10,000 pending proposals
- **WHEN** another payer proposes valid terms and all other admission gates permit it
- **THEN** the first payer's exhaustion alone does not prevent admission for the second payer

#### Scenario: Provider ledger reaches its total ceiling

- **GIVEN** a provider ledger has 10,000 unexpired pending proposals
- **WHEN** another otherwise valid proposal names a payer that still has individual capacity
- **THEN** the provider rejects it with HTTP 429 and `rate_limit`
- **AND** it does not evict another live proposal or borrow capacity from an unrelated provider ledger

### Requirement: Pending proposal admission is atomic

A provider SHALL evaluate pending capacity and insert the new signed proposal atomically with respect to competing writers of the same ledger. Successful concurrent admissions SHALL NOT exceed either ceiling. Rejection at the pending-capacity check SHALL roll back that admission's tentative proposal-rate increments and pending-state mutations; it SHALL NOT insert the rejected proposal, consume additional proposal-rate allowance or alter existing signed terms. This requirement does not define proposal-rate window lengths or numerical rate limits.

#### Scenario: Writers compete for the final payer slot

- **GIVEN** seven unexpired pending proposals for a payer and otherwise available admission budgets
- **WHEN** two independent ledger connections concurrently submit distinct valid proposals for that payer
- **THEN** exactly one can occupy the final pending slot
- **AND** the capacity-rejected proposal is not retrievable as an issued proposal

#### Scenario: Rejection cannot drain unrelated rate allowance

- **GIVEN** a payer's pending capacity is exhausted
- **WHEN** otherwise valid proposals for that payer are repeatedly rejected by that capacity check
- **THEN** those rejections do not consume additional per-payer or shared quote-rate allowance
- **AND** another payer retains the allowance it had before those rejected attempts, subject to its own successful operations and other admission gates

### Requirement: Quote expiry reclaims pending capacity without erasing accepted evidence

Before evaluating pending capacity for a new ledger admission, the provider SHALL exclude or remove pending proposal records whose `expires_at` is at or before the admission's ledger time. Proposals expiring later SHALL remain pending. This SHALL NOT require a background cleanup timer or permit eviction of still-live proposals.

Removing an expired pending record SHALL NOT erase a separately retained account agreement, funding-intent acknowledgement, raw transaction evidence, accepted balance, fee obligation or original sender/nonce commitment. Expiry-based capacity reclamation SHALL NOT authorize a refund, credit reversal, credential rotation or changed commercial terms.

#### Scenario: Exact expiry boundary restores proposal capacity

- **GIVEN** a payer's pending capacity is exhausted by proposals expiring at time T
- **WHEN** a new otherwise valid proposal is considered at T with other admission budgets available
- **THEN** the expired pending records no longer occupy those slots
- **AND** at T minus one those same records still occupy the slots

#### Scenario: Acknowledged agreement survives pending cleanup

- **GIVEN** a pending proposal also has a durably retained funding-intent acknowledgement
- **WHEN** quote expiry allows its pending record to be reclaimed
- **THEN** the original signed agreement and acknowledgement remain recoverable
- **AND** sender/nonce ownership and the accepted funding/recovery contract are not weakened by freeing the proposal slot

### Requirement: Successful first opening frees a pending slot while preserving the agreement

Successful first account opening SHALL retain the complete original signed agreement with the account and remove its pending-proposal record in the same atomic account-creation operation. A failed opening SHALL NOT free that pending slot as though an account had been created. Freeing a proposal slot SHALL NOT bypass independent account, funding-intent, credential or inference limits.

#### Scenario: Account creation transfers retention responsibility

- **GIVEN** an issued proposal occupies a pending slot
- **WHEN** authorized first opening successfully creates its account
- **THEN** the pending slot is freed without waiting for quote expiry
- **AND** the exact signed agreement remains retrievable through account recovery
- **AND** this slot transfer alone does not change balances, prices, fees or credential authority

### Requirement: Named-payer quota attribution is not payer authentication

Pending-proposal quotas SHALL apply to the canonical payer named by the public proposal, not an assumed authenticated caller. Enforcing these quotas SHALL NOT add an owner-proof requirement to public proposal contrary to the whitepaper, or grant account/funding/credential authority merely because a quote was issued. The policy SHALL NOT be represented as preventing another caller from exhausting a named payer's proposal budget or as evidence of paid execution capacity.

#### Scenario: Public proposal consumes capacity but grants no owner authority

- **WHEN** an unauthenticated caller obtains a valid quote naming a payer
- **THEN** it consumes that payer's pending-proposal budget in that provider ledger
- **AND** subsequent owner-authorized operations still require their existing proof and payment conditions
- **AND** quota exhaustion does not erase or renegotiate the payer's existing accepted agreements
