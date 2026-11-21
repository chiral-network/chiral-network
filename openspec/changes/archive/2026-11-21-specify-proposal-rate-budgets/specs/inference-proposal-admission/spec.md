## ADDED Requirements

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
