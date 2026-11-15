## Why

The whitepaper's paid-opening order is a financial safety contract, not merely a sequence of HTTP calls: preserve one signed original, obtain and save the provider's acknowledgement, then permit client broadcast. Its approved non-reserving-quote decision places paid capacity admission at that authenticated acknowledgement boundary, not at public quote issuance. Existing wire, commercial and owner-control requirements and the pending replacement proposal-admission contract supply the related terms and resource rules.

Production has separate consumer/journal and provider/ledger paths, with substantial isolated test evidence. Their guarantees must not be confused with real-chain qualification, an on-chain reservation or escrow. Registration previously checked quote expiry without rejecting a sample before issuance, even though the native consumer rejects such an acknowledgement. Task 2.4 adds that producer guard and deterministic interval/writer-wait regressions without weakening consumer validation.

## What Changes

- Define durable original-payment preparation and shared signing-wallet journal ownership before disclosure or client broadcast.
- Specify the funding-intent request and signed acknowledgement fields, payment/hash/quote binding, and native registration-time acceptance.
- Distinguish overbookable signed quotes and provider-wide standalone retention from authenticated paid admission; preserve signed-envelope, current-nonce, full-gas balance, qualified-execution, expiry and independent 10,000-active-intent gates.
- Require atomic final-capacity admission and acknowledgement persistence, permanent sender/nonce ownership, unchanged authorized replay and retention independent of admission leases. A definite full rejection creates no acceptance or submission by either an honest provider or the client; a lost reply remains ambiguous.
- Require acknowledgement verification and durable local storage before a durable broadcast-start marker and submission of the original bytes; preserve exact-operation recovery after ambiguous responses/restart.
- Preserve accepted-original recovery and safe-frontier credit eligibility despite later capacity pressure, expiry or restart. Keep free first-opening capacity authenticated with no fake payment, and accepted-account/top-up/settlement recovery independent of fresh quote gates.
- Track executable acceptance and missing fault/time cases without claiming full settlement or credential-lifecycle compliance. Earlier source-ownership blockers are historical now that the authorized native migration is committed.

## Capabilities

### New Capabilities

- `inference-opening-funding-intent`: The paid-opening pre-credit lifecycle from original preparation through durable provider acknowledgement, client broadcast gating and exact-original recovery.

### Modified Capabilities

None. Reuse the canonical wire foundation, pending commercial/owner-control contracts and `specify-non-reserving-quotes` replacement retention/admission contract with the aligned `specify-proposal-rate-budgets` committed-issuance rules. The canonical named-wallet quote quotas are superseded, not current authority: the native cutover removed both wallet gates and separately implemented provider-wide quote work and committed issuance. Their remaining acceptance keeps those changes open. Do not redefine signing, prices, authorization, credential rules or the independent quote budgets.

## Impact

- Read-only authority: `docs/chiral-book.md`, especially shared signing-wallet storage, agreements/exact funding order and settlement/recovery risks.
- Production correspondence: `protocol.rs`, `consumer.rs`, `chain_client.rs`, `contracts.rs` and `billing_store.rs`; existing unit and owner-control/proposal acceptance targets.
- Implementation includes the fresh-registration interval guard and object-only funding request, acknowledgement envelope/payload and nested network decoding, with real-router/native regressions. Valid object serialization and signatures remain unchanged; positional array aliases fail closed without a compatibility shim or state conversion. No monetary schema, deployment, wallet/account, network or whitepaper change. Cached acceptance retains its separate recovery rules.
- Full safe-credit/reorg accounting, explicit cancellation, top-ups, fee remittance and credential lifecycle remain separate work; this change preserves their recovery boundaries. The provider can technically broadcast disclosed signed bytes before acknowledging, but an honest provider must not broadcast a definitely refused original. This change cannot remove that trust boundary.
- The author answered the availability report and updated **Approved non-reserving quotes**. The report remains open for native implementation and acceptance; specification alignment and historical old-quota tests are not mitigation or release completion.
