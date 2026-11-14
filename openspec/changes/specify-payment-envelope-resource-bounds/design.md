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
| Bounded canonical representation; ambiguous framing and nested/broad RLP | `decode`/`canonical_rlp`; core `rejects_cross_chain_malleable_noncanonical_and_trailing_evidence`; raw-envelope, root/header, malformed-child-length and freshly signed byte-string groups in 2.5.1–2.5.4 below; conservative size derivation above | Remaining scalar-shape and direct defensive-budget evidence in 2.5–2.6; do not manufacture an unreachable valid raw-ceiling control |
| Exact numeric/opaque widths; wide low bits and calldata boundary | `uint`, `uint64`, `field_bytes`, `decode`, `wallet::recover_digest`; core binding tests and opening-funding signed-substitution, re-signed-integer and same-payer high-s groups | Tasks 2.4 and 2.5.2 cover local calldata boundaries; 2.5.4 covers freshly signed byte-string encodings and 127/128 fee controls; access-list widths/shape have 2.2.2 acceptance. The scalar/list shape defect remains blocked in 2.6 |
| Aggregate occurrence budgets; inclusive counts, duplicates and entry shapes | `access_list`; the old typed core fixture has only one address/key, and prior route controls have empty access lists | Task 2.2.1 count cases and 2.2.2 shape cases below cover both typed profiles; exact occurrence-based gas is covered separately by 2.3 |
| Structural gas; repeated occurrence costs and equality | `intrinsic`, `sign_transaction`, `estimate_gas`, `decode`; core fixed-count arithmetic plus existing route insufficient-gas and full-signed-cost groups | Task 2.3 below covers exact local/route floors for empty, distinct and repeated access lists; this is not native execution-estimate or settlement qualification |
| Financial rejection; valid original after a refused candidate | `contracts` fresh admission before ledger registration, exact cached lookup and native original checks; existing route refusal/recovery groups | Count, shape, gas, raw-envelope, RLP root/header, malformed-child and byte-string local/HTTP controls below; shared callers are not independent relay, native-crash or live-settlement qualification |

## Task 2.2.1 access-list occurrence boundaries

Two new `inference_funding_admission_test` groups use the existing independent signer and production router. The signer now accepts an encoded access list; every previous caller explicitly retains the empty list, without changing prior assertions. A fixture builder emits exact 20-byte addresses and 32-byte keys and spreads keys evenly across entries. Distinct cases have distinct address/key pairs; duplicate cases repeat zero-filled opaque values, which must remain legal bytes within the occurrence limits.

`fresh_intent_enforces_access_list_address_occurrence_limit` compares 64 with 65 entries for both typed envelopes, using distinct and repeated addresses with no keys. `fresh_intent_enforces_aggregate_storage_key_occurrence_limit` compares 256 with 257 keys within one entry and across 64 entries, with distinct or repeated occurrences, for both typed envelopes. The distributed over-limit case has at most five keys per entry, so per-entry-only counting cannot pass it. Distributed positive controls also exercise both inclusive caps simultaneously.

Each of the twelve isolated cases requires direct local rejection of the over-limit candidate and HTTP 400 `invalid_payment`, no acknowledgement/submission/credit, and the retained quote. Its separately signed valid boundary control is locally validated before the negative request and acknowledged afterward, with exact original hash/signature binding and reopened-ledger assertions. Fixed one-million signed gas is above the structural floor even for these over-limit candidates, and synthetic RPC supplies the current nonce and ample balance, so a low-gas/balance failure cannot stand in for count rejection.

Task 2.2.1 closes count/duplicate coverage only; task 2.2.2 below separately covers malformed shapes and opaque widths, and task 2.3 covers precise structural-gas boundaries. No production decoder, native preparation, wallet, settlement or whitepaper behavior changes.

## Task 2.2.2 access-list shape and opaque-width acceptance

Three further `inference_funding_admission_test` groups add 32 independently signed refusal/control pairs across EIP-2930 and EIP-1559:

