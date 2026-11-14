## Purpose

Define the exact network, money and signed-byte identities shared by inference offers, agreements, authentication and payment commitments, so independent consumers and providers cannot reinterpret the same terms differently.

## ADDED Requirements

### Requirement: Complete and explicit network identity

The system SHALL identify a network by the complete tuple `{chain_id, genesis_hash, deployment_id}` and SHALL require equality of all three fields when accepting a network-bound protocol object. Network-scoped persistence and discovery SHALL distinguish different tuples, not only different chain IDs. The network deployment identifier SHALL remain distinct from a provider's deployment identifier.

The current application configuration SHALL accept only Conflux eSpace testnet chain ID `71` and genesis `0x3fb9d350f95424c3c27fcf248d9a6f633a83c2a75c11ef887ea41540c85e7804`, with an explicitly selected deployment identifier of 1–128 ASCII letters, digits, hyphens or underscores. It SHALL NOT infer identity from an endpoint, enable mainnet/private-chain funding through a development flag, supply an implicit RPC endpoint or fall back to a legacy chain.

#### Scenario: Same chain with another deployment

- **WHEN** a received object's chain and genesis match the selected network but its network deployment identifier differs
- **THEN** the object is rejected as belonging to another network and cannot reuse the selected network's state

#### Scenario: Chain or genesis mismatch

- **WHEN** either the chain ID or genesis differs while the other identity fields remain unchanged
- **THEN** network-bound acceptance rejects the object

#### Scenario: Invalid application network selection

- **WHEN** a configuration selects another chain, another genesis, an uppercase spelling of the pinned genesis, or an empty, oversized or nonconforming deployment identifier
- **THEN** configuration validation fails without choosing replacement values

### Requirement: Strict primitive schemas and canonical wallet identities

A signed envelope SHALL contain exactly `payload` and `signature`. A network identity SHALL contain exactly `chain_id`, `genesis_hash` and `deployment_id`; its chain ID SHALL be an unsigned 64-bit integer JSON number, and its other fields SHALL be strings. A price schedule SHALL contain exactly the string fields `input_wei_per_1k` and `output_wei_per_1k`. Receivers SHALL reject unknown fields in these schemas rather than discarding them before signature verification. Each payload SHALL additionally obey its own typed schema; this foundation does not define every payload's fields.

Wallet identities in accepted signed protocol objects SHALL be lowercase `0x` followed by exactly 40 lowercase hexadecimal digits. Signature verification SHALL compare the recovered canonical wallet with the required canonical signer without case-folding a noncanonical signed identity into acceptance.

#### Scenario: Unknown field in a signed envelope or primitive

- **WHEN** an input adds an unrecognized field to an envelope, its network identity or its price schedule
- **THEN** typed decoding rejects the input rather than verifying a projection with that field removed

#### Scenario: Wrong primitive JSON type

- **WHEN** a network chain ID is represented as a string or floating-point number, or a price is represented as a JSON number
- **THEN** typed decoding rejects the input

#### Scenario: Noncanonical signer identity

- **WHEN** an otherwise valid signature is checked against a wallet identity containing uppercase hexadecimal letters, a wrong length or a missing `0x` prefix
- **THEN** the signed protocol object is not accepted under that noncanonical identity

### Requirement: Exact monetary representation and CFX conversion

Monetary wire values SHALL be unsigned canonical decimal wei strings in the inclusive range `0` through `340282366920938463463374607431768211455` (`2^128 - 1`). The only representation of zero SHALL be `"0"`. Empty strings, leading zeroes on multi-digit values, signs, whitespace, fractions, exponent notation, non-ASCII digits and out-of-range values SHALL be rejected. Money SHALL NOT be represented or calculated through floating point; monetary operations SHALL detect overflow rather than wrap or truncate.

One CFX SHALL equal exactly `1000000000000000000` wei. Native human-facing CFX decimal conversion SHALL preserve every wei, accept no more than 18 fractional digits, reject missing whole-number digits or a trailing decimal point, and reject values outside the wei range. This human-input syntax is distinct from canonical wire wei: whole-part leading zeroes do not change the numeric amount. CFX formatting SHALL omit unnecessary fractional trailing zeroes and omit the decimal point for whole CFX amounts.

