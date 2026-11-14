## Context and scope

Authority is the unchanged `docs/chiral-book.md`, especially its authorization matrix and protected-state/recovery boundaries. The existing `specify-owner-control-authorization` change separates challenge occupancy (eight per wallet, 10,000 per ledger) from rate-window policy. This change supplies that omitted policy as a separate capability; it does not move or duplicate owner-control requirements or imply that its blocked tasks have passed.

`BillingStore::create_challenge` validates network/lifetime, then uses one immediate transaction for pending cleanup, the `challenge:global` 600 counter, the `challenge:<wallet>` 12 counter, occupancy checks and insertion. `contracts::control` canonicalizes the named wallet, samples time before the ledger write lock, and persists before disclosing the response. `consume_challenge` conditionally deletes the exact stored proof and never touches rate counters. The public handler maps `rate_limit` to HTTP 429.

`rate_limit` currently keeps a count only for `previous == now / 60`; otherwise it starts from zero. `cleanup_pending`, invoked by both quote and challenge admissions, deletes every older rate row. [INFERENCE] Older samples and later-time maintenance from another resource can consequently restore challenge allowance. This is a source-derived gap, not a new runtime exploit result.

## Decisions

### Preserve numerical limits and charge durable issuance

Keep 12 per canonical named wallet and 600 per provider ledger. Both count insertions, not proof verification or response delivery. Varying method, target account/query or body cannot create another wallet bucket. Separate ledgers and proposal/inference budgets remain independent. Input validation, rate/capacity refusal, duplicate nonce and transaction failure debit neither bucket; an undisclosed but committed challenge still costs one unit in each.

Consumption and expiry may free occupancy but cannot refund the rate charge. A subsequent business refusal cannot reverse the earlier successful issuance transaction. The policy does not define request-arrival throttling or bound all rejected-request work; those are different security/load gates.

### Reuse non-regressing minute selection with independent state

Use the same principle already specified for proposal rates: `W = floor(t / 60)` and `E = max(W, L)`, selected inside the existing SQLite write transaction. Here L belongs only to challenge issuance. Keep separate resource watermarks and counter namespaces; do not couple challenge clock advancement to a quote's timestamp or adopt a single ledger-wide rate clock as a shortcut.

Both counters and the candidate's challenge-minute advancement commit together with insertion. A failed candidate cannot advance L. A new minute grants only its normal budget; older samples consume remaining units in L or are refused. This accommodates reordered requests without granting extra allowance. A large forward clock jump can reduce fresh-proof availability after clock correction until time passes that committed minute. Do not conceal this tradeoff by deleting state or bypassing authorization.

This is not a clock for signed payloads, challenge expiry, quote/admission leases, money or chain observations. Keep the existing 120-second issuer/admission lifetime and native receiver plausibility distinction. The rate clock does not resurrect an expired proof.

### Protect each resource's retained evidence

Follow the proposal-rate design's existing ledger/metadata persistence pattern; no extra service, timer or unused standalone helper. Generic pending cleanup may reclaim expired pending records under their existing contracts, but must not discard or advance the currently relevant rate evidence of either resource. Prune obsolete challenge-rate rows only as part of a committed challenge-minute advance; quote evidence remains governed by proposal admission.

On first watermark adoption, examine only retained challenge-rate evidence, choose its greatest minute, and retain that minute's global and per-wallet spent units. With no watermark or retained challenge rows, the first committed admission establishes the baseline. Missing historical rows already discarded by old code cannot be certified or reconstructed.

Use conservative validation before issuing: malformed/negative/out-of-range windows or counts, a watermark inconsistent with retained current evidence, missing current global evidence despite spent wallet units, or disagreement between the current global count and the sum of current wallet counts must fail closed rather than grant guessed allowance. Older, superseded wallet rows are not current-minute charges. No metadata initialization, failed admission or fault repair may silently zero another resource's counters or mutate accepted money/credential state. This is admission bookkeeping within the current ledger schema, not conversion of old financial schemas.

### Keep authority distinct from issuance availability

