## ADDED Requirements

### Requirement: The relay catalog serves experimental payment-gated reputation and carries no prompts, inference or authority

The relay catalog (`relay_server`) SHALL serve payment-gated reputation through exactly two routes: `POST /api/ratings/feedback`, which admits a `FeedbackSubmission`, and `GET /api/ratings/:wallet`, which returns a provider's reputation snapshot (book lines 367–372). It SHALL offer no batch-rating route and no public route that lists stored feedback events (book line 372). The relay SHALL NOT receive, forward or store prompts, model output or inference requests, and it SHALL hold no signing wallet for reputation (book lines 43, 51, 59 and 60; `CLAUDE.md`: "`relay_server` serves discovery, reputation, policy, and bounded chain RPC access — not prompts or inference").

Reputation SHALL be the selected catalog's own registry and calculation. It is not blockchain consensus, not a provider-signed score and not merged between catalogs; each independently operated catalog holds its own store, and two catalogs MAY hold different votes (book lines 60, 65, 246 and 415). Which catalog a native consumer reads from and submits to is governed by `inference-offer-discovery` (*Consumer discovery uses a fresh registry and exactly one configured source*).

Reputation is experimental, subjective and scoped to the testnet network. Every success response of both routes SHALL carry `experimental_testnet: true` (book lines 370 and 372). Feedback is not proof of a completed request, usage, model correctness or model execution, and faucet-funded identities, colluding wallets and multiple paid agreements can influence it (book lines 33 and 365). No ranking authority, slashing, escrow, automatic refund or automatic rerouting SHALL follow from a score: discovery, quotes, funding and admission SHALL NOT read it (book line 443). Reputation from the retired file-transfer network SHALL NOT be imported (book line 679).

#### Scenario: Only the two reputation routes exist

- **WHEN** a client sends `GET /api/ratings/0x2222222222222222222222222222222222222222/events` or `POST /api/ratings/batch` to the relay
- **THEN** the relay answers with a 4xx status, returns no stored feedback and stores nothing

#### Scenario: The relay carries no inference

- **WHEN** a client sends `POST /v1/chat/completions` with a prompt to the relay
- **THEN** the relay answers 404 and forwards the prompt nowhere

#### Scenario: A low score gates nothing

- **GIVEN** a provider whose snapshot reports `elo` 12.5 with 40 votes
- **WHEN** a consumer discovers offers and requests a quote from that provider
- **THEN** its offers are listed and its quote is issued exactly as for a provider without votes, and nothing is rerouted

### Requirement: A feedback submission has exactly the book's shape, and every malformed body is refused as invalid_feedback

The body of `POST /api/ratings/feedback` SHALL be a JSON `FeedbackSubmission` with exactly three members (book lines 374–391):

- `feedback`: a signed object whose payload has exactly the members `network`, `provider_wallet`, `consumer_wallet`, `quote_id`, `tx_hash`, `score`, `revision` and `issued_at`;
- `quote`: a signed quote, decoded under the strict schemas of `inference-wire-foundation` (*Strict primitive schemas and canonical wallet identities*);
- `raw_transaction`: the original signed opening transaction as a hexadecimal string.

A submission SHALL carry no prompt text, comment, top-up list or client-reported transfer outcome (book line 393). `score` SHALL be an integer from 0 to 100, and `revision` a positive integer (book line 393). Finer categories, such as stars, are not part of the contract.

A request body larger than 64 KiB (65,536 bytes) SHALL be refused with HTTP 413 before it is decoded (book line 372). Every other request that does not decode as such a `FeedbackSubmission` SHALL be refused with HTTP 400 and the catalog's JSON error object with code `invalid_feedback`, like every other invalid submission (book line 441). This includes:

- a missing or non-JSON `Content-Type`;
- a body that is not valid JSON, or has trailing content;
- a missing member, or an unknown member at any level;
- a member of the wrong type;
- a `score` or `revision` that is not an integer, or is outside the representable range.

A refused body SHALL take no verification slot (*Verification work is bounded by four shared slots and a background pass*), SHALL cause no RPC request and SHALL store nothing.

#### Scenario: An unknown member is invalid feedback

- **WHEN** a submission's feedback payload carries an extra member `"comment": "slow"`
- **THEN** the catalog answers HTTP 400 with JSON error code `invalid_feedback`
- **AND** no RPC request is made and no vote is stored

