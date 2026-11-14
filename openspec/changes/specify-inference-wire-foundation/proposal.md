## Why

The whitepaper defines the network, monetary and cryptographic identity of every inference agreement, but OpenSpec has no requirements yet. Specify these shared primitives first so subsequent funding, discovery and accounting changes cannot silently invent incompatible bytes or identities.

## What Changes

- Add requirements for the complete network identity, the selected eSpace testnet, strict primitive/envelope schemas and canonical wallet addresses.
- Specify exact decimal wei, integer-only canonical JSON, length-framed domains, recoverable personal signatures, offer identifiers and opening/top-up commitments.
- Preserve the existing implementation's public behavior, including exact native CFX conversion and case-insensitive hex decoding where explicitly accepted. Do not change the whitepaper or claim a new protocol version.
- Schedule deterministic interoperability and rejection tests for primitive edges not directly covered by the inspected tests. This turn creates the specification artifacts, not those tests.

## Capabilities

### New Capabilities

- `inference-wire-foundation`: Network identity and exact monetary, serialization, signing and payment-commitment primitives shared by the inference protocol.

### Modified Capabilities

None; the baseline specification directory is empty.

## Impact

- Authoritative design: read-only `docs/chiral-book.md`, especially “Protocol identities, signed offers, and canonical bytes,” “Configuration and state,” and “Agreements and exact funding order.”
- Implementation/test correspondence: `src-tauri/src/protocol.rs`, `network.rs`, `wallet.rs`, `offer_registry.rs` and `auth.rs`.
- No dependency, endpoint, database, payment, deployment or legacy-data changes. Full offer/model validation, authentication lifecycle, transaction acceptance, accounting and discovery remain separate requirements to specify after this foundation.
- Existing Rust all-targets test compilation is blocked in the pre-existing dirty tree. This change must not repair or commit other people's source changes; record available checks separately from that failure.
