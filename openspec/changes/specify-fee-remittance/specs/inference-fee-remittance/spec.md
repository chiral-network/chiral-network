## ADDED Requirements

### Requirement: The platform fee is a funding charge, never a request charge

The fee amount, the 50-basis-point rate and the credit arithmetic are governed by `inference-funding-settlement` (*The ledger balance identity covers reversals and loss credits*) and, for top-ups, by `inference-account-topup-request`. This capability governs when a fee exists (book lines 154, 528, 571 and 575).

The platform fee SHALL be charged once for each accepted funding payment, opening or top-up, at the moment that payment is first credited. It is a funding charge, not an additional charge per inference request (book line 528). No other event SHALL create, increase or decrease a platform fee or a fee obligation. The only exceptions are the orphan reversal and restoration of that same payment, which `inference-funding-settlement` defines (*A proven orphan freezes spending, reverses unused credit and journals loss* and *Orphan restoration compensates the reversal exactly once*; book line 540, `net_fee_charges`). The events that SHALL NOT change a fee include quote issuance, funding-intent acknowledgement, opening or top-up replay, request reservation, request settlement, unknown-usage release, an inference waiver, credential operations and evidence-only renewal. A free opening carries no funding payment, so it SHALL create no fee and no fee obligation. A funding payment whose fee is zero wei, which is any payment below 200 wei, SHALL create no fee obligation. An inference waiver or a released hold SHALL NOT refund or reduce a platform fee (book line 571), as `inference-request-accounting` also requires.

#### Scenario: A paid account's fee does not grow with requests

- **GIVEN** a 1,000-wei opening credited with a 5-wei fee
- **WHEN** three requests settle for 50 wei each and a fourth request settles with an inference waiver
- **THEN** the account's `fee_wei` is still 5 and its `spent_wei` is 150
- **AND** exactly one fee obligation exists for the account, the 5-wei obligation of the opening, and none exists for any request

#### Scenario: A free opening creates no obligation

- **GIVEN** a free agreement whose input price, output price and minimum funding are all zero
- **WHEN** the account is opened and requests settle on it
- **THEN** no fee and no fee obligation exist for it
- **AND** an explicit `fees/remit` seals no batch for it

#### Scenario: A fee below one wei creates no obligation

- **WHEN** a 199-wei top-up is accepted
- **THEN** its fee is 0 wei and no fee obligation is recorded for it
- **AND** a later 200-wei top-up on the same account records exactly one obligation of 1 wei

#### Scenario: An acknowledgement is not a fee

- **GIVEN** a durably acknowledged opening intent whose payment has not been credited
- **THEN** no fee obligation exists for that agreement

### Requirement: Each nonzero fee creates one durable obligation owed to the agreement's platform wallet

In the same ledger transaction that credits a funding payment with a nonzero fee, the provider SHALL record exactly one fee obligation for that payment (book line 528). The obligation SHALL hold the funding transaction hash, the platform wallet, the exact fee amount as a canonical decimal wei string, and the acceptance time, and it SHALL start in the pending state.

The platform wallet SHALL be the `platform_wallet` of the accepted signed agreement under which the payment was credited (book lines 126, 153, 256 and 480). It SHALL NOT be taken from the provider's current configuration, a refreshed offer or any peer (book line 63). A later configuration change SHALL NOT redirect an existing obligation.

Obligations SHALL be durable. Restart, drain, offer expiry, performance withdrawal and stopping a desktop share SHALL NOT erase, reduce or reassign them (book lines 631, 634 and 737). Replaying an accepted payment SHALL NOT create a second obligation.

#### Scenario: A top-up records one obligation to the agreement's platform wallet

- **GIVEN** an active paid agreement whose signed quote names platform wallet `0x3333333333333333333333333333333333333333`
- **WHEN** a 1,000-wei top-up is accepted at Unix time 1,700,000,000
- **THEN** exactly one obligation is recorded: the top-up's transaction hash, platform wallet `0x3333333333333333333333333333333333333333`, amount `"5"`, created 1,700,000,000, state pending
- **AND** resubmitting the same top-up records no second obligation

