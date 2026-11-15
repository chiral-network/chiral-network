## ADDED Requirements

### Requirement: Request identity prevents automatic paid replay

The public inference boundary SHALL accept at most one `Idempotency-Key`, containing 1–128 ASCII letters, digits, dots, underscores, colons or hyphens. Without a supplied key it SHALL generate a new UUID; this SHALL NOT be represented as recovery of a prior uncertain operation. The ledger SHALL bind the client key to its account and persist a separate opaque globally unique request identifier with the reservation. The same client key in another account SHALL be independent.

Once reserved, an account/key SHALL NOT create another execution or hold, including after terminal settlement, unknown-usage recovery or restart. A public duplicate SHALL return HTTP 409 with `duplicate_request`, the original request identifier and its current status, rather than replay generation or retained output. Request status and receipt access SHALL retain account isolation and existing read authorization. Native workflows and supplied SDK examples SHALL NOT automatically retry paid inference with a new key or another provider after an ambiguous response, cancellation or interrupted stream.

#### Scenario: Duplicate reserved or terminal request

- **GIVEN** an account/key already identifies a reserved or terminal request
- **WHEN** another inference request supplies that key for the same account
- **THEN** the provider returns its original identifier/status with `duplicate_request` without another hold or engine execution
- **AND** restart or a different payload does not free that key for another paid operation

#### Scenario: Account-scoped keys and absent keys

- **WHEN** different accounts supply the same valid client key
- **THEN** their independent authorized requests may be admitted under their own limits
- **AND** omitting a key creates a new operation, not an inferred retry of either account's previous request

### Requirement: Maximum reservation and nominal usage cost round once

Money SHALL obey the canonical decimal `u128` wei contract in `inference-wire-foundation`. For the immutable agreement's input and output rates `Ri` and `Ro`, exact prepared input count `P` and authorized maximum output count `G`, maximum reservation SHALL be `M = ceil((P * Ri + G * Ro) / 1000)`. Nominal actual cost for validated usage `(I, O)` SHALL be `N = ceil((I * Ri + O * Ro) / 1000)`. Each sum SHALL be rounded once, not per component, token, output fragment or stream event. Intermediate arithmetic SHALL preserve the full products and sum; final monetary overflow SHALL fail rather than wrap, clamp, truncate or use floating point.

Exact prompt/template tokenization and supported bounded options SHALL precede reservation. Generation SHALL use bounded output and `n = 1`; unsupported expansion SHALL be rejected rather than under-reserved. Embeddings SHALL reserve input-only cost with zero output budget and retain their separate bounded batch/context contract. Prompt/cache inputs SHALL use the quoted input rate. Genuinely free requests SHALL retain authentication, identity and admission accounting even when `M` and `N` are zero. This calculation SHALL NOT impose another funding/platform fee or include chain gas.

#### Scenario: Combined fractions are rounded once

- **WHEN** input and output rates are each one wei per thousand tokens and actual usage is one input plus one output token
- **THEN** nominal request cost is one wei, not two wei
- **AND** splitting that output across stream events does not change its cost

#### Scenario: Full-width price and overflow

- **WHEN** the input rate is `u128::MAX`, output rate is zero and input usage is 1,000 tokens
- **THEN** nominal cost is exactly `u128::MAX`
- **AND** an otherwise equivalent 1,001-token amount is rejected as out of range without a partial monetary mutation

### Requirement: Reservation atomically binds authority limits and credit

Before paid engine dispatch the provider SHALL durably create the request and reserve `M` in one transaction with all account/credential checks and updates. The credential SHALL belong to the account, be unrevoked, unexpired at the provider-selected operation-time sample and carry `infer` scope. The account SHALL be active or genuinely free. Exact model/context/output limits and compatible admitted execution terms SHALL apply; a model or price substitution SHALL NOT make an old balance usable under different terms.

The transaction SHALL require available account credit at least `M`, account and credential active-request counts each below their respective concurrency limits, and any credential spend cap at least `credential.spent + credential.reserved + M`. It SHALL subtract `M` from account available credit and add `M` to both account and credential reservations. It SHALL persist request/account/credential identity, client key, prepared input count, maximum output count, maximum cost, creation time and applicable admitted performance evidence before dispatch. Competing writers SHALL NOT overspend one balance, exceed a cap/concurrency bound or admit the same account/key twice.

