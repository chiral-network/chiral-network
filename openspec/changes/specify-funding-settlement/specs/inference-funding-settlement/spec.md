## ADDED Requirements

### Requirement: Provider credit requires authenticated settlement under the policy

Steps one to five of the paid-opening funding order (book lines 482–486) and the survival of acknowledged terms after quote or admission-lease expiry (book line 491) are governed by `inference-opening-funding-intent` and `inference-commercial-terms`. This capability governs step six (book line 487) and SHALL NOT weaken those capabilities.

The provider SHALL credit a paid opening only through owner-authenticated `POST /v1/contracts/open`, and a top-up only through owner-authenticated `POST /v1/contracts/:id/topup`. Request shape, authority order and eligibility of those routes are governed by `inference-account-opening-request` and `inference-account-topup-request`. Credit SHALL require that the provider independently decode the submitted signed bytes and that the payment satisfy the settlement policy in this capability. A transaction that is only signed, journaled, acknowledged, submitted, in the mempool, or executed but not yet accepted under that policy SHALL NOT create an account, spendable credit, a fee obligation or a credential.

Verification refusal SHALL use these responses:

- HTTP 409 with `funding_pending` for evidence that has not yet settled under the policy.
- HTTP 400 with `funding_rejected` only for a payment that the chain-evidence requirement of this capability classifies as rejected.
- HTTP 503 with `settlement_unavailable` for evidence that the chain-evidence requirement classifies as unavailable, including untrustworthy chain evidence.

In each case the provider MAY retain the bounded original funding attempt as recovery evidence. That record SHALL NOT be spendable funding.

#### Scenario: Executed but not yet safe is not credit

- **GIVEN** an acknowledged paid opening whose original has a successful receipt above the healthy `safe` frontier
- **WHEN** the owner submits the authenticated opening request with that original
- **THEN** the provider returns HTTP 409 with `funding_pending`
- **AND** no account, credential, funding credit or fee obligation is created, and the original remains recoverable

#### Scenario: Failed execution at safe is a rejected payment

- **GIVEN** an acknowledged paid opening whose original executed with status `0` and is canonical at `safe` in two consistent observations
- **WHEN** the owner submits the authenticated opening request with that original
- **THEN** the provider returns HTTP 400 with `funding_rejected` and creates no account, credential, credit or fee obligation

#### Scenario: Acknowledgement alone is not money

- **WHEN** only a durable acknowledgement exists and no policy-accepted payment has been verified
- **THEN** no account, credential, credit or fee obligation exists for that agreement

### Requirement: Funding ambiguity is disclosed and recovered on native surfaces

Every native surface that starts a paid opening or presents a pending opening SHALL carry the residual registration-risk disclosure required by `inference-opening-funding-intent` (*Native broadcast follows verified durable acknowledgement*) and `inference-commercial-terms` (book line 489). These surfaces are the CLI `fund` command, the desktop Marketplace funding panel and its pending-funding notice (book line 894). Each such disclosure SHALL say that the provider receives independently broadcastable signed bytes and could broadcast them before acknowledging or without acknowledging, that the client's ordering is not cryptographic escrow, and that an HTTP error, timeout or missing reply does not prove that no money moved. No surface SHALL state or imply that the payment can be broadcast only after the provider's acknowledgement. A surface MAY state that the client itself broadcasts only after a durable acknowledgement.

A top-up has no registration step, so the registration-risk wording SHALL NOT be applied to it. After an error, timeout or lost reply during a top-up, a native client SHALL present the top-up as unresolved funding to resume and SHALL NOT state that no money moved (book lines 489 and 814).

After any ambiguous funding outcome, a client SHALL show the journaled funding stage and SHALL offer to resume the same original operation (`resume CONTRACT_ID`, or *Resume* on the desktop) (book lines 489, 814, 894 and 929). It SHALL NOT prompt for or prepare a second payment while one is unresolved. It SHALL NOT automatically replace, re-price or cancel the original.

#### Scenario: A lost reply is not a refusal

- **GIVEN** a funding attempt whose provider reply or RPC submission response was lost
- **WHEN** the user inspects pending funding
- **THEN** the journaled stage and transaction hash are shown, without signed bytes or signatures
- **AND** resuming uses the same original operation, and no new payment is prepared or broadcast

#### Scenario: Opening disclosure does not overstate the ordering

- **WHEN** the CLI `fund` command or the desktop Marketplace presents paid opening funding or guidance about a pending opening
- **THEN** it says that the provider holds broadcastable signed bytes and could broadcast them before or without acknowledging, that the ordering is not cryptographic escrow, and that an error does not prove no money moved
- **AND** it does not claim that the payment can be broadcast only after the provider's acknowledgement

#### Scenario: A top-up error is unresolved funding

- **WHEN** a native top-up returns an error, times out or loses its reply
- **THEN** the client presents the top-up as unresolved, offers to resume the same operation, and does not say that no money moved

### Requirement: Settlement risk is disclosed before desktop funding and shown on affected accounts

Before any opening or top-up, the desktop SHALL show the statement that chain reorganizations can freeze or reverse credit, and SHALL require the user to actively accept it before funding (book line 813, A.1.6). The Marketplace funding panel and the Usage top-up card are such surfaces. The other A.1.6 disclosure items are outside this capability.