- `fresh_intent_rejects_malformed_access_list_shapes`: empty bytes or a byte-string-encoded access list instead of a list; a byte-string entry; entries with zero, one or three fields; a list instead of address bytes; empty bytes or a flat key instead of the key list; and a list instead of key bytes. These ten shapes exercise all list/opaque-byte boundaries rather than relying on a count overflow.
- `fresh_intent_requires_exact_access_list_address_width`: zero-, 19- and 21-byte addresses, with a correctly shaped single 32-byte key.
- `fresh_intent_requires_exact_storage_key_width`: zero-, 31- and 33-byte keys, with a correctly shaped 20-byte address.

Each positive control has one zero-filled 20-byte address and one zero-filled 32-byte key, proving those opaque values are admissible rather than subject to integer normalization. Existing empty-list, empty-key-list and duplicate/budget controls remain unchanged. A small private RLP builder composes the malformed entry fields; the existing signer signs their actual payload, not stale signatures over a different control.

All malformed candidates are canonically RLP-encoded and comfortably within raw/item/count budgets. The list-valued key is deliberately empty at depth four, so a depth-five refusal cannot masquerade as key-type validation. The existing fixture supplies sufficient signed gas, balance, current nonce and exact quote fields. It requires local `Rejected`, owner-authenticated HTTP 400 `invalid_payment`, reopened-ledger absence of acknowledgement/credit/fees and no submission, then successful acknowledgement of the unchanged valid control under the retained quote.

These tests complete the structural shape/width portion of parent 2.2, not the exact gas, calldata or defensive parser-budget tasks. No source adoption, new production API, live-chain execution or settlement qualification is claimed.

## Task 2.3 exact structural-gas acceptance

Two further `inference_funding_admission_test` groups add nineteen signed one-below/equality pairs. `fresh_intent_enforces_exact_calldata_gas_for_all_envelopes` exercises legacy and both typed envelopes with no access-list occurrences. `fresh_intent_charges_every_access_list_occurrence` exercises eight layouts for each typed envelope: address-only, one address/key pair, repeated addresses without keys, repeated keys in one entry, repeated address/key pairs, repeated keys across repeated entries, and the simultaneous 64-address/256-key caps with distinct or repeated occurrences.

The expected calldata charge is independently calculated as `16 * length - 12 * zero_byte_count` on the actual opening commitment, then added to the 21,000 base and literal access charges in the test matrix. The oracle does not call the production `intrinsic` helper or decode occurrence counts from production output. The opening prefix guarantees both zero and nonzero calldata bytes. Every duplicate case's one-below full-occurrence limit still covers a deduplicated interpretation, so accepting it would expose undercharging.

The control is signed with the exact full floor; the adversarial candidate differs only in its signed gas limit, one unit smaller, and its freshly computed signature. A shared private fixture assertion preserves the previous access-list checks: locally validate the original and reject the candidate, supply ample balance/current nonce, require authenticated HTTP refusal with no acknowledgement/submission/credit/fees, reopen retained state, then acknowledge the unchanged exact-floor original. No existing assertion is dropped or weakened.

This proves the application's structural minimum, not Conflux execution gas sufficiency. Native `eth_estimateGas`, full signed-cost spendability, canonical execution, safe-frontier credit and live qualification remain distinct. Calldata-size acceptance is recorded in task 2.4 below; defensive parser-budget tasks remain open.

## Task 2.4 local calldata-size acceptance

`local_envelope_enforces_calldata_limit_without_truncation` exercises legacy, EIP-2930 and EIP-1559 through the synchronous `validate_payment_envelope` API. It uses an explicit test-network identity, isolated journal directory and the existing unavailable-loopback-RPC convention; no RPC server, signed quote or funding route is needed. The independent fixture signer is reused, without a new production API.

Each 4,096-byte control repeats all byte values, begins with zero and ends with `0xff`. It must validate against the matching expected calldata, preserve every decoded byte and return the hash of the original signed envelope. Appending one zero byte yields a newly signed 4,097-byte candidate. Both full-length expected calldata and the 4,096-byte expected prefix must reject it: testing only the former could miss a decoder that clips before comparing terms. The unchanged original must still return the same complete validated envelope afterward.