A definite reservation refusal or failed reservation transaction SHALL leave no partial request or hold and SHALL NOT dispatch inference. Credential revocation/rotation SHALL prevent newly unauthorized work but SHALL NOT erase, release or move an already-running request's hold; terminal accounting SHALL still use its original account/credential. A returned reservation is not model-execution or authoritative-usage evidence.

#### Scenario: Competing requests cannot overspend

- **GIVEN** two independently connected writers each attempt a reservation that the same balance can cover individually but not together
- **WHEN** their reservation transactions compete
- **THEN** at most one succeeds and the refused request leaves no hold or request record
- **AND** account available/reserved amounts and credential holds reflect only the winner

#### Scenario: Cap, scope or concurrency refusal is atomic

- **WHEN** the credential lacks inference authority, its spent-plus-held amount would exceed its cap, or an account/credential concurrency bound is reached
- **THEN** reservation refuses without partial financial mutation or dispatch
- **AND** free pricing does not bypass these authority and concurrency checks

#### Scenario: Revocation during an admitted request

- **GIVEN** an authorized request already holds prepaid credit
- **WHEN** its credential expires, is revoked or is explicitly rotated before settlement
- **THEN** the original hold survives and the request settles against that original credential's accounting
- **AND** no replacement credential gains its hold or authorizes a duplicate execution

### Requirement: Inference abuse budgets remain independent

Before reservation the ledger SHALL refuse when that credential already has at least 120 request records created at or after `max(0, now - 60)`, or that account already has at least 30 `*_unknown_usage` request records created at or after `max(0, now - 3600)`. `now` SHALL be the provider-selected operation-time sample, not a client-supplied timestamp. Retained future-dated records SHALL not be omitted solely because the current sample regressed. The windows SHALL use request creation time, not pretend to measure engine work or time of terminal settlement.

These inference-specific bounds SHALL remain separate from proposal/challenge budgets and public named-wallet quote policy. Terminal status changes SHALL NOT delete request identity to restore allowance. They SHALL NOT gate completion or recovery of an already-reserved request. This bounded policy SHALL NOT be described as Sybil protection or a non-regressing effective-minute issuance clock.

#### Scenario: Exhausted inference budget does not strand a hold

- **GIVEN** an already-reserved request and exhausted fresh inference admission allowance
- **WHEN** validated terminal usage or unknown-usage recovery is applied to that request
- **THEN** its original hold can be settled or released without admitting another request
- **AND** unrelated quote or challenge allowance is neither charged nor replenished

### Requirement: Billable usage is authoritative and matches the reservation

Only usage validated from the configured private engine under the admitted model/tokenizer/template contract SHALL become billable usage. Input and output counts SHALL be unsigned integers; any reported total SHALL agree with their checked sum under the supported response contract. Input usage SHALL equal the exact prepared count and output usage SHALL not exceed the authorized maximum. Missing, malformed, inconsistent, early-truncated or unsupported usage SHALL NOT be replaced by text length, retokenization estimates, stream-event counts or assumed zero execution. Embedding normalization MAY supply structural zero output when the input and total evidence are valid and consistent; it SHALL NOT invent an input count.

Known usage after cancellation, disconnection or a failed response SHALL remain eligible for its exact charge. Receiving a cancellation acknowledgement such as 202 `cancelling`, or failing to receive the final response/event, SHALL NOT establish a free execution. Absence of validated usage SHALL follow the explicit unknown-usage rule instead. Clients SHALL NOT submit arbitrary token counts as authoritative settlement input through a public accounting endpoint.

#### Scenario: Prompt or output mismatch

- **WHEN** final engine usage changes the prepared input count, exceeds the reserved output bound or contradicts its total
- **THEN** the request is not charged from those counts
- **AND** its unsettled hold is resolved under the explicit unknown-usage policy rather than left locked or charged an estimate

#### Scenario: Known usage survives client interruption

- **GIVEN** valid authoritative usage for the original reservation
- **WHEN** the client cancels or loses the output connection
- **THEN** accounting may charge that known usage, subject to any applicable inference waiver
- **AND** transport outcome does not create a second request or refund the funding deposit

### Requirement: Terminal request accounting is atomic and at most once

