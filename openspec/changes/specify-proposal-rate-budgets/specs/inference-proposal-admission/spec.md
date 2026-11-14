## ADDED Requirements

### Requirement: Committed proposals consume scoped minute budgets

A provider SHALL admit at most 12 committed proposals for one canonical named consumer wallet and at most 600 committed proposals in one provider ledger per effective proposal minute. Both budgets SHALL apply to each proposal, shared across models, offer revisions, funding amounts and free/paid modes. The effective minute SHALL be selected by the non-regressing clock requirement below, not by assuming transaction order equals timestamp order.

The provider SHALL count successful durable proposal insertions, not request arrivals, cryptographic signing attempts or successful delivery of the HTTP response. If either budget is exhausted it SHALL reject fresh proposal admission with HTTP 429 and `rate_limit`, without storing the rejected proposal. Other admission/validity gates remain necessary; available rate allowance is not a promise of pending capacity or inference availability.

Proposal budgets SHALL be provider-ledger-local and separate from challenge and inference budgets. Named-payer attribution SHALL NOT be represented as caller authentication or Sybil resistance. This fixed-minute policy SHALL NOT be represented as a rolling-window, rejected-request CPU or paid-performance guarantee.

#### Scenario: Payer consumes its twelfth rate unit with pending capacity available

- **GIVEN** one payer has eleven committed proposals charged to the current effective minute and the next proposal has pending capacity and passes other gates
- **WHEN** its twelfth proposal is admitted
- **THEN** that proposal consumes the last payer unit
- **AND** a thirteenth is rejected with 429/`rate_limit` even if earlier proposals no longer occupy pending slots
- **AND** selecting another model, offer or free/paid mode does not restore its payer allowance

#### Scenario: Ledger consumes its six-hundredth rate unit

- **GIVEN** 600 proposals have been committed in the current effective minute across eligible payers in one provider ledger
- **WHEN** another otherwise admissible proposal names a payer with unused individual allowance
- **THEN** that ledger rejects it with 429/`rate_limit` without storing it
- **AND** an independent provider ledger with available budgets is not exhausted by those admissions

#### Scenario: Another payer retains its individual allowance

- **GIVEN** one named payer has exhausted its individual budget but the ledger budget and pending capacities remain available
- **WHEN** a proposal names a different eligible payer
- **THEN** the first payer's exhaustion alone does not refuse it
- **AND** address-case aliases of the exhausted payer remain the same canonical payer at native issuance

### Requirement: Proposal budget minutes never regress

For sampled unsigned Unix-second time `t`, the provider SHALL compute the sampled minute `W = floor(t / 60)`. Within the ledger write transaction it SHALL choose the effective minute `E = max(W, L)`, where `L` is the last durably committed effective proposal minute. Both proposal counters SHALL use that same `E`. A strictly newer effective minute SHALL have its normal 12/600 budgets without accumulated unused allowance; an equal or older sample SHALL use the retained counters for `L` and SHALL NOT reopen or refill an earlier budget.

The last effective minute and applicable counters SHALL survive process restart and competing connections. Pending-state maintenance and operations outside proposal admission SHALL NOT erase or advance that proposal-rate evidence in a way that restores consumed allowance. A proposal-rate window SHALL advance only with successful proposal commitment. Budget-time clamping SHALL NOT rewrite signed quote timestamps, change ordinary freshness checks or alter owner authorization, funding deadlines or accepted financial evidence.

When introducing the watermark to a ledger with retained quote-rate evidence, the provider SHALL preserve its applicable counts and initialize from the greatest retained quote-rate minute rather than treating that ledger as unused. With neither watermark nor retained quote-rate evidence, the first admission sample SHALL establish the initial minute. Unreadable or inconsistent retained rate state SHALL refuse fresh proposal admission rather than silently invent allowance; missing historical evidence already discarded by the old limiter SHALL NOT be presented as reconstructed history. Initialization SHALL NOT reset unrelated ledger state or convert old financial schemas.

#### Scenario: Normal forward minute boundary

