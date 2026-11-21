# inference-proposal-admission Specification

## Purpose
Bound provider-ledger-wide retention of unaccepted signed quotes, pre-signing quote work and committed quote issuance (a durable, non-regressing minute budget), with no named-wallet allowance, and define atomic storage admission, rejection rollback and retention reclamation. Quotes reserve no opening capacity: paid capacity is decided atomically at authenticated funding-intent registration, and accepted originals stay recoverable after capacity fills or their quote expires. Preserve signed agreements, acknowledged funding commitments and account credentials across retention handoffs; distinguish quote retention from authorization, funding-intent admission and payment acceptance.
## Requirements
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

### Requirement: Committed proposals consume a provider-wide minute budget

A provider SHALL admit at most 600 committed proposals in one provider ledger per effective proposal minute, shared across all named wallets, models, offer revisions, funding amounts and free/paid modes. Public proposals SHALL NOT consume or be refused against a named-wallet proposal-rate allowance. Canonical wallet identity in signed terms SHALL NOT be used as a public quota key. The effective minute SHALL follow the non-regressing clock requirement below, not assumed timestamp/commit ordering.

This counter SHALL charge successful durable quote insertions, not request arrivals, signing attempts or delivery of the response. Exhaustion SHALL reject fresh quote admission with HTTP 429 and `rate_limit`, without storing the candidate. Other validity and provider-wide resource gates remain necessary; available issuance allowance SHALL NOT reserve paid/free opening or inference capacity.

The committed-issuance budget SHALL be ledger-local and separate from pre-signing work, challenge and inference budgets. A pre-signing work permit already claimed under its separate contract SHALL remain spent when later quote insertion fails; it SHALL NOT be substituted for this committed counter or vice versa. This fixed-minute limit SHALL NOT be represented as a rolling-window, rejected-request CPU, per-caller availability or paid-performance guarantee.

#### Scenario: Former wallet rate exhaustion does not refuse another quote

- **GIVEN** unrelated public callers have committed twelve proposals naming one wallet in the current effective minute and all provider-wide resources still permit another
- **WHEN** another valid public proposal names that wallet
- **THEN** the thirteenth commits and consumes only one global committed-issuance unit
- **AND** it is not refused by either former named-wallet pending or rate ceiling
- **AND** canonical wallet spelling, model and free/paid variants do not create wallet-specific rate state

#### Scenario: Ledger consumes its six-hundredth rate unit

- **GIVEN** 599 proposals have committed in the current effective minute and other gates permit the next candidate
- **WHEN** the six-hundredth proposal commits
- **THEN** it spends the final global unit and a further candidate naming any wallet is refused with 429/`rate_limit` without storage
- **AND** another ledger with available resources is not exhausted by those admissions

#### Scenario: One ledger budget spans all named parties and terms

- **GIVEN** proposals naming one or several wallets have spent part of a ledger's committed-issuance allowance
- **WHEN** another proposal changes wallet, model, offer revision or free/paid funding mode
- **THEN** it uses the same remaining ledger allowance if other gates permit it
- **AND** the name alone neither grants fresh units nor exhausts a wallet-specific allowance

### Requirement: Proposal budget minutes never regress

For sampled unsigned Unix-second time `t`, the provider SHALL compute `W = floor(t / 60)`. Within the quote write transaction it SHALL choose `E = max(W, L)`, where `L` is the last durably committed effective proposal minute. The global committed-issuance counter SHALL use E. A strictly newer minute SHALL have 600 units without accumulated unused allowance; equal or older samples SHALL use the retained count for L and SHALL NOT reopen or refill an earlier budget.

The committed minute and global counter SHALL survive restart and competing connections. Pending-state maintenance and operations outside successful quote insertion SHALL NOT erase, advance or refill this evidence. The committed-issuance watermark SHALL advance only with a successful proposal, independently of pre-signing work claims. Budget-time clamping SHALL NOT rewrite signed quote timestamps or change freshness, authorization, funding deadlines or accepted financial evidence.

When introducing the watermark, the provider SHALL preserve the retained global quote-rate row's minute and count. Legacy wallet rows SHALL NOT be summed into a supposed global total or used to enforce a wallet allowance. If such rows exist without a global row, or show a newer minute than that global row, fresh quote admission SHALL fail closed rather than invent missing totals. With neither watermark nor any retained quote-rate evidence, the first successful admission SHALL establish the initial minute and consume one global unit.

Unreadable or otherwise inconsistent retained state, including an installed watermark without its required coherent global counter, SHALL refuse fresh quote admission without deleting evidence or resetting financial state. Obsolete wallet-keyed quote-rate rows SHALL be retired only in a successful atomic adoption that preserves the global evidence; failed adoption/insertion SHALL leave them intact. After adoption they SHALL NOT be created, charged or consulted as admission allowances. Missing history already discarded by the old limiter SHALL NOT be represented as reconstructed, and initialization SHALL NOT convert old financial schemas.

#### Scenario: Normal forward minute boundary

