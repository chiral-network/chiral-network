# inference-account-opening-request Specification

## Purpose
Define strict account-opening request decoding, exact-body payer authority, free/paid evidence rules and credential-preserving recovery of accepted agreements without fresh admission or duplicate credit.

## Requirements
### Requirement: Opening requests use a strict named-field object

`POST /v1/contracts/open` SHALL accept a JSON object whose only members are required string `quote_id` and optional nullable string `raw_transaction`. A missing or null `raw_transaction` SHALL decode as absence; a string, including an empty string, SHALL remain a supplied value for subsequent payment validation and SHALL NOT be converted to absence. Missing or mistyped `quote_id`, a non-string/non-null payment, unknown members and duplicate members SHALL be rejected with HTTP 400 and `invalid_request`. Root arrays, including complete positional projections and arrays omitting the optional payment, SHALL likewise be rejected instead of projected into an object.

The quote identifier SHALL select the exact retained agreement in this provider's ledger; it SHALL NOT be inferred from a wallet, offer, transaction hash or payment. Schema acceptance of a string SHALL NOT imply that the identifier exists or that transaction evidence is valid. The existing bounded control reader SHALL apply before processing; this requirement adds no new caller, wallet or quote allowance.

The native writer SHALL serialize the named-field object, including `raw_transaction: null` for absence. Server acceptance of an omitted optional member SHALL remain supported. Neither representation SHALL alter the retained signed quote or financial evidence.

#### Scenario: Optional payment syntax remains distinct from authorization

- **WHEN** a request contains a string quote identifier and either omits `raw_transaction` or supplies null
- **THEN** both object forms pass structural decoding as absent payment
- **AND** ownership, agreement-derived payment requirements and admission remain necessary before any account result

#### Scenario: Positional or malformed requests are not aliases

- **WHEN** a request is an array containing the quote identifier with or without a payment value, or has an unknown, duplicate, missing required or mistyped member
- **THEN** the provider returns 400 / `invalid_request` without normalizing it into a valid opening request
- **AND** no quote, account, credential, funding or fee mutation results from that schema failure

#### Scenario: Empty payment is not missing payment

- **WHEN** the request supplies `raw_transaction` as an empty string
- **THEN** it remains supplied evidence and is rejected under the selected agreement's payment rules
- **AND** it is not treated as a free opening or repaired into another signed transaction

### Requirement: Opening parses before consuming exact owner authority

The provider SHALL finish opening-request structural decoding before attempting owner authentication. A schema rejection SHALL NOT consume a supplied owner challenge. After decoding, the provider SHALL authenticate the one-use owner proof against the exact received body bytes, method and path under `inference-owner-control`, then look up the retained quote and require the authenticated wallet to equal its consumer. Parsing SHALL NOT replace the raw body used by authentication with a canonicalized or reserialized representation.

A valid body without owner authority SHALL NOT open an account, even for free terms. A successful authentication followed by quote lookup, ownership, payment, expiry or admission refusal SHALL leave that proof consumed, without treating consumption as evidence of a successful financial operation. A retry SHALL obtain fresh authority for its exact bytes and retain the original agreement/payment identity.

#### Scenario: Malformed request preserves an unconsumed supplied proof

- **GIVEN** a stored unexpired owner proof binds the exact malformed opening bytes
- **WHEN** structural decoding rejects those bytes
- **THEN** the provider returns 400 / `invalid_request` before owner consumption
- **AND** its stored challenge remains unconsumed and financial/account state remains unchanged

#### Scenario: Omission and null do not share a body signature

- **GIVEN** a proof binds an object omitting `raw_transaction`
- **WHEN** the sender instead transmits a different body containing explicit null, with the same proof
- **THEN** exact-body authentication fails without consuming the correctly bound challenge
- **AND** semantic equivalence after decoding does not authorize reserialization

#### Scenario: Business refusal does not restore authority

