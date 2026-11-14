## Context

`docs/chiral-book.md` is the human-owned whitepaper, unchanged from initialization (SHA-256 `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`). There are no main OpenSpec specs yet. This change records the first independently useful protocol slice; it does not certify alignment of the entire running product.

The checkout contains extensive pre-existing uncommitted inference implementation. Inspect it as evidence, but do not adopt it into this worker's commit. The inspected primitives match the specified algorithms; `src-tauri/tests/inference_wire_foundation_test.rs` supplies deterministic boundary tests and independent byte-level vectors without modifying those implementation files.

## Goals / Non-Goals

Goals:
- Establish unambiguous network and monetary identities before specifying payment and accounting state machines.
- Preserve exact signed bytes and distinguish signatures, content IDs and transaction commitments.
- Make acceptance and rejection edges testable without real credentials, chain access, model execution or deployment.

Non-goals:
- Change the whitepaper, protocol version, money behavior or supported network.
- Define every offer/model field, admission rule, chain acceptance condition, credential workflow or persistence layout in this first slice.
- Repair unrelated dirty sources, clear build caches or claim public-release qualification.

## Decisions

1. **One foundation capability.** Keep the closely coupled encoder/signature/commitment rules together in `inference-wire-foundation`. Later capabilities depend on it rather than duplicating serialization rules.
2. **Specify bytes rather than a generic canonicalization standard.** The whitepaper's typed `serde_json` encoder is the interoperability reference. Integer-only numbers, recursive key sorting, nulls, array order, string escaping and byte-counted framing are externally visible contracts. Switching libraries is allowed only with identical output; `preserve_order` must not silently change it.
3. **Separate wire wei from human CFX input.** `validate_money` enforces canonical unsigned decimal wei. `wallet::parse_units` accepts exact human decimal CFX, including whole-part leading zeroes. Preserve that distinction instead of applying the wire syntax to existing user input.
4. **Preserve explicit hex decoding behavior.** The signer emits lowercase unprefixed signatures; personal-sign verification accepts an optional lowercase `0x` and either hex case, but only recovery bytes 27/28 and low `s`. Top-up construction accepts either hex case after `0x`; canonical public identifier generation remains lowercase. None of this weakens canonical wallet identity checks.
5. **Require independent byte-level evidence before completion.** Existing behavior is not a substitute for expected bytes derived separately. The integration target covers missing consumer-visible boundaries while retaining existing useful tests, including high-s signature rejection and exact CFX conversion. It does not prescribe a deterministic signer nonce algorithm.

## Source and coverage correspondence

| Contract | Inspected source | Contract coverage |
|---|---|---|
| Explicit testnet and full identity | `network.rs::NetworkConfig::{validate,namespace}`, `protocol.rs::NetworkIdentity`, `offer_registry.rs::OfferRegistry::insert_at` | Existing offer/discovery tests reject mismatched networks. New integration tests distinguish every tuple field in namespaces and reopened ledgers, preserve provider binding after rejected opens, and exercise deployment/genesis configuration boundaries. |
| Envelope and primitive shapes | `protocol.rs::{Signed,NetworkIdentity,PriceSchedule}` | New tests reject unknown and missing fields in signed envelopes/nested primitives, plus wrong JSON types. Valid signatures do not excuse schema projection. |
| Money and CFX | `offer_registry.rs::validate_money`, `wallet.rs::{parse_units,format_units}` | Existing `exact_units` covers precise CFX round trips and invalid human syntax. New tests cover canonical wire syntax, non-ASCII aliases and the exact u128 boundary. |
| Canonical JSON and framing | `protocol.rs::{canonical_json,signing_payload}` | Fixed canonical UTF-8/escape/null/nesting bytes, integer extrema, recursive float rejection, array ordering, all six domain frames and a multibyte-domain length case. |
| Recoverable signatures | `wallet.rs::{personal_digest,sign_message,recover_digest,recover_signer}`, `protocol.rs::{sign,verify}` | An OpenSSL-generated and independently verified signature tests accepted encodings, wrong signer/domain and changed terms. Independently hashed quote/Unicode messages verify emitted signatures without relying on Chiral's recovery path or pinning nonce choice. Malformed encodings/recovery IDs are rejected; existing high-s rejection coverage remains. |
| IDs and payment commitments | `protocol.rs::{digest,hash_hex,offer_id,opening_data,topup_data,random_id}` | Independent Keccak/offer/opening/top-up vectors, changed agreement dimensions, offer revisions/prices/validity and invalid top-up identifiers. No random-ID quality claim follows from a length-only test. |
| Control-domain reuse | `auth.rs::{AuthPayload,challenge_payload}` | Exact domain framing is covered; existing challenge-binding tests remain. Challenge lifetime, authorization and one-use consumption belong to a later capability. |

Known-answer tests must have expected bytes/digests independent of the implementation under test, and explain their derivation. Use isolated in-memory test identities and fixed timestamps. Do not add real keys, paid requests, external services or wall-clock-sensitive expectations. A sign/verify round trip alone does not prove interoperability.

Vector derivation used Python's standard JSON encoder with explicit UTF-8 and four-byte big-endian lengths, independently of Rust serialization. Installed OpenSSL 3.6.3 supplied `dgst -KECCAK-256 -binary`; the known empty digest and scalar-one Ethereum address were checked. `pkeyutl` signed the already computed personal digest using the public test scalar one, `digest:sha256` and `nonce-type:1`; here SHA-256 selects RFC6979's nonce function, not a second hash of the signing input. After low-s normalization, OpenSSL verified the signature. Recovery parity was derived from `R = ((digest + r) / s mod n) G` using OpenSSL curve multiplication for this scalar-one fixture. Only fixed values/provenance remain in the tests: no dependency installation, generator files, live chain access or runtime OpenSSL requirement.

## Risks / Trade-offs

- An initial all-targets check failed with Axum/Tokio type identity mismatches while the headless build passed. A later sequential all-targets run passed without dependency/configuration changes or cache deletion. The compiler failure is no longer a recorded blocker, but its cause was not established; do not invent a source fix or duplicate-version diagnosis.
- Both build and test invocations use the same un-hashed `libchiral_network` artifact family. A build-artifact problem is possible, not demonstrated. Preserve existing artifacts and run distinct Rust verification invocations sequentially; do not claim an environmental workaround without evidence.
- Specification validation proves document structure, not runtime compliance. Keep contract-test tasks unticked until executed successfully, and record unavailable platform/release checks honestly.
- Native desktop prerequisites, release policy authority and out-of-checkout qualification remain blocked as recorded in APAS progress. They do not authorize installs, invented keys or remote operations.

## Migration Plan

No runtime or data migration. Once the boundary/vector tests and available project checks pass, archive with `openspec archive specify-inference-wire-foundation --yes` so this delta becomes the main specification. Do not use `--skip-specs`. Platform packaging, release authority and live qualification are separate gates, not consequences of this primitive-contract archive.

## Open Questions

None requiring the whitepaper's author. The byte contracts are explicit in the whitepaper; the preserved input-decoding details are compatible implementation choices, not reinterpretations of ambiguous design.
