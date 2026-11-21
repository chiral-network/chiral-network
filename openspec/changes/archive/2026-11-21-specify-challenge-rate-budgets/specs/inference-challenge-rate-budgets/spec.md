## ADDED Requirements

### Requirement: Committed challenge issuance consumes scoped minute budgets

A provider SHALL admit at most 12 durably inserted challenges for one canonical named wallet and at most 600 durably inserted challenges in one provider ledger per effective challenge minute. Both budgets SHALL apply to every issuance, shared across accepted methods, target paths and queries, body digests and intended accounts. Address-case aliases SHALL name the same canonical wallet. The effective minute SHALL be selected by the non-regressing challenge clock requirement below.

The provider SHALL count committed challenge insertions, not request arrivals, nonce generation, signatures, successful authentication or HTTP response delivery. Exhaustion of either budget SHALL reject new issuance with HTTP 429 and `rate_limit`, without storing or returning a new challenge as successfully issued. Pending-capacity and input/lifetime gates SHALL remain additional requirements; unused rate units do not promise an available slot or a valid operation.

Challenge budgets SHALL be provider-ledger-local, separate from proposal and inference budgets, and independent of request source address or whether the named wallet already owns an account. They SHALL NOT be described as a network-wide limiter, proof of wallet ownership, Sybil resistance, a rolling-window limit, a bound on rejected-request CPU cost or paid-performance qualification. Public challenge issuance SHALL remain unauthenticated.

#### Scenario: Twelfth wallet issuance is distinct from pending occupancy

- **GIVEN** a named wallet has eleven committed challenge issuances in the effective minute and pending capacity remains available
- **WHEN** another valid challenge is issued for that wallet
- **THEN** it consumes the twelfth wallet unit and one ledger unit
- **AND** a thirteenth issuance is refused with 429/`rate_limit` even if previous challenges have been consumed or expired
- **AND** changing address letter case, method, account, query or body digest does not restore that wallet's allowance

#### Scenario: Another wallet retains its individual allowance

- **GIVEN** one wallet has exhausted its challenge rate budget while the ledger rate budget and pending capacities remain available
- **WHEN** another eligible wallet requests a challenge
- **THEN** the first wallet's exhaustion alone does not prevent its issuance
- **AND** the named wallet does not acquire ownership authority merely because issuance succeeds

#### Scenario: Ledger rate boundary applies across wallets

- **GIVEN** 600 challenges have been committed in the effective minute across eligible wallets in one provider ledger
- **WHEN** a valid challenge names a wallet with unused individual allowance and available pending capacity
- **THEN** that ledger refuses issuance with 429/`rate_limit` without storing the candidate
- **AND** an independently scoped provider ledger with available budgets is not exhausted by those issuances

### Requirement: Effective challenge minutes never regress or share another resource's clock

For provider-sampled unsigned Unix-second time `t`, the provider SHALL compute `W = floor(t / 60)`. Inside the admission write transaction it SHALL select `E = max(W, L)`, where `L` is the last durably committed effective challenge minute in that ledger. Both challenge counters SHALL use the same `E`. A strictly newer effective minute SHALL start with the normal 12/600 budgets, not accumulated unused allowance; an equal or older sample SHALL use the retained counts in `L` without reopening an earlier budget.

The challenge watermark and applicable counts SHALL survive restart and competing ledger connections. Only successful challenge commitment SHALL advance the challenge minute. Proposal issuance, pending maintenance, challenge consumption and other operations SHALL NOT erase or advance this evidence so as to restore spent allowance. Conversely, challenge issuance and its cleanup SHALL preserve the independent proposal-rate watermark and applicable counters. A proposal's newer effective minute SHALL NOT advance the challenge minute, nor vice versa.

The effective minute SHALL be a rate-accounting clock only. Clamping SHALL NOT rewrite a challenge's signed expiry, extend its 120-second issuer/admission lifetime, authorize an expired proof, or change quote validity, funding deadlines, accepted balances, credentials or original transaction/nonce evidence. Normal advancing time SHALL use fixed Unix-minute boundaries; a forward clock jump followed by correction MAY retain the later committed budget until a strictly newer sample is successfully admitted.

When no challenge watermark exists but challenge-rate evidence is retained, initialization SHALL preserve the applicable counts in the greatest retained challenge-rate minute. With neither watermark nor retained challenge-rate evidence, the first committed issuance SHALL establish the initial minute from its sample. Unreadable or inconsistent retained rate evidence SHALL refuse fresh issuance rather than invent unused allowance. Initialization SHALL NOT reset unrelated counters, alter financial schemas/state or claim to reconstruct history already discarded by the previous limiter.

#### Scenario: Normal forward boundary does not accumulate quota

- **GIVEN** challenge allowance is exhausted at `60*k + 59` with otherwise valid candidates and available pending capacity
- **WHEN** a challenge commits at `60*(k + 1)`
- **THEN** it consumes one unit in each applicable bucket of the new effective minute
- **AND** skipping several minutes grants only that minute's normal budgets

