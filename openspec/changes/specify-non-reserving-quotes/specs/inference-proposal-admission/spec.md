## ADDED Requirements

### Requirement: Public quotes do not reserve opening capacity

Public quote issuance SHALL bind exact signed commercial terms but SHALL NOT allocate or consume paid opening-intent leases or free-account capacity. Otherwise eligible quoting SHALL remain possible when those opening capacities are full, subject to separate provider-wide quoting resources and ordinary offer/readiness validation. A quote SHALL NOT be represented as an acceptance acknowledgement, capacity reservation, guarantee of later admission or authorization to broadcast payment.

#### Scenario: Quotes overbook paid admission

- **GIVEN** the provider's paid-intent lease capacity is full but quote resources and offer validity permit issuance
- **WHEN** valid public proposals are issued
- **THEN** signed quotes may be returned without changing any paid-intent lease or account count
- **AND** their holders must still obtain authenticated admission before payment submission

### Requirement: Authenticated paid admission preserves accepted commitments

A fresh paid opening SHALL decide capacity during payer-authenticated funding-intent registration, after validating the exact issued quote and original signed payment and before durable acknowledgement or client broadcast. The final check and accepted-evidence insertion SHALL be atomic across writers. At most 10,000 original un-opened intent leases with deadlines later than the transaction time SHALL be active in a provider ledger; their original quote-expiry deadlines SHALL NOT be extended by replay.

When full, the provider SHALL return 429/`rate_limit` without creating an acknowledgement, accepted intent, paid account, credit or submission. An honest provider SHALL NOT broadcast the refused signed original. A conforming consumer SHALL obtain and durably validate/persist the acknowledgement before submitting those same prepared bytes. Receipt of the signed bytes still technically lets a dishonest provider broadcast them; clients SHALL retain that trust-boundary disclosure.

Once acknowledged, the original agreement SHALL NOT be refused credit or recovery merely because capacity subsequently fills or its quote/lease expires. Exact cached-original recovery SHALL bypass fresh quote, work, issuance and opening-capacity gates, without allocating another lease. Healthy safe-frontier chain evidence, original-operation sender/nonce ownership and all existing financial validity checks SHALL remain necessary. A lost reply or timeout SHALL be treated as ambiguous, not proven refusal: recover the original intent/acknowledgement/payment rather than automatically obtaining a replacement operation.

#### Scenario: Writers compete for the last paid admission lease

- **GIVEN** exactly one paid lease remains and two independently authorized original payments otherwise qualify
- **WHEN** two writers attempt fresh registration concurrently
- **THEN** exactly one durable acknowledgement and lease are created
- **AND** the refused original creates no partial intent, credit or broadcast

#### Scenario: Accepted recovery outlives reply loss and capacity changes

- **GIVEN** an original acknowledgement committed but its reply was lost
- **WHEN** the ledger restarts, the original quote/lease expires and fresh admission capacity fills again
- **THEN** authenticated recovery of the exact original returns the same retained acknowledgement without a new lease
- **AND** a valid original payment remains eligible for safe-frontier credit without a replacement quote, payment or automatic paid retry
