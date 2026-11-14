## 1. Specify the existing bounded profile

- [x] 1.1 Compare the whitepaper and current normative specs with the shared decoder; record uncovered access-list, calldata and RLP limits without widening or removing them.
- [x] 1.2 Define canonical widths, occurrence-based budgets/gas and financial rejection boundaries; record the conservative size proof and distinguish it from executable byte-boundary evidence.
- [x] 1.3 Run strict noninteractive OpenSpec validation and available project checks; record scope, blockers and next work without changing the whitepaper. All six open changes validated; all 416 existing tests and available type/build/script checks passed. No new executable acceptance is claimed by this specification step.

## 2. Complete executable acceptance

- [x] 2.1 Map each requirement/scenario to local decoder, native preparation, provider/relay verification and existing tests. Reuse opening-funding numeric/signature/binding/cost/nonce evidence; identify missing cases rather than duplicating it. The design records the initial caller/evidence map and distinguishes source reuse from end-to-end qualification.
- [x] 2.2 Add real-signature local and route acceptance for type-1/type-2 access-list structure and 64/65 address plus aggregate 256/257 key boundaries, including distributed and duplicate occurrences. Prove the otherwise-valid inclusive controls and persistent no-acknowledgement/no-credit/no-submission refusals. The five groups in 2.2.1–2.2.2 pass 44 isolated refusal/control pairs.
  - [x] 2.2.1 Exercise 64/65 address and aggregate 256/257 key boundaries for both typed envelopes, including distributed/duplicate occurrences and simultaneous valid caps. Require direct local rejection, real-router refusal without financial effects and subsequent exact valid-original acknowledgement. Two new groups pass twelve refusal/control pairs; all 418 available tests and available builds/checks pass.
  - [x] 2.2.2 Complete malformed access-list/entry/key-list shape and 19/21-byte address or 31/33-byte key rejection with correctly signed candidates and valid controls. Reuse existing empty-list and zero-filled opaque-byte positives; do not infer shape checks from budget failures. Three new groups pass 32 refusal/control pairs, including empty byte strings and zero-width values; all 421 available tests and available builds/checks pass.
- [x] 2.3 Cover full occurrence-based structural gas at equality and one below, including repeated entries/keys that a deduplicating implementation would undercharge; keep current nonce and sufficient full-cost balance so another gate does not mask the gas rule. Two new groups pass nineteen signed local/route refusal and exact-floor control pairs; all 423 available tests and available builds/checks pass. This does not qualify native execution gas.
- [ ] 2.4 Cover the local calldata 4,096/4,097-byte boundary with matching expected data and sufficient gas, separately from the opening route's exact commitment. Preserve existing tests and avoid fabricated payment execution.
- [ ] 2.5 Complete raw framing and defensive RLP byte/item/depth evidence, distinguishing reachable public cases from the derived size ceiling. Direct private decoder seam/unit work is BLOCKED where it requires adopting owner-untracked native source. Do not weaken other limits, expose test-only production APIs or commit a failing regression alone.
- [ ] 2.6 Confirm any discovered native mismatch only with an independently committable fix and regression. Preserve exact historical acknowledgement/original recovery, authorization and signing-wallet nonce ownership; no truncation, automatic replacement or migration.

## 3. Integrate the completed capability

- [ ] 3.1 Record the final requirement/scenario evidence map and honest remaining platform/live-chain limitations; run available project checks and strict validation.
- [ ] 3.2 Archive only after all tasks are complete with `openspec archive specify-payment-envelope-resource-bounds --yes`, never `--skip-specs`. Do not imply completion of the separate opening-funding or financial settlement changes.