An account whose status is neither `active` nor `free`, or whose `reversed_wei` is not zero, SHALL show a reorganization or freeze warning together with its `reversed_wei` and `provider_loss_wei` amounts (book line 929, A.5.3).

#### Scenario: A top-up requires accepting reorganization risk

- **WHEN** the user prepares a desktop top-up
- **THEN** the top-up card states that reorganizations can freeze or reverse credit
- **AND** funding stays disabled until the user actively accepts that disclosure

#### Scenario: A frozen account shows its reversal and loss

- **GIVEN** a `frozen` account with `reversed_wei` 1,000 and `provider_loss_wei` 505
- **WHEN** the account is displayed on the desktop
- **THEN** a reorganization or freeze warning shows both amounts

### Requirement: Fund and resume recover the saved agreement without rotating credentials

Native `fund` with the identical signed terms and `resume CONTRACT_ID` SHALL recover the saved agreement and its original operation as `inference-opening-funding-intent` (*Restart recovery does not manufacture another opening*) and `inference-account-opening-request` (*Opening replay preserves accepted account state*) require (book line 493). Top-up recovery is governed by `inference-account-topup-request`.

In addition, `fund` with signed terms that differ from those of a saved agreement SHALL be refused without changing the saved agreement, its original operation or its credentials. Explicitly issuing a credential SHALL restore a missing usable default without replacing an existing valid default. Explicit rotation and revocation SHALL remain available (book lines 493 and 495). Recovery SHALL NOT abandon accepted balances.

#### Scenario: Different signed terms are refused

- **WHEN** `fund` is called for a saved agreement with a signed quote that differs from the saved one
- **THEN** the call is refused and the saved agreement, original operation and credentials are unchanged

#### Scenario: Explicit issue restores only a missing default

- **GIVEN** a saved account whose default credential is valid
- **WHEN** the owner explicitly issues another credential
- **THEN** the existing default is not replaced
- **AND** after the default is revoked or expires, `resume` creates no replacement, and an explicit issue restores a usable default

### Requirement: Chain evidence comes from signed bytes and the configured identity-checked RPC

The chain client SHALL decode only supported signed envelopes, within the bounds of `inference-payment-envelope-bounds` (book line 512). It SHALL verify the configured chain binding, the canonical signature, the recovered sender and the recipient. It SHALL verify the value: exactly, where the agreement fixes it, and otherwise as positive. It SHALL also verify the exact calldata commitment and the transaction hash computed from the signed bytes. Signed bytes supply the value and calldata evidence; a receipt alone SHALL NOT.

A receipt SHALL match the signed transaction's hash, sender, recipient and envelope type, and its gas used SHALL NOT exceed the signed gas limit. A receipt that does not is contradictory chain evidence and SHALL NOT support acceptance.

Receipt and canonical-block evidence SHALL come only from the configured RPC endpoint. It SHALL NOT come from receipts, blocks or URLs supplied by the payer or provider. The client SHALL confirm the endpoint's chain ID on every settlement view. It SHALL confirm the endpoint's genesis hash against the configured network identity at least once per client instance, and again after a restart. A mismatch SHALL make that endpoint's evidence untrustworthy. The operation SHALL fail without credit and SHALL NOT be retried against another endpoint, chain or network identity. The client SHALL use native `eth_*` methods, not the historical `chiral_*` contract. Wallet history SHALL be presented as the durable local operation journal and labelled as not a complete chain index.

Verification outcomes SHALL be classified as follows (book line 514):

- **Rejected:** the signed bytes do not satisfy the agreement (decoding, chain binding, sender, recipient, value or commitment), or the payment's own execution failed with status `0` and that receipt satisfies the rest of the settlement policy.
- **Unavailable:** required chain evidence is missing, malformed, contradictory or from an untrustworthy endpoint. This includes a configured-identity mismatch and a receipt that contradicts the signed transaction. Unavailable evidence SHALL NOT be reported or recorded as a rejection of the payment and SHALL NOT be treated as proof of a reorg.

#### Scenario: Supplied evidence cannot substitute for signed bytes

- **WHEN** a signed payment differs from the agreement in sender, recipient, value, calldata or chain
- **THEN** verification is rejected without credit

#### Scenario: A contradictory receipt is not a payment rejection

- **WHEN** the configured RPC returns a receipt for the transaction hash that names another sender, recipient or envelope type, or reports gas above the signed limit
- **THEN** verification is unavailable, no credit is granted, and the payment is not recorded or reported as rejected

#### Scenario: Identity mismatch is not retried elsewhere

- **WHEN** the configured RPC reports another chain ID or genesis hash
- **THEN** the operation fails as unavailable without credit, no other endpoint or network is consulted, and the payment is not recorded or reported as rejected

#### Scenario: History is a local journal

- **WHEN** wallet history is requested
- **THEN** it lists the locally journaled operations and reports that it is not a complete chain history

### Requirement: Chain RPC transport is bounded