#### Scenario: A configuration change does not redirect existing obligations

- **GIVEN** a pending 5-wei obligation owed to `0x3333333333333333333333333333333333333333`
- **WHEN** the operator restarts the provider with `platform_wallet` set to `0x4444444444444444444444444444444444444444`
- **THEN** the existing obligation is still owed, and later remitted, to `0x3333333333333333333333333333333333333333`
- **AND** only payments credited under agreements that name `0x4444444444444444444444444444444444444444` create obligations owed to it

#### Scenario: Drain and restart keep obligations

- **GIVEN** a pending 5-wei obligation
- **WHEN** the operator drains the provider, its offers expire and the daemon restarts
- **THEN** the obligation is unchanged and still pending

### Requirement: Obligations move only forward through pending, batched, submitted and confirmed

Each ordinary fee obligation SHALL be in exactly one of four states, which the provider SHALL track separately (book line 528):

- **pending**: recorded and not yet sealed into a remittance batch;
- **batched**: sealed into a batch whose remittance transaction is not yet recorded;
- **submitted**: its batch's signed remittance transaction is durably recorded with the batch and is not confirmed;
- **confirmed**: its batch's remittance is confirmed under *A remittance is confirmed only under the funding settlement policy*.

Sealing SHALL move an obligation from pending to batched. Durably recording the batch's signed transaction SHALL move it to submitted. Confirmation SHALL move it to confirmed. Monitoring that no longer confirms the remittance SHALL move it back to submitted. Apart from the orphan transitions in the next paragraph, no other transition SHALL occur. A sealed obligation SHALL NOT return to pending and SHALL NOT join another batch.

Obligations of orphaned funding are governed by `inference-funding-settlement` (*A proven orphan freezes spending, reverses unused credit and journals loss* and *Orphan restoration compensates the reversal exactly once*): an obligation not yet sealed is cancelled, and a sealed one is marked orphaned while its batch keeps its state. Restoration returns a cancelled obligation to pending and an orphaned one to the state of its batch. The names a ledger stores for these states are implementation values; the operator view uses the four names above.

#### Scenario: One obligation through all four states

- **GIVEN** a pending 5-wei obligation
- **WHEN** its batch is sealed, the batch's signed remittance is recorded, and the remittance is confirmed
- **THEN** the obligation is batched, then submitted, then confirmed, and its amount stays 5 wei throughout

#### Scenario: A sealed obligation never returns to pending

- **GIVEN** a batched 5-wei obligation
- **WHEN** its remittance fails to submit, the daemon restarts, or monitoring stops confirming its remittance
- **THEN** the obligation stays in the same batch, batched or submitted, and is never pending again or part of a second batch

### Requirement: Automatic aggregation seals a platform wallet's obligations at 10^15 wei or 86,400 seconds

While the provider daemon runs, maintenance SHALL periodically evaluate each platform wallet's pending obligations (book line 528). It SHALL seal them when either condition holds:

- the sum of all of that wallet's pending obligations is at least 10^15 wei (1,000,000,000,000,000 wei); or
- the oldest of them has reached 86,400 seconds, meaning the current time minus its acceptance time is at least 86,400 seconds.

Both thresholds are inclusive. The amount condition SHALL be evaluated over every pending obligation of the wallet, not a bounded subset. When either condition holds, maintenance SHALL seal every pending obligation of that wallet in the same pass, into one batch or, where a work or monetary bound requires, into several batches. A batch SHALL pay exactly one platform wallet. Obligations owed to different platform wallets SHALL NOT share a batch, and their sums SHALL NOT be combined. Sums SHALL use exact checked integer arithmetic.

Sealing and remittance SHALL continue while the provider is draining, while its offers are withdrawn, during a performance withdrawal and while new funding is not ready, because none of these erases obligations (book lines 631 and 737).

#### Scenario: The amount threshold is inclusive