- **GIVEN** a structurally valid body and a correctly bound owner proof
- **WHEN** the quoted owner, retained agreement or later payment/admission checks refuse the operation after authentication
- **THEN** the proof stays consumed without partial account or credential creation
- **AND** fresh authorization for recovery is not permission to create a replacement payment

### Requirement: Retained agreement terms determine payment presence

Opening SHALL use the retained valid commercial agreement to distinguish genuinely free terms from a paid opening; absent payment SHALL NOT select a free branch for a paid quote. A genuinely free agreement SHALL require absent payment, accept either omission or explicit null at the request boundary, and reject every supplied transaction string with `invalid_payment`. It SHALL NOT prepare, register, submit or credit a fabricated zero-value payment. First free opening SHALL still require an unexpired quote, quoted-owner authority, ordinary new-admission readiness and atomic authenticated account-capacity admission under the commercial/non-reserving contract.

A paid opening SHALL require the exact originally registered raw-transaction string and matching durable funding-intent evidence before considering first creation or existing-account replay. Missing/null payment SHALL fail with HTTP 409 and `funding_intent_required`; unregistered, altered or invalid originals SHALL NOT be inferred, replaced or accepted merely because the account already exists. First paid credit SHALL additionally retain all original-envelope, canonical execution and healthy-safe-frontier checks; syntax, owner authority and acknowledgement alone SHALL NOT credit funds.

#### Scenario: Supplied payment cannot fund a free agreement

- **WHEN** an owner submits a transaction string, including an empty string, for a genuinely free agreement
- **THEN** the provider refuses it with `invalid_payment` after successful owner authentication
- **AND** no payment, paid intent, credited funding or fee obligation is created

#### Scenario: Paid omission is refused before creation and replay

- **GIVEN** a paid agreement, whether unopened or already associated with an account
- **WHEN** its authenticated opening request omits the original transaction or supplies null
- **THEN** the request receives 409 / `funding_intent_required`
- **AND** existing financial and credential state is preserved rather than silently inferring payment or reopening as free

#### Scenario: Exact evidence remains necessary for an existing paid account

- **GIVEN** an account already retains its acknowledged paid opening
- **WHEN** an owner submits another raw string or payment instead of that original
- **THEN** replay is refused while the original agreement, acknowledgement and credential authority remain intact
- **AND** the exact-original request remains eligible for authorized recovery

### Requirement: Opening replay preserves accepted account state

Request shape, quoted-owner binding and agreement-derived payment-presence checks SHALL apply to both first opening and replay. Once those checks succeed for an existing account, opening replay SHALL return the retained account without new account creation, funding credit, fee obligation or credential issuance/rotation. It SHALL preserve revoked/expired credential authority rather than mint a replacement secret. Credential recovery SHALL remain explicit under the existing owner-control contract.

Existing-account replay SHALL NOT require an unexpired original free quote, new-funding readiness, available engine/qualification, a new quote-work/issuance permit or fresh opening capacity. These recovery exclusions SHALL NOT bypass the required paid original or weaken canonical-chain monitoring and original-operation recovery. A lost response SHALL remain ambiguous; neither absence of a response nor proof consumption SHALL authorize another payment.

#### Scenario: Free account replay outlives fresh admission

- **GIVEN** a free account already exists and its original quote has expired or fresh admission is unavailable
- **WHEN** its owner submits a correctly bound object request with absent payment
- **THEN** the existing account is returned without fresh admission or replacement credentials
- **AND** a distinct unopened expired free quote remains ineligible for first creation

#### Scenario: Paid replay does not create another credit or secret

- **GIVEN** a paid account exists with its original acknowledged payment and exported credential
- **WHEN** its owner repeats a valid opening request using that exact original after readiness or capacity is lost
- **THEN** the retained account is returned without duplicate funding, fees, quote-budget charges or credential rotation
- **AND** missing or substituted evidence remains refused under the preceding requirements

