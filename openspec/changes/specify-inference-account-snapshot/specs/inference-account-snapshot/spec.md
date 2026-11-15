## ADDED Requirements

### Requirement: Account snapshots have one complete named-field schema

An `AccountView` SHALL be a JSON object containing exactly the required members below. Receivers SHALL reject positional arrays, scalars, null, unknown members, duplicate members in raw JSON, missing members and wrong member types. Object member order SHALL NOT affect acceptance. A typed reserialization SHALL preserve the same named fields and values, without introducing defaults or aliases. Nested network, model/profile and price values SHALL obey their own wire/commercial schemas; a nullable nested model field SHALL retain its existing contract rather than making the account's entire model optional.

This payload SHALL be shared by `GET /v1/contracts/:id`, `OpenResult.account`, successful top-up account responses and `UsageReceipt.account`. Its use in a local cache or provider ledger SHALL NOT justify a different public monetary representation or automatic conversion of older financial state.

| Member | Type and meaning |
|---|---|
| `contract_id` | String identifying the original quote/account |
| `network` | Complete network identity object |
| `provider_wallet` | Canonical provider wallet string |
| `consumer_wallet` | Canonical payer wallet string |
| `model` | Complete original agreed model/profile object |
| `price` | Original input/output price object |
| `status` | Required lifecycle-status string, interpreted under the status requirement below |
| `funded_wei` | Canonical wei string: gross funding credited for original accepted payments, with reversals tracked separately |
| `fee_wei` | Canonical wei string: current funding-fee liability retained in account accounting, not proof of platform receipt |
| `spent_wei` | Canonical wei string: settled inference charges, excluding funding fees and gas |
| `reserved_wei` | Canonical wei string: current monetary holds for unsettled requests |
| `available_wei` | Canonical wei string: currently available prepaid credit under this account's accounting |
| `reversed_wei` | Canonical wei string: current funding reversals, reduced by verified restoration rather than counted as new funding |
| `provider_loss_wei` | Canonical wei string: current funding-reconciliation loss compensation, not cumulative request unknown-usage exposure |
| `usage` | Complete accounting usage object defined below |
| `waived_wei` | Canonical wei string: cumulative foregone nominal inference charges for known usage, not a new balance or refundable amount |
| `max_concurrency` | Unsigned 32-bit integer JSON number equal to the original agreement's concurrency ceiling, not a count of remaining slots |

#### Scenario: Complete producer snapshot survives representation changes

- **GIVEN** a valid snapshot emitted by the provider
- **WHEN** its object members are reordered without changing their values
- **THEN** typed decoding and reserialization preserve every named field and exact value
- **AND** its canonical signed account representation is unchanged

#### Scenario: Snapshot aliases and omitted members are refused

- **WHEN** an account is supplied as a positional array, with an extra or duplicate member, or with a required member omitted, null or of the wrong type
- **THEN** decoding rejects the snapshot rather than normalizing it to a different typed statement
- **AND** the refusal applies inside an opening response or signed receipt as well as to a standalone account response

#### Scenario: A signature does not authenticate discarded additions

- **GIVEN** an otherwise valid signed receipt
- **WHEN** its account object contains an unrecognized member while retaining the old signature
- **THEN** account decoding refuses the receipt rather than removing the member and accepting the signature over a projection

### Requirement: Accounting token usage preserves two exact counters

A Chiral accounting `TokenUsage`, including account and request-record usage, SHALL be a JSON object containing exactly required `prompt_tokens` and `completion_tokens` members. Each SHALL be an unsigned integer JSON number in the inclusive range `0` through `18446744073709551615` (`2^64 - 1`). Missing, null, string, Boolean, negative, floating-point/exponent, out-of-range, duplicate or unknown members and non-object containers SHALL be rejected. Zero SHALL be an explicit counter value, not a default supplied for an absent member. Producers and consumers processing signed accounting values SHALL preserve integer precision through decoding, canonicalization and export; floating-point rounding or a narrower language-native range SHALL NOT silently change a statement.

This two-counter object SHALL NOT be substituted for the separately defined OpenAI-compatible endpoint/engine usage schema. In particular, accepting or validating an OpenAI `total_tokens` field elsewhere SHALL NOT make it an extra member of accounting `TokenUsage`.

Account counters SHALL retain settled authoritative known usage even when inference charges are waived. Unknown-usage outcomes SHALL remain distinguishable through request status and SHALL NOT invent counts or turn zero counters into evidence of zero-token execution. Wire-valid counters SHALL NOT bypass request token bounds, authoritative-usage validation or checked accumulation.

#### Scenario: Exact counter boundaries

- **WHEN** either accounting counter is zero or `18446744073709551615`
- **THEN** its integer value is represented exactly in the typed accounting statement
- **AND** `18446744073709551616`, `-1`, `1.0`, `1e0` and numeric strings are refused rather than rounded or coerced

#### Scenario: A complete usage array is still not an object

- **WHEN** usage is `[1, 2]`, lacks a counter or adds `total_tokens`
- **THEN** accounting decoding rejects it
- **AND** the valid named object with `prompt_tokens: 1` and `completion_tokens: 2` remains usable in either member order

#### Scenario: Waived or unavailable work is not erased or invented

- **GIVEN** a known-usage waived request and a separate unknown-usage request
- **WHEN** their account and request usage is reported
- **THEN** known authoritative counts remain recorded while the unknown outcome retains its explicit status without invented counters
- **AND** a zero charge or zero counter does not independently establish that no work occurred