Each JSON-RPC request body SHALL be at most 64 KiB (65,536 bytes) (book line 512). An ordinary response, and every error response, SHALL be accepted only if it is at most 64 KiB. The only exception is a successful `eth_getBlockByNumber` response requested with `false` for full transactions (hash-only), which MAY be up to 8 MiB (8,388,608 bytes).

The applicable limit SHALL be enforced against a declared `Content-Length` and while reading streamed or chunked bodies, before anything beyond that limit is buffered. A hash-only block response cannot be identified as an error before it is read, so its body MAY be buffered up to the 8 MiB bound before the 64 KiB error limit is applied. Responses SHALL be JSON-RPC 2.0 objects that echo the request identifier and carry either a result or an integer-coded error, never both. Redirects SHALL NOT be followed, because evidence comes only from the configured endpoint. Hexadecimal quantities SHALL be canonical: `0x`-prefixed, without leading zeros and within range. A violation SHALL be treated as unavailable evidence and never coerced. A relay chain-RPC gateway SHALL NOT widen these bounds through batching.

#### Scenario: A large hash-only block remains verifiable

- **GIVEN** a block requested with hash-only transactions whose response exceeds 64 KiB but not 8 MiB
- **WHEN** funding at that block is verified directly or through the relay gateway
- **THEN** the block read is accepted and verification proceeds under the settlement policy

#### Scenario: Oversized ordinary, error or streamed responses are refused

- **WHEN** an ordinary method response exceeds 64 KiB, an error response for any method exceeds 64 KiB, or a chunked body exceeds the method's limit
- **THEN** the call fails as an RPC protocol failure and is treated as unavailable evidence, never as credit

#### Scenario: Oversized requests are refused

- **WHEN** a request body would exceed 64 KiB
- **THEN** it is not sent, and the gateway refuses such a request with HTTP 413

#### Scenario: Redirects are not followed

- **WHEN** the configured RPC endpoint answers with an HTTP redirect
- **THEN** the redirect target is not contacted and the call is treated as unavailable evidence

### Requirement: Credit uses the canonical-successful-receipt-at-espace-safe-v1 policy

The provider credit path and the native wallet journal SHALL accept a payment only under `canonical-successful-receipt-at-espace-safe-v1` (book lines 487 and 514). Acceptance SHALL require all of the following:

1. A healthy settlement view, which means all of these hold:
   - The endpoint identity is confirmed and the node reports that it is not syncing.
   - The `safe` frontier's height and timestamp do not exceed the latest head's, and equal heights have equal hashes.
   - The head timestamp is within a configured, finite freshness limit of the local clock.
   - The `safe` frontier's age is within a configured, finite freshness limit. A stale `safe` frontier SHALL make the view unhealthy even when the head is fresh.
2. A receipt for the signed transaction with status `1`, at a height at or below the `safe` height.
3. The canonical block at the receipt height, read from the configured RPC and consistent with the `safe` frontier, has the same hash as the receipt's block hash.
4. Two consecutive complete observations of items 1–3 agree on the receipt and the canonical block.

An observation in which the receipt is missing, lies above `safe`, does not match the canonical block, or disagrees with the other observation SHALL be pending, not credit. A receipt with status `0` that otherwise satisfies items 1, 3 and 4 SHALL be rejected as failed execution. Malformed or contradictory chain data, an unhealthy or stale view, or an unavailable RPC SHALL be unavailable. A confirmation count, a first receipt, mempool presence or PoS `finalized` SHALL NOT substitute for this policy.

#### Scenario: The first receipt waits for safe

- **WHEN** a legacy, type-1 or type-2 payment has a successful receipt above the current `safe` height
- **THEN** verification is pending
- **AND** after `safe` reaches that height and two consistent observations agree, the payment is accepted at its receipt block

#### Scenario: Non-canonical or unstable evidence is pending

- **WHEN** the receipt's block hash differs from the canonical block at its height, or the second observation sees a different canonical block
- **THEN** verification is pending and no credit is granted

#### Scenario: An unhealthy view is unavailable

- **WHEN** the node is syncing or sync health is missing, the `safe` frontier is malformed or ahead of the head, the head is outside its freshness limit, the `safe` frontier is older than its freshness limit while the head is fresh, or the receipt status is neither `0` nor `1`
- **THEN** verification is unavailable and no credit is granted

#### Scenario: Failed execution at safe is rejected

- **WHEN** the canonical receipt at or below `safe` has status `0` in two consistent observations
- **THEN** verification is rejected as failed execution and no credit is granted

### Requirement: Settlement stages remain distinct and safe acceptance is not finality

Four stages SHALL remain distinct (book line 514):

- Pending: signed, journaled or submitted, without a settled receipt.
- Executed: a receipt exists but is not accepted under the policy.
- Safe-accepted.
- PoS `finalized`.

Only safe-accepted payments SHALL be spendable. Chain status SHALL report the head, `safe` and `finalized` frontiers separately, with acceptance `safe`. The provider SHALL identify its policy as `canonical-successful-receipt-at-espace-safe-v1`. No surface SHALL describe safe acceptance as irreversible finality, as a refund right or as protection against later reversal. Credit accepted at `safe` SHALL remain subject to monitoring.

#### Scenario: Status separates the frontiers

