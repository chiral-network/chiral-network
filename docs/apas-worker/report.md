Answer an entry with a line starting `Answer:` under that entry. The worker never edits the whitepaper.

## 2026-11-14 — Public proposals can exhaust a named payer's quota

Status: open

Where: `docs/chiral-book.md`, authorization matrix row for `POST /v1/contracts/propose` (currently line 270).

Current text: “Public proposal for an unexpired local offer, subject to provider funding/admission rules. **This endpoint is not owner-authenticated.** The resulting quote binds its named consumer.”

Evidence: `contracts.rs::control` accepts the caller's canonical named payer without owner proof, and `BillingStore::store_quote` limits that payer to eight pending quotes. The isolated loopback regression `public_pending_quota_follows_the_canonical_named_payer_not_caller_authority` now demonstrates eight unauthenticated proposals for one payer, shared capacity across address-case aliases, HTTP 429 / `rate_limit` on subsequent proposals and continued admission for another payer. Issued signed agreements remain intact, with no account creation or transaction submission.

A caller can therefore exhaust another payer's fresh-quote allowance without paying or owning the wallet. Replenishing expired proposals to sustain that obstruction remains a source-derived risk, not a tested live-deployment claim. This does not grant access to an account, credentials or funds; existing owner-authorization tests continue to pass. No live attack was performed.

Proposed addition after the authorization matrix:

> Public quote issuance does not prove control of the named payer. A caller can consume that payer's pending-proposal allowance and repeatedly obstruct new quote issuance while keeping the allowance full. These quotas are availability controls, not payer ownership proofs or Sybil resistance. Quota expiry must not erase acknowledged funding terms or settled accounts. Public-rollout security and load review must account for this availability exposure.

Why: the current public-proposal design and its per-payer bounds have a concrete availability tradeoff that should not be mistaken for authenticated per-caller protection.

Author action requested: add an equivalent disclosure, or specify a different public-proposal design if this exposure is unacceptable. Only the author can change that whitepaper boundary. Acceptance of this proposed text does not authorize the worker to edit it; the entry remains open until the whitepaper reflects the decision.

Nonblocking for specification/testing of the current pending-quota contract. No authentication change, mitigation, live attack or public rollout is performed or claimed here.
