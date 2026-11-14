## ADDED Requirements

### Requirement: Native preparation durably owns one exact opening transaction

Before preparing a paid opening, the native consumer SHALL verify and durably save the selected signed offer and quote under the existing commercial and complete-network identity rules. It SHALL use one stable `open:<quote_id>` operation within the signing wallet's network-scoped journal, binding the payer, provider recipient, exact quoted funding value and complete opening commitment defined by `inference-wire-foundation`.

Fresh preparation SHALL validate the configured chain and available sender state, reject an unresolved conflicting wallet operation or occupied pending nonce, and check funds for value plus gas before journaling. Native gas estimation SHALL use the chain's estimate for the actual payment, require at least the transaction's structural minimum and no more than 1,000,000 gas, and retain that signed limit without treating Ethereum intrinsic gas as a complete eSpace cost estimate. Preparation SHALL durably retain the original signed bytes, transaction hash and sender/nonce assignment before returning them or allowing client broadcast. Retrieving a prepared original SHALL NOT sign another transaction merely because time, fee estimates or the node's nonce changed.

Processes spending the same wallet on the selected network SHALL share the configured base data directory and coordinated protected nonce journal, including across provider deployments. Independent copies or hosts without that journal coordination SHALL NOT be represented as safe nonce coordination; such deployments require distinct signing wallets. Terminal operation status or chain regression SHALL NOT erase original nonce ownership or authorize silent reuse.

#### Scenario: Preparation is durable without submission

- **WHEN** valid paid terms are prepared for the first time
- **THEN** the signed original and its nonce ownership are durable before preparation returns
- **AND** preparation alone does not send a transaction, acknowledge funding, open an account or credit funds
- **AND** reopening the journal returns that original for the same operation and rejects changed recipient, value or commitment

#### Scenario: Unavailable funds or nonce prevent fresh preparation

- **WHEN** chain identity/health, value-plus-gas balance or available nonce cannot satisfy fresh preparation, or a competing unresolved operation owns the wallet's nonce
- **THEN** preparation creates neither another signed original nor a broadcast
- **AND** an existing original operation remains recoverable rather than being replaced

### Requirement: Funding intent messages bind the original signed payment

`POST /v1/contracts/:id/funding-intent` SHALL accept a JSON object containing exactly the required string `raw_transaction`, with the quote identifier supplied by the route. Missing, mistyped or unknown request members SHALL be rejected. The request SHALL use the quoted consumer's one-use owner authorization under `inference-owner-control`; a raw transaction, transaction hash, public quote or bearer credential is not a substitute.

The response SHALL be a strict signed envelope under `chiral-inference-funding-intent-v1`, signed by the quoted provider. Its payload SHALL contain exactly these required fields and reject unknown fields:

|Field|Meaning|
|---|---|
|`network`|Complete quoted network identity|
|`provider_wallet`|Canonical quoted provider signer|
|`consumer_wallet`|Canonical quoted payer|
|`quote_id`|Original quote/account identifier|
|`tx_hash`|Canonical Keccak-256 identifier of the complete original signed transaction bytes|
|`registered_at`|Unsigned 64-bit provider Unix-second registration time|

The native consumer SHALL verify the provider signature and exact equality of the network, both parties, quote identifier and computed original transaction hash. It SHALL independently validate that the original signed envelope commits to the quoted payer, recipient, funding value and complete quote calldata, and require its saved transaction hash to agree before accepting the acknowledgement. A matching hash supplied without those original bytes SHALL NOT suffice. Acknowledgement registration SHALL be within `[quote.issued_at, quote.expires_at)`; native acceptance SHALL additionally refuse a registration time more than 30 seconds ahead of its current clock. Quote expiry at recovery time alone SHALL NOT invalidate an otherwise valid acknowledgement registered within that interval.

#### Scenario: Strict acknowledgement and request schemas

- **WHEN** the request or signed acknowledgement omits a required field, supplies a wrong primitive type or adds an unknown field
- **THEN** typed acceptance fails rather than authenticating a projection with the offending data discarded
- **AND** the client does not broadcast on the strength of that response

#### Scenario: Signed substitution is not acknowledgement of this operation

- **WHEN** a response has the wrong signer/domain or changes any network component, party, quote identifier or original transaction hash
- **THEN** the consumer rejects it even if some fields or a provider signature otherwise appear valid
- **AND** no new original transaction is prepared to match the substituted response

#### Scenario: Registration time is distinct from recovery time