- **GIVEN** pending obligations to platform wallet A totalling 999,999,999,999,999 wei, the oldest 3,600 seconds old
- **WHEN** maintenance runs
- **THEN** no batch is sealed
- **AND** after a further 1-wei obligation to A is recorded, the next maintenance pass seals one batch of exactly 1,000,000,000,000,000 wei containing all of them

#### Scenario: The age threshold is inclusive

- **GIVEN** a single pending 5-wei obligation accepted at time T
- **WHEN** maintenance runs at T + 86,399
- **THEN** it is not sealed
- **AND** when maintenance runs at T + 86,400 it is sealed into a 5-wei batch

#### Scenario: Platform wallets aggregate separately

- **GIVEN** 600,000,000,000,000 wei of pending obligations owed to platform wallet A and 600,000,000,000,000 wei owed to platform wallet B, all younger than 86,400 seconds
- **WHEN** maintenance runs
- **THEN** no batch is sealed
- **AND** when A's oldest obligation reaches 86,400 seconds, only A's obligations are sealed, into a batch that pays A

#### Scenario: Many small obligations still reach the amount threshold

- **GIVEN** 1,200 pending obligations of 900,000,000,000 wei each, owed to platform wallet A and totalling 1,080,000,000,000,000 wei, all younger than 86,400 seconds
- **WHEN** maintenance runs
- **THEN** all 1,200 obligations are sealed for A in that pass, without waiting for the age threshold

#### Scenario: A drained provider still remits

- **GIVEN** a drained provider with its offers withdrawn and one pending 5-wei obligation accepted at time T
- **WHEN** maintenance runs at T + 86,400
- **THEN** the obligation is sealed and its remittance is submitted

### Requirement: A sealed batch is immutable and pays the exact sum of its members

Sealing SHALL atomically record a batch with a unique identifier, its platform wallet, its amount and its creation time, and assign each member obligation to it. The amount SHALL equal the exact sum of the members. An obligation SHALL belong to at most one batch.

After sealing, a batch's platform wallet, amount and members SHALL NOT change. Once a remittance transaction is recorded for a batch, its transaction hash and signed bytes SHALL NOT change. The ledger SHALL refuse any update that would alter them and leave the stored batch unchanged. A restarted provider SHALL find the same batch.

Orphaned funding SHALL NOT rewrite a sealed batch, whatever its state. A member obligation whose funding is proven orphaned keeps its batch and amount, the batch is remitted in full, and the account accounting of the shortfall as provider loss follows `inference-funding-settlement` (its design decision 3). An obligation cancelled before sealing SHALL NOT be sealed or remitted while it stays cancelled; only restoration of its funding under `inference-funding-settlement` returns it to pending.

#### Scenario: A batch pays the exact sum of its members

- **GIVEN** pending obligations of 5 wei and 7 wei owed to platform wallet A
- **WHEN** they are sealed
- **THEN** one batch of 12 wei paying A contains both, and neither belongs to any other batch

#### Scenario: Sealed terms cannot change

- **GIVEN** a sealed 5-wei batch whose remittance transaction is recorded
- **WHEN** an update would change its amount to 6 wei, its platform wallet, its transaction hash or its signed bytes
- **THEN** the update is refused and the stored batch is unchanged

#### Scenario: Restart returns the same batch

- **GIVEN** a sealed 5-wei batch with identifier X and no recorded transaction
- **WHEN** the provider restarts and maintenance runs
- **THEN** batch X with amount 5 wei is the one submitted, and no second batch contains its obligation

#### Scenario: Orphaned funding does not rewrite a sealed batch

- **GIVEN** a 1,000-wei opening whose 5-wei obligation is batched, submitted or confirmed, with 500 wei spent
- **WHEN** the opening is proven orphaned
- **THEN** the batch still pays 5 wei and keeps its state, the obligation is reported as orphaned, and the account shows `fee_wei` 5 and `provider_loss_wei` 505
- **AND** had the obligation still been pending, it would have been cancelled and not sealed while the opening stays orphaned

