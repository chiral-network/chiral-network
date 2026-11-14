## 1. Specification

- [x] 1.1 Read the whitepaper and compare existing wire/proposal/commercial requirements with the owner-authentication, ledger, native consumer, route, and test sources.
- [x] 1.2 Specify public challenge fields, bound payload, issuer/native receiver distinctions, pending capacity, exact one-use consumption, account authority, and retry semantics without changing the whitepaper or runtime.
- [x] 1.3 Pass strict validation and the available project checks; record existing environmental and ownership limits in APAS progress.

## 2. Implementation acceptance

Reuse existing tests and source evidence first. Add deterministic regression coverage only for uncovered consumer-visible behavior; any required source fix must preserve unrelated owner changes. Leave a task blocked if its fix requires absorbing an owner-untracked implementation file. Do not use a live provider or real funds.

- [x] 2.1 Verify public HTTP challenge issuance: strict request schema, canonical wallet/digest aliases, supported methods, path byte/control-character limits, complete provider/network binding, durable response, and absence of account authority. Cover missing boundary cases.
- [ ] 2.2 Verify native reconstruction before signing and exact outgoing body/target preservation, including malformed payload hex, substitutions of every bound identity/operation field, nonce length, and exact receiver freshness boundaries. Distinguish existing path-only native calls from query-bearing HTTP authorization.
  - [x] 2.2.1 Exercise public native calls against the production router: reject substitutions of every signing-context field, domain and signed-field set; malformed encodings/response fields; empty/oversized nonces and timeless invalid expiry values before transmitting owner proof. Verify real signatures and actual POST/GET/DELETE bytes on successful calls, including harmless unsigned additions.
  - [ ] 2.2.2 Cover the remaining exact nonce byte-length acceptance boundary and multibyte cases; distinguish native receiver plausibility from the provider's fixed hexadecimal issuer nonce and header limits.
  - [ ] 2.2.3 Cover exact receiver expiry boundaries at current time and current time plus 600 seconds with an explicit-time seam. Blocked while the private helper and its unit-test module remain in owner-untracked `consumer.rs`; do not use sleeps or loosen bounds. Current native callers construct path-only URLs; query-bearing provider authorization remains task 2.4.
- [ ] 2.3 Verify valid signatures, rejected bindings/signatures without consumption, expired/unknown nonces, conditional complete-payload consumption, concurrent one-winner behavior, unconsumed restart recovery, and consumed replay after restart. Reuse the existing real-signature and two-connection cases.
- [ ] 2.4 Verify the quote/account ownership and bearer read matrix over actual control routes: free/paid opening, funding intent, top-up, reads, credential mutations, foreign-account or insufficient-scope bearers, explicit invalid-owner plus valid-bearer refusal, empty GET bodies, and bound cursor queries. Prove a post-authentication business refusal consumes authority without implying business success.
- [ ] 2.5 Verify per-wallet and global pending challenge caps independently of quotes, expiry/consumption slot release, concurrent issuance, collision refusal, and transactional failure without partial insertion or rate debit. Preserve acknowledged funding and unrelated account state.

## 3. Completion

- [ ] 3.1 Record the requirement-to-source/test acceptance correspondence and any resulting operator/native usage documentation changes; leave full payment recovery, credential lifecycle, and challenge rate-window policy to their explicit follow-on slices.
- [ ] 3.2 Run the project checks and strict change validation after acceptance work, then archive with `openspec archive specify-owner-control-authorization --yes` only when every task is complete; never use `--skip-specs`.