- **WHEN** registration predates quote issuance, is at or after quote expiry, or exceeds the native receiver's 30-second future allowance
- **THEN** the consumer refuses that acknowledgement
- **AND** an unchanged valid acknowledgement can still be accepted after the quote itself expires

### Requirement: Fresh intent admission verifies current full-cost spendability

Before acknowledging a new paid opening, the provider SHALL retrieve the exact issued signed quote, bind its owner authorization and decode the signed original for the selected chain. It SHALL accept only valid supported EIP-155 legacy, EIP-2930 or EIP-1559 envelopes with canonical encoding/signature and recovered payer, a 20-byte provider recipient rather than contract creation, exact quoted value and exact opening calldata. Input SHALL be nonempty lowercase-`0x`-prefixed hexadecimal, at most 16 KiB decoded; malformed or unsupported envelopes, insufficient structural gas and inconsistent typed fee fields SHALL be rejected. The transaction hash SHALL be computed from the complete signed bytes, not accepted as caller evidence.

Fresh admission SHALL use the provider's configured identity-checked, healthy RPC evidence. The signed nonce SHALL equal the current latest sender nonce, be less than `u64::MAX`, and have no competing pending nonce. The signed maximum gas price SHALL be nonzero. Available balance SHALL cover the value plus the **entire signed gas limit multiplied by its maximum gas price**, using exact wide arithmetic; a value-only or intrinsic-gas-only balance check SHALL NOT suffice. For legacy/type-1 transactions the maximum price is `gasPrice`; for type-2 it is `maxFeePerGas`, with priority fee no greater than that maximum.

The provider SHALL recheck latest/pending nonce and chain health after the balance observation. Changed nonce state, head regression or a different hash at the same head height SHALL prevent fresh acknowledgement. This admission is an observation, not a reservation of chain balance/nonce, a broadcast or verified settlement; sender state can change afterward.

#### Scenario: Every signed payment field must match

- **WHEN** a validly signed transaction has a different chain, payer, recipient, value or opening commitment, or an invalid envelope/gas/fee structure
- **THEN** new intent admission is refused without acknowledgement, submission or credit

#### Scenario: Full signed gas cost determines the balance boundary

- **GIVEN** the signed original otherwise satisfies fresh admission
- **WHEN** balance is one wei below value plus the signed gas limit times its maximum price
- **THEN** admission is refused
- **AND** an exact full-cost balance can pass this check without implying that the transaction has executed

#### Scenario: Future spent or unstable nonce is not current admission

- **WHEN** the signed nonce is future, already spent or exhausted, another nonce is pending, or nonce/head evidence changes during admission
- **THEN** no fresh acknowledgement is created
- **AND** refusal does not delete an earlier acknowledged original or require a replacement payment

### Requirement: Fresh registration uses current quote time and bounded admission

Public quotes SHALL be overbookable signed commercial terms, not reservations of paid-intent leases, free-opening capacity or inference capacity. Their retention and issuance SHALL follow the pending `specify-non-reserving-quotes` replacement of `inference-proposal-admission` and the aligned `specify-proposal-rate-budgets`, not the superseded named-wallet quote allowances. Otherwise eligible quoting may continue while opening capacity is full. Receipt of a quote SHALL NOT authorize payment submission.

Fresh registration SHALL require the exact signed quote previously issued by this provider, paid rather than genuinely free terms, current acceptance of new funding and compatible qualified execution under the existing commercial/evidence-only-renewal rules. Qualification SHALL be checked again after asynchronous chain admission; changed or unavailable admission execution SHALL NOT silently substitute terms. These observations do not promise atomicity with external engine failure or a concurrent operator transition after observation.

The provider SHALL sample registration time after acquiring the ledger write transaction and require `quote.issued_at <= registered_at < quote.expires_at`. A request waiting for another writer SHALL NOT backdate acceptance using its earlier arrival time. A regressed sample before issuance SHALL refuse new registration rather than clamp or fabricate an acceptable signed timestamp.

There SHALL be at most 10,000 active opening-intent leases per provider ledger, counted as retained intents whose admission deadline is strictly later than the transaction's registration time and whose accounts have not opened. A new lease SHALL end at the original quote's expiry. After payer authentication and exact quote/payment validation, the final capacity check and accepted-evidence insertion SHALL be atomic across writers, before durable acknowledgement and client broadcast. Active-intent capacity is independent of the 10,000 provider-wide standalone quote records, quote-work/issuance budgets, retained historical agreements, credentials and inference admission; it is not a promise of paid execution speed or concurrent generation capacity.