### Requirement: Explicit remittance is a local operator action on the loopback administration listener

`fees/remit` SHALL be an explicit operator action (book line 528) served only as `POST /admin/fees/remit` on the provider's administration listener. The fee view SHALL be `GET /admin/fees` on the same listener. That listener SHALL bind an explicit nonzero loopback port (book line 274) and SHALL require the protected administrator bearer on every request (book line 658). An account bearer or wallet control challenge SHALL NOT be accepted as that bearer. Every request carrying an `Origin` header SHALL be refused (`CLAUDE.md`). `/admin/*` SHALL NOT be mounted on the public listener.

`fees/remit` SHALL seal every pending nonzero obligation, whatever its amount or age, into batches by platform wallet, as the automatic pass does once a threshold is met. It SHALL then submit and verify the unconfirmed batches under the remittance, confirmation and recovery requirements of this capability, and SHALL return the fee view. Work per call MAY be bounded only per batch; it SHALL NOT leave pending an obligation that existed when the call began. A failure SHALL be answered with a fixed error that carries no RPC content, secret or path, and its fixed category SHALL be retained as the latest fee failure in the operator status. `fees/remit` SHALL NOT send a second transaction for a batch or resend a confirmed batch. `GET /admin/fees` SHALL change no state.

The operator helper `scripts/provider-admin.py` SHALL read the bearer from a protected file, never from its arguments, and SHALL refuse a token file that is group- or world-accessible. It SHALL connect only to `127.0.0.1` with environment proxies disabled. The native desktop and CLI `provider_admin` command SHALL accept only a literal loopback endpoint, read the token file natively and never return the token (book line 944).

#### Scenario: Explicit remittance below the thresholds

- **GIVEN** one pending 5-wei obligation, 100 seconds old
- **WHEN** the operator sends `POST /admin/fees/remit` with the administrator bearer on the loopback listener
- **THEN** one 5-wei batch is sealed and its remittance is submitted
- **AND** the response is the fee view, which shows that batch

#### Scenario: A browser origin is refused

- **WHEN** `POST /admin/fees/remit` carries the valid bearer and the header `Origin: http://example.test`
- **THEN** the provider answers HTTP 401 and seals no batch

#### Scenario: The public listener has no fee administration

- **WHEN** `POST /admin/fees/remit` or `GET /admin/fees` is sent to the public listener
- **THEN** the provider answers HTTP 404 and changes no state

#### Scenario: Another credential is not the administrator bearer

- **WHEN** `POST /admin/fees/remit` carries a consumer account bearer
- **THEN** the provider answers HTTP 401 and seals no batch

#### Scenario: The helper protects the bearer

- **GIVEN** an administrator token file with mode 0644
- **WHEN** the operator runs `provider-admin.py --token-file <file> fees/remit`
- **THEN** the helper refuses before sending any request
- **AND** with mode 0600 it sends the bearer only in the `Authorization` header to `127.0.0.1`, with environment proxies disabled

#### Scenario: Viewing fees changes nothing

- **GIVEN** one pending 5-wei obligation, 100 seconds old
- **WHEN** the operator sends `GET /admin/fees` twice
- **THEN** both responses show 5 wei pending, and no batch is sealed

### Requirement: Each batch is remitted by one provider-wallet transfer through the shared journal, and the provider pays the gas

Each batch SHALL be remitted by exactly one original transaction signed by the provider's signing wallet (book lines 50 and 470). Its recipient SHALL be the batch's platform wallet, its value SHALL equal the batch amount, and its calldata SHALL commit to the batch identifier. The transaction SHALL be durably journaled, under an operation identifier bound to the batch, before any broadcast. It SHALL follow the journal rules of `inference-funding-settlement` (*Every spender of one wallet shares one coordinated nonce journal* and *Original operations keep sender and nonce ownership through terminal status and regression*). While another original of the same wallet is unresolved, a remittance SHALL wait rather than queue behind it or replace it.

