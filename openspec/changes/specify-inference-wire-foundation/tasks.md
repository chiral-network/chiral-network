## 1. Specify the foundation

- [x] 1.1 Compare the whitepaper's primitive contracts with `protocol.rs`, `network.rs`, `wallet.rs`, offer validation and the control-domain adapter; record actual coverage and exclusions in the design.
- [x] 1.2 Write the proposal, design and `inference-wire-foundation` delta without editing the whitepaper or existing implementation.
- [x] 1.3 Validate this change with `openspec validate specify-inference-wire-foundation --strict --no-interactive`.

## 2. Prove the contract

- [x] 2.1 Add missing deterministic network/primitive-schema/canonical-wei boundary tests, reusing existing coverage and preserving unrelated uncommitted work.
- [ ] 2.2 Add independent known-answer canonical JSON, UTF-8 framing, personal-signature and Keccak commitment vectors plus domain/signer/encoding rejection and changed-term cases; avoid self-referential expected values.
- [ ] 2.3 Run the relevant Rust contract tests and the available project checks; resolve only failures belonging to this change, record unrelated/environmental blockers, and do not weaken checks.

## 3. Integrate the verified specification

- [ ] 3.1 Revalidate the completed change strictly and archive with `openspec archive specify-inference-wire-foundation --yes`, incorporating the delta into `openspec/specs` without `--skip-specs`.
