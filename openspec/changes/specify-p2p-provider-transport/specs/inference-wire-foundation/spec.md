## MODIFIED Requirements

### Requirement: Length-framed operation domains

For domain string `D` and canonical JSON bytes `J`, the signed payload SHALL be exactly `u32_be(byte_length(D)) || UTF8(D) || u32_be(byte_length(J)) || J`. Lengths SHALL count bytes, not characters. Lengths outside the unsigned 32-bit range SHALL fail rather than truncate. The frame SHALL contain no additional terminator, newline or signed-envelope wrapper.

Each operation SHALL use its exact domain and required signer:

| Payload | Domain | Signer |
|---|---|---|
| Offer, legacy schema v1 (no `transports` field; verification of retained offers only) | `chiral-inference-offer-v1` | Provider |
| Offer, schema v2 (with `transports` field) | `chiral-inference-offer-v2` | Provider |
| Quote, legacy schema v1 (no `free_allowance` field; verification of retained quotes only) | `chiral-inference-quote-v1` | Provider |
| Quote, schema v2 (with `free_allowance` field) | `chiral-inference-quote-v2` | Provider |
| Funding-intent acknowledgement | `chiral-inference-funding-intent-v1` | Provider |
| Usage receipt | `chiral-inference-receipt-v1` | Provider |
| Control challenge | `chiral-inference-auth-v1` | Payer |
| Subjective feedback | `chiral-inference-feedback-v1` | Payer |

An offer payload SHALL be framed only under the domain of its own schema version as defined by `inference-commercial-terms`; neither offer domain SHALL authenticate a payload of the other schema. Schema v1 is retired without an acceptance window: a native signer SHALL NOT sign a new offer under `chiral-inference-offer-v1`, and that domain SHALL remain in use only to verify retained v1 offers and recompute their offer IDs. A quote payload SHALL likewise be framed only under the quote domain of its own schema version; neither quote domain SHALL authenticate a payload of the other schema, and a native signer SHALL NOT sign a new quote under `chiral-inference-quote-v1`, which SHALL remain in use only to verify retained v1 quotes and recompute their opening commitments. Every verifier of a signed quote, including a relay verifying the quote in feedback funding evidence, SHALL select the quote domain from the quote payload's own schema.

#### Scenario: Retired offer domain is verification-only

- **GIVEN** a native provider implementing schema v2 that previously signed v1 offers
- **WHEN** it signs an offer
- **THEN** it frames the payload under `chiral-inference-offer-v2`
- **AND** `chiral-inference-offer-v1` is used only to verify a retained v1 offer or recompute its offer ID

#### Scenario: UTF-8 byte lengths

- **WHEN** a framed payload contains multibyte Unicode text
- **THEN** its length prefix equals its encoded byte count and the complete frame has exactly the two length prefixes plus the domain and JSON bytes

#### Scenario: Domain substitution

- **WHEN** a valid signed payload is verified under a different operation domain
- **THEN** verification does not authenticate it as the original required signer under that domain

#### Scenario: Offer schema domains are not interchangeable

- **WHEN** a v2 offer payload signature is verified under `chiral-inference-offer-v1`, or a v1 offer payload signature is verified under `chiral-inference-offer-v2`
- **THEN** verification does not authenticate the offer

#### Scenario: Quote schema domains are not interchangeable

- **GIVEN** a native provider implementing quote schema v2 that previously signed v1 quotes
- **WHEN** it signs a quote, or a v2 quote payload signature is verified under `chiral-inference-quote-v1`, or a v1 quote payload signature under `chiral-inference-quote-v2`
- **THEN** the new quote is framed under `chiral-inference-quote-v2`, and neither cross-domain verification authenticates the quote
- **AND** `chiral-inference-quote-v1` is used only to verify a retained v1 quote or recompute its opening commitment
- **AND** a relay verifying feedback funding evidence that carries a paid v2 quote verifies that quote under `chiral-inference-quote-v2`

### Requirement: Distinct offer identities and payment commitments

A protocol hash identifier SHALL be `0x` followed by the lowercase hexadecimal Keccak-256 digest of its specified bytes, not SHA-256. An offer ID SHALL hash the canonical offer payload framed under the offer domain of that payload's own schema version, directly, excluding the signature and the Ethereum personal-sign prefix. Fresh quote/account identifiers SHALL be cryptographically random 32-byte values emitted as `0x` plus 64 lowercase hexadecimal digits; the quote identifier SHALL also identify its agreement/account.

Opening calldata SHALL be exactly the six bytes `CHRL1\0` followed by Keccak-256 of the complete quote payload framed under the quote domain of that payload's own schema version, so that the commitment of a retained v1 quote is unchanged. Top-up calldata SHALL be exactly the six bytes `CHRL2\0` followed by the 32 decoded bytes of the existing account identifier. Both SHALL be 38 bytes long. Account-identifier decoding for top-up construction SHALL require the lowercase `0x` prefix and exactly 32 decoded bytes, while accepting either case of hexadecimal digits. Named artifact/evidence SHA-256 fields SHALL retain their separate SHA-256 meaning.

#### Scenario: Offer identity commits to signed terms

- **WHEN** a signed offer field changes, including its network, terms, transports, endpoint, revision or validity times
- **THEN** its content identifier is recomputed from the changed canonical payload
- **AND** changing only the signature representation does not change that offer identifier

#### Scenario: Offer identity is bound to its schema version

- **GIVEN** a retained quote whose `offer_id` was computed from a v1 offer
- **WHEN** that offer ID is recomputed after schema v2 is introduced
- **THEN** it is computed from the v1 payload framed under `chiral-inference-offer-v1` and equals the original identifier
- **AND** a v2 offer's identifier is computed only under `chiral-inference-offer-v2`

#### Scenario: Opening commitment covers the entire quote

- **WHEN** any quote term changes, including network, parties, model/profile, price, free allowance, funding, fee, concurrency, nonce or validity
- **THEN** the opening commitment is computed from that changed complete quote and cannot be reused as the unchanged quote's commitment
- **AND** a retained v1 quote's commitment is recomputed under `chiral-inference-quote-v1` and equals the commitment in its original opening calldata

#### Scenario: Top-up commitment targets an existing account

- **WHEN** a valid 32-byte account identifier is supplied, with either uppercase or lowercase hexadecimal digits after `0x`
- **THEN** top-up construction produces `CHRL2\0` plus those exact decoded bytes
- **AND** a missing prefix, invalid hex or any other decoded length is rejected

#### Scenario: Opening and top-up operations remain distinct

- **WHEN** opening and top-up calldata are constructed
- **THEN** their distinct six-byte prefixes identify different commitment schemes and neither is treated as the other's payment evidence