#### Scenario: An out-of-range score is invalid feedback, not a framework error

- **WHEN** a submission carries `score` 300, `score` 101, `score` 50.5 or `revision` 0
- **THEN** each is answered HTTP 400 with JSON error code `invalid_feedback`, never HTTP 422 or a plain-text body

#### Scenario: A missing member or malformed JSON is invalid feedback

- **WHEN** a submission omits `quote`, or the body is `{"feedback":`, or the body is valid JSON sent without a JSON `Content-Type`, or the body is an otherwise valid submission followed by ` x`
- **THEN** each is answered HTTP 400 with JSON error code `invalid_feedback` and stores nothing

#### Scenario: An oversized body is refused before decoding

- **WHEN** a submission body of 65,537 bytes is posted
- **THEN** the catalog answers HTTP 413, makes no RPC request and stores nothing

#### Scenario: A malformed body never waits for a verification slot

- **GIVEN** all four verification slots are held by snapshot reads
- **WHEN** a body with an unknown member is posted
- **THEN** it is answered HTTP 400 `invalid_feedback`, not HTTP 429 `feedback_busy`

### Requirement: Admission accepts only a payer-signed vote bound to its quote, parties and network

The catalog SHALL admit a decoded submission for payment verification only when all of the following hold (book line 393). Otherwise it SHALL answer HTTP 400 `invalid_feedback`, or `expired_feedback` for the freshness rule below, SHALL make no RPC request and SHALL store nothing (book line 441):

- The feedback signature recovers `consumer_wallet` under the subjective-feedback domain `chiral-inference-feedback-v1`, and the quote signature recovers the quote's `provider_wallet` under the quote domain (book lines 125, 205 and 207; `inference-wire-foundation`, *Length-framed operation domains* and *Canonical recoverable personal signatures*).
- The feedback's and the quote's `network` both equal the catalog's complete network identity: chain ID, genesis hash and deployment ID (book lines 123 and 393).
- The feedback's `provider_wallet`, `consumer_wallet` and `quote_id` equal the quote's `provider_wallet`, `consumer_wallet` and `id`.
- `consumer_wallet` differs from `provider_wallet`. Only this self-feedback is refused; other beneficial-owner overlap is not detected (book line 365).
- The two wallets and the quote's `platform_wallet` are canonical lowercase 20-byte hexadecimal, and `tx_hash`, `quote_id` and the quote's `offer_id` are canonical lowercase 32-byte hexadecimal.
- The quote's `expires_at` is later than its `issued_at`, its `max_concurrency` is positive, its `fee_bps` is 50, and its model passes the offer model validation of `inference-commercial-terms` (*Model identity and capability limits*). The quote need not be unexpired when the vote is submitted.
- The feedback's `issued_at` is not earlier than the quote's `issued_at` and not more than 30 seconds after the catalog's current time.
- The feedback's `issued_at` is not more than 300 seconds before the catalog's current time. A submission that fails only this rule SHALL be answered `expired_feedback`.

The route SHALL need no catalog credential, provider account bearer or owner-challenge exchange; the payer's signature is its authority (book line 397).

#### Scenario: A changed score breaks the payer signature

- **GIVEN** a submission signed by the payer with `score` 100
- **WHEN** its `score` is changed to 1 without signing again
- **THEN** the catalog answers HTTP 400 `invalid_feedback` and stores no vote

#### Scenario: Self-feedback is refused

- **WHEN** the feedback's and the quote's consumer wallet equal the provider wallet
- **THEN** the catalog answers HTTP 400 `invalid_feedback`

#### Scenario: Another deployment is another network

- **WHEN** a submission's network differs from the catalog's only in `deployment_id`
- **THEN** the catalog answers HTTP 400 `invalid_feedback` and makes no RPC request

#### Scenario: Non-canonical identifiers are refused

- **WHEN** a submission's `tx_hash` uses upper-case hexadecimal digits
- **THEN** the catalog answers HTTP 400 `invalid_feedback`

#### Scenario: Freshness boundaries

- **GIVEN** the catalog's current time is T and the quote was issued before T − 400
- **WHEN** feedback is submitted with `issued_at` T − 300, T + 30, T − 301, T + 31, and the quote's `issued_at` minus 1
- **THEN** T − 300 and T + 30 pass these checks, T − 301 is answered `expired_feedback`, and T + 31 and the time before the quote are answered `invalid_feedback`

