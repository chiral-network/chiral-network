## Authority and scope

`docs/chiral-book.md` is read-only. Its canonical monetary contract explicitly bounds money to `u128`; its funding and settlement sections require one exact original, canonical signed envelopes and separate safe-frontier credit. It does not set the access-list/RLP budgets. Preserve the existing conservative profile rather than widening supported input or inventing a consensus rule. No author answer is required.

The new `inference-payment-envelope-bounds` capability describes the shared local decoder. It complements, rather than modifies, the opening-funding capability's owner proof, fresh full-cost spendability and durable acknowledgement lifecycle. It was evaluated independently of that change's native time/schema fixes; neither change may claim the other's acceptance or full financial qualification.

Dated note (2026-11-22): this section previously called the opening-funding capability "pending". That change has since been archived as `openspec/changes/archive/2026-11-22-specify-opening-funding-intent`, and its requirements are now the current `openspec/specs/inference-opening-funding-intent` spec. This change still does not modify that spec, and its own acceptance does not depend on that archive.

## Existing implementation correspondence

- `chain_client.rs::decode`: nonempty lowercase-prefixed even hex, 16 KiB decoded maximum before hex allocation, only legacy/type-1/type-2, exact field counts, selected chain, 20-byte recipient, 4 KiB calldata, integer widths, typed fee consistency and structural gas.
- `canonical_rlp`: root depth zero, maximum depth four, at most 1,024 immediate list items, minimal lengths, checked declared-length arithmetic and exact payload consumption.
- `field_bytes` requires an RLP data item before accessing its payload; `uint`/`uint64` then enforce minimal unsigned representation and no truncation before conversion to 128/64-bit values. Canonical RLP framing alone does not establish a transaction field's semantic type.
- `access_list`: at most 64 entries and 256 keys in aggregate, exact entry/address/key shapes, duplicate occurrences counted without deduplication.
- `intrinsic`: checked structural cost including every address/key occurrence; this is not a chain execution estimate.
- `wallet.rs::recover_digest`: valid scalar/recovery interpretation and low-s enforcement before recovering the payer.
- `contracts.rs` and `billing_store.rs`: fresh admission refusal versus exact cached acknowledgement recovery remain separate. An existing acknowledgement lookup precedes fresh envelope/spendability checks; do not remove that recovery path.

The initial step was specification-only. Task 2.6 now corrects the native shared field-type boundary with signed regressions; the migration source is committed and no longer ownership-blocked. Public proposal-work admission and HTTP reopening/root rotation remain separate capabilities. Historical acceptance sections below retain the scope and limitations of their original steps; current field-shape status is recorded in task 2.6, and current defensive-budget status in "Task 2.5 direct defensive budget evidence".

## Why the raw ceiling has no otherwise-valid equality fixture

The byte ceiling is an early defensive check, not a promise that every size up to it can be reached by a valid profile. A conservative upper bound for the largest supported layout is:

- Access list: at most `3 + 64 * (3 + 21 + 3) + 256 * 33 = 10,179` bytes. This deliberately overestimates list-header overhead for small entries.
- Type-2 fields other than calldata/access list: at most `9 + 9 + 17 + 17 + 9 + 21 + 17 + 1 + 33 + 33 = 166` bytes, using even a full-width chain identifier rather than the shorter selected testnet value.
- Type prefix plus outer list header: at most four bytes for these sizes.
- Maximum allowed calldata: 4,099 encoded bytes, yielding at most **14,448 decoded envelope bytes**. For the 38-byte opening commitment, the bound is **10,388 bytes**. Both are strictly below 16,384; legacy/type-1 are smaller.

An in-memory Python calculation checked those conservative sums this turn. It is source-backed size reasoning, not a runtime parser boundary test. The separate 96 KiB control-body limit does not explain these smaller bounds. Do not weaken the 64/256/4 KiB rules, add opaque padding, or count an oversized transaction already rejected for another reason as isolated proof of the raw ceiling.

The 1,024-item RLP guard is likewise looser than valid envelope/access-list shapes. Direct defensive parser-limit evidence now uses the committed private in-crate test module; see "Task 2.5 direct defensive budget evidence". No new public API or weaker transaction profile is justified to construct those controls.

## Acceptance design

Reuse the existing core canonical envelope tests and the opening-funding admission groups for signed payment binding, full-cost arithmetic, nonce/head rechecks, numeric canonicality and same-payer high-s refusal. Do not duplicate those as tests of wiring.

Add independently signed type-1/type-2 transactions at and just beyond the reachable access-list boundaries: 64/65 address entries; 256/257 keys both within one entry and spread across entries; duplicates that still count; exact byte widths; and occurrence-based structural gas equal/below cases. Keep calldata bound to the actual quote, use sufficient funds/current nonce and retain signed original controls so unrelated binding or signature failures cannot mask the intended guard. Assert persistent quote/evidence preservation and no submission/credit.