- **WHEN** chain status is requested
- **THEN** the head, `safe` and `finalized` frontiers are reported separately, with acceptance `safe`
- **AND** a `finalized` height below the receipt height does not prevent safe acceptance

#### Scenario: The provider names its policy

- **WHEN** the provider's operator summary is read
- **THEN** it identifies the settlement policy as `canonical-successful-receipt-at-espace-safe-v1`

#### Scenario: Safe credit remains monitored

- **GIVEN** a payment credited at `safe`
- **WHEN** later monitoring proves that its block was replaced
- **THEN** the credit is reversed under the orphan requirement, not treated as final

### Requirement: Credited funding is monitored and a missing receipt is not a reorg

The provider SHALL periodically recheck every credited funding payment, including payments already pending or orphaned (book line 514). It SHALL work in bounded batches such that every payment is eventually revisited. A payment whose recheck result cannot be applied SHALL NOT prevent the other payments from being rechecked, and while such a failure persists the provider SHALL stay closed to new funding admission. Each recheck SHALL first require the stored evidence to equal a fresh decode of the original signed bytes. Outcomes SHALL be:

- **Accepted:** the same transaction is successful and canonical at `safe` in two consistent observations. If it was included in a different block, the inclusion evidence (block hash and height) SHALL be refreshed without changing immutable fields and without duplicate credit or fee.
- **Pending:** the previously accepted height is above the current `safe` frontier, the observations disagree, or a new receipt lies above `safe`. Spending SHALL be withheld (`settlement_pending`) without reversing credit.
- **Orphaned:** two consistent reads of the canonical block at the previously accepted height, at or below the healthy `safe` frontier, show a hash different from the accepted block hash.
- **Unavailable:** the receipt is missing while the accepted block is still canonical, a receipt contradicts that block, the view is unhealthy, or the RPC fails.
- **Contradictory:** the configured RPC fails its identity check, a receipt contradicts the signed transaction, or stored evidence disagrees with its signed bytes. The funding SHALL be handled as for unavailable evidence: its last observed state is retained, so accepted credit stays spendable, pending funding stays withheld and orphaned funding stays reversed. No reversal, loss or orphan classification SHALL follow. The provider SHALL also stop accepting new admission and report a fixed rejection category until a healthy, identity-confirmed observation returns.

A missing receipt alone SHALL NOT be treated or described as proof of a reorg or of a funding reversal (book lines 439 and 514). The native wallet journal SHALL classify its accepted operations in the same way.

#### Scenario: A missing receipt with an unchanged canonical block retains credit

- **GIVEN** credited funding whose accepted block is still canonical at its height
- **WHEN** the RPC returns no receipt for the transaction
- **THEN** the observation is unavailable and the credit and account status are unchanged

#### Scenario: A stably replaced block proves an orphan

- **WHEN** two consistent reads at or below a healthy `safe` frontier show a different canonical block at the accepted height
- **THEN** the payment is classified as orphaned

#### Scenario: Re-inclusion refreshes evidence once

- **WHEN** the identical signed transaction is accepted in a different canonical block at or below `safe`
- **THEN** only its inclusion evidence changes, with no second credit, fee or broadcast
- **AND** an observation that changes any immutable field of the evidence is refused without changing the account

#### Scenario: Safe regression withholds spending without reversal

- **WHEN** the current `safe` frontier falls below the accepted height
- **THEN** the funding becomes pending, the account becomes `settlement_pending` and new reservations are refused
- **AND** a later accepted observation restores `active` without re-crediting

#### Scenario: Contradictory evidence does not withhold or reverse accepted credit

- **GIVEN** an `active` account with credited funding
- **WHEN** monitoring finds that the configured RPC reports another chain ID, or returns a receipt that contradicts the signed transaction
- **THEN** the account stays `active` and spendable, with no reversal or loss
- **AND** the provider stops accepting new admission until a healthy, identity-confirmed observation returns

#### Scenario: One unrecoverable payment does not stall monitoring

- **GIVEN** more credited payments than one batch holds, and one payment whose recheck result is refused by the ledger
- **WHEN** monitoring runs repeatedly
- **THEN** every other payment is still rechecked, and the provider stays closed to new funding admission while the refused payment remains unresolved

### Requirement: A proven orphan freezes spending, reverses unused credit and journals loss

When monitoring proves a credited payment orphaned, the provider SHALL do all of the following in one ledger transaction (book lines 514 and 540):

- Settle every reserved request on the account with zero charge and an explicit unknown-usage outcome (`funding_orphan_unknown_usage`), releasing its hold. A later engine completion SHALL NOT charge that request.
- Add the payment's gross value to `reversed_wei`.
- If the payment's fee obligation is not yet sealed into a fee batch, cancel the obligation so that it is never batched or remitted, and remove its fee from `fee_wei`. If it is sealed or remitted, retain the fee liability and mark the obligation as orphaned, distinguishable from ordinary obligations.
- Recompute `available_wei = max(0, funded − fee − reversed − spent − reserved)` and `provider_loss_wei = max(0, fee + reversed + spent + reserved − funded)`. Available balance is therefore never negative, and consumed orphaned funding appears as provider loss.
- Set the account status to `frozen`, which refuses new reservations. A frozen account's refusal of top-up settlement is governed by `inference-account-topup-request`.

