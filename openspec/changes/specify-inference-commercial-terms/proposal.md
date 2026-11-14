## Why

The archived wire foundation specifies identities and signing bytes, but not the commercial payloads that consumers agree to fund. The whitepaper already defines signed offers, model/profile declarations, quote formation and immutable terms; these need requirements before downstream funding and accounting work.

## What Changes

- Add the complete allowed offer, model, serving-profile and quote field sets with typed limits and cross-field consistency rules.
- Separate remote declaration validation from stricter native deployment and qualification requirements; signatures are attribution, not execution or performance attestation.
- Specify bounded public quote proposal, exact copying of selected terms, free-versus-paid funding rules, consumer verification and preservation of existing signed agreements.
- Specify the whitepaper's input-only embedding deployment rule at the native configuration boundary as well as provisioning. Source inspection found that the provisioner enforces zero output price while native configuration currently validates only its canonical monetary syntax; leave a concrete regression/fix task open.
- Preserve current issuer-versus-receiver distinctions, including 300-second local quote issuance and the separate freshness checks used for new onboarding versus historical recovery. Do not invent a new wire version or reinterpret existing money.

## Capabilities

### New Capabilities

- `inference-commercial-terms`: Complete signed commercial declarations, their static admission constraints, quote formation and validation, and unchanged-agreement execution compatibility.

### Modified Capabilities

None. `inference-wire-foundation` remains the dependency for network identity, canonical money, encoding, signatures and commitments.

## Impact

- Read-only authority: `docs/chiral-book.md`, especially the signed schemas, hardware participation, immutable terms, authorization matrix, configuration, funding order, and API/model qualification sections.
- Corresponding code: `protocol.rs`, `offer_registry.rs`, `performance.rs`, `provider_config.rs`, `contracts.rs`, `consumer.rs`, `billing_store.rs`, `engine_runtime.rs`, and `scripts/provision-provider.py`.
- This turn changes specifications and APAS notes only. Follow-on work must preserve the extensive pre-existing dirty implementation and must not include others' changes in a commit.
- No deployment, account/server operation, database conversion, dependency installation or public-release approval. Discovery reconciliation, safe-chain settlement, request accounting and qualification measurement remain separate capabilities to specify.