For a reserved request with validated usage and nominal cost `N`, the actual charge `C` SHALL equal `N` unless an applicable validated inference waiver sets it to zero. The terminal transaction SHALL remove its full original hold `M` from account and credential reservations, return exactly `M - C` to account available credit, add exactly `C` to both spent totals, add known usage to cumulative account usage with checked arithmetic, and persist the terminal request's usage, cost, status, settlement time and applicable performance/waiver outcome together. It SHALL NOT create a negative balance or charge more than the hold.

The first committed terminal outcome SHALL win. Repeating a valid settlement operation, including with later different usage/status/evidence or after restart, SHALL return the existing terminal result without rewriting it or applying another monetary/usage update. Terminal settlement is not permission to issue a new request with the old key. Financial persistence failure SHALL NOT be acknowledged as successful durable settlement; the gateway SHALL stop new admission and retain the original request for recovery rather than suppress the accounting failure. An explicit invalid-usage terminal release is distinct from a transaction failure.

Absent independent funding-reversal/loss transitions, `funded = fees + spent + reserved + available` SHALL remain true. With such transitions, the existing extended funding identity SHALL remain true without request accounting altering the independent funding/reversal/fee evidence. Releasing unused hold SHALL NOT be described as refunding an on-chain deposit.

#### Scenario: Partial usage releases exactly the unused hold

- **GIVEN** a 300-wei hold and validated nominal usage cost of 150 wei without waiver
- **WHEN** settlement commits
- **THEN** both reservation totals fall by 300, both spent totals rise by 150 and available credit rises by 150
- **AND** a repeated terminal call changes none of those amounts or the recorded usage/time

#### Scenario: Persistence failure cannot acknowledge an unrecorded debit

- **WHEN** a terminal transaction cannot durably commit its request/account/credential updates
- **THEN** no partial accounting state is accepted as completion and the gateway stops admitting new work
- **AND** the same retained request remains subject to reconciliation, not automatic generation replay

### Requirement: Unknown usage releases the hold without inventing execution

Absent, invalid or lost authoritative usage SHALL resolve an unsettled request with zero charge, its whole hold returned, and an explicit `*_unknown_usage` terminal status. Stored zero token counters in that outcome SHALL mean unknown usage, not evidence that zero tokens executed. The original request's `provider_loss_wei` SHALL equal its reservation-sized exposure `M`, not an estimate of GPU work or known actual monetary loss. This request exposure SHALL NOT be added to spendable balance or confused with the account's separate funding-orphan compensation field of the same name.

At the ledger boundary, mismatched input counts, output over the reservation or cumulative usage-counter overflow SHALL commit an `invalid_usage_unknown_usage` release and surface the invalid-usage error rather than leave the hold locked. Unknown release SHALL not increment spent or cumulative known usage, create a funding/platform fee, erase request identity or rotate credentials. Its full release and terminal record SHALL have the same atomicity and first-terminal-result-wins protection as a known-usage debit.

#### Scenario: Unknown usage releases account and credential holds

- **GIVEN** an unsettled 300-wei reservation whose authoritative usage is unavailable
- **WHEN** unknown-usage settlement commits
- **THEN** its charge is zero, 300 wei returns to available credit and both holds fall by 300
- **AND** the request records 300 wei of exposure and an unknown-usage status without inventing token counts, funding or another credit

#### Scenario: Late usage cannot re-charge an unknown terminal result

- **GIVEN** a request already terminally released for unknown usage
- **WHEN** later completion or retry supplies plausible token counts
- **THEN** the original zero-charge terminal result remains unchanged and the old request key remains occupied

### Requirement: Restart recovers orphaned reservations before new admission

Provider startup SHALL recover durable requests still marked reserved before accepting new inference. Each orphaned hold SHALL settle once as `restart_unknown_usage`, using the unknown-usage release rule and retaining original identity, agreement, credential and admitted performance evidence. Already-terminal requests SHALL not be changed by restart recovery. No generation, new payment or implicit credential replacement SHALL be initiated to discover an absent outcome.

#### Scenario: Repeated startup recovery is idempotent

- **GIVEN** a durable reserved request from a terminated process
- **WHEN** a fresh provider recovers its ledger before admission and later repeats recovery
- **THEN** the hold is released only once with `restart_unknown_usage`
- **AND** terminal evidence, original deduplication and exported credential authority survive subsequent reopening

### Requirement: Performance remedies preserve usage and admitted evidence

