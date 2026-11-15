## Why

The whitepaper defines authenticated free/paid opening, exact acknowledged originals and credential-preserving recovery. Existing changes specify owner proof and funding-intent admission, but do not define the opening request's complete wire shape or distinguish its nullable syntax from payment eligibility. That boundary must be specified before changing `OpenRequest` or extending its acceptance tests.

## What Changes

- Specify a named-field JSON opening request with required string `quote_id` and optional nullable string `raw_transaction`; reject positional aliases and unknown, duplicate or mistyped members.
- Preserve omitted/null payment syntax while requiring the retained agreement, not omission, to select free versus paid rules. Free openings reject supplied transaction strings; paid openings and replays require their exact acknowledged original.
- Preserve parsing before owner-proof consumption, followed by exact-body authorization, quoted ownership and business checks. A later refusal consumes successfully authenticated authority without implying account creation or payment failure.
- Keep existing-account recovery independent of fresh readiness/capacity gates and credential rotation, without relaxing first-credit chain verification.
- Record source/test correspondence and explicit decoder, HTTP and recovery acceptance tasks. Implement the decoder/schema slice without treating it as completion of the broader payment-presence and recovery inventory.

## Capabilities

### New Capabilities

- `inference-account-opening-request`: Opening request syntax, exact-body authorization ordering and agreement-derived payment presence rules for first opening and replay.

### Modified Capabilities

None. Reuse `inference-wire-foundation` and the pending owner-control, commercial, funding-intent and non-reserving contracts without changing their requirements or archiving their unfinished work.

## Impact

- Read-only authority: `docs/chiral-book.md`, authorization matrix, request/account integration, exact funding order and approved non-reserving quotes.
- Implementation correspondence: `protocol::OpenRequest`, `contracts::control`, `BillingStore::check_open_funding`, native `Consumer::resume_inner` and existing owner-control/recovery tests.
- Reject positional requests at the shared decoder while retaining object serialization, optional-field semantics and existing financial/authentication guards. Production-router regressions cover malformed bodies and successful free opening/paid replay controls. No compatibility shim or persisted-state conversion.
- No whitepaper edit, top-up/request-accounting schema expansion, new credential lifecycle, deployment, live payment, installation or public-release qualification.
