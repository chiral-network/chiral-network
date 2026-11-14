## Authority and scope

`docs/chiral-book.md` is read-only. Its canonical monetary contract explicitly bounds money to `u128`; its funding and settlement sections require one exact original, canonical signed envelopes and separate safe-frontier credit. It does not set the access-list/RLP budgets. Preserve the existing conservative profile rather than widening supported input or inventing a consensus rule. No author answer is required.

The new `inference-payment-envelope-bounds` capability describes the shared local decoder. It complements, rather than modifies, the pending opening-funding capability's owner proof, fresh full-cost spendability and durable acknowledgement lifecycle. It can be evaluated independently of that change's blocked native time/schema fixes; neither change may claim the other's unfinished acceptance or full financial qualification.

## Existing implementation correspondence

- `chain_client.rs::decode`: nonempty lowercase-prefixed even hex, 16 KiB decoded maximum before hex allocation, only legacy/type-1/type-2, exact field counts, selected chain, 20-byte recipient, 4 KiB calldata, integer widths, typed fee consistency and structural gas.
- `canonical_rlp`: root depth zero, maximum depth four, at most 1,024 immediate list items, minimal lengths, checked declared-length arithmetic and exact payload consumption.
- `uint`/`uint64`: minimal unsigned representation and no truncation before conversion to 128/64-bit values.
- `access_list`: at most 64 entries and 256 keys in aggregate, exact entry/address/key shapes, duplicate occurrences counted without deduplication.
- `intrinsic`: checked structural cost including every address/key occurrence; this is not a chain execution estimate.
- `wallet.rs::recover_digest`: valid scalar/recovery interpretation and low-s enforcement before recovering the payer.
- `contracts.rs` and `billing_store.rs`: fresh admission refusal versus exact cached acknowledgement recovery remain separate. An existing acknowledgement lookup precedes fresh envelope/spendability checks; do not remove that recovery path.

There is no production behavior change in this specification step. The standalone proposal's public availability report, HTTP reopening/root-rotation mismatch and owner-untracked native source remain outside this change's authority.

## Why the raw ceiling has no otherwise-valid equality fixture

The byte ceiling is an early defensive check, not a promise that every size up to it can be reached by a valid profile. A conservative upper bound for the largest supported layout is:

- Access list: at most `3 + 64 * (3 + 21 + 3) + 256 * 33 = 10,179` bytes. This deliberately overestimates list-header overhead for small entries.
- Type-2 fields other than calldata/access list: at most `9 + 9 + 17 + 17 + 9 + 21 + 17 + 1 + 33 + 33 = 166` bytes, using even a full-width chain identifier rather than the shorter selected testnet value.
- Type prefix plus outer list header: at most four bytes for these sizes.
- Maximum allowed calldata: 4,099 encoded bytes, yielding at most **14,448 decoded envelope bytes**. For the 38-byte opening commitment, the bound is **10,388 bytes**. Both are strictly below 16,384; legacy/type-1 are smaller.

An in-memory Python calculation checked those conservative sums this turn. It is source-backed size reasoning, not a runtime parser boundary test. The separate 96 KiB control-body limit does not explain these smaller bounds. Do not weaken the 64/256/4 KiB rules, add opaque padding, or count an oversized transaction already rejected for another reason as isolated proof of the raw ceiling.

The 1,024-item RLP guard is likewise looser than valid envelope/access-list shapes. Direct defensive parser-limit evidence may need a private native test seam. That source and its existing unit-test file are owner-untracked; a source-only note is not a claim that the missing seam or regression is complete.

## Acceptance design

Reuse the existing core canonical envelope tests and the opening-funding admission groups for signed payment binding, full-cost arithmetic, nonce/head rechecks, numeric canonicality and same-payer high-s refusal. Do not duplicate those as tests of wiring.

Add independently signed type-1/type-2 transactions at and just beyond the reachable access-list boundaries: 64/65 address entries; 256/257 keys both within one entry and spread across entries; duplicates that still count; exact byte widths; and occurrence-based structural gas equal/below cases. Keep calldata bound to the actual quote, use sufficient funds/current nonce and retain signed original controls so unrelated binding or signature failures cannot mask the intended guard. Assert persistent quote/evidence preservation and no submission/credit.

Calldata at 4,096/4,097 bytes belongs to the local envelope-validation API with the matching expected calldata, not a fresh opening whose commitment is exactly 38 bytes. Separate that profile test from the opening route's exact-commitment requirement. For otherwise unreachable defensive byte/list budgets, use honest source derivation and narrowly scoped direct tests only when the private source is independently committable; never publish a new API solely for a test or copy the decoder into a fixture.

All RPC fixtures are synthetic. Successful structural decoding is neither executed funding, an on-chain reservation, public testnet qualification nor an authorized deployment. No automatic signing replacement, monetary conversion or migration is introduced.

## Initial requirement and caller map (task 2.1)

The common local path is `validate_payment_envelope -> decoded_payment -> decode`. Native `Consumer::verify_funding_intent` and original opening/top-up cancellation recovery use that path. Provider funding-intent uses `admit_payment_envelope` before acknowledgement registration. `verify_payment` also begins with `decode`, then verifies canonical execution; provider funding/fees and `RatingState::verify` use it. Stored-payment rechecks decode the retained original too. Native `prepare_payment_once -> payment_once` checks calldata before acquiring sender locks, decodes recovered originals, and uses the bounded legacy `sign_transaction` producer before journaling a new original. These are source correspondences, not new end-to-end credit or recovery evidence.

