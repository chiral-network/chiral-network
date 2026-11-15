# inference-account-topup-request Specification

## Purpose
Define the strict owner-authorized top-up request boundary for existing paid accounts: exact object decoding, active-account and verified-payment eligibility, once-only credit and fee accounting, and original-preserving native recovery without fresh opening admission.

## Requirements
### Requirement: Top-up requests contain exactly one required payment string

`POST /v1/contracts/:id/topup` SHALL accept a JSON object containing exactly one member, `raw_transaction`, whose value SHALL be a string. Missing, null or non-string payment values, unknown or duplicate members, malformed JSON and non-object roots SHALL fail request decoding with HTTP 400 and `invalid_request` after the authority checks defined below. A complete positional array SHALL NOT be projected into the named-field object. An empty string SHALL remain a supplied string subject to payment validation, not become an omitted payment or a free-session operation.

The existing bounded control reader SHALL remain in force. The account SHALL be selected from the route identifier, not inferred from a payment, wallet, quote, amount or additional body field. This request SHALL NOT carry replacement commercial terms or a caller-asserted credit amount. Successful decoding SHALL NOT imply valid signed payment evidence or permission to credit it.

#### Scenario: Named-field object is the interoperable request

- **GIVEN** a valid account owner's proof bound to an existing account's top-up route and exact request bytes
- **WHEN** the body is an object containing only a string `raw_transaction`
- **THEN** it reaches the account/payment checks with that supplied string preserved
- **AND** no payment validity or credit is inferred from decoding alone

#### Scenario: Invalid shape is not an alternate payment submission

- **GIVEN** ownership has been established for the route account
- **WHEN** the body is a positional array, scalar, malformed JSON or an object with missing, null, mistyped, unknown or duplicate members
- **THEN** the provider returns HTTP 400 and `invalid_request`
- **AND** it performs no payment verification, funding-attempt recording, account credit, fee mutation or credential mutation

#### Scenario: Empty supplied evidence is not absence

- **WHEN** the authenticated request supplies `raw_transaction` as an empty string
- **THEN** the schema preserves the string and the payment/business checks refuse it without credit
- **AND** the request cannot create a free account or substitute a zero-value funding operation

### Requirement: Top-up ownership is established before decoding the payment request

After bounded body reading and route validation, the provider SHALL authenticate the existing account owner's one-use proof against its configured full network identity, provider wallet, actual method, exact request target including any query, and digest of the exact received body bytes. It SHALL then look up the route account and require equality with its consumer wallet before decoding `TopupRequest`. A bearer, public payment hash, signed payment or opening acknowledgement SHALL NOT substitute for that proof. The native consumer SHALL sign and transmit the same serialized object bytes.

The route identifier SHALL contain lowercase `0x` followed by exactly 64 hexadecimal digits; invalid syntax SHALL return HTTP 400 and `invalid_contract_id` before consuming authority. Account lookup SHALL use the literal accepted identifier, without inferring another account from equivalent commitment bytes. A valid challenge for a syntactically valid path SHALL NOT establish that an account exists.

Invalid proof binding/signature SHALL return HTTP 401 and `invalid_api_key` without consuming a valid challenge. After successful authentication, a missing account SHALL return HTTP 404 and `not_found`, and another account owner's proof SHALL return HTTP 403 and `forbidden`, before body decoding or payment verification. Successfully authenticated authority SHALL remain consumed after these failures and after malformed-body, eligibility or payment refusal. This ordering is intentionally distinct from the opening request's parse-before-authentication contract.

#### Scenario: Malformed body does not bypass account authority

- **WHEN** a malformed top-up body is sent without a usable owner proof, including with a valid account bearer
- **THEN** the provider refuses owner authentication rather than exposing top-up schema acceptance or invoking chain verification
- **AND** no account, credential or financial state changes

#### Scenario: Authenticated malformed request consumes its proof

- **GIVEN** an existing account's owner has signed the exact malformed request bytes
- **WHEN** the top-up request reaches decoding and is rejected
- **THEN** its challenge is already consumed and cannot authorize a second attempt
- **AND** the rejection does not imply a payment was submitted or credited

#### Scenario: Exact body and target cannot be substituted

- **GIVEN** a usable proof bound to one top-up body and literal request target
- **WHEN** either body bytes or target spelling is changed, even if parsed values would be equivalent
- **THEN** binding fails without consuming the original proof
- **AND** its exact original request remains eligible for authentication before expiry

#### Scenario: Ownership and existence precede schema errors

- **WHEN** a genuine proof authenticates another account's owner, or names a syntactically valid absent account, with a malformed body
- **THEN** the provider returns the ownership or not-found refusal respectively, not a decoding result
- **AND** consumes the authenticated proof without invoking payment verification

### Requirement: Top-up settlement credits only the existing active paid agreement

After request decoding, the provider SHALL require the existing account to be active and paid. Free or frozen accounts SHALL receive HTTP 409 and `account_frozen` before chain verification; a top-up SHALL NOT convert a free account into a paid agreement. This eligibility check SHALL remain enforced atomically with credit, not only by the HTTP handler.

