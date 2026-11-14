## Why

Reviewing acknowledged-funding recovery exposed a worker specification error in the still-open `specify-pending-proposal-quotas` change. Its definition/design retained a standalone proposal until expiry or account opening, but `BillingStore::register_funding_intent` already removes that proposal atomically when it durably records the funding acknowledgement and full agreement. Existing tests explicitly cover this removal and later agreement recovery.

The whitepaper requires accepted terms and sender/nonce commitments to survive expiry, not duplicate pending-proposal storage. Preserve the implementation's existing lifecycle and specify its missing transition rather than changing financial behavior to match the mistaken draft.

## What Changes

- Correct the open pending-quota delta, proposal and design: durable acknowledgement frees its matching pending-proposal slot; funding-intent admission capacity remains separate.
- Add an explicit atomic acknowledgement-transfer requirement, with failure, idempotent recovery and expiry scenarios.
- Correct the pending-quota expiry scenario so it no longer assumes acknowledgement normally leaves a standalone pending record behind.
- Keep first-opening acceptance tests pending; they must distinguish still-pending free proposals from already-acknowledged paid agreements.
- Add follow-on consumer-visible coverage tasks. No production code, schema, money, authentication or whitepaper change.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `inference-proposal-admission`: Add acknowledgement-driven slot transfer to the capability introduced by the open `specify-pending-proposal-quotas` change. This correction updates that unarchived definition directly and contributes a distinct ADDED requirement; it does not replace or duplicate an archived requirement.

## Impact

- Read-only authority: `docs/chiral-book.md`, durable pre-broadcast acknowledgements, immutable agreements and expiry-safe sender/nonce ownership.
- Source evidence: `billing_store.rs::register_funding_intent`, `funding_intent`, `quote`, `open_account` and pending cleanup.
- Existing evidence: `preexpiry_funding_intent_preserves_quote_through_settlement_stall_and_restart`, `equivalent_intents_reuse_ack_and_conflicting_sender_nonce_survives_quota_expiry`, and registration rejection tests.
- Both this change and the corrected pending-quota change remain open until their acceptance tasks and checks are complete. Their ADDED requirement names are distinct and must both be applied through normal archive; never skip spec integration.
