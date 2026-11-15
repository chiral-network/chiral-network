## Why

The whitepaper's funding order depends on proving the payer's authority over one exact control request. `inference-wire-foundation` specifies the signing domain, while `inference-proposal-admission` distinguishes public proposals from owner authority, but neither defines challenge issuance, client verification, one-use consumption, or the account authorization matrix. Specify this prerequisite before opening, funding-intent acknowledgement, and original-transaction recovery.

## What Changes

- Specify the public challenge request, canonical wallet/body digest, provider-generated nonce, complete request-bound payload, and 120-second issuer lifetime.
- Specify native reconstruction and verification before signing, separately from the provider's issuance lifetime.
- Require durable, atomic one-use consumption after signature and binding checks, including concurrent and restarted processes.
- Define which account reads accept an account-scoped read bearer and which operations require the quote/account owner's proof; public proposals and signed payments do not substitute for that proof.
- Preserve bounded pending challenges and distinguish proof consumption from successful completion of a financial or credential operation.

## Capabilities

### New Capabilities

- `inference-owner-control`: Request-bound owner challenges and authorization of account control operations.

### Modified Capabilities

None. Canonical encoding and signatures remain in `inference-wire-foundation`; proposal admission and commercial terms retain their existing requirements.

## Impact

- Read-only authority: `docs/chiral-book.md`, owner-challenge exchange, authorization matrix, control API limits, and funding order.
- Implementation correspondence: `auth.rs`, `protocol.rs`, `contracts.rs`, `billing_store.rs`, `consumer.rs`, and their existing tests.
- Acceptance includes the native/ledger correction for authenticated opening retries: return the existing account without issuing or revoking a credential, and allow explicit native credential recovery after a lost first-opening reply. The unsigned opening result has a nullable credential; first creation still returns its newly issued secret.
- Challenge issuance rate-window policy, complete credential lifecycle, payment admission/settlement, and recovery of original signed transactions remain separate work. Existing behavior is retained, not removed because this slice does not specify it.
- No whitepaper edit, deployment, live account operation, installation, migration of money, or public-release qualification.
