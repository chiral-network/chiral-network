## ADDED Requirements

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