Calldata at 4,096/4,097 bytes belongs to the local envelope-validation API with the matching expected calldata, not a fresh opening whose commitment is exactly 38 bytes. Separate that profile test from the opening route's exact-commitment requirement. For otherwise unreachable defensive byte/list budgets, use honest source derivation and narrowly scoped direct tests only when the private source is independently committable; never publish a new API solely for a test or copy the decoder into a fixture.

All RPC fixtures are synthetic. Successful structural decoding is neither executed funding, an on-chain reservation, public testnet qualification nor an authorized deployment. No automatic signing replacement, monetary conversion or migration is introduced.

## Initial requirement and caller map (task 2.1)

The common local path is `validate_payment_envelope -> decoded_payment -> decode`. Native `Consumer::verify_funding_intent` and original opening/top-up cancellation recovery use that path. Provider funding-intent uses `admit_payment_envelope` before acknowledgement registration. `verify_payment` also begins with `decode`, then verifies canonical execution; provider funding/fees and `RatingState::verify` use it. Stored-payment rechecks decode the retained original too. Native `prepare_payment_once -> payment_once` checks calldata before acquiring sender locks, decodes recovered originals, and uses the bounded legacy `sign_transaction` producer before journaling a new original. These are source correspondences, not new end-to-end credit or recovery evidence.

| Requirement/scenarios | Source and existing evidence | Remaining acceptance |
|---|---|---|
| Bounded canonical representation; ambiguous framing and nested/broad RLP | `decode`/`canonical_rlp`; core `rejects_cross_chain_malleable_noncanonical_and_trailing_evidence`; raw-envelope, root/header, malformed-child-length and freshly signed byte-string groups in 2.5.1–2.5.4 below; conservative size derivation above | Task 2.5's direct private byte/item/depth/opaque/declared-length evidence is mapped clause by clause below; task 2.6 supplies the field-type correction and regression matrix. No unreachable valid raw-ceiling control is manufactured |
| Exact numeric/opaque widths; wide low bits and calldata boundary | `uint`, `uint64`, `field_bytes`, `decode`, `wallet::recover_digest`; core binding tests and opening-funding signed-substitution, re-signed-integer and same-payer high-s groups | Tasks 2.4 and 2.5.2 cover local calldata boundaries; 2.5.4 covers freshly signed byte-string encodings and 127/128 fee controls; 2.2.2 covers access-list widths/shape. Task 2.6 adds same-payload list-versus-byte-string discrimination |
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

## Task 2.6 shared field-type correction

`Rlp::data()` returns an item's payload without requiring that the item be a byte string. A canonically framed list can therefore supply the same numerical or opaque bytes as a valid field. The retained failure was a freshly signed legacy gas-price field `c1 0b`, interpreted as price 11. `field_bytes` now requires `is_data()` before payload extraction. All its existing integer, recipient, calldata and access-list byte-field callers share the correction, without new allocation on accepted input or a new public API.

Four regression groups in `inference_funding_admission_test` isolate semantic type from canonical framing, widths, payment binding and signature validity:

- `fresh_intent_rejects_resigned_list_valued_integers`: fifteen independently signed local/HTTP refusal-and-original-admission pairs cover nonce, gas price/limit and value in all three profiles, typed chain ID and the type-2 priority/max price. The original numerical payload is a sequence of complete single-byte RLP items, including empty zero-nonce payloads; the candidate signs the actual enclosing list.
- `fresh_intent_rejects_list_encoded_typed_signature_parity`: two typed-profile pairs preserve numerical recovery parity and the original r/s. Parity lies outside the signing preimage; no stale or invalid signature is substituted to explain refusal.
- `local_envelope_rejects_list_valued_recipient_and_calldata`: nine local pairs cover a 20-byte zero recipient and empty/nonempty opaque calldata across all three profiles. Exact original data, recipient and transaction hash are preserved. These deliberate non-opening payloads use matching local expectations, not the funding route's exact commitment.
- `fresh_intent_rejects_list_valued_access_list_addresses`: two typed-profile pairs preserve the exact 20-byte zero address payload in a canonically framed list whose children reach depth four, not five. Address width and nesting cannot mask the required field-type refusal.

All nineteen HTTP pairs use the existing owner-authenticated admission fixture, requiring no acknowledgement, credit, funding/fee records or submission for the candidate, retained signed terms after reopening, and subsequent acknowledgement of the unchanged valid original. Nine additional local pairs do not claim HTTP admission. Existing signature, historical acknowledgement, journal/nonce and recovery tests remain intact. All available checks pass as recorded in APAS progress; direct defensive RLP budgets remain a separate incomplete task. No payment execution, automatic replacement or repair is claimed.

## Task 2.5 direct defensive budget evidence