#### Scenario: An expired quote can still carry a vote

- **GIVEN** a paid agreement whose quote expired one day ago and whose opening payment is canonical at `safe`
- **WHEN** its payer submits current feedback
- **THEN** the quote's expiry does not refuse the vote

#### Scenario: Quote terms outside the contract are refused

- **WHEN** a submission's quote has `fee_bps` 49, `max_concurrency` 0, or `expires_at` equal to `issued_at`
- **THEN** the catalog answers HTTP 400 `invalid_feedback`

### Requirement: Free sessions cannot vote

The catalog SHALL admit feedback only for a paid agreement: the quote's `funding_wei` SHALL be positive, and at least one of its input and output token rates SHALL be nonzero (book line 395). A submission whose quote fails this SHALL be answered HTTP 400 `invalid_feedback`, without an RPC request and without storing anything. Free sessions authenticate and are admitted without a fabricated transfer, and therefore cannot establish reputation eligibility (book lines 351 and 506).

The native client SHALL refuse feedback for an account with no saved opening payment, which is a free session, before signing feedback or contacting the catalog (book line 397).

The pending target-design change `specify-provider-launch-reputation` keeps this rule (*Launch reputation never enters payment-gated reputation, and free service still cannot vote*). It adds a local native refusal for a positive-funding quote whose rates are both zero, and a desktop statement for free accounts. Those additions are target design and are not required of the current runtime.

#### Scenario: Zero funding cannot vote

- **WHEN** a submission's quote has `funding_wei` `"0"`
- **THEN** the catalog answers HTTP 400 `invalid_feedback` and stores no vote

#### Scenario: Positive funding with zero rates cannot vote

- **WHEN** a submission's quote has `funding_wei` `"1000"` and both rates `"0"`
- **THEN** the catalog answers HTTP 400 `invalid_feedback` and stores no vote

#### Scenario: One nonzero rate is a paid agreement

- **WHEN** a submission's quote has `funding_wei` `"1000"`, input rate `"0"` and output rate `"2"`
- **THEN** this rule does not refuse it

#### Scenario: The native client refuses a free session locally

- **GIVEN** a free account saved without an opening payment
- **WHEN** the user requests `feedback` for it
- **THEN** the native operation fails with the statement that free sessions cannot submit payment-backed feedback
- **AND** no feedback is signed and no request reaches the catalog

### Requirement: Admission verifies the exact canonical opening payment at the eSpace safe frontier

`raw_transaction` SHALL be at most 32,768 characters; a longer value SHALL be answered HTTP 400 `invalid_feedback` (book line 395). The catalog SHALL then decode the signed payment itself and SHALL require all of the following (book lines 207 and 395):

- the transaction is bound to the catalog's chain;
- its sender is the feedback's `consumer_wallet`, and its recipient is the provider wallet;
- its value is exactly the quote's `funding_wei`;
- its calldata is exactly the six bytes `CHRL1\0` followed by the quote commitment `Keccak256(P_quote)`;
- the transaction hash computed from the signed bytes equals the feedback's `tx_hash`.

The payment SHALL be accepted only under the settlement policy that `inference-funding-settlement` defines (*Credit uses the canonical-successful-receipt-at-espace-safe-v1 policy*): successful canonical execution at or below the healthy eSpace `safe` frontier, observed consistently twice. Safe acceptance is the application's settlement policy, not irreversible PoS finality (*Settlement stages remain distinct and safe acceptance is not finality*; book line 395). Receipt and block evidence SHALL come only from the catalog's configured RPC endpoint and SHALL be classified as that capability's *Chain evidence comes from signed bytes and the configured identity-checked RPC* classifies it. A bare transaction hash, a payment of any other amount or to another recipient, a top-up commitment, a failed transaction, a pending transaction and a free quote are each insufficient (book line 395).

Outcomes SHALL be (book line 441):

- **Pending:** no receipt, a receipt above `safe`, a receipt whose block is not the canonical block at its height, or observations that disagree. HTTP 409 `payment_pending`.
- **Rejected:** the signed bytes do not satisfy the quote, the hash does not match, or the payment's own execution failed with status `0` at `safe`. HTTP 400 `payment_rejected`.
- **Unavailable:** the RPC fails, the settlement view is unhealthy, the configured RPC fails its chain-ID or genesis identity check, or a receipt contradicts the signed transaction. HTTP 503 `chain_unavailable`. Unavailable evidence SHALL NOT be reported as a rejected payment.
- **Deadline:** verification not complete within 30 seconds. HTTP 503 `chain_timeout`.