Repeating an orphan observation SHALL be idempotent. A pending, contradictory or unavailable observation SHALL NOT undo a reversal.

#### Scenario: A partly consumed orphaned opening

- **GIVEN** a 1,000-wei opening with a 5-wei fee obligation not yet in a batch, 500 wei spent and an outstanding hold
- **WHEN** the opening is proven orphaned
- **THEN** the account is `frozen` with `reversed_wei` 1,000, `fee_wei` 0, `reserved_wei` 0, `available_wei` 0 and `provider_loss_wei` 500
- **AND** the held request later settles with zero charge, and no fee batch is prepared for the cancelled obligation

#### Scenario: A sealed or remitted fee becomes loss

- **GIVEN** the same account with its fee obligation already batched or remitted
- **WHEN** the opening is proven orphaned
- **THEN** `fee_wei` remains 5 and `provider_loss_wei` is 505

#### Scenario: Repeated or weaker observations do not change a reversal

- **WHEN** the orphan is observed again, or a pending, contradictory or unavailable observation follows
- **THEN** the reversed account is byte-for-byte unchanged

### Requirement: Orphan restoration compensates the reversal exactly once

Orphaned funding SHALL remain recoverable (book line 518). Only a later accepted observation of the same signed transaction, with identical immutable evidence, SHALL restore it. Restoration SHALL do all of the following in one ledger transaction:

- Subtract the payment's gross value from `reversed_wei`.
- Restore the fee charge if it was reversed.
- Return the fee obligation to its pre-orphan state: pending if it was never batched, and its batch's state (sealed or remitted) otherwise.
- Recompute `provider_loss_wei` and `available_wei`.
- Recompute the account status from all of its funding: `frozen` if any funding is orphaned, `settlement_pending` if any is pending, and `active` otherwise.

Restoration SHALL NOT create a second funding record, fee obligation or credit. Holds released when the orphan was proven SHALL stay released.

#### Scenario: Restoration across restart for every fee state

- **GIVEN** an orphaned 1,000-wei opening with 500 wei spent and its fee obligation pending, batched or remitted
- **WHEN** the reopened ledger observes the same transaction accepted in a new canonical block, twice
- **THEN** the account is `active` with `fee_wei` 5, `available_wei` 495, `reversed_wei` 0 and `provider_loss_wei` 0
- **AND** the fee obligation returns to its prior state without a second obligation or batch

#### Scenario: Restoring one payment does not unfreeze another

- **GIVEN** an account whose opening and top-up are both orphaned
- **WHEN** only one of them is restored
- **THEN** the account stays `frozen` with loss recomputed from the remaining reversal
- **AND** it becomes `active` only after the other payment is also accepted

### Requirement: Chain outages withhold new credit without inventing loss or switching networks

When required evidence is unavailable or untrustworthy, the provider SHALL refuse new credit with HTTP 503 `settlement_unavailable` (book line 514). It MAY record a bounded unavailable funding attempt. It SHALL close new-funding readiness until a healthy observation returns. It SHALL NOT create an account, reverse or charge.

Monitoring under unavailable evidence SHALL retain the last observed funding state and update only the check time. Accepted credit stays spendable, pending funding stays withheld and orphaned funding stays reversed. RPC unavailability SHALL NOT be evidence of an orphan or a loss. It SHALL NOT be resolved by switching to another endpoint, chain, network identity or deployment. Operator and client status messages SHALL use fixed categories rather than raw RPC content. A configured-identity mismatch SHALL additionally stop new admission until the configured identity is confirmed again.

#### Scenario: An outage preserves accepted credit

- **GIVEN** an active credited account
- **WHEN** monitoring finds the RPC unavailable
- **THEN** the account stays `active` and spendable, and no reversal or loss is recorded

#### Scenario: Verification during an outage grants nothing and closes readiness

- **GIVEN** a provider whose new-funding readiness is open
- **WHEN** an opening or top-up is submitted while required chain evidence is unavailable
- **THEN** the provider returns HTTP 503 `settlement_unavailable` without creating an account, credit, fee obligation or credential
- **AND** new-funding readiness is closed until a healthy observation returns

#### Scenario: Weaker observations do not change a freeze

- **GIVEN** funding that is already pending or orphaned
- **WHEN** later observations are unavailable
- **THEN** the pending or reversed state and its balances are unchanged

### Requirement: The ledger balance identity covers reversals and loss credits

Without reversals or loss, every account SHALL satisfy `funded = fees + spent + reserved + available` (book lines 524–526). With orphaned funding, the provider ledger SHALL satisfy `funded_wei + provider_loss_wei = fee_wei + reversed_wei + spent_wei + reserved_wei + available_wei` (book line 540). This is the same as `net_credit = accepted_funding − net_fee_charges − funding_reversals + provider_loss_credits = spent + reserved + available`.

Every account write SHALL validate the identity in exact integer arithmetic wide enough that no sum of canonical wei amounts overflows, without floating point, and SHALL refuse to persist an unbalanced account. Money SHALL follow the canonical decimal wei contract of `inference-wire-foundation`.