Capacity exhaustion SHALL return HTTP 429 with `rate_limit`, without creating an acknowledgement, accepted intent, account, credit or submission, or evicting an acknowledged agreement. An honest provider SHALL NOT broadcast the refused signed original, and the consumer SHALL NOT broadcast it without a valid durably saved acknowledgement. Later capacity pressure, quote/lease expiry or restart SHALL NOT cancel accepted terms or justify refusing recovery or credit of a valid acknowledged original; canonical successful execution and the healthy `safe` frontier remain required.

#### Scenario: Issued quotes do not decide paid admission

- **GIVEN** paid-intent capacity is full but separate provider-wide quote resources and offer/readiness validity permit issuance
- **WHEN** valid public quotes are issued
- **THEN** their signed terms are retained without allocating a paid lease or free account
- **AND** an otherwise valid authenticated fresh funding intent receives 429/`rate_limit` without acknowledgement, acceptance, credit or honest-provider/client submission

#### Scenario: Writers compete for the final paid lease

- **GIVEN** exactly one active-intent lease remains and two distinct independently authorized originals satisfy fresh admission
- **WHEN** independent writers register them concurrently
- **THEN** exactly one acknowledgement and original lease commit
- **AND** the loser receives 429/`rate_limit` without partial intent, quote removal, nonce assignment or submission, while earlier accepted evidence remains intact

#### Scenario: Waiting cannot turn an expired quote into a new acceptance

- **GIVEN** a request arrived before quote expiry but waited for the ledger writer
- **WHEN** registration samples time at or after expiry after obtaining that writer
- **THEN** no new acknowledgement or lease is committed
- **AND** a sampled time before quote issuance also refuses registration without a signed unusable acknowledgement

#### Scenario: New funding gates do not grant substitute terms

- **WHEN** the provider is draining, new-funding readiness is unavailable, or qualification/execution becomes incompatible during admission
- **THEN** a new intent is refused without rewriting the quote or its original transaction
- **AND** cached recovery remains governed by its separate original-acknowledgement requirement

#### Scenario: Lease expiry frees admission but not the agreement

- **GIVEN** all active-intent slots are occupied
- **WHEN** one original lease reaches its deadline or its account opens
- **THEN** that lease no longer consumes active-intent capacity
- **AND** the original signed agreement, acknowledgement and sender/nonce ownership remain retained while a new eligible intent may use the available slot

### Requirement: Acknowledgement commits evidence and nonce ownership atomically

The provider SHALL atomically persist the complete original signed quote, verbatim submitted raw-transaction string, computed hash, original payer/nonce assignment, signed acknowledgement and admission lease before returning a successful acknowledgement. It SHALL apply the retention handoff specified by the pending `specify-non-reserving-quotes` replacement contract in the same transaction, removing only the matching standalone proposal after durable intent retention succeeds. This releases one provider-wide unaccepted-quote storage slot, not a named-wallet allowance or a refund of quote-work/committed-issuance charges.

Within that provider ledger, an acknowledged transaction hash and original sender/nonce SHALL NOT be assigned to another agreement. Competing writers SHALL observe that ownership atomically. Expiry, a rejected funding attempt, opening, terminal status or later chain regression SHALL NOT release the original nonce commitment for conflicting acknowledgement. This is provider-ledger ownership, not a network-wide acknowledgement coordinator.

Failed admission, signing or persistence SHALL leave no partial acknowledgement/lease/nonce assignment and SHALL NOT free another proposal's storage or modify earlier accepted terms. Acknowledgement alone SHALL NOT create an account, issue a credential, credit funding or create a platform-fee obligation. Paid opening SHALL require the registered original and independent verified canonical-successful healthy-safe evidence; it SHALL NOT implicitly register an unacknowledged payment, repeat fresh opening-capacity admission for an accepted original or treat a pending/mempool transaction as credit.

#### Scenario: Durable success precedes acknowledgement disclosure

- **WHEN** the provider returns a successful signed acknowledgement
- **THEN** a reopened ledger can retrieve its complete original evidence and unchanged acknowledgement
- **AND** no account, fee obligation or spendable balance is created merely by that acknowledgement

#### Scenario: Registration failure does not partially transfer retention

- **WHEN** registration fails before its atomic commit, including signing or insertion failure
- **THEN** the still-pending original proposal and any previously accepted state remain intact
- **AND** no partial acknowledgement, nonce assignment or released replacement slot survives

#### Scenario: Competing agreements cannot share an acknowledged nonce