The earlier 2.5.x groups used signed local/HTTP admission. That can prove a candidate is refused without financial effects, but every refusal looks the same over HTTP (`invalid_payment`), so it cannot show which private guard fired. It also cannot give an otherwise-valid control at a budget that no valid envelope reaches. Six new tests in the existing in-crate module `src-tauri/src/chain_client_tests.rs`, compiled only as `chain_client::tests`, close that gap. They call the private `decode` and `canonical_rlp` directly and check each refusal's exact error text, which differs from guard to guard. They add no production code, no new public or `#[cfg(test)]` API, no copy of the decoder and no change to any limit.

Two local builders support these tests. `shaped_envelope` makes envelope-shaped bytes whose chain ID, parity, fee order, recipient and value pass every check that runs before the calldata, access-list and gas checks. Its signature scalars are placeholders, so these bytes are never otherwise valid, and every refusal asserted on them happens before signature recovery. `sized_envelope` picks the calldata length that makes the decoded size, including any typed prefix, exactly equal to a target.

| Spec clause | Reachable public evidence (existing) | Direct private evidence (task 2.5) | Derived ceiling / note |
|---|---|---|---|
| Lowercase `0x`, nonempty even hex, letter case keeps the hash | 2.5.1 `malformed_raw_envelopes_preserve_fresh_funding_admission`, `hex_letter_case_preserves_payment_identity_and_retained_spelling` | Not repeated | — |
| At most 16,384 decoded bytes, typed prefix included | None can be otherwise valid | `defensive_raw_byte_budget_is_inclusive_and_counts_the_typed_prefix`: for each of the three envelopes, exactly 32,768 digits (16,384 bytes) pass the budget and are then refused by the stricter calldata limit (`payment commitment exceeds size limit`). One more calldata byte (32,770 digits) is refused by the budget itself (`raw transaction size is invalid`). The same 16,384-byte RLP list passes the budget without a prefix (then `wrong transaction field count`) and fails it with the type-2 prefix | Valid layouts stay ≤ 14,448 bytes (≤ 10,388 for the opening commitment), so the 16,384-byte input must break a stricter rule; this one has more than 4,096 calldata bytes |
| Hex longer than twice the budget refused before the decoded buffer is allocated | — | `defensive_raw_byte_budget_is_checked_before_hex_decoding`: at 32,768 digits, invalid first or last digits reach hex decoding (`invalid raw transaction hex`). At 32,770 digits, the same invalid digits (including ones past the budget) and a 1 MiB run of `g` produce only the size error | This proves ordering only. Allocation is not instrumented; `hex::decode` is the only decoded-buffer allocation and it follows the check |
| One canonical list with 9/11/12 fields; trailing, truncated and inconsistent lengths | 2.5.1 trailing/concatenated/truncated; 2.5.2 `malformed_rlp_roots_preserve_fresh_funding_admission`; 2.5.3 child-length groups; core `rejects_cross_chain_malleable_noncanonical_and_trailing_evidence` | Declared-length test below: lengths one past the available payload; overflowing children whose valid prefix must not be accepted (`invalid RLP list payload`) at depths zero to three | — |
| Nonminimal encodings | 2.5.2 `nonminimal_nested_access_list_headers_preserve_original`; 2.5.4 `fresh_intent_rejects_resigned_nonminimal_byte_strings` | Incomplete eight-byte length header (`non-canonical RLP length`) | — |
| Declared lengths that overflow arithmetic | 2.5.2 overflowing root; 2.5.3 overflowing string/list children | `defensive_rlp_refuses_overflowing_and_overlong_declared_lengths`: for strings and lists, every length-of-length 1–8 with all-`ff` bytes (with and without 300 following bytes). Eight-byte declared lengths `u64::MAX`, `u64::MAX - 8` (header addition overflows by exactly one), `u64::MAX - 9` (no overflow, but more than the input holds) and `2^56`. All return an error and none panics | With a 64-bit `usize`, the fold over at most eight length bytes cannot overflow, so checked header addition and the exact-length comparison report `truncated or trailing RLP bytes`. Only a narrower `usize` reaches the `RLP length overflow` branch, which the test also accepts there; no such target was run |
| At most 1,024 immediate items per list | Not reachable as an otherwise-valid control: the root has 9/11/12 fields and the access list ≤ 64/256 | `defensive_rlp_limits_each_list_to_1024_immediate_items`: 1,024 items are accepted and 1,025 refused (`RLP item limit`), for byte, empty-string and empty-list items at list depths zero to three. 4,096 descendants spread across lists are accepted. A depth-five or nonminimal last child is refused for its own reason as the 1,024th item, but as the 1,025th it is reported as `RLP item limit`, so a deeper refusal cannot mask the breadth guard. In `decode`, 1,025 root fields hit the item guard while 1,024 get `wrong transaction field count` (no prefix, type 1 and type 2). 1,025 access-list entries or keys hit the item guard, while 1,024 are refused by the stricter `access list address limit` or `access list storage-key limit` | The item guard fires before field-count and access-list checks and before the excess child is examined, and 1,024 never overrides them |
| Depth at most four, root at zero | Every typed control's storage key sits at depth four; 2.2.2/2.6 use list-valued fields at depth four | `defensive_rlp_depth_is_at_most_four_edges_below_the_root`: list, empty-string, single-byte and 32-byte leaves are accepted as the root and at depth four. They are refused at depth five (`transaction RLP nesting exceeds limit`), including when a later sibling follows an accepted depth-four branch. In `decode`: the signed `typed()` fixture's key is shown to sit at depth four and is accepted. A depth-four list in the key position passes framing, then the field-type rule refuses it (`transaction field must be bytes`). One more wrapper (depth five) is refused by the depth guard first. Legacy calldata shows the same split | Depth four never overrides envelope or access-list shape |
| Opaque byte-string contents not recursed into | 2.4 all-byte-value calldata; 2.2.2 zero-filled keys | `defensive_rlp_never_recurses_into_opaque_byte_strings` uses five payloads that are each refused when parsed as RLP: an eight-deep list, a 32-byte value that reads as 31 nested lists, a 1,025-item list, an overflowing header and a nonminimal single byte. Each is accepted as a byte string at depths zero to four, and each signed legacy payment carrying it as calldata decodes with exact bytes, hash and payer | — |
| Inclusive necessary limits; a refusal is an error and never an alternate envelope | Prior groups' no-effect refusal and original-recovery assertions | Every test asserts the exact `Err` (no panic), and every at-limit input is then held to a stricter named rule | No limit is relaxed to build a raw-ceiling fixture |

