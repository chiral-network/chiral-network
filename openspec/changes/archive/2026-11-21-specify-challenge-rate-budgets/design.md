## Context and scope

Authority is the unchanged `docs/chiral-book.md`, especially its authorization matrix and protected-state/recovery boundaries. The existing `specify-owner-control-authorization` change separates challenge occupancy (eight per wallet, 10,000 per ledger) from rate-window policy. This change supplies that omitted policy as a separate capability; it does not move or duplicate owner-control requirements or imply that its blocked tasks have passed.

`BillingStore::create_challenge` validates network/lifetime, then uses one immediate transaction for pending cleanup, `challenge_rate_limit` (effective-minute selection, the `challenge:global` 600 counter, the `challenge:<wallet>` 12 counter and the `challenge-minute` watermark), occupancy checks and insertion. `contracts::control` canonicalizes the named wallet, samples time before the ledger write lock, and persists before disclosing the response. `consume_challenge` conditionally deletes the exact stored proof and never touches rate counters. The public handler maps `rate_limit` to HTTP 429.

Before task 2.4, the shared `rate_limit` kept a count only for `previous == now / 60` and otherwise started from zero, and `cleanup_pending`, invoked by both quote and challenge admissions, deleted every older non-quote rate row. Older samples and later-time maintenance from another resource could therefore restore challenge allowance; the task 2.5 regressions reproduce this against that code. See [code difference 1](#code-differences).

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

## Requirement-to-source/test acceptance map

This index covers the three requirements and thirteen scenarios in the [challenge-rate delta](specs/inference-challenge-rate-budgets/spec.md). Test targets are in [`src-tauri/tests`](../../../../src-tauri/tests). It records acceptance evidence for the current ledger, including the task 2.4 clock fix, not new requirements.

### Committed challenge issuance consumes scoped minute budgets

For this [requirement](specs/inference-challenge-rate-budgets/spec.md#requirement-committed-challenge-issuance-consumes-scoped-minute-budgets), [`contracts::control`](../../../../src-tauri/src/contracts.rs) canonicalizes the public request before [`BillingStore::create_challenge`](../../../../src-tauri/src/billing_store.rs) charges the wallet and ledger buckets. The numeric tests use canonical identities and explicit-time storage/authentication APIs; they do not send 600 HTTP requests within a real minute.

|Scenario|Executable evidence|Boundary|
|---|---|---|
|Twelfth wallet issuance is distinct from pending occupancy|`inference_owner_authentication_test::wallet_challenge_rate_spans_operations_and_survives_consumption_and_reopening`; `inference_owner_challenge_test::wallet_hex_aliases_share_the_same_pending_challenge_allowance`|Real signatures consume earlier proofs so eight pending slots cannot mask 12/13. Operation/account/query/body changes share the wallet bucket. Native alias normalization is separate HTTP evidence, not an HTTP rate-edge test.|
|Another wallet retains its individual allowance|`inference_proposal_quota_test::challenge_wallet_rate_preserves_other_wallets_and_independent_control_authority`|A different wallet is admitted after the first exhausts its rate. Proposal allowance, a read credential, an existing inference reservation and an independent provider ledger remain separate.|
|Ledger rate boundary applies across wallets|`inference_proposal_quota_test::challenge_ledger_rate_is_independent_of_proposals_providers_and_existing_accounts`|600 actual insertions across eligible wallets; the next candidate is refused without wallet/occupancy exhaustion. Reopening preserves evidence. Another provider ledger and the full remaining proposal allowance are independent.|

The reverse direction is covered by `inference_proposal_quota_test::ledger_rate_budget_is_shared_across_terms_but_not_providers_or_control_budgets` and `inference_proposal_quota_test::payer_rate_scope_spans_terms_without_blocking_other_payers_or_control_budgets`: proposal exhaustion leaves challenge admission and existing credential/reservation authority available. These are same-minute resource-isolation checks, not evidence of independent durable rate clocks.

### Effective challenge minutes never regress or share another resource's clock

For this [requirement](specs/inference-challenge-rate-budgets/spec.md#requirement-effective-challenge-minutes-never-regress-or-share-another-resources-clock), the production paths are `challenge_rate_limit`, `cleanup_pending` and `BillingStore::transaction` in [`billing_store.rs`](../../../../src-tauri/src/billing_store.rs). Unit cases are in [`billing_store_tests.rs`](../../../../src-tauri/src/billing_store_tests.rs) (`billing_store::tests`); they read the watermark and counter rows directly.

|Scenario|Executable evidence|
|---|---|
|Normal forward boundary does not accumulate quota|`inference_owner_authentication_test::wallet_challenge_rate_uses_fixed_forward_minutes_without_accumulating_allowance` exercises an exhausted minute's last second, the next minute and skipped partially used windows with explicit times. The new cases below also check that a newer minute starts at one unit in each bucket and prunes superseded wallet rows.|
|An older sample consumes only remaining current units|`inference_owner_authentication_test::wallet_challenge_rate_keeps_older_and_reordered_samples_in_the_committed_minute` spends minute 2's remaining eleven wallet units from minute-0/1 samples with real proofs, refuses the thirteenth and checks that a delayed sample's lifetime is still bounded by its own admission time. `billing_store::tests::challenge_rate_older_and_reordered_samples_spend_only_the_committed_minute` checks the watermark after each delayed sample, that the rate minute does not extend expiry, and the shared ledger unit. `inference_proposal_quota_test::challenge_ledger_rate_holds_600_601_across_older_samples_and_later_proposal_cleanup` spends 599 ledger units from older samples after minute 3 commits.|
|Reordered writers cannot revive an exhausted minute|`billing_store::tests::challenge_rate_older_and_reordered_samples_spend_only_the_committed_minute` races two connections with older samples for the last wallet unit (exactly one commits) and refuses older, equal-minute and reopened samples. The two integration cases above refuse the excess on another connection and after reopening.|
|Other control maintenance cannot reset challenge rates|`billing_store::tests::challenge_and_quote_clocks_survive_each_others_later_maintenance` runs later-minute proposal storage (with its cleanup), quote signing work, inference reservation/settlement, credential issuance, fee batching, request recovery and proof consumption, and compares every challenge-rate row. It then advances the challenge minute and checks that both quote clocks neither advance nor reset, and that a later proposal minute leaves the challenge watermark alone. `inference_proposal_quota_test::challenge_ledger_rate_holds_600_601_across_older_samples_and_later_proposal_cleanup` covers the same through public ledger APIs.|
|Retained counters survive initial watermark adoption|`billing_store::tests::retained_challenge_counters_are_adopted_without_fresh_allowance` restores old-limiter rows (599 and 600 units in minute 3, a superseded minute-2 row, no watermark). It shows that a failed later candidate installs nothing, that older and equal samples cannot reopen the twelve wallet units, and that adoption grants only the 600th unit. `billing_store::tests::superseded_challenge_wallet_rows_are_not_charges_in_the_adopted_minute` shows that a wallet's older exhausted row neither refuses nor carries into the adopted minute. `billing_store::tests::inconsistent_challenge_rate_evidence_fails_closed_without_erasing_state` covers 30 malformed or contradictory states, before and after adoption, at the same and a later minute.|

These cases fail against the pre-2.4 limiter and pass with the fix; see [code difference 1](#code-differences). Do not archive on the strength of these tests alone; task 3.2 still requires the full checks.

### Challenge rate charging is atomic and not refunded by proof lifecycle

For this [requirement](specs/inference-challenge-rate-budgets/spec.md#requirement-challenge-rate-charging-is-atomic-and-not-refunded-by-proof-lifecycle), `BillingStore::transaction` uses an immediate SQLite write transaction. `create_challenge` combines cleanup, both counters, pending capacity and insertion; `contracts::control` constructs the successful response afterward. [`auth::authenticate_owner`](../../../../src-tauri/src/auth.rs) consumes the exact stored proof before subsequent business checks. Consumption, bearer authentication and reservation admission do not debit or refund challenge counters.

|Scenario|Executable evidence|Boundary|
|---|---|---|
|Wallet refusal preserves another wallet's last shared unit|`inference_proposal_quota_test::challenge_wallet_refusals_preserve_the_last_global_unit_and_accepted_state`|Repeated refusals and reopening leave the last global unit usable by another wallet. Free-account credentials, signed terms and an offline-validated acknowledgement survive.|
|Writers compete for the last rate unit|`inference_proposal_quota_test::challenge_wallet_rate_race_commits_one_unit_without_leaking_global_allowance`; `inference_proposal_quota_test::challenge_global_rate_race_commits_only_one_candidate_across_connections`|Both connections open before the barrier. Exactly one candidate commits, the loser leaves no row, and the wallet race preserves all 588 remaining shared units. Two-connection thread races, not subprocess/load qualification.|
|Failed future-minute issuance rolls back clock and cleanup|`billing_store::tests::failed_future_minute_challenge_rolls_back_watermark_counters_and_cleanup`|With minute 2 committed, a candidate sampling minute 3 or 10 fails on nonce collision, an injected `AFTER INSERT` trigger, or pending capacity. Each failure happens after tentative cleanup, counter and watermark changes. Every table is unchanged before and after reopening, the tentatively cleaned proof is still consumable, and an older sample sees exactly the owner's remaining unit. `billing_store::tests::retained_challenge_counters_are_adopted_without_fresh_allowance` shows that a failed later candidate also leaves adoption uninstalled.|
|Consumed or undisclosed challenges do not refund issuance|`inference_owner_authentication_test::wallet_challenge_rate_spans_operations_and_survives_consumption_and_reopening`; `inference_owner_authentication_test::wallet_challenge_rate_survives_expiry_cleanup_and_reopening`; the refusal-conservation case above|Consumption, expiry and an insertion with no constructed HTTP reply do not restore units across reopening. Nondelivery is a durable-ledger observation, not injected wire response loss. HTTP business-refusal evidence is described below.|
|Exhausted issuance does not revoke existing authority|`inference_owner_authentication_test::wallet_challenge_rate_spans_operations_and_survives_consumption_and_reopening`; the ledger/resource-isolation cases above|A retained unexpired proof authenticates after issuance refusal; read-capable bearers and inference reservations still work under their own rules. This does not dispatch inference or guarantee access to a fresh owner proof.|

Reuse the six challenge-matching cases in `inference_owner_control_test` for pending-capacity refusals, competing slots, nonce collision and injected `AFTER INSERT` failure. They check preservation of both rate buckets and pending records, including through an independent connection. The [pending-capacity acceptance scope](../../specify-owner-control-authorization/design.md#task-25-pending-challenge-capacity-acceptance) explains the restored 10,000-record snapshot: it is not native issuance within the issuer lifetime. The clock-specific failure regression is the unit case in the table above.

`inference_owner_control_test::credential_business_refusals_consume_proof_without_partial_mutations` and `inference_owner_control_test::topups_require_account_owner_and_business_refusals_consume_only_authority` exercise real HTTP refusal after authentication. Numeric nonrefund is established at the common consumption/ledger boundary above; this is not an exhaustive rate-counter assertion for every HTTP business-error branch.

Read/inference isolation here means scoped bearer authentication and reservation admission, not timed HTTP exhaustion, engine output or paid settlement. Storage-only challenge targets are intended signed bytes, not proof that every target is an implemented route. The conserved funding acknowledgement uses a locally signed, offline-validated envelope and the ledger registration boundary after assumed ownership/spendability admission; it proves neither RPC spendability, broadcast, execution nor credit. Cached acknowledgement recovery is not proof that a client can acquire new authority during exhaustion.

Reuse these acceptance targets without changing owner source:

```bash
cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml \
  --test inference_owner_authentication_test --test inference_proposal_quota_test \
  --test inference_owner_challenge_test --test inference_owner_control_test
```

The all-target project check also runs the unit evidence, including the clock cases. Passing either command does not resolve credential-preserving HTTP reopening or the native receiver-expiry boundary.

## Code differences

1. **Challenge rate minutes regressed, and another resource's cleanup reset them** (requirement 2 and requirement 3's failed future-minute scenario). `create_challenge` charged `challenge:global` and `challenge:<wallet>` through the generic `rate_limit`. It used `W = now / 60` and restarted a bucket from zero whenever its stored window differed, so an older or reordered sample reopened an exhausted budget. `cleanup_pending` runs in proposal storage and challenge issuance at the caller's sample time, and it deleted every non-quote rate row older than that minute. A later-time proposal could therefore erase challenge counters. There was no watermark, adoption or consistency rule. Task 2.4. Fixed 2026-11-20 by task 2.4.

   What changed in [`billing_store.rs`](../../../../src-tauri/src/billing_store.rs):
   - `challenge_rate_limit` replaces `rate_limit`, whose only caller was challenge issuance. Inside the issuance transaction it reads only challenge state and selects `E = max(floor(t / 60), L)`. It charges both buckets in `E` and refuses at 600 or 12 with the unchanged `rate_limit: control request limit exceeded` (HTTP 429). It writes the watermark row `challenge-minute` as (`E`, ledger units committed in `E`) beside `challenge:global`, and it prunes `challenge:<wallet>` rows older than `E`. Cleanup, the charges, the watermark, pruning, capacity checks and insertion commit or roll back together.
   - `cleanup_pending` no longer deletes `challenge:*` or `challenge-minute` rows. Challenge wallet rows are pruned only when a committed challenge advances its own minute. Quote rows remain governed by proposal admission and signing work, and the challenge path never reads or writes them.
   - Adoption: with no watermark, the retained `challenge:global` row and its same-minute wallet rows stay authoritative. The consistency rules below make that row's minute the greatest retained one. The first committed issuance installs the watermark, and a failed or refused one installs nothing. With no rows at all, the first committed issuance sets the baseline from its sample.
   - Each of the following refuses fresh issuance and changes nothing. The error is `ledger invariant: inconsistent challenge rate evidence`, or `ledger database:` for unreadable values; both are internal errors, not 429. A strictly newer sample does not bypass these checks.
     - The ledger counter's minute is above `u64::MAX / 60`, or its units are outside 1–600.
     - An installed watermark differs from the ledger counter in minute or units, or exists without it.
     - Wallet units exist without a ledger counter.
     - A wallet row is ahead of the counter's minute (and so of the watermark), or its units are outside 1–12.
     - The counter's units differ from the sum of wallet units in its minute: wallet units above the global count, or global units without wallet evidence.
     - A window or count is negative, fractional or not an integer.

     Wallet rows from an older minute are superseded, not current charges. They are tolerated and pruned on the next commit.

     The pre-2.4 limiter could leave such contradictory rows itself. The provider samples time before taking the ledger write lock, so two issuances on either side of a minute boundary could commit out of order. That left a wallet row ahead of the ledger counter, or a counter below its same-minute wallet rows, until a later minute's cleanup removed them. A ledger upgraded in that state refuses fresh issuance until an operator restores consistent challenge rows; no repair command exists. This follows the conservative-validation decision above rather than guessing allowance.
   - The watermark is a `rate_limits` row, not a `metadata` key as the quote clocks use. Existing accepted-state snapshots treat `metadata` as protected non-challenge state while they issue challenges at wall-clock time: `inference_owner_control_test` (`protected()`) and `inference_funding_admission_test` (`protected_state`). A metadata watermark would change whenever issuance crossed a minute. Keeping every challenge-rate fact in `rate_limits` confines issuance to challenge rows. It stays disjoint from `quote:*`, `quote-work:global`, `quote_committed_minute` and `quote_work_minute`, and needs no schema change. The ledger schema stays 3, and no financial table is touched.
   - Unchanged: the 12/600 values; network and lifetime checks, still against the supplied admission sample; pending capacity (8 per wallet, 10,000 per ledger); consumption; authorization; and the error mapping.

   One existing test was adjusted, with no request or assertion removed. `billing_store::tests::control_acknowledges_only_spendable_current_nonce_and_recovers_without_rpc_or_broadcast` issues fourteen wall-clock owner proofs for one wallet. It simulates elapsed time with `cleanup_pending(now + 301)`, and it passed only because that cleanup deleted the challenge counters, which requirement 2 forbids. The new `owner_post_at` helper now issues the four proofs between that cleanup and the restored second quote at the same simulated time, so they enter the newer challenge minute. The last proof stays at wall-clock time, as an older sample that spends that minute's units.

   Task 2.5 regressions:
   - `billing_store::tests`: `challenge_rate_older_and_reordered_samples_spend_only_the_committed_minute`, `challenge_and_quote_clocks_survive_each_others_later_maintenance`, `failed_future_minute_challenge_rolls_back_watermark_counters_and_cleanup`, `retained_challenge_counters_are_adopted_without_fresh_allowance`, `superseded_challenge_wallet_rows_are_not_charges_in_the_adopted_minute` and `inconsistent_challenge_rate_evidence_fails_closed_without_erasing_state`.
   - `inference_owner_authentication_test::wallet_challenge_rate_keeps_older_and_reordered_samples_in_the_committed_minute`.
   - `inference_proposal_quota_test::challenge_ledger_rate_holds_600_601_across_older_samples_and_later_proposal_cleanup`.

   All eight fail against the pre-2.4 `billing_store.rs` and pass with the fix. The two integration cases and the reordered-writer, failed-future, adoption, superseded-row and inconsistency unit cases also fail on behaviour alone: a thirteenth or 601st unit is admitted, both reordered writers commit, inconsistent evidence is admitted, or an unspent wallet is refused. Thirteen mutations each make at least one of them fail: neutralising the watermark selection or write, the cleanup protection, pruning, adoption, a consistency rule, the single issuance transaction, or the 12/600 bounds.

   Review on 2026-11-20 found three more mutations that passed the first version of these regressions:
   - counting a wallet's superseded older-minute row as its spend in the adopted minute, which refuses an unspent wallet and then breaks the ledger sum;
   - accepting a watermark whose ledger counter and wallet rows were erased, which restarts from the sample and can move the minute backwards;
   - accepting a ledger counter above 600 when its wallet rows add up to the same total.

   `superseded_challenge_wallet_rows_are_not_charges_in_the_adopted_minute` and two more fail-closed states, a watermark alone and 601 consistent units, now catch them. The review's independent versions of three earlier mutation kinds also failed the regressions: tolerating wallet rows without a ledger counter, using the sample instead of the retained minute during adoption, and charging in a separate transaction before insertion. No production code changed in the review.

## Operator and native-client usage consequences

This work adds no endpoint, flag, configuration, signing format or recovery shortcut. Use the existing [native CLI/SDK workflow](../../../../README.md#developer-api-quick-start) and [authorization matrix](../../../../docs/chiral-book.md#authorization-matrix); private keys remain in protected native storage.

- Count committed issuance, not delivered responses or successful account operations. Consumption or expiry can release a pending slot without refunding its minute charge. Changing operation, account, query, body or wallet letter case does not create another allowance; unrelated provider ledgers are separate, not a shared account balance.
- HTTP 429 with `rate_limit` is not an exact retry time or proof of a specific exhausted bucket. Pending occupancy and the separate control-concurrency gate can also refuse requests. Issuance admission limits neither bound all rejected-request work nor certify public DoS resistance. Public named-wallet requests do not prove the named owner caused the traffic.
- Reuse an already-issued proof only for its exact intended operation while it remains unexpired and unconsumed. An eligible account read can use its read-scoped bearer; a supplied invalid owner proof does not fall back to a bearer. Inference credentials remain subject to their own balance, scope and admission rules. None authorizes an owner mutation without owner proof.
- A lost challenge reply still may have spent an issuance unit. A lost control reply or post-authentication refusal may also leave its proof consumed. Obtain fresh authority for an otherwise permitted retry when admission is available, retaining the original agreement and signed transaction; never infer a new payment, paid inference retry, fee bump or credential rotation is needed.
- Fresh owner-proof availability is not guaranteed: third parties can exhaust the named-wallet budget. Accepted agreements, acknowledgements, account state and exported credentials must survive refusal, but their preservation does not promise immediate owner mutation or recovery access. The separate [HTTP reopening mismatch](../../specify-owner-control-authorization/design.md#task-243-existing-root-recovery-mismatch) still forbids treating raw repeated opening as a credential-preserving refresh.
- The non-regressing clock is implemented (task 2.4). After a large forward clock jump is corrected, fresh issuance stays limited to the committed minute's remaining units until time passes that minute. The logical rate clock does not extend proof validity. A ledger whose challenge-rate evidence is inconsistent refuses fresh issuance with an internal error, not 429, until an operator restores consistent rows; existing proofs, bearers and accounts keep working. Neither the availability cost nor a refusal justifies deleting a ledger, rolling back protected state, changing identity to regain quota or bypassing authorization.

These usage notes remain with the worker-owned acceptance change instead of absorbing the owner's README or operational-document edits. Full payment recovery, credential lifecycle, platform qualification and public security/load gates remain separate.

## Verification and integration

Strict validation and the available README/CI checks are required. The clock regressions establish the ledger contract; they do not establish public-load resilience. Implement shared cleanup consistently with `specify-proposal-rate-budgets`; this change neither resolves that change's blockers nor alters its requirements. Preserve native platform, actual release-policy key and live qualification gates.

Do not archive until all implementation/acceptance tasks pass. Use normal `openspec archive specify-challenge-rate-budgets --yes`, never `--skip-specs`. Owner-control freshness and HTTP root-recovery gaps, full payment recovery and broader credential lifecycle remain separate work.