The provider SHALL pay the remittance gas from its own wallet (book lines 37, 528 and 733). Remittance gas SHALL NOT be deducted from or charged to any consumer account, and SHALL NOT change any account's `fee_wei`, `spent_wei`, `reserved_wei` or `available_wei` or reduce the fee owed. When the wallet cannot pay the batch amount plus gas, no transaction SHALL be signed, the batch SHALL stay batched, and the failure SHALL be reported in a fixed category. A remittance failure is operator debt. It SHALL NOT be evidence against consumer credit and SHALL NOT change any account's status, available balance or the provider's new-funding readiness.

#### Scenario: The remittance transaction

- **GIVEN** a sealed 5-wei batch with identifier X paying platform wallet A
- **WHEN** maintenance remits it
- **THEN** the shared wallet journal records, before broadcast, one transaction from the provider wallet to A with value 5 wei and calldata committing to X
- **AND** that transaction is the only original ever journaled for batch X

#### Scenario: Insufficient gas defers remittance without touching consumers

- **GIVEN** a sealed 5-wei batch and a provider wallet that cannot pay 5 wei plus the estimated gas
- **WHEN** maintenance runs
- **THEN** no transaction is signed and the batch stays batched
- **AND** the fee failure names the fixed category "wallet balance insufficient for payment and gas"
- **AND** every consumer account keeps its status and balances, and new-funding readiness is unchanged by the failure

#### Scenario: A busy wallet defers remittance

- **GIVEN** the provider wallet's journal holds an unresolved consumer opening that shares the same base directory
- **WHEN** maintenance tries to remit a sealed batch
- **THEN** no remittance is signed in that pass and the batch stays batched
- **AND** the batch is remitted in a later pass after the opening resolves

#### Scenario: Gas is never a consumer charge

- **GIVEN** an active account with funded 1,000, fee 5, spent 150 and available 845 wei
- **WHEN** the batch containing its 5-wei obligation is remitted and confirmed
- **THEN** the account still shows funded 1,000, fee 5, spent 150 and available 845 wei, and the gas is paid only from the provider wallet

### Requirement: A remittance is confirmed only under the funding settlement policy

A remittance SHALL be confirmed only after the exact signed transaction recorded with its batch verifies against the configured identity-checked RPC. The decoded sender SHALL be the provider wallet, the recipient the batch's platform wallet, the value the batch amount, the calldata the batch commitment, and the network the configured network identity. Execution SHALL be successful and canonical at the healthy eSpace `safe` frontier under `canonical-successful-receipt-at-espace-safe-v1`, observed consistently twice, as `inference-funding-settlement` defines that policy (book line 514). A broadcast acknowledgement, a mempool observation, a first receipt or execution above the `safe` frontier SHALL NOT confirm a remittance. The ledger SHALL refuse to record a confirmation whose evidence does not match the batch.

Confirmation is safe acceptance, not PoS finality (book line 514). No surface SHALL describe a confirmed remittance as final or irreversible. Confirmation is the only evidence of on-chain arrival at the platform wallet that the provider records.

#### Scenario: Executed but not yet safe

- **GIVEN** a submitted 5-wei remittance whose successful receipt is in block 120 while the `safe` frontier is block 110
- **WHEN** maintenance verifies it
- **THEN** the batch stays submitted

#### Scenario: Confirmed at the safe frontier

- **GIVEN** the same remittance, with block 120 canonical and the `safe` frontier at block 130 in two consistent observations
- **WHEN** maintenance verifies it
- **THEN** the batch and its obligations are confirmed, with block 120's hash and height recorded as evidence

#### Scenario: Mismatched evidence cannot confirm

- **GIVEN** a submitted 5-wei batch paying A
- **WHEN** a confirmation is offered whose evidence has value 1 wei, a sender other than the provider wallet, a recipient other than A, other calldata or another network
- **THEN** the ledger refuses it and the batch stays submitted

### Requirement: Confirmed remittances are monitored, and a reorganization never causes a second transfer