Both lengths have ample signed gas, empty access lists and envelopes well below 16 KiB, isolating the calldata rule. No large payload is submitted as an opening commitment: the opening route still requires its exact 38-byte commitment, independently covered by its binding tests. These local results are not acknowledgement, execution, spendability or credit evidence. Raw framing is covered below; remaining canonical RLP and defensive parser-budget acceptance stay in 2.5.

## Task 2.5.1 raw-envelope acceptance

`inference_funding_admission_test` adds three production-API groups:

- `malformed_raw_envelopes_preserve_fresh_funding_admission`: twelve representations for each supported envelope, yielding 36 refusal/control pairs. Cases cover an empty string, missing or uppercase `0x` prefix, empty or odd payload, invalid hex, leading/trailing whitespace, trailing decoded byte, concatenated envelopes, truncated final byte and an inserted zero type prefix. Each starts from an independently signed, otherwise admissible opening transaction.
- `unsupported_typed_prefixes_do_not_admit_type_two_shaped_payments`: three refusal/control pairs for prefixes `0x03`, `0x04` and `0x7f`. The existing private test signer takes an explicit unsigned field count so these type-2-shaped payloads are signed over the actual unsupported prefix; they are not stale-signature mutations, valid blob transactions or valid authorization transactions.
- `hex_letter_case_preserves_payment_identity_and_retained_spelling`: uppercase and mixed-case hex digits, retaining lowercase `0x`, for all three supported envelopes. Six local equality controls prove the full decoded payment identity is unchanged; six fresh authenticated acknowledgements and exact reopened-ledger lookups preserve the submitted spelling without credit or transaction submission. No exact-replay rule is relaxed.

All 39 refusal pairs require local `ChainError::Rejected`, authenticated HTTP 400 `invalid_payment`, no cached acknowledgement, account credit, funding/fee rows or transaction submission, followed by acknowledgement of the unchanged original. These isolated synthetic-RPC observations cover local parsing and fresh admission, not execution, chain finality or paid inference.

Independent root/header and malformed-child-length acceptance follows in 2.5.2–2.5.3. Scalar-shape and direct defensive byte/item/depth evidence remains open in 2.5–2.6. Malformed framing is not an otherwise-valid 16 KiB equality control and does not isolate a private parser budget; the size derivation and owner-source restrictions still apply.

## Task 2.5.2 independent RLP root/header acceptance

This slice does not resolve the separately recorded scalar/list shape defect in `field_bytes`. It adds two `inference_funding_admission_test` groups without changing any requirement or decoder behavior:

- `malformed_rlp_roots_preserve_fresh_funding_admission`: eighteen refusal/control pairs across all three envelopes. A root is wrapped as an opaque string, loses its last field, gains an empty field after the valid signature, has a zero-prefixed length, has an incomplete length header, or declares eight maximal length bytes. Extra-field and padded-length candidates retain the original unsigned fields/signature. Missing fields and incomplete headers are structural negatives, not otherwise-valid signatures. Legacy string roots fail envelope selection; typed string roots reach root validation.
- `nonminimal_nested_access_list_headers_preserve_original`: six pairs cover a padded outer-list length, long-form short entry and long-form short key in both typed profiles. A valid single-address/single-key control has a 55-byte entry payload and 56-byte outer-list payload. The malformed key sits at depth four; these cases do not exceed occurrence, depth or byte-width budgets. Both access-list encodings are independently signed.

All 24 pairs require local rejection, authenticated HTTP 400 `invalid_payment`, persisted no acknowledgement/credit/funding/fee effects, no transaction submission and successful unchanged-original admission. These are isolated synthetic-RPC observations, not payment execution or finality evidence.

The existing local calldata test additionally accepts 55 and 56 bytes under every supported envelope, checking exact bytes and original transaction hash in six controls. Its complete 4,096/4,097-byte and clipped-expected-data checks remain unchanged. No non-opening payload is submitted as an opening commitment; the earlier all-byte-value payload remains the opacity evidence.

