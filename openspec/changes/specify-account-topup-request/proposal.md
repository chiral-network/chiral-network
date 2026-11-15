## Why

The book requires owner-authorized, exact-original top-ups of an existing immutable paid agreement, without fresh opening admission or duplicate credit. The wire foundation defines the `CHRL2\0` commitment and pending owner/opening contracts define authority and recovery, but none defines the complete top-up request schema or its observable authorization/decoding order.

`TopupRequest` currently derives a strict member decoder yet also accepts a complete positional array. A temporary executable confirmed that typed behavior; it did not submit a payment or establish an HTTP exploit. Specify the missing boundary before changing its implementation.

## What Changes

- Define `POST /v1/contracts/:id/topup` as one named-field object with required string `raw_transaction`, no nullable/omitted payment variant, no extra fields and no positional aliases.
- Preserve current route ordering: validate the route identifier, authenticate the exact raw request and establish account ownership, then decode the top-up body. Unlike opening, a correctly authenticated malformed top-up consumes its proof.
- Preserve active-paid-account eligibility, independently verified positive payment evidence, exact account commitment, atomic checked credit/fee accounting and transaction-identity deduplication.
- Preserve same-original native preparation/recovery, unchanged agreement/credential authority and settlement without fresh quote/opening/readiness gates. Reuse existing safe-credit and native lost-response evidence rather than claiming it from a decoder probe.

## Capabilities

### New Capabilities

- `inference-account-topup-request`: strict top-up request shape, authority ordering and existing-account settlement/recovery boundaries.

### Modified Capabilities

None. Existing wire, owner-control, non-reserving and opening-intent requirements remain in force; this change supplies their missing request-level contract.

## Impact

- Planned implementation: `src-tauri/src/protocol.rs::TopupRequest`, using the existing direct object visitor without changing serialization or adding a compatibility shim.
- Acceptance: production owner-control routes and existing ledger/native top-up cases, with explicit proof consumption, financial-state and RPC observations.
- This specification step changes no runtime or permanent tests. Account/receipt/other accounting-response schemas, complete reorg/cancellation/remittance qualification, deployments and whitepaper edits are outside scope.
