# APAS progress

## 2026-11-15 — Specify the shared account snapshot

- Entry was clean at `96a90823`. The previously fully read README-designated `docs/chiral-book.md` and report are unchanged; both existing Answers were already acted on. Book SHA-256: `f57f7ac5af27081616d3e241ac7bb8a326d8919d2edbc9b8919ad7fb8aaadf7e`. No new author-only decision or report entry is needed.
- Created OpenSpec change **`specify-inference-account-snapshot`** with proposal, design, tasks and a new capability delta. It specifies the 17-member account object, exact two-counter accounting usage, original agreement binding, wide monetary conservation and status/attribution limits. It retains `settlement_pending`, current reversal/loss/fee meanings and readable unfamiliar status strings without granting active authority.
- A temporary Rust decoder/signature executable observed that extra account members, a positional account and positional usage all decode to the original typed receipt and verify with its unchanged provider signature. The named-object control passed; an extra accounting `total_tokens` member was rejected. This is synthetic typed-wire evidence, not native HTTP, funding or engine evidence. The probe was removed.
- This is specification-only: production code, permanent tests and base specs are untouched. Closed account members and object-only account/usage decoding are planned implementation work, not claimed enforcement. Keep the change open; full receipt/request/performance/credential/open-result envelope schemas remain separate work.

## Verification and blockers

- Strict snapshot-change validation, 452 headless Rust tests across 20 suites (two existing ignored tests), headless binary build, Svelte checking (zero errors/warnings), 15 frontend tests, frontend build, 46 provisioning tests, CI Python compilation and protected book/report checks passed. Two existing Rust warnings and stale Browserslist data remain. Task **1.3** is complete; these baseline checks do not establish enforcement of the newly specified decoder contract.
- Native desktop/release prerequisites remain blocked by missing WebKit and the actual policy authority; external chain/engine/platform/hardware/public-release qualification remains outside this checkout. No installation, invented authority, live payment, deployment or whitepaper edit is authorized.

## Next steps, in order

1. Implement snapshot **2.1–2.2**: reuse the direct object visitor for `AccountView`/`TokenUsage`, close account members and prove raw/pre-parsed shape, duplicate/type, counter-range and unchanged canonical-signature behavior without defaults, aliases or financial-state migration.
2. Complete snapshot **2.3–3.4**: full original-term/money/state-preservation matrix, lifecycle reads, exact client/export integers, containing native replies and post-commit original-operation recovery. Reuse completed request-accounting **3.2.1** renewal receipt acceptance. Then specify remaining accounting-response envelopes before changing their uncovered semantics.
3. Finish challenge-clock non-regression (**2.4–2.5**) and exact private time/fault acceptance: offer/qualification expiry (**commercial 2.1.3.2**), native receiver 600/601 seconds (**owner-control 2.2.3**), acknowledgement 30-second edges (**opening 2.7**) and start-state-only persistence failure (**opening 2.5.3**). Source ownership no longer blocks fixes; no sleeps, watcher races or changed accepted timestamps.
4. Complete accepted-paid-intent/elapsed-expiry recovery (**non-reserving 3.3/4.3**, opening **2.3.4.3/2.8.2**) and top-up/settlement under both exhausted quote budgets. Finish envelope private byte/item/depth acceptance (**2.5**) and mapping/archive; preserve fixed list/data rejection (**2.6**, `30ad8fd1`). The 14,448-byte profile ceiling cannot supply an otherwise-valid 16,384-byte fixture; never weaken limits.
5. Continue request-accounting credential expiry during a hold (**2.2.4**), invalid-usage/budget edges (**2.5**), receipt/snapshot/sequence failures (**2.3**), gateway failure (**3.1**), remaining performance binding (**3.2**) and resource/recovery isolation (**3.3**). Snapshot and ledger tests are not engine, live-chain or power-loss evidence.
6. Continue safe-credit/reorg, cancellation/top-up/fees, discovery, credential lifecycle, runtime/performance and reputation alignment. Keep incomplete changes open. The report's named-payer quota entry needs remaining acceptance and the proposed author-only factual book update, not another policy answer.

## Checkout discipline

This turn owns only the five files under `openspec/changes/specify-inference-account-snapshot/` and these notes. Commit those six explicit files with per-command APAS identity; preserve unrelated work. The temporary example was deleted, and no build output, report, production source, base spec or whitepaper belongs in this commit.