None of these outcomes SHALL store or change a vote. The amount stored with an admitted vote SHALL be the value decoded from the signed bytes.

#### Scenario: A canonical opening payment admits the vote

- **GIVEN** a paid agreement whose 1 CFX opening payment is successful and canonical at `safe` in two consistent observations
- **WHEN** its payer submits revision 1 with score 80
- **THEN** the catalog answers `{"accepted":true,"quote_id":<quote id>,"revision":1,"experimental_testnet":true}`
- **AND** one vote is stored with amount `"1000000000000000000"`

#### Scenario: A receipt above safe is pending

- **WHEN** the opening payment has a successful receipt above the current `safe` height
- **THEN** the catalog answers HTTP 409 `payment_pending` and stores no vote

#### Scenario: A top-up or a different amount is not opening evidence

- **WHEN** a submission's raw transaction is a top-up carrying `CHRL2\0` calldata, or an opening whose value is 1 wei less than the quote's `funding_wei`
- **THEN** the catalog answers HTTP 400 `payment_rejected` and stores no vote

#### Scenario: A hash that is not the signed payment's is rejected

- **GIVEN** a `raw_transaction` whose opening payment is successful and canonical at `safe`
- **WHEN** the feedback's `tx_hash` differs from the hash of that `raw_transaction`
- **THEN** the catalog answers HTTP 400 `payment_rejected`

#### Scenario: Failed execution is rejected

- **WHEN** the opening payment's canonical receipt at `safe` has status `0` in two consistent observations
- **THEN** the catalog answers HTTP 400 `payment_rejected`

#### Scenario: Undecodable evidence is rejected

- **WHEN** `raw_transaction` is `"0x01"`
- **THEN** the catalog answers HTTP 400 `payment_rejected` and stores no vote

#### Scenario: Oversized evidence is invalid feedback

- **WHEN** `raw_transaction` has 32,769 characters
- **THEN** the catalog answers HTTP 400 `invalid_feedback` without an RPC request

#### Scenario: An identity mismatch is unavailable, not a rejected payment

- **WHEN** the catalog's configured RPC reports another chain ID while a valid submission is verified
- **THEN** the catalog answers HTTP 503 `chain_unavailable`, not `payment_rejected`, and stores no vote

#### Scenario: RPC failure and deadline

- **WHEN** the configured RPC refuses connections, or verification has not finished after 30 seconds
- **THEN** the catalog answers HTTP 503 `chain_unavailable` or HTTP 503 `chain_timeout` respectively, and stores no vote

### Requirement: One vote per agreement, and a revision replaces it

The store SHALL hold at most one vote per agreement, keyed by `quote_id`, and at most one vote per opening transaction hash, so that one payment cannot vote for several agreements (book line 413). A vote for another agreement that reuses an already stored payment SHALL be refused and SHALL store nothing. Because the opening calldata commits to exactly one quote, payment verification refuses such a submission first, as `payment_rejected` (*Admission verifies the exact canonical opening payment at the eSpace safe frontier*). The store's unique opening hash SHALL refuse any such vote that reaches it, as `feedback_conflict`.

When an admitted submission names an agreement that already has a vote:

- Its opening transaction hash and its verified amount SHALL equal the stored ones; otherwise it SHALL be answered `feedback_conflict` (book line 415).
- A submission identical to the stored evidence, at the same revision, SHALL succeed without change except that it restores the vote's eligibility (book line 413). It is still subject to every admission and payment check, including the 300-second freshness rule, so a later repeat needs a new revision.
- Otherwise its `revision` and its `issued_at` SHALL both be strictly greater than the stored ones (book line 413). The stored score, revision, `issued_at` and evidence SHALL then be replaced in the same row. A revision SHALL NOT add a vote or weight (`CLAUDE.md`: "revisions replace one agreement's vote instead of multiplying its weight").
- Any other submission SHALL be answered `feedback_conflict` and leave the stored vote unchanged. This includes the same revision with different evidence, a lower revision, and a higher revision whose `issued_at` is not later.

