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

## Integration

Run strict OpenSpec validation and the available README/CI checks. Keep the change open until acceptance tasks and evidence mapping are complete; archive normally with `openspec archive specify-payment-envelope-resource-bounds --yes`, never `--skip-specs`. Preserve all unrelated owner work and the whitepaper.