The provider SHALL independently verify the signed payment's network/chain binding, canonical signature and identity, account payer, provider recipient and exact `CHRL2\0` plus decoded account-ID commitment. The verified value SHALL be positive and within the checked monetary range; it need not equal the original opening deposit. A new quote, opening-intent acknowledgement, client-supplied amount or receipt alone SHALL NOT replace this evidence. Stable successful canonical execution at the healthy eSpace `safe` frontier SHALL remain necessary before credit, including on settlement replay. Free-string or request-shape acceptance SHALL NOT waive those conditions.

Within one ledger transaction, first acceptance SHALL add the verified gross value to funded money, `floor(value * 50 / 10000)` to funding fees, and the remainder to available credit using checked integer arithmetic. It SHALL retain the original platform payee and durably track any nonzero fee obligation. A transaction already accepted for that account SHALL NOT be credited or charged a second fee; one payment SHALL NOT fund another account. Existing spent money, reservations, commercial terms and credential authority SHALL remain unchanged. Success SHALL return the existing account's resulting view, not a new credential or an opening result.

Verification refusal SHALL NOT grant credit. It MAY persist the bounded original funding-attempt evidence required for recovery; this is distinct from a schema/ownership refusal and is not spendable funding. An unavailable RPC or missing receipt alone SHALL NOT reverse already accepted credit as if it proved a reorg. Proven orphan restoration and other chain-reconciliation transitions remain governed by their separate recovery requirements, not an unconditional top-up replay shortcut.

#### Scenario: Free or frozen account cannot be funded by top-up

- **GIVEN** the route owner is authenticated and the body is valid
- **WHEN** the target is free or frozen
- **THEN** the provider refuses before payment verification and leaves balances, fees and credentials unchanged

#### Scenario: Top-up is not the opening commitment

- **WHEN** a signed payment targets another account, uses opening calldata, names a different payer/payee/network or supplies zero value
- **THEN** it cannot become credit for the route account
- **AND** successful authentication does not override the payment mismatch

#### Scenario: Safe credit and same-payment retry preserve accounting

- **GIVEN** an active paid account and a matching 1,000-wei top-up independently verified at the healthy safe frontier
- **WHEN** it is first accepted
- **THEN** gross funding increases by 1,000 wei, funding fees by 5 wei and available credit by 995 wei atomically
- **AND** a freshly authorized retry of the same accepted transaction rechecks evidence without duplicating those changes, altering held/spent money or issuing a credential

#### Scenario: Pending or unavailable settlement is not accepted funding

- **WHEN** execution is not stably canonical at `safe` or required chain evidence is unavailable
- **THEN** the request remains uncredited and its recoverable original may remain recorded as a pending/unavailable attempt
- **AND** the client must not infer either safe acceptance or a funding reversal merely from the response or its loss

### Requirement: Top-up recovery preserves originals without new admission

Top-up settlement of an existing immutable agreement SHALL remain independent of fresh quote retention, quote-work/issuance allowances, free-opening or paid-intent capacity, current offer publication, engine/model readiness and new-funding readiness. Those exclusions SHALL NOT waive account-owner authority, active-paid eligibility, chain evidence or ledger integrity. A retry SHALL obtain a fresh owner proof for the same intended request; settlement SHALL NOT require another opening-intent acknowledgement or renegotiate model, price, fees or limits.

The native consumer SHALL persist a distinct top-up intent with positive canonical wei, prepare/journal one exact signed original under that operation, and preserve its sender/nonce ownership through uncertainty and restart. A different requested amount SHALL NOT silently replace a pending top-up. If saved raw bytes exist, missing or different original journal evidence SHALL fail closed before preparing or submitting a replacement. Explicit recovery SHALL reuse that operation and its exact original, retaining pending local evidence until successful acceptance is confirmed and persisted. A lost response SHALL NOT trigger another payment, automatic fee replacement, credential rotation or paid inference replay.

#### Scenario: Recovery survives admission pressure and unavailable serving

- **GIVEN** an existing active paid agreement and original top-up, with exhausted quote resources or opening capacity and serving/new-funding readiness closed
- **WHEN** its owner retries settlement with valid safe payment evidence
- **THEN** the existing agreement can be credited or recognized without fresh admission, re-quoting or credential replacement

#### Scenario: Lost credit response retains the pending original

- **WHEN** the provider commits credit but its response is lost
- **THEN** the native consumer retains its pending original and existing exported credential until acceptance is confirmed
- **AND** reopened recovery with fresh owner authority resubmits the same evidence without another credit, fee or replacement payment

#### Scenario: Inconsistent journal cannot authorize a different top-up

- **WHEN** saved pending raw bytes have no matching original journal entry, or the caller requests a different amount while that operation remains pending
- **THEN** native recovery refuses replacement while retaining the original agreement, pending evidence and credential authority