The provider SHALL periodically recheck confirmed remittances against the same policy. Each pass MAY check a bounded number, but every confirmed batch SHALL be rechecked within a bounded number of passes, however far apart the passes start. Monitoring SHALL apply these outcomes:

- **Accepted again**, in the same or a different canonical block: only the inclusion evidence (block hash and height) SHALL be refreshed.
- **Pending**, for example because the `safe` frontier regressed below its block: the batch and its obligations SHALL return to submitted.
- **Proven orphaned**: the batch and its obligations SHALL return to submitted.
- **Unavailable or contradictory evidence**: the state SHALL stay unchanged and the failure SHALL be reported in a fixed category.

A missing receipt alone SHALL NOT prove an orphan; only stable canonical evidence that the accepted block was replaced does (book line 514). A remittance that returns to submitted SHALL be recovered only by resubmitting its original signed bytes. The provider SHALL NOT sign a new transaction for that batch, raise its fee or replace it, and the same batch SHALL NOT be paid twice (book line 518, `CLAUDE.md`).

#### Scenario: A safe-frontier regression returns a remittance to submitted

- **GIVEN** a batch confirmed in block 120
- **WHEN** a recheck finds block 120 above the healthy `safe` frontier
- **THEN** the batch and its obligations are submitted, and no new transaction is signed

#### Scenario: A proven orphan resubmits the original bytes

- **GIVEN** a 5-wei batch confirmed in block 120 with block hash H1
- **WHEN** stable canonical evidence shows block 120 replaced by a block without the remittance
- **THEN** the batch returns to submitted and only its original signed bytes are broadcast again
- **AND** when the same transaction is later accepted in block 125, the batch is confirmed with block 125's evidence, and exactly one 5-wei transfer exists for the batch

#### Scenario: Every confirmed remittance is rechecked whatever the pass timing

- **GIVEN** 150 confirmed batches, of which each pass may recheck 100
- **WHEN** four maintenance passes run, starting 60 seconds apart
- **THEN** each of the 150 batches has been rechecked at least once

#### Scenario: An outage keeps a confirmation

- **GIVEN** a confirmed batch
- **WHEN** the configured RPC is unavailable during its recheck
- **THEN** the batch stays confirmed and a fixed-category fee failure is reported

### Requirement: Restart and ambiguity recover the original remittance without replacement

A crash or restart at any point SHALL recover the same batch, the same operation identifier and the same signed bytes. This covers a crash after sealing, after journaling, after broadcast, and before the ledger recorded the transaction with its batch. The provider SHALL NOT sign a second transaction for a batch (book lines 470 and 634, `CLAUDE.md`). An uncertain broadcast, such as a timeout, an error or a different returned hash, SHALL leave the remittance unconfirmed and SHALL be recovered by resubmitting identical bytes. A remittance SHALL NOT be automatically fee-bumped, replaced or cancelled.

When a remittance's execution failed canonically at the `safe` frontier, or its nonce was consumed without its receipt, its obligations SHALL NOT be confirmed. They SHALL stay in their batch and SHALL NOT be automatically re-batched or remitted by a new transaction. Maintenance SHALL report the failure in a fixed category on every pass until the operator resolves it (design open question 2). Before resuming after an incident, the operator reconciles pending chain submissions and fee batches (book line 677). Backups and restores SHALL preserve exact batch records and journaled bytes (book line 673).

#### Scenario: Crash after journaling

- **GIVEN** a remittance signed and journaled for batch X, and a daemon killed before the ledger recorded the transaction with X
- **WHEN** the daemon restarts and maintenance runs
- **THEN** the same signed bytes are resubmitted and recorded with X
- **AND** the wallet journal holds exactly one transaction for X

#### Scenario: An ambiguous broadcast is retried with identical bytes

- **GIVEN** a remittance broadcast that timed out
- **WHEN** maintenance runs again
- **THEN** it broadcasts the identical signed bytes, and no other transaction is signed for the batch

#### Scenario: A failed remittance is not confirmation and is not replaced