The previously failing, freshly signed legacy list-scalar reproduction remains in APAS progress and requires an independently committable native fix with its regression. Do not infer scalar-shape correctness from these passing header controls. Malformed child lengths are covered below; direct defensive-budget evidence remains open. Root overflow is not allocation instrumentation or private guard isolation. No public test seam or relaxed limit is introduced.

## Task 2.5.3 malformed child lengths and complete consumption

The locked `rlp` 0.5.2 dependency checks payload-length addition and bounds before slicing, but its iterator stops on a child error and `item_count` counts that valid prefix. `canonical_rlp` must therefore reject unconsumed bytes rather than accepting an otherwise valid payment prefix. The tests assert production behavior, not the dependency's iterator count or error wording.

Six bounded suffixes cover incomplete eight-byte length headers, an ordinary declared 256-byte payload with no body, and eight maximal length bytes, for both strings and lists. `append_list_with_malformed_tail` copies only valid original children and includes every actual byte in the enclosing list length before appending the malformed child. It does not parse or repair that child.

- `malformed_root_child_lengths_cannot_authorize_a_valid_prefix` covers eighteen refusal/control pairs across all three supported envelopes. The original fields and signature stay intact before the invalid extra child inside the root list.
- `malformed_nested_child_lengths_preserve_original_admission` covers thirty-six pairs across both typed envelopes, with the child appended after the valid access-list, entry or key-list prefix. Enclosing lists are rebuilt with correct actual lengths and the full malformed access-list bytes are freshly signed. These positions extend through depth four without relying on an over-depth tree or oversized occurrence budget.

All 54 pairs require local rejection, authenticated HTTP 400 `invalid_payment`, no retained acknowledgement, credit, funding/fee rows or transaction submission, and successful unchanged-original admission. Complete parent framing distinguishes these from bytes merely appended outside the transaction. Isolated synthetic RPC is not execution, safe-frontier or paid-inference qualification.

These bounded malformed inputs do not isolate the raw byte, immediate-item or depth guards. Direct guard evidence remains blocked where it requires owner-untracked source; do not exploit the scalar/list defect to construct an invalid positive boundary control. Freshly signed scalar/recipient byte-string minimal-encoding acceptance follows in 2.5.4, separately from that shape defect and these malformed tails. No guard is relaxed and no production test API is added.

## Task 2.5.4 freshly signed byte-string minimal encoding

`fresh_intent_rejects_resigned_nonminimal_byte_strings` adds twelve refusal/original-recovery pairs across legacy, type 1 and type 2:

- Maximum fee value 127 is encoded as `81 7f` instead of the canonical single byte `7f`.
- Maximum fee value 128 is encoded as `b8 01 80` instead of the canonical `81 80`.
- The unchanged 20-byte recipient uses long form `b8 14` or a zero-prefixed length `b9 00 14` instead of `94`.

Each candidate is freshly signed over its actual nonminimal field bytes; unchanged numeric values, recipient, commitment, nonce and sufficient signed gas/funds prevent unrelated binding or stale-signature failures from explaining refusal. Valid originals at 127 and 128 exercise both sides of the single-byte encoding boundary, rather than accepting a blanket ban on `81` wrappers.

Every pair requires local rejection, authenticated HTTP 400 `invalid_payment`, no retained acknowledgement, account credit, funding/fee rows or transaction submission, and successful unchanged-original admission. The existing signer and admission fixture are reused; no decoder, dependency or wire requirement changes.

All mutated fields remain byte-string encodings. This is not evidence that a list used as a scalar is rejected: that distinct, previously observed defect still requires an independently committable native fix and its regression. Direct private budget evidence remains open; these synthetic-RPC results do not establish execution, finality or public-release qualification.

## Integration

Run strict OpenSpec validation and the available README/CI checks. Keep the change open until acceptance tasks and evidence mapping are complete; archive normally with `openspec archive specify-payment-envelope-resource-bounds --yes`, never `--skip-specs`. Preserve all unrelated owner work and the whitepaper.