The first acceptance time (`created_at`) and the verified amount SHALL be kept across revisions, so a revision does not make a vote younger or heavier (book line 415). A successful submission SHALL be answered `{accepted: true, quote_id, revision, experimental_testnet: true}` (book line 372).

#### Scenario: A revision replaces the vote in place

- **GIVEN** a vote with revision 1, score 100, `issued_at` 100, first accepted at 100
- **WHEN** revision 2 with score 0 and `issued_at` 101 is admitted at 101
- **THEN** the provider still has exactly one vote, with score 0, revision 2 and `created_at` 100
- **AND** the snapshot's `vote_count` is 1 and its `elo` is below 50

#### Scenario: Equivocation at one revision is refused

- **GIVEN** a stored vote with revision 2 and score 0
- **WHEN** a submission with revision 2 and score 90 is admitted
- **THEN** it is answered `feedback_conflict` and the stored score stays 0

#### Scenario: A revision needs a later timestamp

- **GIVEN** a stored vote with revision 2 and `issued_at` 500
- **WHEN** a submission with revision 3 and `issued_at` 500 is admitted
- **THEN** it is answered `feedback_conflict`

#### Scenario: An identical resubmission restores eligibility

- **GIVEN** a stored vote that revalidation marked ineligible while its payment was pending
- **WHEN** the identical submission is posted again within 300 seconds of its `issued_at`, after the payment is canonical at `safe` again
- **THEN** it succeeds, the vote is eligible, and the provider still has exactly one vote

#### Scenario: An old identical resubmission must be revised

- **WHEN** a stored submission is posted again 301 seconds after its `issued_at`
- **THEN** it is answered `expired_feedback`
- **AND** a new revision signed with a current `issued_at` is admitted

#### Scenario: One payment cannot vote for two agreements

- **GIVEN** a stored vote for agreement A with opening transaction H
- **WHEN** a submission for agreement B carries the signed bytes and hash of the same transaction H
- **THEN** it is answered `payment_rejected`, because H's calldata commits to A's quote, and the provider still has one vote

### Requirement: The feedback store is bound to its network, durable and bounded

The catalog's feedback store SHALL be bound to the complete network identity it was created for, and SHALL refuse to open for any other identity (book lines 123 and 413). It SHALL live under the catalog's network-scoped data directory. Each vote SHALL keep the complete admitted evidence (signed feedback, signed quote and raw transaction), the verified amount, the first acceptance time and its eligibility. An admitted vote SHALL be durably committed before its success response, and SHALL survive a catalog restart.

The store SHALL hold at most 100,000 votes in total and at most 1,000 per provider wallet (book line 415). A vote for a new agreement beyond either bound SHALL be refused without storing anything; a revision of a stored vote SHALL remain possible. These bounds are admission bounds. The catalog SHALL NOT promise automatic expiry or pruning, and it is not an unlimited public registry (book line 415).

A store failure (an I/O error, a lock timeout or a corrupt stored record) SHALL be reported as a catalog failure, not as `invalid_feedback`, `expired_feedback` or `feedback_conflict`. The response SHALL NOT include database, file-system or SQL text.

#### Scenario: A store of another network is refused

- **GIVEN** a feedback store created for deployment `test`
- **WHEN** the catalog opens it for deployment `reset` of the same chain
- **THEN** opening fails and the store is unchanged

#### Scenario: Votes survive a restart

- **GIVEN** an admitted vote
- **WHEN** the catalog restarts
- **THEN** the vote, its evidence, amount, first acceptance time and eligibility are unchanged

#### Scenario: The per-provider bound refuses new agreements only

- **GIVEN** 1,000 stored votes for provider P
- **WHEN** a valid vote for a 1,001st agreement with P is admitted, and then a valid revision of one of the 1,000 votes
- **THEN** the new agreement's vote is refused and P still has 1,000 votes
- **AND** the revision is accepted

#### Scenario: A store fault is not a conflict

- **WHEN** the store cannot be written because its database is locked beyond its busy timeout
- **THEN** the submission is not answered `feedback_conflict`, and the response contains no database text

### Requirement: The Elo score is computed exactly from eligible votes

A snapshot SHALL contain `network`, `provider_wallet`, `experimental_testnet: true`, `elo`, `base_elo: 50.0` and `vote_count` (book line 370). The pending target-design change `specify-provider-launch-reputation` adds `votes_14d` (*The reputation snapshot reports recent paid votes*) and leaves this calculation unchanged.

