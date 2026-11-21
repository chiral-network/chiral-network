## ADDED Requirements

### Requirement: Public challenge requests bind an intended control operation without granting authority

`POST /v1/auth/challenge` SHALL be public and SHALL accept a JSON object containing exactly `wallet`, `method`, `path`, and `body_hash`, all strings. Missing, mistyped, or unknown members SHALL be rejected. The named wallet SHALL be a 20-byte hexadecimal address with a lowercase `0x` prefix; hexadecimal letter case SHALL be accepted and normalized to lowercase. `body_hash` SHALL be a 32-byte hexadecimal digest with a lowercase `0x` prefix, likewise normalized to lowercase. `method` SHALL be exactly `GET`, `POST`, or `DELETE`. `path` SHALL start with `/v1/contracts/`, contain at most 512 UTF-8 bytes, and contain neither ASCII control characters nor `#`.

The path SHALL represent the intended HTTP request path including its query string, if any. Its spelling SHALL be preserved rather than decoded, reordered, or normalized. The body digest SHALL be the Keccak-256 hash of the exact intended outgoing body bytes, not a parsed or reserialized equivalent. Issuing a challenge for a syntactically accepted path SHALL NOT establish that a route exists or that the caller owns the named wallet, quote, account, or balance. Public quote proposal SHALL remain unauthenticated and SHALL NOT acquire owner authority from an accompanying challenge.

#### Scenario: Public issuance does not authorize the named payer

- **WHEN** an unauthenticated caller requests a challenge naming another wallet with a valid method, path, and body digest
- **THEN** the provider MAY issue a bounded challenge but SHALL NOT thereby open an account, credit a payment, issue a credential, or authorize the caller as that wallet

#### Scenario: Canonical wallet and digest aliases

- **WHEN** two valid requests differ only in the case of hexadecimal wallet or body-digest digits
- **THEN** their payloads SHALL use the same lowercase wallet and digest, without creating separate named-wallet pending capacity

#### Scenario: Unsupported challenge request

- **WHEN** a request has an extra member, a non-string field, an unsupported method, an invalid wallet/digest, a path outside the control prefix, a fragment or ASCII control character, or a path exceeding 512 bytes
- **THEN** the provider SHALL reject challenge issuance

#### Scenario: Body or query spelling matters

- **WHEN** the intended operation has an empty body and a path ending in `/requests?before=200`
- **THEN** the challenge SHALL bind the digest of zero bytes and that exact query, not `{}`, another cursor, or a query-free path

### Requirement: Challenges carry provider-scoped signing bytes and a bounded issuer lifetime

The provider SHALL construct an authorization payload containing exactly `network`, `provider_wallet`, `wallet`, `method`, `path`, `body_hash`, `nonce`, and `expires_at`. `network` SHALL be the provider's configured complete chain-ID, genesis-hash, and deployment-ID tuple; `provider_wallet` SHALL be its signing wallet. The remaining request fields SHALL be the validated values from the challenge request. `nonce` SHALL be a cryptographically random 32-byte identifier encoded as lowercase `0x`-prefixed hexadecimal. `expires_at` SHALL be an unsigned Unix-second timestamp issued 120 seconds after the provider's current time, using overflow-safe arithmetic.

The provider SHALL persist the exact payload in its network/provider-scoped ledger before returning `{nonce, payload_hex, expires_at}`. `payload_hex` SHALL encode the `inference-wire-foundation` signing payload for domain `chiral-inference-auth-v1`, including its canonical serialization and length framing; the subsequent personal signature SHALL be the payer's, not the provider's. A duplicate nonce SHALL NOT overwrite an existing challenge. Ledger admission SHALL reject a payload for another network, an already-expired payload, or one more than 120 seconds ahead of the admission time.

Challenge validity SHALL end at `expires_at`, not one second later. A still-valid, unconsumed challenge SHALL survive a ledger restart; expiry or consumption SHALL never create account or payment authority.

#### Scenario: Trusted provider identity supplies the payload

- **WHEN** a public caller obtains a challenge
- **THEN** the signing bytes SHALL contain the serving provider's complete configured network identity and signing wallet, rather than caller-supplied replacements for either

#### Scenario: Issuance is durable before disclosure

- **WHEN** a challenge response is successfully returned and the provider restarts before it is used
- **THEN** the same unexpired stored payload SHALL remain eligible for one valid owner proof

#### Scenario: Expiry boundary

- **WHEN** a proof is presented at exactly its stored `expires_at`
- **THEN** the provider SHALL reject it without treating its signature as current authority

#### Scenario: Nonce collision cannot replace a challenge

- **WHEN** another challenge attempts to use an already-stored nonce
- **THEN** admission SHALL fail without changing the original stored payload