- **WHEN** two ledger writers attempt different agreements with the same original sender/nonce
- **THEN** at most one agreement can be acknowledged
- **AND** expiry, rejected settlement attempts and reopening do not let the loser take that original commitment

### Requirement: Authorized acknowledgement recovery preserves the original acceptance

After fresh owner authentication and quoted-payer binding, an exact cached acknowledgement SHALL be returned before fresh spendability, engine qualification, draining/readiness or active-lease checks. Recovery SHALL use the retained original signed quote, require the originally submitted raw-transaction string and its computed hash, and return the stored acknowledgement payload and signature unchanged. A differently cased or reserialized raw-transaction string is not the persisted original representation and SHALL NOT replace it, even if it decodes to the same bytes.

Recovery SHALL NOT require a fresh proposal, quote-retention capacity, quote-signing work permit, committed-issuance allowance, current quote validity, opening-capacity admission, an active lease, an unspent current sender nonce, current value-plus-gas balance or a working fresh-admission RPC. It SHALL NOT renew the lease, register another transaction, free another proposal's slot or renegotiate terms. Different bytes/hash, repricing or a different same-nonce transaction SHALL be refused while the accepted original remains intact. Required owner authority and bounded control/challenge admission remain in force; durable recoverability is not guaranteed immediate access to a fresh proof.

#### Scenario: Cached acceptance survives expiry and unavailable admission

- **GIVEN** the original was durably acknowledged before expiry
- **WHEN** authorized recovery follows expiry/restart with full fresh quote/admission resources, draining, unavailable qualification, changed sender balance/nonce or unavailable RPC
- **THEN** the same acknowledgement and original agreement are returned without new admission or broadcast
- **AND** the original nonce commitment remains protected and a valid original payment remains eligible for healthy-safe credit, not credit from acknowledgement alone

#### Scenario: Repricing or alternate original representation cannot replace acceptance

- **WHEN** recovery presents another signed transaction, another hash, repriced same-nonce bytes or a different raw-string representation
- **THEN** recovery is refused without changing the original acknowledgement, lease or nonce ownership
- **AND** an exact original retry remains eligible for authorized recovery

### Requirement: Native broadcast follows verified durable acknowledgement

The native consumer SHALL accept only an acknowledgement satisfying all message, original-envelope and timestamp checks above, then durably save it with the original signed terms/payment before any client broadcast. It SHALL durably record broadcast-started intent before submitting those same journaled bytes. Failure to verify or persist either prerequisite SHALL prevent client submission; a saved broadcast-started flag without a valid saved acknowledgement SHALL fail closed, not authorize a fresh payment.

Missing, malformed, mismatched or lost acknowledgement responses SHALL NOT authorize broadcast, replacement, automatic fee bump or newly signed funding. A lost registration reply or timeout SHALL be treated as ambiguous, not proven refusal: the provider may already have durably accepted the original. Recovery SHALL seek that exact intent/acknowledgement and original transaction rather than automatically obtain a replacement quote/payment. Once acknowledgement and start state are durable, a missing/mismatched RPC submission response SHALL likewise retain an ambiguous original operation rather than prove rejection. Explicit recovery may resubmit only its original bytes when reconciliation permits it; an already accepted original SHALL NOT be submitted again merely to obtain a response.

Client/operator guidance SHALL disclose that the provider already has independently broadcastable signed bytes during registration. An honest provider's prohibition on broadcasting a definitely refused original is not cryptographic prevention of dishonest early broadcast. A lost acknowledgement or timeout cannot prove that no money moved; client ordering is not escrow, and an off-chain rejection cannot reverse a transfer or make funding refundable.

#### Scenario: Lost acceptance reply is not a definite full refusal

- **GIVEN** an original acknowledgement committed but its response was lost before native persistence
- **WHEN** restart, quote/lease expiry and later full admission capacity precede recovery
- **THEN** fresh owner authority recovers the exact retained acknowledgement, quote, original raw string/hash and sender/nonce ownership without a new lease or quote gate
- **AND** the client persists that acknowledgement before any permitted original submission and does not automatically replace the operation

#### Scenario: Missing or invalid acknowledgement stops client broadcast

- **WHEN** registration fails, returns an invalid acknowledgement, or local acknowledgement/start-state persistence fails
- **THEN** the client does not submit a payment on that attempt
- **AND** it retains recoverable original evidence rather than constructing a new payment

#### Scenario: Durable prerequisites are observable at submission

- **WHEN** the native client submits opening bytes
- **THEN** the saved verified acknowledgement, original byte/hash identity and broadcast-started marker are already durable
- **AND** the submitted bytes equal the prepared and provider-registered original