The catalog SHALL compute `elo` as follows (book lines 421–433):

1. Select the eligible votes of exactly the catalog's network and the requested provider wallet whose first-acceptance age, `now − created_at`, is at most 15,552,000 seconds (180 days).
2. Order them ascending by `(created_at, quote_id)`.
3. Start from `E = 50` and, for each vote in order, with `amount` the verified opening amount in wei divided by 10^18:

   ```text
   age_days = max(0, now - created_at) / 86400
   w_time   = max(0, 1 - age_days / 180)
   w_amount = 1 + clamp(ln(1 + amount) / ln(51), 0, 1)
   signal   = score / 100
   expected = 1 / (1 + 10^((50 - E) / 12))
   E        = clamp(E + 4 * w_time * w_amount * (signal - expected), 0, 100)
   ```

4. Round the final `E` to one decimal place as `elo`. `vote_count` is the number of selected votes.

The amount SHALL be the gross canonical opening value. It SHALL NOT be token spend, balance, net credit after the fee, cumulative top-ups or any amount the submitter claims (book line 433). `w_amount` ranges from 1 to 2 and reaches 2 at 50 CFX. A vote exactly 180 days old SHALL count in `vote_count` with zero weight. A revision SHALL keep its vote's position and age (book line 433). With no selected vote, the snapshot SHALL be `elo` 50.0 with `vote_count` 0: the neutral baseline, which means no eligible history in this catalog, not a successful service history and not the absence of payments (book lines 433 and 443). This arithmetic uses floating point and SHALL stay separate from exact integer monetary settlement (book line 433).

#### Scenario: No votes is the neutral baseline

- **WHEN** a provider with no eligible vote is read
- **THEN** the snapshot has `elo` 50.0, `base_elo` 50.0 and `vote_count` 0

#### Scenario: One positive vote

- **WHEN** the only vote has score 100, a 1 CFX opening and age 0
- **THEN** `elo` is 52.4, because `w_amount` is 1 + ln 2 / ln 51 and `E` = 50 + 4 × 1.17629 × 0.5

#### Scenario: The amount weight is capped at 50 CFX

- **WHEN** the only vote has score 0 and age 0, once with a 50 CFX opening and once with a 100 CFX opening
- **THEN** `elo` is 46.0 in both cases

#### Scenario: The 180-day boundary

- **WHEN** the only vote has score 0 and its first acceptance was exactly 15,552,000 seconds ago
- **THEN** `elo` is 50.0 and `vote_count` is 1
- **AND** one second later `vote_count` is 0

#### Scenario: Order is fixed by acceptance time and quote ID

- **GIVEN** two votes with 1-wei openings, both first accepted at the time of the snapshot: quote `0x11…11` with score 100 and quote `0x22…22` with score 0
- **WHEN** the snapshot is computed, whatever order the votes arrived in
- **THEN** `elo` is 49.6, the result of applying `0x11…11` first (the reverse order would give 50.4)

#### Scenario: Top-ups and spending do not weigh

- **GIVEN** an agreement opened with 10 CFX, later topped up with 40 CFX, with 3 CFX spent
- **WHEN** its payer's vote is weighed
- **THEN** `amount` is 10 and `w_amount` is 1 + ln 11 / ln 51

### Requirement: Snapshots revalidate stored evidence and never serve a score from unavailable evidence

Before returning a provider's snapshot, the catalog SHALL revalidate every stored vote of that provider (book line 439): the admission checks of *Admission accepts only a payer-signed vote bound to its quote, parties and network* and *Free sessions cannot vote*, except the 300-second freshness rule, including both signatures; and the opening payment under *Admission verifies the exact canonical opening payment at the eSpace safe frontier*. A vote SHALL be eligible exactly when every check passes and the verified value equals its stored amount.

Invalid evidence, rejected execution, and a pending or non-canonical payment SHALL make the vote ineligible. Its evidence SHALL be kept, and a later revalidation that passes SHALL make it eligible again (book line 439). A missing receipt SHALL be treated as pending for this purpose and removes the vote from the current score. It SHALL NOT be treated or described as a ledger funding reversal or a reorganization (book line 439). The funding ledger's rules are separate and unaffected: `inference-funding-settlement` (*Credited funding is monitored and a missing receipt is not a reorg*, *A proven orphan freezes spending, reverses unused credit and journals loss* and *Orphan restoration compensates the reversal exactly once*).

