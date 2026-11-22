# inference-payment-envelope-bounds Specification

## Purpose
Bound the shared local decoder for signed payment evidence: lowercase `0x` hex of at most 16,384 decoded bytes, exactly one canonical RLP list for the configured-chain EIP-155 legacy, EIP-2930 or EIP-1559 profile, at most four levels of nesting and 1,024 immediate items, exact-width integers and byte strings, and typed access lists with aggregate occurrence budgets and full occurrence-based structural gas. These are necessary limits, not acceptance: a candidate within them must still pass signature, network, payment-binding and operation-specific rules. Rejecting a candidate never disturbs the valid original or its accepted evidence.
## Requirements
### Requirement: Local payment evidence has bounded canonical representation

The shared payment envelope decoder SHALL accept only the configured-chain EIP-155 legacy, EIP-2930 and EIP-1559 profiles with valid canonical signatures and field structure. Raw input SHALL have the lowercase `0x` prefix, a nonempty even number of hexadecimal digits and at most 16,384 decoded bytes. The decoded-byte budget SHALL include any typed-envelope prefix. Hexadecimal letter case SHALL NOT change the decoded transaction hash; this does not authorize rewriting a retained original string or changing exact-replay rules.

Before allocating the decoded byte buffer, the decoder SHALL reject a hexadecimal payload exceeding twice the decoded-byte budget. After removing the optional supported typed prefix, it SHALL consume exactly one canonical RLP list: nine fields for legacy, eleven for type 1 and twelve for type 2. It SHALL reject nonminimal encodings, truncated or trailing bytes, inconsistent list lengths and declared lengths that overflow arithmetic. Each RLP list SHALL contain at most 1,024 immediate items. Nesting SHALL be at most four edges below the transaction-list root, with the root at depth zero. Bytes inside an opaque RLP byte string SHALL NOT be recursively treated as RLP children.

These are inclusive necessary limits, not sufficient acceptance conditions. A transaction at or below a limit SHALL still satisfy all other shape, signature, network, payment-binding and operation-specific rules. A configured chain accepting a transaction does not by itself make it part of this application profile.

#### Scenario: Oversized or ambiguously framed evidence

- **WHEN** the raw hexadecimal payload exceeds 32,768 digits, a declared RLP length overflows, or the encoded transaction has trailing or truncated bytes
- **THEN** local validation returns an error rather than allocating an unbounded decoded buffer, accepting a prefix or repairing the encoding
- **AND** another structural limit is not relaxed merely to construct an otherwise-valid raw-ceiling fixture

#### Scenario: Nested or excessively broad RLP

- **WHEN** an RLP item lies at depth five or a list contains 1,025 immediate items
- **THEN** the decoder rejects the evidence without treating it as an alternate supported envelope
- **AND** depth four and 1,024 items do not override the stricter envelope and access-list shape requirements

### Requirement: Payment numbers and byte strings preserve their exact widths

Transaction chain identifiers, nonces and gas limits SHALL decode as unsigned values no greater than `u64::MAX`; legacy `v` SHALL match the selected chain's EIP-155 value and recovery parity rather than an unprotected signature. Transaction value, `gasPrice`, `maxFeePerGas` and `maxPriorityFeePerGas`, where present, SHALL be no greater than `u128::MAX`. Integer zero SHALL use empty RLP bytes; nonzero integers SHALL have no leading zero bytes. Oversized or nonminimal values SHALL be rejected, never clipped, wrapped, saturated or reduced to their low bits.

The recipient SHALL be exactly 20 bytes, not contract creation. Calldata SHALL contain at most 4,096 bytes. Signature scalars SHALL use at most 32 canonical integer bytes and satisfy the existing signature recovery and low-s requirements. Type-2 priority price SHALL not exceed its maximum price. Opaque recipient, calldata, access-list address and storage-key bytes SHALL retain their contents, including leading zero bytes; numeric minimality SHALL NOT be applied to those opaque byte strings.

Successful local decoding SHALL NOT waive an operation's exact payer, provider, value or commitment binding. In particular, canonical zero price and a decodable maximum nonce do not override fresh opening admission's nonzero-price and nonexhausted-current-nonce requirements.

#### Scenario: Wide values cannot masquerade as admissible low bits

- **GIVEN** a correctly signed candidate has an over-width nonce, gas limit, value or price whose low bits would otherwise match an admissible payment
- **WHEN** it is locally validated
- **THEN** validation rejects it instead of truncating it into a different payable interpretation

