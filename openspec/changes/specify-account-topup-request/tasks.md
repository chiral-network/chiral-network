## 1. Specify the missing request boundary

- [x] 1.1 Compare the read-only book, wire foundation and pending owner/opening/non-reserving requirements with the actual top-up decoder, native writer, authority order, verification/credit code and existing acceptance.
- [x] 1.2 Specify required object/string shape, literal account selection, owner-before-decoding behavior, payment/credit invariants and same-original recovery without fresh admission. Record source/evidence limits and keep response schemas separate.
- [x] 1.3 Observe actual typed decoding with a temporary executable and remove it. The complete positional array is accepted; missing/null/mistyped/unknown/duplicate members and a scalar root are rejected. This is not HTTP/payment evidence.
- [x] 1.4 Validate this specification change strictly and run available project checks; record actual results and external platform/release blockers without claiming implementation completion. Strict validation, 438 Rust tests (two pre-existing ignored), 15 frontend tests, 46 provisioning tests, headless/frontend builds, Svelte checking, CI Python compilation and the protected-book checksum pass. Implementation/acceptance tasks remain open.

## 2. Implement and prove the request contract

- [x] 2.1 Enforce object-only `TopupRequest` decoding through the existing direct map visitor and strict field decoder. Preserve the required string, empty-string semantics and unchanged native object serialization; no compatibility shim, extra allocation/`Value` round-trip or state conversion.
- [x] 2.2 Add missing production-router schema/authority regressions: complete positional array and malformed/missing/null/mistyped/unknown/raw-duplicate members; authenticated schema refusal consumes only its proof, while invalid binding does not. Preserve route validation, missing/wrong-owner account precedence, bearer refusal and exact-body/target substitution behavior. Observe no premature RPC or financial/credential/attempt mutation, with useful valid object controls.
- [x] 2.3 Complete source/test correspondence for active-paid versus free/frozen eligibility, supplied invalid/empty/zero-value evidence, account/opening commitment mismatch and native required-string serialization. Reuse existing safe-credit, once-only fee/credit, original-journal and lost-response tests; add only genuinely missing consumer-visible cases, preserving their evidence limits.
- [ ] 2.4 Complete top-up recovery correspondence under independent exhausted quote-work/committed-issuance and opening-capacity gates, retaining current full-quote-storage/readiness coverage. Preserve pending originals, credentials, held/spent money, safe verification and same-payment deduplication; reference the parent resource tasks without claiming this completes unrelated settlement/reorg/expiry work.
  - [x] 2.4.1 Prove provider HTTP top-up recovery with signing-work and committed-issuance budgets exhausted independently: real refusal controls, pending/unsafe/safe/reopened replay, unchanged original terms and credentials, retained prior spend/live holds and exact once-only credit/fee.
  - [ ] 2.4.2 Complete provider top-up correspondence at independent paid/free opening-capacity bounds without bypassing authority, eligibility or safe verification; retain the existing full-quote-storage coverage and disclose restored capacity prerequisites.
  - [ ] 2.4.3 Complete native same-original recovery under the independent quote budgets and applicable opening-capacity pressure. Inventory remaining positive-canonical/pending-amount edge acceptance; preserve prior journal/lost-response coverage rather than duplicating it.

## 3. Verify and integrate

- [ ] 3.1 Run relevant acceptance and available project checks after implementation. Fix failures at their cause without weakening checks, inventing a policy key or changing unrelated work.
- [ ] 3.2 Strictly validate and archive with `openspec archive specify-account-topup-request --yes` only after all implementation/acceptance tasks pass; never use `--skip-specs`.

No whitepaper edit, live provider/account operation, installation or deployment is a task in this change. Accounting-response schemas and broader chain/remittance/platform qualification remain separate work.