When required RPC evidence is unavailable for any vote, the snapshot SHALL fail with HTTP 503 `chain_unavailable` instead of serving a known-stale score (book line 441). Unavailable evidence includes a configured RPC that fails its identity check and a receipt that contradicts the signed transaction, and it SHALL NOT change that vote's eligibility. When revalidation does not finish within 30 seconds, the snapshot SHALL fail with HTTP 503 `chain_timeout` (book line 441). A wallet path parameter that is not a canonical lowercase wallet SHALL be refused with HTTP 400 before any verification.

#### Scenario: A payment that leaves safe stops counting and keeps its evidence

- **GIVEN** an eligible vote counted in its provider's snapshot
- **WHEN** its opening payment's receipt is missing, or lies above the current `safe` frontier, at the next snapshot
- **THEN** the snapshot excludes it and its evidence is still stored
- **AND** when the payment is canonical at `safe` again, a later snapshot counts it again

#### Scenario: A missing receipt is not a funding reversal

- **GIVEN** a vote whose opening payment's receipt is missing at revalidation
- **THEN** the vote is ineligible, and no statement or record of the catalog calls the payment orphaned or reversed

#### Scenario: Corrupted stored evidence is ineligible

- **WHEN** a stored vote's signed quote no longer verifies under the provider wallet
- **THEN** the vote is excluded from the snapshot and kept in the store

#### Scenario: An RPC outage serves no score

- **GIVEN** a provider with eligible votes
- **WHEN** the RPC is unreachable while its snapshot is computed
- **THEN** the catalog answers HTTP 503 `chain_unavailable` without `elo` or `vote_count`
- **AND** no vote's eligibility is changed by the unavailable observation

#### Scenario: An identity mismatch serves no score

- **GIVEN** a provider with eligible votes
- **WHEN** the configured RPC reports another chain ID while its snapshot is computed
- **THEN** the catalog answers HTTP 503 `chain_unavailable`, and no vote becomes ineligible

#### Scenario: Revalidation past its deadline

- **WHEN** a snapshot's revalidation has not finished after 30 seconds
- **THEN** the catalog answers HTTP 503 `chain_timeout`

### Requirement: Verification work is bounded by four shared slots and a background pass

The catalog SHALL run at most four verifications at once. Submission, snapshot reads and background reconciliation SHALL share these four slots (book line 372). A submission or snapshot read that finds no free slot SHALL be refused at once with HTTP 429 `feedback_busy`, not queued (book line 441). Chain verification under a slot SHALL end by the 30-second deadline, after which the slot is released with the request's answer.

While the catalog runs, background reconciliation SHALL revalidate stored votes without any reader, with the same revalidation as a snapshot and keeping the evidence of ineligible votes (book line 441). It SHALL visit provider wallets in a stable order, in pages of at most 32, nominally every 30 seconds, and SHALL start again from the first provider after the last. It SHALL use a slot only when one is free and skip that provider otherwise. Its cadence is not a guaranteed bound on when every vote has been revalidated (book line 441). It SHALL stop when the catalog shuts down.

#### Scenario: Busy verification is refused, not queued

- **GIVEN** four snapshot reads are in progress
- **WHEN** a valid submission and a fifth snapshot read arrive
- **THEN** both are answered HTTP 429 `feedback_busy` at once, and no vote is stored

#### Scenario: Background reconciliation without readers

- **GIVEN** an eligible vote whose opening payment then leaves the `safe` frontier, and no snapshot reads
- **WHEN** background reconciliation reaches its provider
- **THEN** the vote is marked ineligible and its evidence is kept

#### Scenario: Pages of 32 providers

- **GIVEN** votes for 70 provider wallets
- **WHEN** background reconciliation runs with free slots
- **THEN** successive passes visit 32, 32 and 6 providers in wallet order, then start again from the first

### Requirement: The native client submits feedback signed locally from the saved opening evidence

The native `feedback {contract_id, score, revision}` operation (desktop and CLI) SHALL (book lines 246, 397 and 933):