#### Scenario: Monetary boundary values

- **WHEN** a canonical wei value is `"0"` or `"340282366920938463463374607431768211455"`
- **THEN** it is represented exactly without precision loss
- **AND** `"340282366920938463463374607431768211456"` is rejected as overflow

#### Scenario: Noncanonical monetary syntax

- **WHEN** a wire value is `""`, `"00"`, `"01"`, `"+1"`, `"-1"`, `" 1"`, `"1.0"`, `"1e3"` or contains non-ASCII digits
- **THEN** it is rejected without normalization into a chargeable amount

#### Scenario: Exact human CFX conversion

- **WHEN** the native conversion receives `"0.000000000000000001"`, `"01.000000000000000001"` or the formatted representation of the maximum wei value
- **THEN** it returns respectively one wei, `1000000000000000001` wei or the exact maximum wei value
- **AND** formatting and parsing a valid wei value preserves that value

#### Scenario: Invalid human precision or overflow

- **WHEN** human CFX input has more than 18 fractional digits, omits the whole part, ends with a decimal point, uses exponent notation, or overflows after scaling by `10^18`
- **THEN** conversion rejects it instead of rounding, estimating or truncating it

### Requirement: Deterministic integer-only canonical JSON

Signed payload canonicalization SHALL serialize the typed value into compact UTF-8 JSON, recursively sorting object keys lexicographically by their UTF-8 bytes and preserving array order. Optional fields serialized as `null` by the typed schema SHALL remain present. JSON numbers SHALL be representable as signed or unsigned 64-bit integers; floating-point numbers, including integral-looking floats such as `1.0`, SHALL be rejected recursively.

The byte representation SHALL use ordinary decimal integer text without exponent notation or insignificant whitespace. Strings SHALL escape quotation marks and backslashes, use `\b`, `\t`, `\n`, `\f` and `\r` for those control characters and lowercase `\u00xx` escapes for other U+0000–U+001F characters. Other Unicode characters SHALL remain UTF-8 without Unicode normalization; `/` SHALL NOT be escaped. There SHALL be no byte-order mark or trailing newline. These rules define the typed protocol encoder, not arbitrary floating-point RFC 8785 canonicalization.

#### Scenario: Object order is irrelevant but array order is significant

- **WHEN** two typed payload values differ only in object insertion order, including nested objects
- **THEN** their canonical bytes are equal
- **AND** reversing an array of distinct elements changes the canonical bytes

#### Scenario: Null and Unicode are preserved

- **WHEN** a typed payload includes a null optional field, non-ASCII text, quotation marks, backslashes and control characters
- **THEN** its canonical bytes retain the null field and use the specified UTF-8 and escaping rules without normalizing the text

#### Scenario: Nested floating-point value

- **WHEN** any payload value contains a floating-point number, including inside a nested object or array
- **THEN** canonicalization fails before producing signing bytes

### Requirement: Length-framed operation domains

For domain string `D` and canonical JSON bytes `J`, the signed payload SHALL be exactly `u32_be(byte_length(D)) || UTF8(D) || u32_be(byte_length(J)) || J`. Lengths SHALL count bytes, not characters. Lengths outside the unsigned 32-bit range SHALL fail rather than truncate. The frame SHALL contain no additional terminator, newline or signed-envelope wrapper.

Each operation SHALL use its exact domain and required signer:

| Payload | Domain | Signer |
|---|---|---|
| Offer | `chiral-inference-offer-v1` | Provider |
| Quote | `chiral-inference-quote-v1` | Provider |
| Funding-intent acknowledgement | `chiral-inference-funding-intent-v1` | Provider |
| Usage receipt | `chiral-inference-receipt-v1` | Provider |
| Control challenge | `chiral-inference-auth-v1` | Payer |
| Subjective feedback | `chiral-inference-feedback-v1` | Payer |

#### Scenario: UTF-8 byte lengths