Public challenge requests name a signer without proving ownership. A third party can spend that named wallet's issuance budget; successful consumption requires the real signer but does not refund those units. No authentication redesign, IP quota or reserved recovery allowance is introduced. This is consistent with the whitepaper's public endpoint and the existing nonblocking named-wallet availability report, not a guarantee that a consumer can always obtain a fresh proof.

An already-issued valid challenge remains usable subject to normal expiry/binding/business rules. Existing read-capable bearers and inference authorization do not acquire an issuance-rate gate. Owner mutations still need valid owner authority; exhaustion cannot justify a bearer fallback, new payment, automatic retry, implicit key rotation or deleting persistent identity/state.

## Existing evidence and gaps

|Evidence|What it establishes|What remains|
|---|---|---|
|`inference_owner_challenge_test` wallet-alias/public issuance cases|Canonical named-wallet attribution and native HTTP 429/`rate_limit` for pending exhaustion|Challenge rate edges independent of pending occupancy|
|`inference_owner_control_test` six challenge-capacity cases|Independent pending quote/challenge slots, exact-expiry/consumption release, pending-slot races, collision preservation and transactional insertion rollback|12/13 and 600/601 rate edges, final-rate-unit races and future-minute/watermark rollback|
|`inference_proposal_quota_test::{ledger_rate_budget_is_shared_across_terms_but_not_providers_or_control_budgets,payer_rate_scope_spans_terms_without_blocking_other_payers_or_control_budgets}`|Proposal exhaustion does not spend challenge allowance or invalidate accepted-account reservation/credentials|Reverse exhaustion/isolation and independent retained clocks|
|`inference_owner_authentication_test`|Real signature binding, conditional durable consumption, expiry and replay|Issuance-rate exhaustion while a valid proof remains available|
|Current limiter and shared cleanup source|Fixed-minute numerical limits and transactional counter placement|Durable non-regression, protected cleanup and retained-state adoption are not implemented|

`billing_store.rs` and its unit tests are owner-untracked work. The production clock/pruning/adoption fix cannot be committed by copying that source, introducing an unused helper or absorbing its surrounding migration. Leave the incompatible runtime scenarios blocked until the fix is independently committable. Isolated numerical and transactional acceptance can proceed in worker-owned integration tests when current behavior satisfies it.

## Acceptance approach

- Use real canonical provider/wallet identities, isolated disk-backed public ledger APIs and explicit admission times. Follow existing fixture/protected-state conventions; no live provider, chain, engine or real funds.
- Isolate the 12/13 wallet rate edge from eight pending slots by consuming properly authenticated challenges. Use distinct eligible wallets to isolate 600/601 from occupancy and wallet limits. Short-lived storage fixtures may exercise expiry without claiming a change to native issuance lifetime.
- Vary method, path/account/query and body; reuse existing native alias and HTTP error mapping evidence. Verify other wallets, independent ledgers and proposal/inference operations remain separate. Avoid sleeps and a 600-request HTTP timing race.
- Check normal minute edges, no accumulated skipped-window quota, storage reopening and no rate refund after consumption, expiry, undisclosed response or post-authentication refusal. Retain accepted-account/credential and funding evidence in conservation checks.
- Race independent connections for the last wallet rate unit and the last global rate unit, with pending slots available. Confirm the loser consumes neither bucket. Reuse existing collision/fault/pending-capacity evidence where sufficient; extend it to future-minute failure and the new watermark with the production fix.
- With that fix, exercise older and reordered samples, remaining/exhausted current allowance, restart, cross-resource cleanup in both directions, independent clock advancement, retained pre-watermark evidence and fail-closed corruption. Do not pin the old reset weakness as expected behavior or commit a failing regression alone.

## Verification and integration

Strict validation and the available README/CI checks are required. Passing current tests does not establish the new clock contract or public-load resilience. Implement shared cleanup consistently with `specify-proposal-rate-budgets`; this change neither resolves that change's blockers nor alters its requirements. Preserve native platform, actual release-policy key and live qualification gates.

Do not archive until all implementation/acceptance tasks pass. Use normal `openspec archive specify-challenge-rate-budgets --yes`, never `--skip-specs`. Owner-control freshness and HTTP root-recovery gaps, full payment recovery and broader credential lifecycle remain separate work.