For each accepted funding payment, the fee SHALL be `floor(value * 50 / 10000)`, and only the 50-basis-point launch rate SHALL be accepted (book line 528). `funded`, `fee` and `available` SHALL increase atomically by the value, the fee and the value minus the fee. A transaction hash SHALL credit at most one account, at most once.

The provider loss credit SHALL only offset consumed orphaned funding. Positive loss implies zero available balance, and loss is never spendable. Consumer validation of the same identity remains governed by `inference-account-snapshot`. Request reservation and settlement remain governed by `inference-request-accounting`.

#### Scenario: Fee rounding is floor on funding

- **WHEN** 199 wei, 200 wei and 1,000 wei payments are accepted
- **THEN** their fees are 0, 1 and 5 wei and their net credits are 199, 199 and 995 wei

#### Scenario: Extended identity with loss

- **GIVEN** an orphaned account with funded 1,000, fee 5, reversed 1,000, spent 500, reserved 0 and available 0
- **WHEN** its balances are written
- **THEN** `provider_loss_wei` is 505 and the extended identity holds exactly, without negative balances

#### Scenario: An unbalanced account is not persisted

- **WHEN** an account write would break the extended identity
- **THEN** the write is refused and the stored account is unchanged

#### Scenario: One payment cannot fund twice or fund another account

- **WHEN** an accepted transaction is resubmitted for its own account or submitted for another account
- **THEN** its own account is unchanged, and the other account is refused with `funding_replay`

### Requirement: Explicit cancellation is a fixed-fee same-nonce self-transfer, not a refund

`cancel-funding` SHALL require explicit authorization (book line 516). The CLI requires `--accept-gas-and-race-risk`. The desktop requires `accept_gas_and_race_risk: true`, set after a visible acceptance control (book line 932). Without it the command SHALL be refused before any signing, journaling or chain call. Clients SHALL disclose that cancellation spends gas, can lose the race to the original and leaves the original payment non-refundable.

The replacement SHALL be a fixed-fee same-nonce self-transfer: from the original sender, with the original's nonce, to the sender itself, with value 0 and no calldata. Its gas price SHALL be `P + floor(P/8) + 1` wei, computed with checked arithmetic, where `P` is the original's gas price. Its gas price and gas limit SHALL be fixed when it is first signed.

Before the replacement is first signed:

- The settlement view SHALL be healthy.
- The sender's latest executed nonce SHALL equal the original nonce. Otherwise no replacement is created, and the existing outcome or a pending error is returned.
- The balance SHALL cover the replacement's maximum gas cost.
- The chain SHALL accept the replacement as a plain native transfer.

The replacement SHALL be journaled durably before broadcast, and the original SHALL never be deleted. A repeated cancellation SHALL resubmit the identical replacement bytes. It SHALL NOT re-price or bump again, and no automatic caller SHALL exist. An uncertain submission SHALL leave the operation `cancellation_pending`.

Outcomes SHALL be:

- `original_accepted`: the original is accepted under the settlement policy, and the funding is non-refundable.
- `original_failed`: the original executed with status `0`, and that receipt is canonical at or below `safe` in two consistent observations. The original's nonce is consumed and its gas is spent.
- `cancelled`: the replacement is accepted under the settlement policy, meaning successful and canonical at `safe` in two consistent observations.
- `cancellation_failed`: the replacement executed with status `0`, and that receipt is canonical at or below `safe` in two consistent observations. The nonce is consumed and the replacement's gas is spent.
- `cancellation_pending`: every other case.

Cancellation SHALL NOT be described as a refund. `cancelled` means only that the replacement consumed the nonce.

#### Scenario: Unauthorized cancellation is refused before any chain action

- **WHEN** `cancel-funding` is invoked from the CLI or the desktop without explicit gas and race acceptance
- **THEN** it is refused, and nothing is signed, journaled or submitted

#### Scenario: One fixed replacement survives restart and fee changes

- **GIVEN** an explicit cancellation whose replacement was journaled and whose submission result was uncertain
- **WHEN** the client restarts and cancellation is repeated after the node's gas price rises
- **THEN** the identical replacement bytes are resubmitted at the original-derived price
- **AND** the original's bytes remain in the journal

#### Scenario: The original can win the race

- **WHEN** the original payment is policy-accepted before or after a replacement is submitted
- **THEN** the outcome is `original_accepted`, and the funding is not reported as refunded or cancelled
- **AND** recovering that operation settles the original without broadcasting again

#### Scenario: Only a safe successful replacement confirms cancellation

- **WHEN** the replacement's receipt is above `safe`
- **THEN** the outcome stays `cancellation_pending`
- **AND** it becomes `cancelled` only at `safe`, and returns to `cancellation_pending` without rebroadcast if that receipt later disappears

#### Scenario: Failed executions are distinct from cancellation

- **WHEN** either the original or the replacement executes with status `0` and that receipt is canonical at `safe` in two consistent observations
- **THEN** the outcome is `original_failed` or `cancellation_failed` respectively
- **AND** the failed transaction has consumed the nonce, and the wallet's next original is prepared at the following nonce

### Requirement: Cancellation targets only the intended operation