A profiled reservation SHALL durably bind the qualification report admitted before dispatch. Evidence-only renewal SHALL be limited to an actually qualified strictly newer report for otherwise identical serving terms under the whitepaper's renewal contract; it SHALL NOT rewrite the signed agreement, prices, balances or an old withdrawal latch. Terminal performance accounting SHALL bind the reservation's admitted report and policy, not silently substitute a later report, and the binding SHALL survive settlement and unknown-usage restart recovery.

An eligible `inference_waiver` SHALL set actual inference charge to zero, release the hold, retain known authoritative usage and record the nominal waived inference amount once. It SHALL NOT refund funding/platform fees or chain gas or create a second transferable/spendable credit. If usage is unknown, the nominal waived amount SHALL be zero and the explicit unknown-usage exposure/status SHALL remain; a performance outcome SHALL not invent measured usage. Qualification, timing eligibility and withdrawal classification remain governed by the whitepaper's performance contract, not inferred from a capacity label or this accounting interface.

#### Scenario: Waiver retains known usage without a second credit

- **GIVEN** valid known usage, a compatible admitted report and an eligible inference waiver
- **WHEN** terminal accounting commits and is later replayed
- **THEN** actual charge is zero, known usage remains recorded and the nominal waived amount is accumulated only once
- **AND** funded amounts, funding fees, gas obligations and exported credentials are not rewritten

#### Scenario: Unknown restart retains the admitted report

- **WHEN** a profiled reserved request is recovered with unknown usage
- **THEN** its original admitted report remains attached to the terminal request
- **AND** no replacement report, output-token count or nonzero waived amount is fabricated

### Requirement: Request and receipt history supports attributable recovery

Authorized request reads SHALL expose the durable request identity/key, account and credential identities, status, original token bounds and maximum hold, recorded usage/cost/exposure, creation/settlement times and performance/waiver outcome. Request lists SHALL be account-scoped bounded pages of at most 100, ordered by descending creation time with descending insertion order for ties; an optional `before` timestamp SHALL exclude requests created at or after that timestamp. This timestamp paging SHALL NOT be advertised as an exhaustive tie-safe cursor or retained response-content replay.

Usage receipts SHALL use the receipt domain and provider signature from `inference-wire-foundation`, with `{network, provider_wallet, account, requests, sequence, issued_at}`. They SHALL bind the complete network and provider, a consistent account/request snapshot and at most the 100 most recent account requests. The positive receipt sequence SHALL increase durably per account across restart; signing or persistence failure SHALL not return a successful uncommitted receipt. Reads SHALL preserve financial amounts and original request outcomes; receipt issuance may advance only its receipt sequence. Existing owner/read-bearer authorization SHALL apply without implicit credential rotation.

Neither a signed receipt nor a zero charge SHALL be represented as independent execution attestation, complete output storage, irreversible chain finality, a refund promise or a complete lifetime request history. Retaining prior verified receipts is the consumer's way to keep older attributable statements.

#### Scenario: Reopened receipts preserve terminal accounting

- **GIVEN** a settled request and a previously issued signed receipt
- **WHEN** the provider reopens and an authorized caller requests current status and another receipt
- **THEN** status/account/usage amounts remain unchanged and the new receipt binds them with a strictly higher persisted sequence
- **AND** another account's bearer cannot obtain that request or receipt

### Requirement: Quote pressure and new-admission gates cannot strand settlement

Existing-request settlement, unknown-usage release and authorized status/receipt recovery SHALL NOT require new quote retention, signing-work, committed-issuance allowance or paid/free opening capacity. Draining, offer withdrawal, unavailable engine or loss of new-funding readiness SHALL NOT erase an admitted hold or justify refusing its financial resolution. Settlement SHALL preserve immutable terms, original funding/journal evidence, unrelated accounts and existing credential authority.

This exclusion SHALL NOT waive fresh inference's authentication, compatible execution, qualification, concurrency, inference-rate or worst-case-credit checks. A temporary chain outage SHALL not itself manufacture a request loss, credit or funding reversal; separate proven funding transitions retain their own authority. Missing or corrupt accounting state SHALL fail closed rather than fabricate an available balance or successful receipt.

#### Scenario: Full quote resources preserve terminal accounting

- **GIVEN** an existing reserved or terminal request and later-exhausted quote/opening resources
- **WHEN** its original request is settled, recovered after restart or read with valid authority
- **THEN** the financial operation follows its original request contract without fresh quote/opening admission
- **AND** a fresh inference request still has to satisfy its independent admission and reservation limits