### Requirement: Snapshots preserve original agreement identity and wide monetary conservation

Before accepting a provider account response for a retained agreement, the native consumer SHALL require exact equality of `contract_id`, the complete network, provider wallet, payer wallet, model/profile, price and `max_concurrency` with that original agreement. Refresh, opening recovery, top-up response handling and receipt acceptance SHALL NOT renegotiate these values. Permitted evidence-only renewal SHALL remain per-request admitted evidence; it SHALL NOT replace the original account model/profile or price.

Every account monetary field SHALL satisfy the canonical unsigned decimal `u128` wei contract in `inference-wire-foundation`. A valid snapshot SHALL satisfy:

`funded_wei + provider_loss_wei = fee_wei + reversed_wei + spent_wei + reserved_wei + available_wei`

Validation SHALL use sufficiently wide exact integer arithmetic, without requiring the intermediate sums to fit `u128` or converting money to floating point. `waived_wei` SHALL be validated as money but SHALL NOT be added to either side as a balance credit. Request-level `provider_loss_wei` SHALL NOT be accumulated into the account loss-compensation field. Snapshot validation SHALL NOT recompute current fee liability from a single percentage of gross funding or erase reversal/loss fields: funding payments are separately rounded and reconciliation retains its own fee-batch/restoration rules.

A malformed or mismatched received snapshot SHALL NOT replace the saved native account, install an accompanying credential, advance the accepted receipt sequence or erase original pending payment evidence. This consumer refusal SHALL NOT be represented as proof that the provider failed to commit an opening, top-up or receipt issuance. Recovery SHALL preserve the original operation and existing credential rules rather than automatically creating another paid operation.

#### Scenario: Re-signed commercial substitution is refused

- **GIVEN** an existing agreement and an otherwise valid provider response
- **WHEN** the response changes the account identity, network, parties, model/profile, price or concurrency
- **THEN** native account acceptance fails even if an enclosing provider signature is valid
- **AND** the original saved agreement and existing credential remain unchanged

#### Scenario: Reversal and loss balances use wide sums

- **GIVEN** individually valid canonical money fields whose conserved totals exceed `u128::MAX`
- **WHEN** the account's extended balance identity is checked
- **THEN** exact equal totals are accepted without overflow or truncation
- **AND** a one-wei mismatch or malformed money field is refused without updating the cached account or accepted receipt watermark

#### Scenario: Renewal changes a request report rather than the account agreement

- **GIVEN** an original profiled account and a request admitted under a permitted newer, otherwise-identical qualification report
- **WHEN** a signed receipt reports that request
- **THEN** its account retains the original agreed model/profile and price
- **AND** the request retains its actual admitted report under the request-accounting contract without requiring the account model to be rewritten

#### Scenario: A malformed post-commit response leaves recovery reachable

- **GIVEN** the provider has committed an opening or top-up and the consumer retains its original payment evidence
- **WHEN** the response contains an invalid account snapshot
- **THEN** the consumer refuses that snapshot without abandoning the original evidence or rotating an existing exported credential
- **AND** a later valid original-operation recovery can expose the existing account without another credit or replacement payment

### Requirement: Lifecycle status and attribution do not create new authority

Producers SHALL retain the existing lifecycle distinctions: `free` for an opened free agreement, `active` for an active paid account, `settlement_pending` for the funding-reconciliation pending state, and `frozen` for an account requiring funding investigation. This snapshot contract SHALL NOT introduce a different funding transition policy, treat unavailable evidence as a proven orphan, or turn a status label into proof of chain finality.

The status wire field SHALL remain a string. Receivers MAY retain and display an unfamiliar status, but SHALL NOT coerce it to `active` or `free`, infer spendability from it, or bypass any operation's explicit eligibility checks. Authorized reads of a frozen or pending account SHALL remain available under the existing owner/read-bearer rules; snapshot decoding SHALL NOT discard those accounts merely because new inference or funding admission is unavailable.

An ordinary account response SHALL NOT be described as a standalone signed attestation. Only an enclosing verified receipt attributes its accounting statement to the provider; neither response proves honest metering, model execution, current engine readiness, current available concurrency, irreversible chain finality, completed platform remittance, transferability or a refund right. Receipts SHALL retain the original provider-specific, non-refundable balance and plaintext-provider trust disclosures. Account snapshots SHALL contain no bearer, private key, prompt or generated content; credential delivery remains a separate authorized object.

#### Scenario: Pending or frozen balances remain inspectable

- **GIVEN** a genuine `settlement_pending` or `frozen` account and valid read authority
- **WHEN** its snapshot is requested
- **THEN** the complete original account and accounting fields remain readable
- **AND** decoding does not reset its status, clear historical money or infer permission for a new paid operation

#### Scenario: An unfamiliar status is not an active account

- **WHEN** a structurally valid otherwise-bound account carries an unfamiliar status string
- **THEN** a receiver can retain that status as an opaque provider statement
- **AND** it cannot silently grant active/free admission or replace the status with a successful default

#### Scenario: Zero monetary holds do not certify idle capacity

- **GIVEN** free requests with zero monetary reservations or an account whose historical requests have settled
- **WHEN** its snapshot reports zero reserved money
- **THEN** that amount is not represented as proof of current engine readiness, unused concurrency slots or absence of past execution