#### Scenario: An older sample consumes only remaining current units

- **GIVEN** effective challenge minute L is committed and its applicable budgets have remaining units
- **WHEN** an otherwise valid challenge uses an admission-time sample from an earlier minute
- **THEN** it consumes remaining units in L without moving L backwards or refilling either budget
- **AND** its signed lifetime remains checked against the supplied provider admission time, not the logical rate minute

#### Scenario: Reordered writers cannot revive an exhausted minute

- **GIVEN** an applicable challenge budget in committed minute L is exhausted
- **WHEN** another connection submits an otherwise admissible challenge with a sample older than L
- **THEN** issuance is refused with 429/`rate_limit`
- **AND** another sample in L still observes the exhausted allowance after reopening the ledger

#### Scenario: Other control maintenance cannot reset challenge rates

- **GIVEN** challenge allowance in L has been consumed
- **WHEN** a proposal or another non-challenge operation commits maintenance using a later wall-clock minute
- **AND** challenge issuance subsequently samples at or before L
- **THEN** it sees its previously consumed challenge allowance rather than an empty or advanced window
- **AND** challenge maintenance likewise cannot erase or advance independent proposal-rate evidence

#### Scenario: Retained counters survive initial watermark adoption

- **GIVEN** consistent challenge-rate evidence exists without a challenge-minute watermark
- **WHEN** initialization accompanies an otherwise valid issuance
- **THEN** the greatest retained challenge minute and its spent units remain authoritative
- **AND** adoption alone cannot grant a thirteenth wallet unit or a six-hundred-and-first ledger unit
- **AND** malformed or contradictory evidence fails closed for fresh issuance without deleting existing proofs or accepted account/funding state

### Requirement: Challenge rate charging is atomic and not refunded by proof lifecycle

Effective-minute selection, both budget checks and charges, pending-state maintenance, capacity checks and challenge insertion SHALL form one atomic admission operation across competing ledger writers. A successful challenge response SHALL follow durable commitment. A rejected or failed issuance SHALL NOT spend a rate unit, advance the committed challenge minute, leave a partial challenge record or preserve partial pending-state mutations. A refusal in one bucket SHALL NOT debit the other bucket.

Once insertion commits, response loss, challenge expiry, successful consumption, failed authentication or a later business refusal SHALL NOT refund its issuance-rate units. Consumption SHALL continue to release pending occupancy under the separate owner-control contract, not rate allowance. Authentication of an already-issued valid proof SHALL NOT acquire another challenge-rate charge or fail solely because fresh issuance is exhausted. Existing bearer-authorized reads and inference SHALL NOT acquire this challenge-issuance gate. Their own authorization, balance, admission and expiry rules SHALL remain in force; operations needing a fresh owner proof can still be unavailable when issuance is exhausted.

Rate accounting, refusal and cleanup SHALL preserve unrelated valid challenges, retained signed agreements, funding acknowledgements, account/credential state and original payment/nonce ownership. None of these rate rules SHALL authorize automatic paid retries, a replacement transaction, implicit credential rotation or a reset of provider/network identity.

#### Scenario: Wallet refusal preserves another wallet's last shared unit

- **GIVEN** a wallet has exhausted its individual challenge budget and one ledger unit remains
- **WHEN** further issuance attempts for that wallet are refused
- **THEN** their tentative ledger charges are rolled back
- **AND** another eligible wallet can commit a challenge using the last shared unit

#### Scenario: Writers compete for the last rate unit

- **GIVEN** one applicable challenge rate unit remains and pending capacity and other gates permit two candidates
- **WHEN** two independent ledger connections concurrently attempt issuance
- **THEN** exactly one candidate commits and consumes the unit
- **AND** the other is refused without a stored challenge, extra charge or leaked minute advancement

#### Scenario: Failed future-minute issuance rolls back clock and cleanup

- **GIVEN** a ledger has committed challenge minute L
- **WHEN** a candidate samples a later minute but fails after tentative cleanup/counter changes, including on nonce collision or insertion failure
- **THEN** its insertion, both charges, proposed minute advancement and pending mutations are rolled back together
- **AND** a subsequent admission at or before L sees the allowance it would have had without that failed candidate

#### Scenario: Consumed or undisclosed challenges do not refund issuance

- **GIVEN** a challenge has been durably stored and charged in the current effective minute
- **WHEN** its response is lost, it expires, or a valid owner proof consumes it before an operation succeeds or is refused
- **THEN** its rate units remain spent until a strictly newer effective minute is successfully entered
- **AND** releasing its pending slot does not grant payment, account or credential authority

#### Scenario: Exhausted issuance does not revoke existing authority

- **GIVEN** fresh challenge issuance is rate-exhausted but an unexpired stored proof or an appropriate account bearer is available
- **WHEN** the holder performs an otherwise authorized operation using that existing authority
- **THEN** challenge issuance exhaustion alone does not refuse it or debit another issuance unit
- **AND** fresh-proof requirements, normal authorization and business gates are not bypassed for callers lacking such authority