### Requirement: Pending challenge capacity is separate and transactional

Each provider ledger SHALL retain at most eight unexpired pending challenges per canonical named wallet and at most 10,000 unexpired pending challenges in total, independently of pending quote/admission capacity. The issuance transaction SHALL remove expired challenges, check both limits, and persist the new challenge atomically. Capacity exhaustion SHALL reject issuance with HTTP 429 rather than evict another unexpired challenge or queue unbounded work. Rejected issuance SHALL leave no partial challenge insertion or partial issuance-budget debit. Successful consumption SHALL release that challenge's pending slot.

A named-wallet challenge limit SHALL NOT be described as proof that the requester controls that wallet or as Sybil resistance. Challenge rate-window policy is separate from these pending-state limits.

#### Scenario: Wallet pending limit does not remove existing proofs

- **WHEN** eight unexpired challenges exist for one canonical wallet and a ninth is requested
- **THEN** issuance SHALL fail without replacing those challenges or consuming another pending slot

#### Scenario: Global capacity applies across wallets

- **WHEN** the ledger already has 10,000 unexpired pending challenges
- **THEN** issuance for another wallet SHALL fail even if that wallet has no pending challenges

#### Scenario: Expiry and consumption release capacity

- **WHEN** an expired challenge is cleaned up during successful issuance or a valid challenge is consumed
- **THEN** its pending slot SHALL be available without erasing unrelated valid challenges, acknowledged funding agreements, or account state

### Requirement: Native consumers reconstruct intended authorization before signing

Before using the wallet key, a native consumer SHALL reconstruct the authorization payload from its selected network, selected agreement's provider wallet, active wallet, exact intended method and request target, and hash of the exact bytes it will transmit, together with the response's nonce and expiry. It SHALL derive the expected domain-framed signing bytes locally and SHALL require the `0x`-prefixed hexadecimal `payload_hex` to decode to those exact bytes. It SHALL NOT sign opaque provider-supplied bytes or trust provider-supplied replacements for the intended operation or identities.

The native receiver SHALL reject malformed hexadecimal encoding, an empty nonce, a nonce longer than 256 UTF-8 bytes, an expiry at or before its current time, or an expiry more than 600 seconds ahead of its current time. This receiver-side plausibility bound SHALL NOT extend the provider's 120-second issuance or ledger-admission lifetime. Unrecognized unsigned response members SHALL NOT change the reconstructed signing payload or confer authority.

After validation, the native consumer SHALL sign the reconstructed bytes and send `X-Chiral-Owner`, `X-Chiral-Nonce`, and `X-Chiral-Signature` with the exact bound method, request target, and body. Wallet private keys SHALL remain behind the native boundary and SHALL NOT be sent to the provider or exposed to frontend state, command-line arguments, or logs.

#### Scenario: Provider substitutes the operation or counterparty

- **WHEN** returned signing bytes bind another body, method, path, query, wallet, provider, or any component of network identity
- **THEN** the native consumer SHALL reject the challenge before producing an owner signature

#### Scenario: Native freshness is distinct from issuer lifetime

- **WHEN** a response's expiry is at the consumer's current time or more than 600 seconds ahead, or its nonce is empty or exceeds 256 bytes
- **THEN** the native consumer SHALL reject it even if its returned bytes otherwise match the reconstruction

#### Scenario: Serialization does not change after authorization

- **WHEN** the native consumer signs a body digest for a control operation
- **THEN** it SHALL transmit those same body bytes, including the empty body for a read, rather than reserializing a semantically equivalent JSON value

### Requirement: Provider owner authentication consumes an exact stored challenge once

The provider SHALL require `X-Chiral-Owner`, `X-Chiral-Nonce`, and `X-Chiral-Signature` for owner authentication, with respective maximum header-value lengths of 42, 66, and 132 bytes. It SHALL canonicalize the owner wallet and load the challenge by nonce from the ledger; a signature without a matching stored challenge SHALL NOT suffice.

Before consumption, the provider SHALL require exact agreement between the stored payload and its complete network identity, signing wallet, canonical claimed owner, actual method, actual path including query, and Keccak-256 digest of the raw body. It SHALL also require `expires_at` strictly greater than its current time and recovery of the claimed owner's wallet from the domain-framed payload's personal signature.

Only after those checks SHALL it atomically consume the still-unexpired record matching both nonce and the complete expected payload. At most one concurrent authenticator SHALL succeed, including across independent connections/processes sharing the ledger. Failed binding or signature validation SHALL NOT consume a valid challenge; successful consumption SHALL remain effective after restart. Authenticating the wallet SHALL precede, not replace, quote/account ownership and operation-specific business checks.