- **GIVEN** a 5-wei remittance whose execution failed canonically at the `safe` frontier
- **WHEN** maintenance runs repeatedly
- **THEN** the batch is not confirmed and its obligation stays in the batch
- **AND** no new transaction is signed for it, and each pass reports the failure

#### Scenario: No fee bump

- **GIVEN** a remittance that has remained unmined for 6 hours
- **WHEN** maintenance runs
- **THEN** only its original signed bytes are broadcast, never a replacement with a higher gas price

### Requirement: The operator fee view separates obligations from actual remittances

The fee view (`GET /admin/fees`, also returned by `fees/remit`) SHALL report:

- the exact totals, as canonical decimal wei strings, of ordinary obligations in each of the pending, batched, submitted and confirmed states (book line 528);
- separately, the totals of cancelled obligations and of orphaned obligations, by their batch's state;
- the remittance batches, newest first and bounded in number, each with its identifier, platform wallet, amount, state, its transaction hash once recorded, and its inclusion block hash and height once confirmed;
- the aggregation thresholds, 10^15 wei and 86,400 seconds;
- that fee collection is provider-remitted and not trustless, and that a recorded obligation is not proof that the platform received a fee.

The latest fee failure SHALL appear in the operator status in a fixed category, never as raw RPC content or a secret. Only confirmed amounts SHALL be described as remitted. The fee view is the operator's means to reconcile fee batches before resuming after an incident (book line 677). Desktop surfaces that show fee state SHALL present this data without describing unconfirmed obligations as remitted (book lines 945, 967 and 1001). These surfaces are the A.5.5 Provider page and, when implemented, the Share dashboard of `desktop-share-online`.

#### Scenario: Each state is reported separately

- **GIVEN** ordinary obligations of 5 wei pending, 7 wei batched, 11 wei in a batch whose remittance is recorded but not confirmed, and 13 wei confirmed
- **WHEN** the operator reads the fee view
- **THEN** it reports pending 5, batched 7, submitted 11 and confirmed 13 wei

#### Scenario: Actual remittances are listed

- **GIVEN** a 13-wei batch paying A, confirmed in block 120
- **WHEN** the operator reads the fee view
- **THEN** the batch appears with its identifier, A, 13 wei, state confirmed, its transaction hash, and block 120's hash and height

#### Scenario: Cancelled and orphaned obligations are not ordinary debt

- **GIVEN** a 5-wei obligation cancelled before sealing and a 7-wei obligation orphaned after sealing in a submitted batch
- **WHEN** the operator reads the fee view
- **THEN** the 5 wei appears only as cancelled and the 7 wei only as orphaned with batch state submitted, and neither appears in the ordinary totals

### Requirement: Fee collection is provider-remitted and not trustless

Remittance depends on the provider, and a provider can fail to remit platform fees (book line 33). Nothing in the protocol forces a remittance. A consumer's fee charge SHALL be fixed when its funding is accepted and SHALL NOT depend on remittance. Remittance success, failure, delay or reorganization SHALL NOT change any consumer account, its status, its available balance or the provider's new-funding readiness.

A recorded fee obligation, and an account's `fee_wei`, SHALL NOT be described on any surface as proof that the platform received the fee (book line 528). No surface SHALL describe fee collection as trustless, guaranteed or escrowed (`CLAUDE.md`). The account-snapshot wording of `fee_wei` is governed by `inference-account-snapshot`.

#### Scenario: Remittance failure does not touch consumers

- **GIVEN** an active account with available 845 wei, and a remittance that fails for lack of gas on every pass for 3 days
- **THEN** the account stays active with available 845 wei, and new-funding readiness is not closed by the fee failure

#### Scenario: An obligation is not a receipt

- **GIVEN** an account with `fee_wei` 5 whose obligation is pending
- **WHEN** the operator reads the fee view
- **THEN** the 5 wei appears as pending, not remitted
- **AND** the view states that collection is provider-remitted and not trustless and that recorded obligations are not proof of platform receipt
