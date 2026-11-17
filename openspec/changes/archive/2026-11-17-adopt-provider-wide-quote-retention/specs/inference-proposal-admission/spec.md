## MODIFIED Requirements

### Requirement: Funding acknowledgement transfers pending proposal retention atomically

A successful fresh paid funding-intent registration SHALL atomically retain the complete signed quote, signed acknowledgement, exact original raw transaction and hash, sender/nonce ownership and original admission deadline, then remove only its matching standalone proposal. This releases provider-wide unaccepted-quote storage, not a named-wallet allowance. A failed registration SHALL preserve all earlier accepted evidence and roll back its tentative intent, quote-removal and admission mutations. Exact cached recovery SHALL be idempotent and SHALL NOT consume a new lease or remove another proposal.

Accepted terms and original-operation ownership SHALL survive quote/lease expiry and restart. Acknowledgement alone SHALL NOT create an account, funding credit, fee obligation or credential.

#### Scenario: Acknowledgement frees exactly one occupied proposal slot

- **GIVEN** a provider ledger retains 10,000 live standalone proposals and one qualifies for paid acknowledgement
- **WHEN** an authenticated exact original is durably acknowledged
- **THEN** its complete acceptance evidence replaces its standalone quote record atomically
- **AND** unrelated proposals are unchanged and one provider-wide quote-storage slot is released
- **AND** one replacement can use that slot if other gates permit it, but a further quote cannot bypass the 10,000-record ceiling
- **AND** no named-wallet allowance or paid account balance is created

#### Scenario: Failed registration leaves pending admission intact

- **GIVEN** an issued proposal occupies a provider-wide storage slot and earlier accepted agreements exist
- **WHEN** registration is rejected or its atomic persistence fails
- **THEN** the pending proposal and its signed terms remain intact without a partial acknowledgement or released slot
- **AND** all earlier acknowledgements, raw originals, nonce ownership and deadlines remain unchanged

#### Scenario: Replaying an acknowledgement cannot free another slot

- **GIVEN** an acknowledged agreement's freed standalone slot has been filled by an unrelated replacement
- **WHEN** its owner recovers the exact accepted original
- **THEN** the original acknowledgement is returned unchanged without recreating a standalone proposal
- **AND** the replacement survives and no additional lease, credit or freed slot is created

#### Scenario: Expiry does not require recreating a proposal for funding recovery

- **GIVEN** the original payment was acknowledged and its standalone quote removed before expiry
- **WHEN** quote/lease expiry and a restart precede authorized recovery
- **THEN** complete original terms, acknowledgement and transaction evidence remain available without a new quote or lease
- **AND** conflicting sender/nonce commitments remain forbidden
- **AND** credit still requires the original operation's verified safe-frontier chain evidence, not acknowledgement alone

### Requirement: Quote expiry reclaims pending capacity without erasing accepted evidence

Before standalone quote admission, the provider SHALL exclude unaccepted quotes whose expiry is less than or equal to the transaction's admission time. It SHALL NOT evict a still-live quote to create space. This cleanup SHALL NOT erase or alter acknowledged agreements, original raw transactions, permanent sender/nonce ownership, account terms, credits, fees, credentials or settlement evidence. Expiry of an accepted intent's admission lease SHALL release only that lease's capacity.

#### Scenario: Exact expiry boundary restores proposal capacity

- **GIVEN** provider-wide standalone quote storage is full and one unaccepted quote expires at T
- **WHEN** another otherwise eligible quote is admitted at T minus one or at T
- **THEN** the former quote still occupies storage at T minus one but no longer does at T
- **AND** all other resource and validity gates remain necessary

#### Scenario: Acknowledged agreement survives pending cleanup

- **GIVEN** an original payment was acknowledged before its quote expired
- **WHEN** quote/lease maintenance runs after expiry and the ledger is reopened
- **THEN** the original acknowledgement, terms and sender/nonce commitment remain recoverable
- **AND** safe accepted-payment recovery does not require a new quote or admission lease

### Requirement: Successful first opening frees a pending slot while preserving the agreement