Without an explicit operation, cancellation SHALL target the agreement's pending top-up (book line 518). If there is none and no account is saved, it SHALL target the unopened opening `open:<contract-id>`. Otherwise the request SHALL be refused, and the user SHALL be directed to `funding-recovery CONTRACT_ID` and an exact `--operation-id`.

An explicit operation SHALL be exactly `open:<contract-id>` or `topup:<contract-id>:<operation>` for the named contract; any other operation SHALL be refused. If the saved original differs from the journaled original, the request SHALL be refused. An old opening SHALL never be the default target while a top-up is being recovered or after an account is open.

If the target is the agreement's pending top-up, or its unopened opening, and no original has been prepared for it, cancellation SHALL report that no payment was prepared (`not_prepared`). It SHALL send nothing and SHALL close the unprepared intent so that a later resume cannot prepare it. An explicit operation that is neither of those targets and has no journaled original SHALL be refused without sending anything.

#### Scenario: An open account without a pending operation needs an exact operation

- **GIVEN** an open account with no pending top-up
- **WHEN** cancellation is requested without an operation ID
- **THEN** it is refused with guidance to choose an exact operation from funding recovery, and nothing is signed

#### Scenario: Another contract's operation is refused

- **WHEN** an explicit operation ID belongs to another contract or is not an opening or top-up operation
- **THEN** cancellation is refused without signing, journaling or submission

#### Scenario: Unprepared pending funding sends nothing

- **WHEN** cancellation targets a pending top-up or an unopened opening that has no prepared original
- **THEN** the result is `not_prepared`, no transfer is sent, and resume cannot later prepare that intent

#### Scenario: An orphaned opening of a saved account is explicitly cancellable

- **GIVEN** a saved account whose opening was orphaned and whose nonce is current again
- **WHEN** the owner explicitly cancels `open:<contract-id>`
- **THEN** one replacement is journaled and submitted, and the saved account and exported credential remain available

### Requirement: Resume and cancellation remain distinct and recovery history is retained

Resume and cancellation SHALL be distinct paths (book line 516). Once cancellation is requested for an operation, no path SHALL broadcast that original again or prepare another payment for that operation (book lines 516 and 518). This block SHALL apply only to the cancelled operation. `fund` and `resume` SHALL still refresh a saved account and recover the agreement's other unresolved operations, including the exact original bytes of a pending top-up.

When any reconciliation reports `original_accepted` for a cancelled operation, including `funding-recovery`, the native consumer SHALL durably record that outcome with its saved agreement, and `resume` SHALL then settle that original with the provider. Recovering an original that won SHALL NOT require cancellation authorization and SHALL NOT invoke an operation that could submit the replacement. Cancellation SHALL NOT broadcast the original.

The wallet journal SHALL retain originals and replacements indefinitely, including after terminal statuses, cancellation and reorg. `funding-recovery CONTRACT_ID` SHALL reconcile the wallet journal under the wallet locks without signing or broadcasting. It SHALL list this contract's opening and top-up operations with hashes, nonces, statuses and cancellation outcomes, and without signed bytes. Historical operations SHALL remain selectable by exact identifier after a cancellation or reorg. The local `pending` view SHALL list stages and hashes without signed bytes or signatures.

#### Scenario: Resume does not rebroadcast a cancelled original

- **GIVEN** a requested cancellation that has not resolved to `original_accepted`
- **WHEN** the user runs `resume`
- **THEN** the cancelled original is not broadcast and no payment is prepared for that operation

#### Scenario: A won original is recovered without cancellation authority

- **GIVEN** a cancelled opening or top-up whose original later becomes accepted under the policy
- **WHEN** the user runs `funding-recovery` and then `resume`
- **THEN** the original is settled with the provider and credited once
- **AND** no cancellation authorization is required and no replacement is submitted

#### Scenario: An opening cancellation does not block account refresh or top-up recovery

- **GIVEN** a saved account whose opening was explicitly cancelled, and a pending top-up
- **WHEN** the user runs `resume`
- **THEN** the account is refreshed and the pending top-up's exact original bytes are recovered
- **AND** the cancelled opening original is not broadcast

#### Scenario: Recovery inspection never broadcasts or exposes bytes

- **WHEN** `funding-recovery` reconciles after restart or a reorg
- **THEN** it reports each historical operation and its cancellation status without submitting a transaction
- **AND** its output contains no signed transaction bytes

### Requirement: A displaced payment that becomes canonical again is reconciled once

Some payments are displaced: orphaned, cancelled at `safe`, or replaced by a transaction that later lost canonicality. If such a payment becomes policy-accepted again, provider reconciliation SHALL restore it under the restoration requirement (book line 518). Native reconciliation SHALL report it as `original_accepted`.

Explicit recovery of that operation SHALL submit the same original bytes for settlement and SHALL credit them at most once. It SHALL NOT sign another payment, re-price the replacement or broadcast anything automatically. A replacement previously reported as `cancelled` whose receipt later loses canonicality SHALL return to `cancellation_pending`. It SHALL block new originals and SHALL NOT be rebroadcast until the user requests it explicitly.

#### Scenario: A cancelled top-up whose original later wins is credited once