| Requirement/scenarios | Source and existing evidence | Remaining acceptance |
|---|---|---|
| Bounded canonical representation; ambiguous framing and nested/broad RLP | `decode`/`canonical_rlp`; core `rejects_cross_chain_malleable_noncanonical_and_trailing_evidence`; conservative size derivation above | Raw syntax/unsupported forms and remaining defensive budget evidence in 2.5; do not manufacture an unreachable valid raw-ceiling control |
| Exact numeric/opaque widths; wide low bits and calldata boundary | `uint`, `uint64`, `field_bytes`, `decode`, `wallet::recover_digest`; core binding tests and opening-funding signed-substitution, re-signed-integer and same-payer high-s groups | Local 4,096/4,097-byte calldata in 2.4; access-list byte widths/shape have task 2.2.2 acceptance below |
| Aggregate occurrence budgets; inclusive counts, duplicates and entry shapes | `access_list`; the old typed core fixture has only one address/key, and prior route controls have empty access lists | Task 2.2.1 count cases and 2.2.2 shape cases below cover both typed profiles; exact occurrence-based gas remains in 2.3 |
| Structural gas; repeated occurrence costs and equality | `intrinsic`, `sign_transaction`, `estimate_gas`, `decode`; core fixed-count arithmetic plus existing route insufficient-gas and full-signed-cost groups | Exact occurrence-based floor/equality and one-below cases with duplicates in 2.3 |
| Financial rejection; valid original after a refused candidate | `contracts` fresh admission before ledger registration, exact cached lookup and native original checks; existing route refusal/recovery groups | Count and shape local/HTTP controls below; shared callers are not independent relay, native-crash or live-settlement qualification |

## Task 2.2.1 access-list occurrence boundaries

Two new `inference_funding_admission_test` groups use the existing independent signer and production router. The signer now accepts an encoded access list; every previous caller explicitly retains the empty list, without changing prior assertions. A fixture builder emits exact 20-byte addresses and 32-byte keys and spreads keys evenly across entries. Distinct cases have distinct address/key pairs; duplicate cases repeat zero-filled opaque values, which must remain legal bytes within the occurrence limits.

`fresh_intent_enforces_access_list_address_occurrence_limit` compares 64 with 65 entries for both typed envelopes, using distinct and repeated addresses with no keys. `fresh_intent_enforces_aggregate_storage_key_occurrence_limit` compares 256 with 257 keys within one entry and across 64 entries, with distinct or repeated occurrences, for both typed envelopes. The distributed over-limit case has at most five keys per entry, so per-entry-only counting cannot pass it. Distributed positive controls also exercise both inclusive caps simultaneously.

Each of the twelve isolated cases requires direct local rejection of the over-limit candidate and HTTP 400 `invalid_payment`, no acknowledgement/submission/credit, and the retained quote. Its separately signed valid boundary control is locally validated before the negative request and acknowledged afterward, with exact original hash/signature binding and reopened-ledger assertions. Fixed one-million signed gas is above the structural floor even for these over-limit candidates, and synthetic RPC supplies the current nonce and ample balance, so a low-gas/balance failure cannot stand in for count rejection.

Task 2.2.1 closes count/duplicate coverage only; task 2.2.2 below separately covers malformed shapes and opaque widths. Precise structural-gas boundaries remain in 2.3. No production decoder, native preparation, wallet, settlement or whitepaper behavior changes.

## Task 2.2.2 access-list shape and opaque-width acceptance

Three further `inference_funding_admission_test` groups add 32 independently signed refusal/control pairs across EIP-2930 and EIP-1559:

- `fresh_intent_rejects_malformed_access_list_shapes`: empty bytes or a byte-string-encoded access list instead of a list; a byte-string entry; entries with zero, one or three fields; a list instead of address bytes; empty bytes or a flat key instead of the key list; and a list instead of key bytes. These ten shapes exercise all list/opaque-byte boundaries rather than relying on a count overflow.
- `fresh_intent_requires_exact_access_list_address_width`: zero-, 19- and 21-byte addresses, with a correctly shaped single 32-byte key.
- `fresh_intent_requires_exact_storage_key_width`: zero-, 31- and 33-byte keys, with a correctly shaped 20-byte address.

Each positive control has one zero-filled 20-byte address and one zero-filled 32-byte key, proving those opaque values are admissible rather than subject to integer normalization. Existing empty-list, empty-key-list and duplicate/budget controls remain unchanged. A small private RLP builder composes the malformed entry fields; the existing signer signs their actual payload, not stale signatures over a different control.

All malformed candidates are canonically RLP-encoded and comfortably within raw/item/count budgets. The list-valued key is deliberately empty at depth four, so a depth-five refusal cannot masquerade as key-type validation. The existing fixture supplies sufficient signed gas, balance, current nonce and exact quote fields. It requires local `Rejected`, owner-authenticated HTTP 400 `invalid_payment`, reopened-ledger absence of acknowledgement/credit/fees and no submission, then successful acknowledgement of the unchanged valid control under the retained quote.

These tests complete the structural shape/width portion of parent 2.2, not the exact gas, calldata or defensive parser-budget tasks. No source adoption, new production API, live-chain execution or settlement qualification is claimed.

## Integration

Run strict OpenSpec validation and the available README/CI checks. Keep the change open until acceptance tasks and evidence mapping are complete; archive normally with `openspec archive specify-payment-envelope-resource-bounds --yes`, never `--skip-specs`. Preserve all unrelated owner work and the whitepaper.
