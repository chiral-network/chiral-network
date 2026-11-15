## Why

The whitepaper requires exact native-CFX amounts, canonical signed payment evidence and preservation of the original transaction. The opening-funding specification covers the 16 KiB raw ceiling and fresh spendability, but the shared decoder additionally enforces unmodeled structural limits: 4 KiB calldata, bounded RLP nesting/items, 64 access-list entries and 256 total storage-key occurrences. These limits affect which correctly signed transactions are supported; they must be explicit rather than silently removed or mistaken for chain consensus rules.

The combined limits also make an otherwise-valid 16 KiB opening impossible under the current profile. An oversized malformed fixture cannot demonstrate the raw-size check independently of those stricter checks.

## What Changes

- Specify the existing application-level payment envelope resource profile, including canonical numeric widths and opaque byte-string widths.
- Define access-list aggregation and duplicate counting, inclusive bounds and the structural gas floor.
- Distinguish necessary resource limits from a promise to accept every transaction below them, execution estimates, fresh admission or safe settlement.
- Plan real-signature acceptance for reachable boundaries and honest evidence for redundant defensive limits without weakening guards or exposing production test-only APIs.

## Capabilities

### New Capabilities

- `inference-payment-envelope-bounds`: The bounded local signed-payment envelope profile shared by native payment validation and provider/relay evidence verification.

### Modified Capabilities

None. Complement `inference-wire-foundation` and the pending `inference-opening-funding-intent` contract without changing authorization, acknowledgement replay, funding economics or settlement policy.

## Impact

- Read-only authority: `docs/chiral-book.md`, particularly canonical `u128` monetary values, exact-original funding order and signed-envelope settlement evidence.
- Source correspondence: `chain_client.rs` local decoding, canonical RLP, access-list validation, structural gas and payment construction; existing wallet/chain tests and `inference_funding_admission_test` provide partial evidence.
- Runtime scope: correct the shared decoder's byte-string field-type check and add signed local/HTTP rejection-and-original-recovery regressions. No ledger schema, wallet operation, server deployment or whitepaper change; no limits are widened and no historical evidence is discarded.
- Direct defensive byte/item/depth acceptance remains open. The native migration is now committed, so the former source-ownership blocker no longer applies; use private unit boundaries rather than a test-only public API.
- No author-only decision is needed. The named-payer availability report remains open and nonblocking.