A successful first opening SHALL durably retain the exact agreement with the account and remove only its matching standalone quote in the same transaction, if that standalone record still exists. Failed opening SHALL NOT free a pending record or partially create an account. Paid opening SHALL use its retained acknowledgement and valid chain evidence, not require re-creation of an already transferred standalone quote or fresh opening capacity. Free first opening SHALL make its capacity decision only after payer authentication, without a payment or funding-intent.

A first free opening SHALL enforce the existing limits of fewer than eight accounts owned by that authenticated wallet and fewer than 10,000 total accounts in the ledger immediately before creation. These are authenticated free-opening limits, not public quote limits or reasons to refuse an accepted paid payment. Existing-account recovery SHALL NOT repeat fresh free-opening capacity admission or rotate its existing exported credential.

#### Scenario: Account creation transfers retention responsibility

- **GIVEN** an issued proposal occupies a standalone quote-storage slot
- **WHEN** authorized first opening successfully creates its account
- **THEN** that matching slot is freed without waiting for quote expiry
- **AND** the exact signed agreement remains retrievable through account recovery
- **AND** this retention transfer alone changes no balance, price, fee or credential authority

#### Scenario: Free quotes exceed opening capacity without reserving it

- **GIVEN** valid free quotes have been issued and only one free-opening slot remains
- **WHEN** two authorized first openings compete for that slot
- **THEN** at most one account is created and the other receives 429/`rate_limit` without partial account or quote removal
- **AND** neither path constructs or submits a payment or paid funding-intent

#### Scenario: An existing agreement survives later capacity exhaustion

- **GIVEN** an account already exists with its retained agreement and credential
- **WHEN** fresh quote or free-opening capacity later fills and its owner recovers that account
- **THEN** recovery does not perform fresh quote or account-capacity admission
- **AND** the retained terms, balance and exported credential are preserved

## REMOVED Requirements

### Requirement: Pending proposals have ledger-scoped capacity

**Reason**: Its named-wallet eight-slot ceiling contradicts the approved non-reserving contract.

**Migration**: Replace it with `Unaccepted quote retention is bounded provider-wide`. Preserve the 10,000-record ledger bound, independent ledgers and accepted-evidence protections; replace the forbidden wallet-ceiling scenarios with acceptance beyond both former wallet limits.

### Requirement: Pending proposal admission is atomic

**Reason**: Its final-payer-slot and per-payer-rate scenarios describe resource identities the author has forbidden.

**Migration**: Replace it with `Unaccepted quote storage admits atomically`. Preserve competing-writer capacity protection and committed-insertion rollback, but use provider-wide storage and distinguish the separately spent pre-signing work permit.

### Requirement: Named-payer quota attribution is not payer authentication

**Reason**: The author's approved non-reserving-quote contract forbids public proposals from consuming named-wallet quote allowances. Describing such attribution as unauthenticated is no longer an acceptable policy.

**Migration**: Remove both wallet-specific pending and proposal-rate enforcement together, and migrate their contract tests. Retain canonical wallet identity in signed terms and authenticated account operations; do not change challenge limits, financial evidence or historical observations into claims of mitigation.

## ADDED Requirements

### Requirement: Unaccepted quote retention is bounded provider-wide

A provider SHALL retain at most 10,000 unaccepted standalone quotes in one provider ledger, shared across models, offer revisions, named wallets and free/paid modes. Independent ledgers SHALL have independent limits. Public proposals SHALL NOT be refused, counted or charged against a named wallet's pending-quote or proposal-rate allowance. Canonicalizing the wallet for signed terms SHALL NOT make it a resource-control identity.

This limit SHALL bound retained unaccepted terms only, independently of paid-intent leases, authenticated free-opening capacity, accepted agreements, credentials and inference reservations. Acknowledged/account-retained quote copies SHALL NOT count as standalone pending quotes. At the storage ceiling, a fresh proposal SHALL return HTTP 429 with `rate_limit` without evicting another live quote or accepted agreement. Separate provider-wide work and issuance budgets remain necessary.

#### Scenario: Another caller cannot spend a named wallet's quote allowance

- **GIVEN** unrelated public callers have obtained eight pending quotes and twelve committed proposals naming a wallet, while provider-wide resources remain available
- **WHEN** another valid public proposal names that same wallet, in any canonical spelling or supported funding mode
- **THEN** neither former wallet ceiling refuses it or charges a wallet-specific bucket
- **AND** the named owner's subsequent authenticated opening is decided by its own authorization, terms and opening-capacity rules

