> **2026-11-25 scope amendment:** The Strata launch selection adds the exact curated `engine:strata` profile described by `strata-curated-runtime`; llama.cpp/GGUF-specific constraints remain legacy-only. Common immutable agreement, accounting, signature, profile identity and evidence-renewal requirements remain unchanged. Changed engines cannot reuse evidence or silently migrate agreements. The author explicitly authorized revising the book; earlier read-only/book-line references record the prior baseline. See `adopt-strata-curated-launch` for supersession and current verification gates.

## Why

The archived wire foundation specifies identities and signing bytes, but not the commercial payloads that consumers agree to fund. The whitepaper already defines signed offers, model/profile declarations, quote formation and immutable terms; these need requirements before downstream funding and accounting work.

## What Changes

- Add the complete allowed offer, model, serving-profile and quote field sets with typed limits and cross-field consistency rules.
- Separate remote declaration validation from stricter native deployment and qualification requirements; signatures are attribution, not execution or performance attestation.
- Specify provider-wide bounded public quote proposals as exact signed, overbookable terms, not opening-capacity reservations or named-wallet allowances. Paid admission belongs at authenticated funding-intent; free admission belongs at authenticated first opening.
- Enforce the whitepaper's input-only embedding deployment rule at the shared native configuration boundary, independently of provisioning and managed-runtime selection. Reject nonzero or noncanonical output rates rather than normalizing them; preserve generation pricing and existing accepted financial state.
- Enforce template-free managed embeddings using the provisioner's explicit null selection and absent selected-template file contract. Preserve complete inventory/digest checks and generation-template handling rather than discarding forbidden template metadata.
- Require named-field objects for offers, quotes, models, serving profiles, device and qualification identities at their shared native decoder boundaries, preserving exact field checks and valid signed representations. Keep control/accounting message acceptance separate rather than claiming commercial guards complete every protocol object.
- Enforce the public proposal's named-field request object through the shared decoder, before signing-work admission or quote insertion. Preserve its public authorization model, strict members and canonical funding; do not normalize positional aliases.
- Preserve current issuer-versus-receiver distinctions, including 300-second local quote issuance and the separate freshness checks used for new onboarding versus historical recovery. Do not invent a new wire version or reinterpret existing money.
- Align pending cutover with the authoritative `specify-non-reserving-quotes` replacement and `specify-proposal-rate-budgets` committed-issuance contract: separate retained quotes, pre-signing work and committed issuance from authenticated opening capacity, without new gates on accepted-account, top-up or settlement recovery.

## Capabilities

### New Capabilities

- `inference-commercial-terms`: Complete signed commercial declarations, their static admission constraints, quote formation and validation, and unchanged-agreement execution compatibility.

### Modified Capabilities

None. `inference-wire-foundation` remains the dependency for network identity, canonical money, encoding, signatures and commitments.

## Impact

- Read-only authority: `docs/chiral-book.md`, especially the signed schemas, hardware participation, immutable terms, authorization matrix, configuration, funding order, and API/model qualification sections.
- Corresponding code: `protocol.rs`, `offer_registry.rs`, `performance.rs`, `provider_config.rs`, `contracts.rs`, `consumer.rs`, `billing_store.rs`, `engine_runtime.rs`, and `scripts/provision-provider.py`.
- Implementation now includes shared advertised-origin, native embedding-price, managed embedding-template, commercial object-boundary and public proposal request corrections, alongside the acceptance tests. The original source-ownership blocker was resolved by the author-approved native migration; preserve any unrelated future edits and commit only this change's files.
- No deployment, account/server operation, database conversion, dependency installation or public-release approval. Discovery reconciliation, safe-chain settlement, request accounting and qualification measurement remain separate capabilities to specify.
