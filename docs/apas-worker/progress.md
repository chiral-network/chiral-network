# APAS progress

## 2026-11-15 — Require public proposal JSON objects

- Entry was clean at `7bc94311`. The retained complete whitepaper/report are unchanged by hash; both author Answers are already acted on, with no new decision needed. The read-only book retains SHA-256 `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`. Pending non-reserving deltas remain authoritative over historical wallet quotas in main specs.
- Completed commercial **2.4.3**: `ProposeRequest` uses the shared object-only visitor with strict derived fields. The public route still accepts exactly the three named strings without owner authentication; canonical funding, wallet identity, signing format, budget policy and stored terms are unchanged.
- Strengthened the existing production HTTP schema regression with a complete positional request; it failed before correction with 200 instead of 400. Invalid request/member/money cases must preserve the exact signing-work evidence and leave quote storage empty, followed by successful valid-object issuance with checked signed terms. The fixture's previous-minute work evidence is a restored prerequisite, not observed historical signing.

## Verification

- **496 passing tests:** 435 Rust across 20 suites (two pre-existing ignored), 15 frontend and 46 provisioning. Headless binary build, Svelte checking (zero errors/warnings), frontend build, CI Python compilation, strict commercial validation and protected-book checksum passed. Remaining commercial time/recovery tasks keep the change open; do not archive.
- Compiler warnings and stale Browserslist data remain. Native desktop/release builds are blocked by missing WebKit and the actual policy authority; external platform/hardware/deployment qualification remains outside this checkout. No installation, invented authority, deployment, live-chain or power-loss claim.

## Next steps, in order

1. Inspect remaining opening/top-up request and accounting-response object boundaries. [INFERENCE] Other derived structs may still admit positional arrays; compare actual decoding with the relevant requirements first, preserving each message's optional fields and allowed unsigned additions. Proposal, challenge and commercial guards do not establish every protocol object's contract.
2. Finish challenge-clock non-regression (**2.4–2.5**) and exact private time/fault acceptance: offer/qualification expiry (**commercial 2.1.3.2**), native receiver 600/601-second boundaries (**owner-control 2.2.3**), acknowledgement 30-second edges (**opening 2.7**) and start-state-only persistence failure (**opening 2.5.3**). Source ownership no longer blocks fixes. No sleeps, watcher races or changed accepted timestamps.
3. Complete accepted-paid-intent/elapsed-expiry recovery (**non-reserving parent 3.3/4.3**, opening **2.3.4.3/2.8.2**) and top-up/settlement paths under both exhausted quote budgets. Preserve completed opening **2.4** and existing free/paid reopening and admitted-hold settlement; neither is every native/HTTP recovery path.
4. Finish envelope private byte/item/depth acceptance (**2.5**) and mapping/archive, preserving fixed list/data rejection (**2.6**, `30ad8fd1`). The 14,448-byte profile ceiling precludes an otherwise-valid 16,384-byte fixture; never weaken limits.
5. Continue request-accounting credential expiry during a hold (**2.2.4**), invalid-usage/budget edges (**2.5**), receipt/snapshot/sequence failures (**2.3**), gateway failure (**3.1**), performance binding (**3.2**) and resource/recovery isolation (**3.3**). Preserve completed **2.2.1–2.2.3/2.4**; ledger assertions are not engine or live-chain evidence.
6. Continue safe-credit/reorg, cancellation/top-up/fees, discovery, credential lifecycle, runtime/performance and reputation alignment. Keep incomplete changes open. The report's named-payer quota entry needs remaining acceptance and the proposed author-only factual book update, not another policy answer.

## Checkout discipline

This step owns the public proposal decoder, its strengthened production HTTP regression, commercial implementation records, bug documentation and these notes. Preserve unrelated work and the whitepaper; no post-edit git audit, push, stash, reset, restore, installation or deployment. Commit only verified files with per-command `APAS Worker <apas-worker@localhost>`; never archive with `--skip-specs`.