#### Scenario: Provider-wide quote storage is full

- **GIVEN** one ledger has 10,000 live standalone quotes
- **WHEN** another otherwise valid proposal is submitted
- **THEN** it is refused with 429/`rate_limit` without changing those quotes or accepted agreements
- **AND** a different ledger is not exhausted by that storage

### Requirement: Unaccepted quote storage admits atomically

The provider SHALL check provider-wide pending-quote occupancy and insert the exact signed quote atomically across competing writers. Failure SHALL roll back tentative quote, pending-cleanup and committed-issuance counter mutations. Successful issuance SHALL be returned only after durable retention. A separately claimed pre-signing work permit SHALL remain spent even if subsequent quote admission fails.

#### Scenario: Writers compete for the final retained-quote slot

- **GIVEN** a ledger contains 9,999 live standalone quotes and other gates permit both candidates
- **WHEN** two independent writers try to insert different valid quotes
- **THEN** exactly one quote commits and the other is refused with 429/`rate_limit`
- **AND** the ledger never retains more than 10,000 standalone quotes

#### Scenario: Failed insertion conserves quote state but not authorized work

- **GIVEN** a proposal has claimed a pre-signing work permit and prior quote/issuance state exists
- **WHEN** insertion fails after tentative transaction writes
- **THEN** quote state and committed-issuance counters are unchanged by that failed transaction
- **AND** the already claimed signing-work unit remains spent without affecting accepted agreements

### Requirement: Public quote work is bounded before signing

Public proposals SHALL use the existing shared non-queuing limit of 64 simultaneous control operations, at most 96 KiB of body data per operation and a five-second body-read deadline. These controls SHALL precede quote signing. After validating input and before signing, the provider SHALL atomically and durably claim one of at most 600 quote-signing work units per provider ledger per effective minute. An exhausted work budget SHALL return HTTP 429 with `rate_limit` without signing a quote. A claimed work unit SHALL NOT be refunded by signing, quote insertion, persistence or response-delivery failure.

For sampled unsigned Unix time t, work admission SHALL use E = max(floor(t / 60), L), where L is that work budget's last committed effective minute. Watermark and count SHALL survive restart, competing connections and unrelated cleanup. Only a strictly newer minute SHALL refill the work budget, without accumulated allowance. If no work-budget evidence exists at cutover or first initialization, the provider SHALL durably exhaust the initialization minute and begin fresh allowance only in a newer minute. Invalid or unreadable work evidence SHALL fail closed for new quoting, without erasing accepted financial state.

Work-budget state SHALL be provider-ledger-wide and bounded independently of caller-controlled wallet strings. It SHALL remain separate from the global committed-proposal counter, whose failed-insertion rollback and nonrefundable successful-issuance semantics are unchanged. Additional request-source limits MAY be used, but SHALL NOT treat the named wallet or an untrusted forwarding header as the source identity. None of these limits SHALL gate cached accepted-payment, existing-account, top-up or settlement recovery as fresh quote work.

#### Scenario: Rejected insertions cannot cause unlimited signing

- **GIVEN** fresh work allowance is available but quote insertion repeatedly fails
- **WHEN** valid public requests reach quote signing
- **THEN** each signing attempt consumes a durable work unit even though no quote commits
- **AND** after 600 units, another request receives 429/`rate_limit` before signer invocation

#### Scenario: Restart and older samples cannot refill work

- **GIVEN** a ledger's work budget is exhausted in effective minute L
- **WHEN** the ledger is reopened or unrelated maintenance samples a later time, followed by quoting with a sample at or before L
- **THEN** no signing work is admitted and the retained exhaustion is unchanged
- **AND** the same named wallet on an independent eligible ledger does not inherit that exhaustion

#### Scenario: Initialization does not invent partial-minute work allowance

- **GIVEN** no signing-work evidence exists for the ledger
- **WHEN** the limiter initializes in minute M
- **THEN** it durably marks M exhausted without altering committed-proposal or financial evidence
- **AND** only a sample in a strictly newer minute can claim the first fresh work unit