- refuse a `score` above 100 or a `revision` of 0 before signing anything or contacting the catalog;
- fail with an error when no reputation catalog is configured, as `inference-offer-discovery` requires;
- refuse a free session as *Free sessions cannot vote* requires;
- build the feedback from the account's saved signed quote and its saved original opening transaction and hash, with the loaded wallet as `consumer_wallet`, the complete configured network identity, and the current time as `issued_at`;
- sign it locally with the consumer wallet under the subjective-feedback domain;
- send exactly `{feedback, quote, raw_transaction}` to the configured catalog's `/api/ratings/feedback` under the outbound HTTPS origin rules.

It SHALL NOT send a private key, a provider account bearer, an application credential or an owner challenge, and SHALL NOT print the raw payment evidence as a diagnostic (book line 397).

It SHALL report the catalog's outcome by the catalog's code: `invalid_feedback`, `expired_feedback`, `feedback_conflict`, `feedback_busy`, `payment_pending`, `payment_rejected`, `chain_unavailable` and `chain_timeout` SHALL each be distinguishable to the user. They SHALL be reported as the catalog's answer, never as a provider failure (book line 441). The desktop form is governed by `desktop-gui-pages` (*Feedback is payment-backed and revisable*).

#### Scenario: The submission is signed locally and carries no credential

- **GIVEN** a paid account with a saved opening payment and a configured catalog
- **WHEN** the user submits score 80 with revision 2
- **THEN** the catalog receives exactly the members `feedback`, `quote` and `raw_transaction`, with feedback signed by the loaded consumer wallet, score 80, revision 2 and the current time
- **AND** the request carries no `Authorization` header and no private key

#### Scenario: Invalid values are refused locally

- **WHEN** the native operation is called with score 101 or revision 0
- **THEN** it fails before signing, and no request reaches the catalog

#### Scenario: No catalog is configured

- **GIVEN** a profile without `catalog_url`
- **WHEN** the user submits feedback for a paid account
- **THEN** the operation fails because no reputation catalog is configured

#### Scenario: Catalog answers are reported by their code

- **WHEN** the catalog answers HTTP 429 `feedback_busy`, HTTP 400 `feedback_conflict` or HTTP 409 `payment_pending`
- **THEN** the native result names `feedback_busy`, `feedback_conflict` or `payment_pending` respectively, as an answer of the reputation catalog
- **AND** none is reported as "Provider operation failed"

### Requirement: Native inspection reports reputation or its unavailability, never a fabricated score

Native provider inspection (`provider_info`) SHALL read the snapshot for the offer's provider wallet from the configured catalog (book line 246) and SHALL report it as available only when all of these hold (book line 443):

- the response is a success;
- its `network` equals the consumer's complete network identity, and its `provider_wallet` equals the offer's;
- `experimental_testnet` is `true`;
- `elo` is a finite number from 0 to 100, and `vote_count` a nonnegative integer.

Otherwise it SHALL report reputation as unavailable, with a reason. This covers no configured catalog, an unreachable catalog, any error status (including HTTP 503 `chain_unavailable` or `chain_timeout` and HTTP 429 `feedback_busy`), and an invalid or mismatching response. The reputation result SHALL stay separate from the health and readiness observations, as `inference-offer-discovery` requires (*Native provider inspection reports observations separately and changes no terms*).

Inspection SHALL NOT substitute zero, the baseline or any other value for an unavailable score. It SHALL NOT word catalog or RPC unavailability as a provider failure or as evidence that the provider is untrustworthy (book line 443). A returned neutral baseline SHALL be reported as available, with its vote count. How the desktop shows the result is governed by `desktop-gui-pages` (*The Marketplace discovers, inspects and funds in one gated order*). The pending target-design change `specify-provider-launch-reputation` reports launch reputation separately (*Native provider inspection reports the launch reputation separately*).

#### Scenario: A catalog outage is not a provider failure

- **WHEN** the catalog answers a snapshot read with HTTP 503 `chain_unavailable`
- **THEN** inspection reports reputation unavailable, with a reason that names the reputation catalog's chain evidence
- **AND** no `elo` or vote count is reported, the reason does not say that the provider failed, and health and readiness are reported as observed

#### Scenario: A mismatching snapshot is unavailable

- **WHEN** the catalog returns a snapshot for another deployment, for another provider wallet, or with `elo` 150
- **THEN** inspection reports reputation unavailable and shows no score

#### Scenario: The neutral baseline is available

- **WHEN** the catalog returns `elo` 50.0 with `vote_count` 0 for the offer's provider
- **THEN** inspection reports reputation available with `elo` 50.0 and 0 votes