- **WHEN** a framed payload contains multibyte Unicode text
- **THEN** its length prefix equals its encoded byte count and the complete frame has exactly the two length prefixes plus the domain and JSON bytes

#### Scenario: Domain substitution

- **WHEN** a valid signed payload is verified under a different operation domain
- **THEN** verification does not authenticate it as the original required signer under that domain

### Requirement: Canonical recoverable personal signatures

Signing SHALL hash `0x19 || "Ethereum Signed Message:\n" || ASCII(decimal byte_length(P)) || P` with Keccak-256, where `P` is the domain-framed payload, and SHALL sign that digest with recoverable secp256k1 ECDSA. Emitted signatures SHALL encode the 32-byte `r`, 32-byte `s` and one-byte recovery ID as 130 lowercase hexadecimal characters without a `0x` prefix; the recovery ID SHALL be `27` or `28`.

Verification SHALL accept an optional lowercase `0x` prefix and case-insensitive hexadecimal digits, require exactly 65 decoded bytes, subtract 27 from the recovery ID and accept only resulting parity zero or one. It SHALL reject malformed scalars, high-`s` signatures, failed recovery and any recovered wallet unequal to the required canonical signer. It SHALL reconstruct the exact framed bytes from the typed payload, not verify a client-supplied alternate byte representation. Attribution SHALL NOT be described as proof of correct model execution, metering, chain acceptance or confidentiality.

#### Scenario: Supported signature encodings

- **WHEN** the same valid signature is supplied with or without a lowercase `0x` prefix, or with uppercase hexadecimal digits
- **THEN** verification recovers the same canonical wallet from the same framed payload

#### Scenario: Malleable or malformed signature

- **WHEN** a signature has high `s`, a raw recovery ID other than 27 or 28, a wrong byte length or invalid hexadecimal digits
- **THEN** verification rejects it

#### Scenario: Wrong signer or altered signed terms

- **WHEN** the required wallet differs from the recovered signer, or any signed field changes while the original signature is retained
- **THEN** verification fails to authenticate the object for that signer

### Requirement: Distinct offer identities and payment commitments

A protocol hash identifier SHALL be `0x` followed by the lowercase hexadecimal Keccak-256 digest of its specified bytes, not SHA-256. An offer ID SHALL hash the domain-framed canonical offer payload directly, excluding the signature and the Ethereum personal-sign prefix. Fresh quote/account identifiers SHALL be cryptographically random 32-byte values emitted as `0x` plus 64 lowercase hexadecimal digits; the quote identifier SHALL also identify its agreement/account.

Opening calldata SHALL be exactly the six bytes `CHRL1\0` followed by Keccak-256 of the domain-framed complete quote payload. Top-up calldata SHALL be exactly the six bytes `CHRL2\0` followed by the 32 decoded bytes of the existing account identifier. Both SHALL be 38 bytes long. Account-identifier decoding for top-up construction SHALL require the lowercase `0x` prefix and exactly 32 decoded bytes, while accepting either case of hexadecimal digits. Named artifact/evidence SHA-256 fields SHALL retain their separate SHA-256 meaning.

#### Scenario: Offer identity commits to signed terms

- **WHEN** a signed offer field changes, including its network, terms, revision or validity times
- **THEN** its content identifier is recomputed from the changed canonical payload
- **AND** changing only the signature representation does not change that offer identifier

#### Scenario: Opening commitment covers the entire quote

- **WHEN** any quote term changes, including network, parties, model/profile, price, funding, fee, concurrency, nonce or validity
- **THEN** the opening commitment is computed from that changed complete quote and cannot be reused as the unchanged quote's commitment

#### Scenario: Top-up commitment targets an existing account

- **WHEN** a valid 32-byte account identifier is supplied, with either uppercase or lowercase hexadecimal digits after `0x`
- **THEN** top-up construction produces `CHRL2\0` plus those exact decoded bytes
- **AND** a missing prefix, invalid hex or any other decoded length is rejected

#### Scenario: Opening and top-up operations remain distinct

- **WHEN** opening and top-up calldata are constructed
- **THEN** their distinct six-byte prefixes identify different commitment schemes and neither is treated as the other's payment evidence