Mutation evidence: each mutation was applied to a backed-up `chain_client.rs` in the private APAS scratchpad, the six tests were run, and the original was restored and checked with `cmp`.

| Mutation | Failing new tests |
|---|---|
| Budget 16,384 → 16,385 | Both raw-budget tests |
| Typed prefix excluded from the budget | Both raw-budget tests |
| Budget checked after hex decoding (as decoded bytes) | `..._checked_before_hex_decoding` |
| Depth 4 → 5 | Depth |
| Depth 4 → 3 | Depth, item limit, opaque, declared length |
| Item limit 1,024 → 1,025 | Item limit, opaque |
| Item limit 1,024 → 1,023 | Item limit |
| Recursion into byte strings whose payload looks like a list | Opaque, depth (the signed `r`/`s` scalars then look like malformed lists) |
| Unchecked `header + length` | Declared length, opaque (debug "attempt to add with overflow" panic) |
| Exact list consumption removed | Declared length |

Two arithmetic mutations survive and are equivalent on 64-bit hosts. Wrapping header addition can never wrap onto the input length: a wrapped sum is below the header length, which is at most the input length. A wrapping length fold over at most eight bytes cannot overflow a 64-bit `usize`. The checked forms stay in place to avoid the debug panic and to protect narrower targets.

Adversarial review (2026-11-22) ran twelve further one-at-a-time mutations of `chain_client.rs` from its own backup, restoring and `cmp`-checking the original after each. Eleven fail a new test:

| Reviewer mutation | Failing new tests |
|---|---|
| Budget `>` → `>=` (16,384 bytes refused) | Both raw-budget tests |
| Budget measured on the whole string, including `0x` | Both raw-budget tests |
| Root decoded at depth one | Depth, item limit (and three older envelope tests) |
| Depth guard applied to lists only | Depth |
| Depth counted by list nesting only | Depth |
| Item guard at the root only | Item limit |
| Item guard after recursing into children | Item limit, through the review's added masked-breadth assertion; it survived before that assertion |
| Short length-header guard removed | Declared length (index panic) |
| Pre-decode bound loosened to twice the budget, exact budget after decoding | `..._checked_before_hex_decoding` |
| Item count silently truncated to 1,024 | Item limit (`invalid RLP list payload` instead) |
| Field count checked before canonical framing | Item limit (through `decode`) |

The twelfth, which moves the depth refusal into a depth-four list that has children, survives because it is equivalent: an item lies at depth five exactly when a depth-four list has a child.

`chain_rpc_api`'s separate `eth_sendRawTransaction` text bound uses the same constant but was not exercised here. Task 2.5 does not cover signature validity, live-chain acceptance, payment execution, allocation instrumentation or a 32-bit target. No code-versus-spec contradiction was found, so no production code changed.

## Integration

Run strict OpenSpec validation and the available README/CI checks. Keep the change open until acceptance tasks and evidence mapping are complete; archive normally with `openspec archive specify-payment-envelope-resource-bounds --yes`, never `--skip-specs`. Preserve all unrelated owner work and the whitepaper.