- **GIVEN** an otherwise valid proposal is refused because its rate budget is exhausted at time `60*k + 59`
- **WHEN** a proposal is admitted at `60*(k + 1)` with pending capacity and other gates available
- **THEN** the new effective minute starts with its normal budgets and consumes one unit in each applicable bucket
- **AND** skipping multiple minutes does not accumulate additional quota

#### Scenario: Older sample uses remaining current allowance

- **GIVEN** the ledger has committed effective minute L with remaining payer and ledger units
- **WHEN** an otherwise admissible proposal carries an admission-time sample from an earlier minute
- **THEN** it consumes those remaining units in L without changing L or creating another minute's allowance

#### Scenario: Reordered or regressing timestamps cannot revive exhausted allowance

- **GIVEN** the relevant proposal budget in committed effective minute L is exhausted
- **WHEN** an independent writer submits an otherwise admissible proposal using a sample whose minute is less than L
- **THEN** it is rejected with 429/`rate_limit` and no new proposal is stored
- **AND** a subsequent sample in L still observes the exhausted budget

#### Scenario: Restart or other control maintenance cannot reset proposal rates

- **GIVEN** a proposal budget in effective minute L is exhausted
- **WHEN** the ledger is reopened or unrelated control maintenance observes a later wall-clock minute
- **AND** an otherwise admissible proposal subsequently supplies a sample at or before L
- **THEN** its exhausted proposal allowance remains exhausted
- **AND** challenge counters and financial recovery retain their separate contracts

#### Scenario: Retained counters survive watermark initialization

- **GIVEN** a ledger has consistent retained quote-rate evidence but no new proposal-minute watermark
- **WHEN** proposal admission initializes that watermark
- **THEN** it retains the applicable spent units in the greatest retained quote-rate minute
- **AND** initializing metadata alone cannot grant a thirteenth payer unit or a six-hundred-and-first ledger unit
- **AND** inconsistent or unreadable evidence refuses new proposal admission without deleting accepted agreements or financial state

### Requirement: Proposal rate charging is atomic and nonrefundable

The provider SHALL select the effective minute, check and increment both applicable proposal counters, enforce pending capacity and store the signed quote as one atomic admission operation across competing writers. It SHALL NOT return successful proposal admission before that state is durably committed. A rejected or failed admission SHALL NOT consume rate units, advance the committed proposal minute, insert its quote or preserve partial pending-state mutations.

Once a proposal is committed, loss of its response, quote expiry, funding acknowledgement or first account opening SHALL NOT refund its rate units. These events may reclaim pending capacity under their separate requirements. Reclaiming pending capacity SHALL NOT renegotiate a signed agreement, credit funding, rotate credentials or erase original sender/nonce ownership.

#### Scenario: One payer's refusal preserves another payer's ledger allowance

- **GIVEN** a payer has exhausted its individual rate budget while the ledger still has rate allowance
- **WHEN** further proposals for that payer are rejected
- **THEN** their tentative ledger increments are rolled back
- **AND** other eligible payers retain the ledger allowance they had before those refusals

#### Scenario: Writers compete for the last rate unit

- **GIVEN** one applicable rate unit remains and pending capacity and other gates permit both candidates
- **WHEN** two independent ledger connections concurrently submit distinct valid proposals for it
- **THEN** exactly one commits and consumes that unit
- **AND** the other proposal is rejected without storing a quote or charging another bucket

#### Scenario: Failed admission cannot move the proposal clock forward

- **GIVEN** a ledger has a committed effective proposal minute L
- **WHEN** a candidate samples a later minute but its admission fails after tentative counter updates
- **THEN** the proposal, counter changes, pending mutations and proposed minute advancement are rolled back together
- **AND** a later admission at or before L sees the same rate allowance it would have had without that failure

#### Scenario: Capacity reclamation does not refund successful issuance

- **GIVEN** a proposal has been durably committed and charged in the current effective minute
- **WHEN** its response is lost or its pending slot is reclaimed by expiry, acknowledgement or first opening
- **THEN** the committed rate unit remains spent until a strictly newer effective minute is successfully entered
- **AND** retained agreements, credentials and financial recovery keep their existing authority and evidence
