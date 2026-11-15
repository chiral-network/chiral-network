# APAS progress

## 2026-11-15 — Complete opening-request recovery acceptance

- Entry was clean at `34951257`. The fully read whitepaper and author report retain their recorded hashes; both Answers are already acted on and no new author decision is needed. The read-only book retains SHA-256 `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`. Pending non-reserving deltas remain authoritative over historical wallet quotas in main specs.
- Selected **specify-account-opening-request 2.4** and its final verification/integration. Actual runtime already distinguishes expired first free opening from accepted-account recovery and bypasses fresh admission on exact-original paid replay. This is missing implementation-layer acceptance, not a runtime policy change or newly reproduced production defect.
- Extended existing recovery acceptance with signed historical free terms, exact expiry-minus-one/at/plus-one ledger cases, authenticated refusal of a pruned unopened quote and successful current free HTTP opening. Reopened-provider HTTP recovery now uses an expired accepted free agreement while retaining existing spend/hold/credential/quote-budget checks. Extended both paid-capacity cases with direct exact-original POST replay and no additional RPC, duplicate credit or credential/journal mutation. Historical state/capacity and synthetic chain observations remain disclosed prerequisites, not live issuance, elapsed-clock or release evidence.

## Verification

- **499 tests passed:** all 438 Rust tests (two pre-existing ignored), 15 frontend and 46 provisioning tests. Headless/frontend builds, Svelte checking (zero errors/warnings), CI Python compilation, strict change validation and protected-book checksum passed. The initial Rust compilation stopped on a new-fixture borrow spanning mutable HTTP calls; reacquiring the ledger afterward fixed it without cloning state or weakening assertions, and the full Rust rerun passed. All tasks are complete; `openspec archive specify-account-opening-request --yes` integrated four requirements into `openspec/specs/inference-account-opening-request/spec.md` and archived the change as `2026-11-15-specify-account-opening-request`. Replaced only the generated purpose placeholder with the actual contract scope.
- Compiler warnings and stale Browserslist data remain. Native desktop/release builds are blocked by missing WebKit and the actual policy authority; external platform/hardware/deployment qualification remains outside this checkout. No installation, invented authority, deployment, live-chain or power-loss claim.

## Next steps, in order

1. Inspect top-up and accounting-response schemas against the book and actual callers, without assuming identical optional/unsigned-extension policies. Preserve the completed opening-request contract and its bounded evidence; archival does not complete its parent lifecycle changes.
2. Finish challenge-clock non-regression (**2.4–2.5**) and exact private time/fault acceptance: offer/qualification expiry (**commercial 2.1.3.2**), native receiver 600/601-second boundaries (**owner-control 2.2.3**), acknowledgement 30-second edges (**opening 2.7**) and start-state-only persistence failure (**opening 2.5.3**). Source ownership no longer blocks fixes. No sleeps, watcher races or changed accepted timestamps.
3. Complete accepted-paid-intent/elapsed-expiry recovery (**non-reserving parent 3.3/4.3**, opening **2.3.4.3/2.8.2**) and top-up/settlement paths under both exhausted quote budgets. Preserve completed opening **2.4** and existing free/paid reopening and admitted-hold settlement; neither is every native/HTTP recovery path.
4. Finish envelope private byte/item/depth acceptance (**2.5**) and mapping/archive, preserving fixed list/data rejection (**2.6**, `30ad8fd1`). The 14,448-byte profile ceiling precludes an otherwise-valid 16,384-byte fixture; never weaken limits.
5. Continue request-accounting credential expiry during a hold (**2.2.4**), invalid-usage/budget edges (**2.5**), receipt/snapshot/sequence failures (**2.3**), gateway failure (**3.1**), performance binding (**3.2**) and resource/recovery isolation (**3.3**). Preserve completed **2.2.1–2.2.3/2.4**; ledger assertions are not engine or live-chain evidence.
6. Continue safe-credit/reorg, cancellation/top-up/fees, discovery, credential lifecycle, runtime/performance and reputation alignment. Keep incomplete changes open. The report's named-payer quota entry needs remaining acceptance and the proposed author-only factual book update, not another policy answer.

## Checkout discipline

This step owns only the owner-control regression, opening-request design/tasks/archive and generated main spec, and these notes. Runtime source, the author report and the whitepaper are unchanged. Preserve unrelated work; no post-edit git audit, push, stash, reset, restore, installation or deployment. Commit only verified files with per-command `APAS Worker <apas-worker@localhost>`.
