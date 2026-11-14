# inference-proposal-admission Specification

## Purpose
Bound provider-ledger pending proposals and define atomic admission, rejection rollback and capacity reclamation. Preserve signed agreements, acknowledged funding commitments and account credentials across retention handoffs; distinguish named-payer quota attribution from authorization, funding-intent admission and payment acceptance.

## Requirements
### Requirement: Funding acknowledgement transfers pending proposal retention atomically

After the existing owner, signed-transaction, spendability and funding-admission checks permit first acknowledgement of a paid proposal, the provider SHALL durably retain the complete original signed quote, signed funding-intent acknowledgement, exact original signed transaction evidence, original sender/nonce commitment and funding-intent admission state before returning success. In that same atomic registration operation it SHALL remove the matching standalone pending-proposal record, freeing exactly that proposal's pending-capacity slot without waiting for quote expiry or account opening.

A rejected or failed registration SHALL NOT remove a still-pending proposal, free another proposal's slot, persist a partial acknowledgement or alter a previously accepted agreement. Authorized replay or recovery of an already acknowledged agreement with its original transaction SHALL return the original acknowledgement without recreating a standalone proposal, freeing another slot or creating an additional funding-intent admission.

The transferred agreement SHALL remain recoverable from durable funding-intent state after quote/admission expiry and process restart. Pending-capacity reclamation SHALL NOT erase transaction evidence or original sender/nonce ownership. Funding-intent admission capacity SHALL remain separate from pending-proposal capacity; freeing the proposal slot SHALL NOT itself release an active intent lease, create an account, credit funding, incur a platform-fee obligation or issue a credential. Account opening and payment acceptance SHALL continue to require their existing authorization and verified chain evidence.

#### Scenario: Acknowledgement frees exactly one occupied proposal slot

- **GIVEN** a payer has eight unexpired standalone proposals and all prerequisites for acknowledging one of them are satisfied
- **WHEN** the provider durably acknowledges that agreement
- **THEN** the original quote, acknowledgement and transaction commitment remain recoverable while its standalone proposal no longer consumes pending capacity
- **AND** one replacement proposal can occupy the freed slot if other admission gates permit it
- **AND** a further proposal cannot bypass the eight-slot ceiling merely because the first agreement was acknowledged

#### Scenario: Failed registration leaves pending admission intact

- **GIVEN** an issued proposal occupies a pending slot
- **WHEN** acknowledgement registration is rejected or its atomic persistence fails
- **THEN** the still-pending proposal and its original signed terms remain intact
- **AND** no replacement slot or partial acknowledgement is created by that failure

#### Scenario: Replaying an acknowledgement cannot free another slot

- **GIVEN** one agreement is already acknowledged and its freed proposal slot has been filled by a replacement
- **WHEN** its owner recovers or repeats the same accepted agreement and original transaction
- **THEN** the provider returns the original acknowledgement
- **AND** it does not recreate the original pending proposal, remove the replacement, acquire an additional funding-intent admission or free another pending slot

#### Scenario: Expiry does not require recreating a proposal for funding recovery

- **GIVEN** the provider durably acknowledged the original transaction before quote expiry and removed the standalone proposal
- **WHEN** quote/admission expiry and a process restart precede authorized recovery
- **THEN** the complete original agreement, acknowledgement and original transaction commitment remain available without issuing a new quote or allocating a pending-proposal slot
- **AND** conflicting sender/nonce commitments remain forbidden
- **AND** account credit still requires the original funding operation's verified chain evidence rather than acknowledgement alone

### Requirement: Pending proposals have ledger-scoped capacity

A provider SHALL admit at most eight simultaneously pending proposals for one canonical named consumer wallet and at most 10,000 simultaneously pending proposals in one provider ledger. The ceilings SHALL be shared across models, offer revisions, funding amounts and free/paid proposals within that ledger. Different provider ledgers SHALL retain independent budgets under their existing persistence scopes; this policy SHALL NOT be represented as a network-wide or cross-host global limiter.

A pending proposal SHALL mean a retained standalone proposal awaiting durable funding-intent acknowledgement, successful first account opening or quote-expiry maintenance. Successful acknowledgement SHALL transfer the signed agreement into durable funding-intent retention and free its matching pending-proposal slot atomically; successful first opening SHALL likewise free any remaining matching pending slot. A copy retained only as an account agreement or funding-intent acknowledgement SHALL NOT count as a pending proposal merely because that historical copy exists. Funding-intent admission capacity SHALL remain a separate budget rather than continuing to occupy the transferred proposal slot.

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

Pending-state cleanup and capacity reclamation SHALL NOT erase a separately retained account agreement, funding-intent acknowledgement, raw transaction evidence, accepted balance, fee obligation or original sender/nonce commitment. Expiry-based capacity reclamation SHALL NOT authorize a refund, credit reversal, credential rotation or changed commercial terms.

#### Scenario: Exact expiry boundary restores proposal capacity

- **GIVEN** a payer's pending capacity is exhausted by proposals expiring at time T
- **WHEN** a new otherwise valid proposal is considered at T with other admission budgets available
- **THEN** the expired pending records no longer occupy those slots
- **AND** at T minus one those same records still occupy the slots

#### Scenario: Acknowledged agreement survives pending cleanup

- **GIVEN** a quote was durably acknowledged before its expiry and its standalone pending slot was freed
- **WHEN** pending-state maintenance runs after quote expiry
- **THEN** the original signed agreement and acknowledgement remain recoverable
- **AND** sender/nonce ownership and the accepted funding/recovery contract are not weakened by freeing the proposal slot

### Requirement: Successful first opening frees a pending slot while preserving the agreement

Successful first account opening SHALL retain the complete original signed agreement with the account and remove any remaining matching pending-proposal record in the same atomic account-creation operation. A failed opening SHALL NOT free a still-pending slot as though an account had been created. An agreement already retained by funding-intent acknowledgement SHALL NOT need a standalone pending record recreated before opening. Freeing a proposal slot SHALL NOT bypass independent account, funding-intent, credential or inference limits.

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

