# APAS progress

## 2026-11-15 — Require complete commercial JSON objects

- Entry was clean at `c80ef299`. The complete whitepaper/report retained from the previous read are unchanged by hash; both author Answers are already acted on. The read-only book retains SHA-256 `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`. No new author decision.
- Completed commercial **2.1.6**: shared object-only decoding for `ModelSpec`, `ServingProfile`, `InferenceOffer` and `Quote`. Retain required/nullable fields, raw duplicate/unknown-field checks, serialization and term comparisons without a `Value` buffer or state migration.
- All four new regressions failed before the fix. Three reproduced acceptance through actual native `Consumer::quote` HTTP requests; the fourth reproduced signed-offer byte decoding. They require raw/preparsed array rejection, raw duplicate-member refusal for every field, and usable original objects with preserved signatures, offer identity and opening commitment. Fixtures are isolated; no production issuance, chain payment or hardware qualification is claimed.

## Verification

- **496 passing tests:** 435 Rust across 20 suites (two pre-existing ignored), 15 frontend and 46 provisioning. Headless binary build, Svelte checking (zero errors/warnings), frontend build, CI Python compilation, strict commercial-change validation and protected-book checksum passed. No check or assertion was weakened. Remaining time and proposal/recovery acceptance keep the commercial change open.
- Compiler warnings and stale Browserslist data remain. Native desktop/release builds are blocked by missing WebKit and the actual policy authority; external platform/hardware/deployment qualification remains outside this checkout. No installation, invented authority, deployment, live-chain or power-loss claim.

## Next steps, in order

1. Inspect remaining specified control-request and accounting-response object boundaries. [INFERENCE] Other derived structs may still admit positional arrays; compare actual decoding with the relevant requirements before changing them. The current step covers commercial declarations, not every protocol object. Preserve commercial **2.1.4–2.1.6/2.2/2.3** and the existing price/envelope/network/funding guards.
2. Finish challenge-clock non-regression (**2.4–2.5**) and exact private time/fault acceptance: offer/qualification expiry (**commercial 2.1.3.2**), native receiver 600/601-second boundaries (**owner-control 2.2.3**), acknowledgement 30-second edges (**opening 2.7**) and start-state-only persistence failure (**opening 2.5.3**). Source ownership no longer blocks fixes. No sleeps, watcher races or changed accepted timestamps.
3. Complete accepted-paid-intent/elapsed-expiry recovery (**non-reserving parent 3.3/4.3**, opening **2.3.4.3/2.8.2**) and top-up/settlement paths under both exhausted quote budgets. Preserve completed opening **2.4** and existing free/paid reopening and admitted-hold settlement; neither is every native/HTTP recovery path.
4. Finish envelope private byte/item/depth acceptance (**2.5**) and mapping/archive, preserving fixed list/data rejection (**2.6**, `30ad8fd1`). The 14,448-byte profile ceiling precludes an otherwise-valid 16,384-byte fixture; never weaken limits.
5. Continue request-accounting credential expiry during a hold (**2.2.4**), invalid-usage/budget edges (**2.5**), receipt/snapshot/sequence failures (**2.3**), gateway failure (**3.1**), performance binding (**3.2**) and resource/recovery isolation (**3.3**). Preserve completed **2.2.1–2.2.3/2.4**; ledger assertions are not engine or live-chain evidence.
6. Continue safe-credit/reorg, cancellation/top-up/fees, discovery, credential lifecycle, runtime/performance and reputation alignment. Keep incomplete changes open. The report's named-payer quota entry needs remaining acceptance and the proposed author-only factual book update, not another policy answer.

## Checkout discipline

This step owns the four commercial object decoders, their native quote/offer regressions, commercial-change implementation records, bug documentation and these notes. Preserve unrelated work and the whitepaper; no post-edit git audit, push, stash, reset, restore, installation or deployment. Commit only verified files with per-command `APAS Worker <apas-worker@localhost>`; never archive with `--skip-specs`.