#### Scenario: Calldata boundary is separate from agreement binding

- **WHEN** calldata has 4,097 bytes
- **THEN** the local profile rejects it even if the signature and other fields are valid
- **AND** 4,096 bytes can satisfy this size check but cannot replace the exact opening or other operation-specific commitment

### Requirement: Typed access lists have aggregate occurrence budgets

A type-1 or type-2 access list SHALL be an RLP list with at most 64 address entries. Each entry SHALL be exactly a two-item list consisting of a 20-byte address and a list of 32-byte storage keys. There SHALL be at most 256 storage-key occurrences across the entire access list, not 256 per entry. Empty lists and entries with no storage keys SHALL be allowed when all other requirements hold.

Address and storage-key occurrences SHALL be counted as encoded, including duplicates. The decoder SHALL NOT deduplicate before enforcing budgets or computing structural gas. Repeated addresses or keys SHALL NOT be rejected solely for being repeated when the encoded list otherwise satisfies the profile. The application SHALL NOT remove excess entries or keys and reinterpret the resulting payload as the signed original.

#### Scenario: Simultaneous inclusive access-list limits

- **GIVEN** an otherwise-valid typed transaction has 64 address entries and 256 total storage-key occurrences
- **WHEN** the access list is validated
- **THEN** those counts satisfy the resource profile
- **AND** independently sufficient structural gas, exact payment binding and fresh admission or settlement remain required

#### Scenario: Aggregate and duplicate budgets cannot be bypassed

- **WHEN** there are 65 address entries or 257 total keys, including keys spread across individually smaller entries or repeated byte-for-byte
- **THEN** the transaction is rejected without deduplicating, truncating or ignoring the excess

#### Scenario: Opaque address and key widths

- **WHEN** an entry is not a two-item list, its address is not 20 bytes, its keys are not a list or a key is not 32 bytes
- **THEN** the transaction is rejected as malformed
- **AND** correctly sized opaque addresses and keys containing leading zero bytes are preserved rather than normalized as integers

### Requirement: Structural gas accounting does not substitute for admission or settlement

The local structural gas floor SHALL be computed without wrapping as `21,000 + 4 * zero_calldata_bytes + 16 * nonzero_calldata_bytes + 2,400 * address_occurrences + 1,900 * storage_key_occurrences`. Legacy transactions SHALL have zero access-list occurrences. A signed gas limit below that floor SHALL be rejected; equality satisfies only this structural check. Every encoded duplicate occurrence SHALL contribute its cost.

This floor SHALL NOT be presented as an execution estimate, a promise that a node will accept the transaction, a balance reservation or evidence of payment. Fresh opening spendability SHALL continue to use the entire signed gas limit times its maximum price plus value, and paid credit SHALL continue to require independent canonical successful execution and the healthy safe frontier.

#### Scenario: Duplicate access costs cannot disappear

- **GIVEN** a typed transaction repeats access-list addresses or storage keys within the occurrence limits
- **WHEN** its signed gas covers a deduplicated floor but not the floor of the actual encoded occurrences
- **THEN** local validation rejects it
- **AND** a gas limit equal to the full occurrence-based floor is not rejected solely for insufficient structural gas

### Requirement: Resource rejection preserves financial evidence and authority boundaries

A local resource-validation failure SHALL return an error rather than rewrite, truncate or replace the signed original. It SHALL NOT authorize submission, create a funding acknowledgement, issue paid credit or erase an existing original, acknowledgement or sender/nonce commitment. The existing owner-authorization and exact historical-replay contracts SHALL continue to apply independently; this resource profile does not authorize purging accepted evidence or reapplying fresh spendability to an exact cached acknowledgement.

For a new owner-authorized opening candidate without an existing acknowledgement, resource rejection SHALL leave the issued quote usable by an otherwise-admissible original. This is a controlled validation property, not permission for automatic replacement or paid retry by the native client.

#### Scenario: Rejected candidate does not poison the valid original

- **GIVEN** a quote and a valid signed original have been prepared in an isolated acceptance fixture
- **WHEN** an adversarial over-budget or malformed candidate is refused through fresh funding-intent admission
- **THEN** no acknowledgement, submission or credit is produced for that candidate
- **AND** the unchanged valid original can still be admitted if all current gates hold