#### Scenario: Binding failure does not burn a valid proof

- **WHEN** the nonce is presented with an altered body, method, path/query, owner, provider, network component, or invalid signature
- **THEN** authentication SHALL fail without consuming the stored challenge, and a later correctly bound valid proof MAY still succeed before expiry

#### Scenario: Concurrent reuse has one winner

- **WHEN** two valid authentications attempt to consume the same stored challenge concurrently through separate ledger connections
- **THEN** exactly one SHALL acquire owner authority and the other SHALL fail

#### Scenario: Restart does not restore a consumed nonce

- **WHEN** a successfully consumed proof is replayed after the provider restarts
- **THEN** authentication SHALL fail even if its signed expiry is still in the future

#### Scenario: Stored payload changes before consumption

- **WHEN** the record no longer matches the complete payload that was validated
- **THEN** atomic consumption SHALL fail rather than authenticate using only a matching nonce

### Requirement: Account control authorization follows ownership and read scope

Opening a free or paid account and registering an opening funding intent SHALL require the quoted consumer's one-use owner proof. Top-ups and credential issuance, revocation, and rotation SHALL require the existing account consumer's one-use owner proof. A public proposal, a signed payment transaction, a provider acknowledgement, or an account bearer SHALL NOT substitute for those owner proofs. Payment verification and other business conditions SHALL remain additional requirements, not consequences of authentication.

The following GET routes SHALL accept either that account owner's proof or a valid bearer credential for that same account with `read` scope: `/v1/contracts/:id`, `/receipt`, `/funding`, `/requests`, `/requests/:request_id`, and `/credentials`, where the suffixes are relative to `/v1/contracts/:id`. These requests SHALL have an empty body. A bearer for another account or without `read` scope SHALL NOT authorize these reads. This allowance SHALL NOT authorize bearer-based credential management or payment submission.

If `X-Chiral-Owner` is present on a read, the provider SHALL select owner authentication and SHALL NOT fall back to bearer authentication if the owner proof is malformed, expired, consumed, or belongs to a different account owner. The request-list query, when present, SHALL be exactly one `before=<unix_seconds>` cursor accepted as an unsigned integer no larger than `i64::MAX`, and SHALL remain part of the owner-signed request target. Inference and model-list bearer authorization remain separate from this control-read allowance.

#### Scenario: A public free quote is not account authority

- **WHEN** a caller possesses a valid zero-funding quote but lacks the quoted consumer's owner proof
- **THEN** account opening and credential issuance SHALL be refused despite the absence of a required payment

#### Scenario: Read bearer cannot manage credentials or submit payments

- **WHEN** a valid account bearer with `read` scope is supplied without an owner proof
- **THEN** the provider SHALL allow eligible reads of that account but SHALL refuse top-up submission and credential issuance, revocation, or rotation

#### Scenario: Another owner cannot use a genuine proof to control the account

- **WHEN** authentication succeeds for a wallet other than the quote/account consumer
- **THEN** the operation SHALL be refused without granting account access or changing its financial or credential state

#### Scenario: Invalid owner proof does not downgrade to bearer

- **WHEN** a read contains both a valid read bearer and an invalid or wrong-owner `X-Chiral-Owner` proof
- **THEN** the provider SHALL refuse the read rather than retry authorization using the bearer

#### Scenario: Read body and cursor are constrained

- **WHEN** an account read has a nonempty body or a request-list read has an unsupported or out-of-range cursor query
- **THEN** the provider SHALL reject the request without returning account data or executing an account mutation

### Requirement: Consuming owner authority does not imply business success

A successfully authenticated proof SHALL remain consumed if later ownership, payment, admission, credential, or other operation-specific checks refuse the request. Clients SHALL NOT interpret challenge consumption, a transport error, or a missing response as evidence of account creation, funding credit, or operation failure. Retrying a control operation SHALL require a fresh challenge and signature for the exact intended retry, while preserving the original agreement and signed payment identity whenever recovering an existing operation. Authentication retry SHALL NOT itself authorize a new payment, automatic paid inference retry, replacement transaction, or credential rotation.

#### Scenario: Business refusal requires fresh authority

- **WHEN** owner authentication succeeds but the operation is subsequently refused
- **THEN** reuse of that proof SHALL fail and a permitted retry SHALL obtain fresh owner authority without inferring that any payment or account mutation occurred

#### Scenario: Response loss does not justify another payment

- **WHEN** a consumer loses the response after sending an owner-authorized opening or funding-intent request
- **THEN** it SHALL use the operation's recovery/status path with fresh authorization and the original agreement/payment identity, rather than automatically funding again