- **GIVEN** an otherwise valid proposal is refused because its rate budget is exhausted at time `60*k + 59`
- **WHEN** a proposal is admitted at `60*(k + 1)` with pending capacity and other gates available
- **THEN** the new effective minute starts with 600 units and consumes one global committed-issuance unit
- **AND** skipping multiple minutes does not accumulate additional quota

#### Scenario: Older sample uses remaining current allowance

- **GIVEN** the ledger has committed effective minute L with remaining global units
- **WHEN** an otherwise admissible proposal carries an admission-time sample from an earlier minute
- **THEN** it consumes one remaining global unit in L without changing L or creating another minute's allowance

#### Scenario: Reordered or regressing timestamps cannot revive exhausted allowance

- **GIVEN** the relevant proposal budget in committed effective minute L is exhausted
- **WHEN** an independent writer submits an otherwise admissible proposal using a sample whose minute is less than L
- **THEN** it is rejected with 429/`rate_limit` and no new proposal is stored
- **AND** a subsequent sample in L still observes the exhausted budget

#### Scenario: Restart or other control maintenance cannot reset proposal rates

- **GIVEN** the global committed-issuance budget in effective minute L is exhausted
- **WHEN** the ledger reopens or unrelated challenge/work maintenance observes a later minute
- **AND** another proposal supplies an admission sample at or before L
- **THEN** the committed-issuance budget remains exhausted
- **AND** independent work/challenge budgets and accepted financial recovery retain their own contracts

#### Scenario: Retained counters survive watermark initialization

- **GIVEN** the retained global row has 599 spent units in minute L, with no watermark and no contradictory legacy evidence
- **WHEN** an otherwise eligible proposal at or before L initializes the watermark and commits
- **THEN** it spends the last global unit rather than resetting the retained total
- **AND** another proposal at or before L is refused with 429/`rate_limit`

#### Scenario: Retiring legacy wallet rows does not retain their quota

- **GIVEN** valid global evidence leaves committed allowance and a legacy named-wallet row records twelve spent units in the same minute
- **WHEN** an otherwise eligible proposal naming that wallet commits the adoption
- **THEN** its old wallet count does not refuse admission or become a new charge
- **AND** the global count is preserved and incremented once while obsolete wallet rows are retired atomically
- **AND** reopening does not recreate or enforce wallet proposal buckets

#### Scenario: Missing or contradictory retained totals cannot create allowance

- **GIVEN** legacy wallet evidence lacks a global total, proves a later minute than the global row, or required retained state is unreadable/inconsistent
- **WHEN** fresh proposal admission attempts adoption or reopening
- **THEN** it is refused without inventing a total, deleting legacy evidence or creating a quote
- **AND** accepted agreements and financial recovery are not erased or subjected to this fresh-quote gate

### Requirement: Proposal rate charging is atomic and nonrefundable

The provider SHALL select the effective minute, check/increment the global committed-issuance counter, enforce provider-wide standalone quote storage and store the signed quote as one atomic operation across writers. Success SHALL be returned only after durable commitment. Rejected or failed insertion SHALL NOT consume committed-issuance units, advance its watermark, insert its quote or preserve partial pending-state/adoption mutations. This rollback SHALL NOT refund or alter a separately committed pre-signing work permit.

After commitment, response loss, quote expiry, funding acknowledgement or first account opening SHALL NOT refund the committed-issuance unit. These events may reclaim quote storage, not change signed terms, credit funding, rotate credentials or erase sender/nonce ownership. Cached acknowledged-original, existing-account, top-up and settlement recovery SHALL NOT be charged or refused as fresh quote issuance.

#### Scenario: Storage refusal preserves committed allowance without refunding work

- **GIVEN** the global committed counter has allowance but provider-wide standalone quote storage is full
- **WHEN** a candidate with a claimed work permit is refused by the storage check
- **THEN** tentative committed-counter and quote-storage mutations roll back
- **AND** the work permit remains spent under its separate contract
- **AND** after storage becomes available, the same committed allowance remains for an otherwise eligible candidate

#### Scenario: Writers compete for the last rate unit

- **GIVEN** one global committed-issuance unit remains and other gates permit both candidates
- **WHEN** two independent ledger connections concurrently submit distinct valid proposals
- **THEN** exactly one commits and consumes that unit
- **AND** the loser is not stored or charged a committed unit, without refunding any prior signing-work permit

#### Scenario: Failed admission cannot move the proposal clock forward

- **GIVEN** a ledger has a committed effective proposal minute L
- **WHEN** a candidate samples a later minute but its admission fails after tentative counter updates
- **THEN** the proposal, counter changes, pending mutations and proposed minute advancement are rolled back together
- **AND** a later admission at or before L sees the same committed allowance, irrespective of the separate work clock or work unit already spent

#### Scenario: Capacity reclamation does not refund successful issuance

- **GIVEN** a proposal has been durably committed and charged in the current effective minute
- **WHEN** its response is lost or its pending slot is reclaimed by expiry, acknowledgement or first opening
- **THEN** the committed rate unit remains spent until a strictly newer effective minute is successfully entered
- **AND** retained agreements, credentials and financial recovery keep their existing authority and evidence

