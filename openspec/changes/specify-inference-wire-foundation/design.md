## Context

`docs/chiral-book.md` is the human-owned whitepaper, unchanged from initialization (SHA-256 `f61bb0cf3926303745f06bf47f211a1169d768480f548a7750f320e5ca4d70c3`). There are no main OpenSpec specs yet. This change records the first independently useful protocol slice; it does not certify alignment of the entire running product.

The checkout contains extensive pre-existing uncommitted inference implementation. Inspect it as evidence, but do not adopt it into this worker's commit. The inspected primitives currently match the specified algorithms; dedicated byte-level regression coverage remains to be added before completing this change.

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
5. **Keep this change open for uncertain byte-level edges.** Existing behavior is not a substitute for independent expected bytes. Add deterministic tests only for missing consumer-visible protocol boundaries; retain existing useful tests rather than duplicating or weakening them.

## Source and coverage correspondence

| Contract | Inspected source | Existing evidence and remaining coverage |
|---|---|---|
| Explicit testnet and full identity | `network.rs::NetworkConfig::{validate,namespace}`, `protocol.rs::NetworkIdentity`, `offer_registry.rs::OfferRegistry::insert_at` | `deployment_namespaces_do_not_collide` checks deployment separation and rejects another chain/genesis; offer tests reject chain mismatch. Add acceptance-boundary cases for same-chain genesis/deployment mismatch where not covered. |
| Envelope and primitive shapes | `protocol.rs::{Signed,NetworkIdentity,PriceSchedule}` | `deny_unknown_fields` and typed fields are present. Add direct malformed-object rejection cases; a valid signature must not excuse schema projection. |
| Money and CFX | `offer_registry.rs::validate_money`, `wallet.rs::{parse_units,format_units}` | `exact_units` covers precise round trips and invalid human syntax. Add canonical wire syntax/range cases not directly covered. |
| Canonical JSON and framing | `protocol.rs::{canonical_json,signing_payload}` | Implementation recursively rejects floats and uses sorted JSON maps. No dedicated canonical-byte/framing tests found in the inspected sources; add known-answer bytes for nested, null, Unicode, escape and array-order cases. |
| Recoverable signatures | `wallet.rs::{personal_digest,sign_message,recover_digest,recover_signer}`, `protocol.rs::{sign,verify}` | `personal_sign_roundtrip_and_tamper` already covers tampering and high-s rejection. Add independent known-answer verification and remaining domain/signer/encoding rejection edges. |
| IDs and payment commitments | `protocol.rs::{digest,hash_hex,offer_id,opening_data,topup_data,random_id}` | Existing consumers use these primitives; add direct byte/digest vectors and term-binding cases rather than tests that calculate expected values through the same helper. |
| Control-domain reuse | `auth.rs::{AuthPayload,challenge_payload}` | Reuses the foundation frame; challenge lifetime, authorization and one-use consumption belong to a later capability. |

Known-answer tests must have expected bytes/digests independent of the implementation under test, and explain their derivation. Use isolated in-memory test identities and fixed timestamps. Do not add real keys, paid requests, external services or wall-clock-sensitive expectations. A sign/verify round trip alone does not prove interoperability.

## Risks / Trade-offs

- The existing all-targets Rust check failed with Axum/Tokio type identity mismatches while the headless binary build passed. `cargo tree` resolves one Axum (`0.7.9`) and one Tokio (`1.49.0`); the committed Cargo configuration contains Windows-only linker flags. This does not establish a duplicate dependency version or a source fix. The affected source/Cargo files are pre-existing dirty work; the root cause remains unresolved. Do not change dependencies or remove checks on the compiler diagnostic's suggestion alone.
- Both build and test invocations use the same un-hashed `libchiral_network` artifact family. A build-artifact problem is possible, not demonstrated. Preserve existing artifacts and run distinct Rust verification invocations sequentially; do not claim an environmental workaround without evidence.
- Specification validation proves document structure, not runtime compliance. Keep contract-test tasks unticked until executed successfully, and record unavailable platform/release checks honestly.
- Native desktop prerequisites, release policy authority and out-of-checkout qualification remain blocked as recorded in APAS progress. They do not authorize installs, invented keys or remote operations.

## Migration Plan

No runtime or data migration. Publish the proposed requirements as a normal OpenSpec delta; add the missing deterministic contract tests in a subsequent APAS step. When all tasks and relevant checks are complete, use `openspec archive specify-inference-wire-foundation --yes` so the delta becomes the main specification. Do not use `--skip-specs`.

## Open Questions

None requiring the whitepaper's author. The byte contracts are explicit in the whitepaper; the preserved input-decoding details are compatible implementation choices, not reinterpretations of ambiguous design.