#### Scenario: Ambiguous submission retains the original operation

- **WHEN** RPC submission fails to return the expected original hash or its response is lost
- **THEN** the journal retains the original as an ambiguous operation and blocks conflicting fresh preparation
- **AND** a permitted explicit recovery uses the same signed bytes without silently replacing or repricing them

### Requirement: Restart recovery does not manufacture another opening

Restart recovery SHALL reconcile the native saved agreement with its original wallet-journal operation. If consumer state already contains original signed bytes but the journal is missing or contains different bytes, recovery SHALL stop without signing or submitting a replacement. If the journal committed before the consumer saved its copy, recovery SHALL retrieve that exact original and repair the missing local copy, not allocate another nonce.

An expired agreement with neither saved nor journaled preparation SHALL NOT initiate a new payment. An existing prepared original without a locally saved acknowledgement SHALL remain eligible to request an earlier provider acknowledgement even after quote expiry; absence of such acceptance SHALL block client broadcast while preserving the original for status/recovery. A provider-accepted acknowledgement whose response/local save was lost SHALL be recovered unchanged before proceeding.

Repeated native `fund` with the same saved signed terms and `resume` SHALL recover the original operation. When an account and exported credential are already saved, native recovery SHALL refresh that account rather than reopen it or implicitly rotate/replace the credential. This does not authorize credential rotation when an opening reply was never saved; that separate opening-recovery case remains subject to `inference-owner-control`. Explicit cancellation remains a separate gas-costing, race-prone operation, not permission for resume to create a replacement or claim a refund.

#### Scenario: Crash checkpoints keep one original

- **WHEN** restart follows journal commit, provider acceptance before local acknowledgement save, local acknowledgement save, broadcast-start save or an uncertain submission
- **THEN** recovery retains one original operation and its exact signed bytes
- **AND** acknowledgement recovery precedes any permitted original submission, including after quote expiry

#### Scenario: Missing or inconsistent journal refuses replacement

- **GIVEN** the consumer has a valid saved original payment
- **WHEN** its original journal entry is absent or contains different signed bytes
- **THEN** recovery fails before a replacement signature or submission and preserves the saved evidence

#### Scenario: Expired unaccepted preparation is not new funding authority

- **WHEN** expired terms have no original preparation, or a prepared original has no recoverable provider acceptance
- **THEN** the client does not create or broadcast a replacement payment
- **AND** any existing preparation remains available for original-operation recovery rather than being erased by expiry

#### Scenario: Saved account recovery preserves exported credentials

- **GIVEN** the native consumer has durably saved the account and exported credential
- **WHEN** the same terms are funded again or the consumer resumes after restart
- **THEN** it recovers original funding and refreshes that account without new opening or implicit credential rotation
- **AND** a revoked or expired credential is not silently replaced by this recovery path

### Requirement: Free opening and accepted recovery keep distinct capacity boundaries

Genuinely free first opening SHALL use authenticated account-opening admission under the pending `specify-non-reserving-quotes` contract, not a paid funding-intent or fabricated zero-value payment. Its existing fewer-than-eight owned-account and fewer-than-10,000 ledger-account checks SHALL occur atomically before first free account creation, not during public quoting. Failed free admission SHALL NOT partially create an account or remove its standalone quote. These authenticated account bounds SHALL NOT become public named-wallet quote gates or fresh capacity grounds for refusing an acknowledged paid opening.

Existing accepted-account, top-up and settlement recovery SHALL NOT require fresh quote retention, work, issuance or opening-capacity admission. Existing authorization, exact original-payment evidence, healthy-safe credit, credential authority and per-inference concurrency/reservation/performance requirements remain in force; these recovery exclusions SHALL NOT waive them or imply complete lifecycle acceptance.

#### Scenario: Free first openings compete without fake funding

- **GIVEN** valid free quotes are issued and only one applicable free-account slot remains
- **WHEN** two otherwise eligible owner-authenticated first openings compete
- **THEN** at most one account is created and the loser receives 429/`rate_limit` without partial account or quote removal
- **AND** neither path prepares or submits a payment or paid funding-intent

#### Scenario: Accepted recovery does not become new quote work

- **GIVEN** an account or acknowledged original already exists and fresh quote/opening resources later fill
- **WHEN** authorized existing-account, original-payment, top-up or settlement recovery proceeds
- **THEN** it is not charged or refused as fresh quoting or opening-capacity admission
- **AND** retained terms, original-operation ownership, balances and existing exported credential authority remain governed by their unchanged recovery contracts