- **GIVEN** a top-up reported as `cancelled` whose replacement is later displaced, after which the original becomes successful and canonical at `safe`
- **WHEN** the owner runs explicit recovery for that exact operation, repeatedly
- **THEN** the outcome is `original_accepted`, the provider credits the top-up exactly once, and no further transaction is submitted

#### Scenario: A displaced opening that becomes canonical again opens once

- **GIVEN** an unopened opening whose cancellation was reported as `cancelled`, after which a reorg makes the original successful and canonical at `safe`
- **WHEN** `funding-recovery` reports `original_accepted` and the user resumes
- **THEN** the account opens with exactly one credit from the original bytes, and no replacement or new payment is signed

### Requirement: Every spender of one wallet shares one coordinated nonce journal

The chain client SHALL derive the wallet journal path `<data_dir>/inference/<hash-of-full-network-identity>/wallet-journal/transactions.sqlite3` from the configured base `data_dir`, appending that suffix itself exactly once (book line 470). Operator configuration and guidance SHALL name the base, not a path with the suffix already appended. Sharing that base, and refusing to present independent journal copies as coordination, is governed by `inference-opening-funding-intent` and, for desktop sharing, `desktop-share-online`. This requirement applies that sharing to every spender of the wallet, including consumer funding, top-up, cancellation and provider fee remittance.

Provider ledger, lock, acknowledgement and offer-revision state SHALL be isolated under `<data_dir>/<network-hash>/<signing-wallet>/<DHT-peer-ID>/`, and the journal SHALL NOT be moved into that directory (book line 472). Different provider deployments of one wallet SHALL have distinct persistent DHT identities and SHALL still share the wallet journal. A deployment's DHT identity SHALL be preserved together with its ledger, acknowledgements, offer revision, wallet journal, keys and credentials across restarts. A new peer ID SHALL be a different deployment, never recovery of its predecessor's ledger or acknowledgements. Changing paths or restoring an older layout without deliberate state preservation SHALL NOT be presented as an upgrade or recovery.

The journal SHALL be an owner-only regular file, opened without following symbolic links, with synchronous durable writes. Preparation, submission, cancellation and reconciliation SHALL hold the in-process sender lock and an exclusive interprocess sender lock in the journal directory. Contention SHALL return a pending error rather than proceed with a competing operation. A wallet SHALL have at most one unresolved original at a time. Fresh preparation SHALL be refused while the chain's pending nonce differs from its executed nonce.

#### Scenario: Consumer funding and provider fee remittance contend for one wallet

- **GIVEN** a consumer and a provider fee remitter configured with the same base directory and wallet
- **WHEN** both try to prepare an original at the same time
- **THEN** at most one original is journaled and at most one is submitted

#### Scenario: A second process cannot take the wallet lock

- **WHEN** another process holds the wallet's interprocess lock
- **THEN** a second process's operation is refused as pending, and it succeeds only after the lock is released

#### Scenario: Deployments share the journal but not ledgers

- **WHEN** two provider deployments use the same wallet and base directory with distinct DHT identities
- **THEN** they share one wallet journal while keeping separate ledgers and instance locks
- **AND** an account opened in one deployment is not visible in the other

#### Scenario: An unsafe journal file is refused

- **WHEN** the journal path is a symbolic link or not a regular file
- **THEN** the chain client refuses to open it, and a newly created journal is readable and writable only by its owner

### Requirement: Original operations keep sender and nonce ownership through terminal status and regression

Each journaled original SHALL own its sender and nonce durably (book line 491). This applies the nonce-ownership rule of `inference-opening-funding-intent` to every original operation of the wallet. The journal SHALL refuse to insert any other original with an already-assigned sender and nonce, whether or not the earlier operation is terminal, including after restart. Legacy duplicate assignments SHALL be preserved, but SHALL NOT allow further allocation.

Before allocating a new nonce, the client SHALL reconcile and recheck the nonce assignment. If the chain nonce regresses to an assigned nonce, the client SHALL return a pending error and SHALL NOT sign a competing original. If a nonce is consumed without this transaction's canonical receipt, the operation SHALL be marked `nonce_conflict`. That status records unresolved evidence. It is not loss and not proof of a reorg. It SHALL be revisited and return to accepted if the receipt returns, and the original SHALL never be re-signed.

Only the explicit cancellation replacement MAY reuse an original's nonce. An operation identifier SHALL remain bound to its original recipient, value and commitment.

#### Scenario: Chain regression does not create a competing original

- **GIVEN** an accepted original whose nonce later appears unconsumed during a new allocation
- **WHEN** another operation is prepared
- **THEN** preparation is refused as pending, and no second original is journaled for that nonce, even through direct insertion after restart

#### Scenario: A receipt gap is recovered without a replacement

- **GIVEN** an original whose nonce advanced while its receipt was temporarily missing, recorded as `nonce_conflict`
- **WHEN** the canonical receipt returns at `safe`
- **THEN** resume reports the same original as accepted, and nothing else is signed or submitted

#### Scenario: An operation cannot be rebound to different terms

- **WHEN** an existing operation identifier is used with a different recipient, value or commitment
- **THEN** the request is rejected, and the original remains bound to its journaled bytes
