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

## Requirement-to-source/test acceptance map

This index covers the three requirements and thirteen scenarios in the [challenge-rate delta](specs/inference-challenge-rate-budgets/spec.md). Test targets are in [`src-tauri/tests`](../../../src-tauri/tests). It records acceptance of existing behavior, not new requirements or completion of the blocked clock contract.

### Committed challenge issuance consumes scoped minute budgets

For this [requirement](specs/inference-challenge-rate-budgets/spec.md#requirement-committed-challenge-issuance-consumes-scoped-minute-budgets), [`contracts::control`](../../../src-tauri/src/contracts.rs) canonicalizes the public request before [`BillingStore::create_challenge`](../../../src-tauri/src/billing_store.rs) charges the wallet and ledger buckets. The numeric tests use canonical identities and explicit-time storage/authentication APIs; they do not send 600 HTTP requests within a real minute.

|Scenario|Executable evidence|Boundary|
|---|---|---|
|Twelfth wallet issuance is distinct from pending occupancy|`inference_owner_authentication_test::wallet_challenge_rate_spans_operations_and_survives_consumption_and_reopening`; `inference_owner_challenge_test::wallet_hex_aliases_share_the_same_pending_challenge_allowance`|Real signatures consume earlier proofs so eight pending slots cannot mask 12/13. Operation/account/query/body changes share the wallet bucket. Native alias normalization is separate HTTP evidence, not an HTTP rate-edge test.|
|Another wallet retains its individual allowance|`inference_proposal_quota_test::challenge_wallet_rate_preserves_other_wallets_and_independent_control_authority`|A different wallet is admitted after the first exhausts its rate. Proposal allowance, a read credential, an existing inference reservation and an independent provider ledger remain separate.|
|Ledger rate boundary applies across wallets|`inference_proposal_quota_test::challenge_ledger_rate_is_independent_of_proposals_providers_and_existing_accounts`|600 actual insertions across eligible wallets; the next candidate is refused without wallet/occupancy exhaustion. Reopening preserves evidence. Another provider ledger and the full remaining proposal allowance are independent.|

The reverse direction is covered by `inference_proposal_quota_test::ledger_rate_budget_is_shared_across_terms_but_not_providers_or_control_budgets` and `inference_proposal_quota_test::payer_rate_scope_spans_terms_without_blocking_other_payers_or_control_budgets`: proposal exhaustion leaves challenge admission and existing credential/reservation authority available. These are same-minute resource-isolation checks, not evidence of independent durable rate clocks.

### Effective challenge minutes never regress or share another resource's clock

For this [requirement](specs/inference-challenge-rate-budgets/spec.md#requirement-effective-challenge-minutes-never-regress-or-share-another-resources-clock), the production paths are `rate_limit`, `cleanup_pending` and `BillingStore::transaction` in [`billing_store.rs`](../../../src-tauri/src/billing_store.rs). Only ordinary forward-window behavior is accepted. No durable challenge watermark or conservative adoption path exists yet.

|Scenario|Executable evidence or blocked task|
|---|---|
|Normal forward boundary does not accumulate quota|`inference_owner_authentication_test::wallet_challenge_rate_uses_fixed_forward_minutes_without_accumulating_allowance` exercises an exhausted minute's last second, the next minute and skipped partially used windows with explicit times.|
|An older sample consumes only remaining current units|Blocked in tasks 2.4–2.5: retain the committed effective minute and its remaining counts instead of resetting to the older sample.|
|Reordered writers cannot revive an exhausted minute|Blocked in tasks 2.4–2.5. The final-unit races below use one timestamp; they do not prove non-regression across older samples or restart.|
|Other control maintenance cannot reset challenge rates|Blocked in tasks 2.4–2.5: generic cleanup currently prunes other resources' older counters. Same-minute isolation is not protected future-time cleanup or independent clock advancement.|
|Retained counters survive initial watermark adoption|Blocked in tasks 2.4–2.5: initialization must preserve consistent retained charges and reject inconsistent evidence, not grant fresh allowance. No adoption/corruption acceptance is claimed.|

`billing_store.rs` and its unit tests remain owner-untracked work. The clock, resource-protected pruning and adoption fix must be independently committable together with its regressions; copying that source, adding an unused helper or adopting its surrounding migration is not a substitute. Do not archive on the strength of the forward-boundary test.

### Challenge rate charging is atomic and not refunded by proof lifecycle

For this [requirement](specs/inference-challenge-rate-budgets/spec.md#requirement-challenge-rate-charging-is-atomic-and-not-refunded-by-proof-lifecycle), `BillingStore::transaction` uses an immediate SQLite write transaction. `create_challenge` combines cleanup, both counters, pending capacity and insertion; `contracts::control` constructs the successful response afterward. [`auth::authenticate_owner`](../../../src-tauri/src/auth.rs) consumes the exact stored proof before subsequent business checks. Consumption, bearer authentication and reservation admission do not debit or refund challenge counters.

|Scenario|Executable evidence|Boundary|
|---|---|---|
|Wallet refusal preserves another wallet's last shared unit|`inference_proposal_quota_test::challenge_wallet_refusals_preserve_the_last_global_unit_and_accepted_state`|Repeated refusals and reopening leave the last global unit usable by another wallet. Free-account credentials, signed terms and an offline-validated acknowledgement survive.|
|Writers compete for the last rate unit|`inference_proposal_quota_test::challenge_wallet_rate_race_commits_one_unit_without_leaking_global_allowance`; `inference_proposal_quota_test::challenge_global_rate_race_commits_only_one_candidate_across_connections`|Both connections open before the barrier. Exactly one candidate commits, the loser leaves no row, and the wallet race preserves all 588 remaining shared units. Two-connection thread races, not subprocess/load qualification.|
|Failed future-minute issuance rolls back clock and cleanup|Blocked in tasks 2.4–2.5: the new watermark must roll back with counters and pending mutations. Existing insertion-fault/collision tests establish transaction rollback but do not exercise a challenge watermark or failed future challenge-minute advance.|
|Consumed or undisclosed challenges do not refund issuance|`inference_owner_authentication_test::wallet_challenge_rate_spans_operations_and_survives_consumption_and_reopening`; `inference_owner_authentication_test::wallet_challenge_rate_survives_expiry_cleanup_and_reopening`; the refusal-conservation case above|Consumption, expiry and an insertion with no constructed HTTP reply do not restore units across reopening. Nondelivery is a durable-ledger observation, not injected wire response loss. HTTP business-refusal evidence is described below.|
|Exhausted issuance does not revoke existing authority|`inference_owner_authentication_test::wallet_challenge_rate_spans_operations_and_survives_consumption_and_reopening`; the ledger/resource-isolation cases above|A retained unexpired proof authenticates after issuance refusal; read-capable bearers and inference reservations still work under their own rules. This does not dispatch inference or guarantee access to a fresh owner proof.|

Reuse the six challenge-matching cases in `inference_owner_control_test` for pending-capacity refusals, competing slots, nonce collision and injected `AFTER INSERT` failure. They check preservation of both rate buckets and pending records, including through an independent connection. The [pending-capacity acceptance scope](../specify-owner-control-authorization/design.md#task-25-pending-challenge-capacity-acceptance) explains the restored 10,000-record snapshot: it is not native issuance within the issuer lifetime. These cases do not replace the blocked clock-specific failure regression.

`inference_owner_control_test::credential_business_refusals_consume_proof_without_partial_mutations` and `inference_owner_control_test::topups_require_account_owner_and_business_refusals_consume_only_authority` exercise real HTTP refusal after authentication. Numeric nonrefund is established at the common consumption/ledger boundary above; this is not an exhaustive rate-counter assertion for every HTTP business-error branch.

Read/inference isolation here means scoped bearer authentication and reservation admission, not timed HTTP exhaustion, engine output or paid settlement. Storage-only challenge targets are intended signed bytes, not proof that every target is an implemented route. The conserved funding acknowledgement uses a locally signed, offline-validated envelope and the ledger registration boundary after assumed ownership/spendability admission; it proves neither RPC spendability, broadcast, execution nor credit. Cached acknowledgement recovery is not proof that a client can acquire new authority during exhaustion.

Reuse these acceptance targets without changing owner source:

```bash
cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml \
  --test inference_owner_authentication_test --test inference_proposal_quota_test \
  --test inference_owner_challenge_test --test inference_owner_control_test
```

The all-target project check also runs existing unit evidence. Passing either command does not resolve the blocked clock scenarios, credential-preserving HTTP reopening or native receiver-expiry boundary.

## Operator and native-client usage consequences

This work adds no endpoint, flag, configuration, signing format or recovery shortcut. Use the existing [native CLI/SDK workflow](../../../README.md#developer-api-quick-start) and [authorization matrix](../../../docs/chiral-book.md#authorization-matrix); private keys remain in protected native storage.

- Count committed issuance, not delivered responses or successful account operations. Consumption or expiry can release a pending slot without refunding its minute charge. Changing operation, account, query, body or wallet letter case does not create another allowance; unrelated provider ledgers are separate, not a shared account balance.
- HTTP 429 with `rate_limit` is not an exact retry time or proof of a specific exhausted bucket. Pending occupancy and the separate control-concurrency gate can also refuse requests. Issuance admission limits neither bound all rejected-request work nor certify public DoS resistance. Public named-wallet requests do not prove the named owner caused the traffic.
- Reuse an already-issued proof only for its exact intended operation while it remains unexpired and unconsumed. An eligible account read can use its read-scoped bearer; a supplied invalid owner proof does not fall back to a bearer. Inference credentials remain subject to their own balance, scope and admission rules. None authorizes an owner mutation without owner proof.
- A lost challenge reply still may have spent an issuance unit. A lost control reply or post-authentication refusal may also leave its proof consumed. Obtain fresh authority for an otherwise permitted retry when admission is available, retaining the original agreement and signed transaction; never infer a new payment, paid inference retry, fee bump or credential rotation is needed.
- Fresh owner-proof availability is not guaranteed: third parties can exhaust the named-wallet budget. Accepted agreements, acknowledgements, account state and exported credentials must survive refusal, but their preservation does not promise immediate owner mutation or recovery access. The separate [HTTP reopening mismatch](../specify-owner-control-authorization/design.md#task-243-existing-root-recovery-mismatch) still forbids treating raw repeated opening as a credential-preserving refresh.
- The specified non-regressing clock is **not implemented**. Do not describe the current limiter as enforcing it after clock correction or cross-resource maintenance. Once implemented, correcting a large forward jump can leave fresh issuance constrained until time passes the committed minute; the logical rate clock will not extend proof validity. Neither current clock weaknesses nor future availability costs justify deleting a ledger, rolling back protected state, changing identity to regain quota or bypassing authorization.

These usage notes remain with the worker-owned acceptance change instead of absorbing the owner's README or operational-document edits. Full payment recovery, credential lifecycle, platform qualification and public security/load gates remain separate.

## Verification and integration

Strict validation and the available README/CI checks are required. Passing current tests does not establish the new clock contract or public-load resilience. Implement shared cleanup consistently with `specify-proposal-rate-budgets`; this change neither resolves that change's blockers nor alters its requirements. Preserve native platform, actual release-policy key and live qualification gates.

Do not archive until all implementation/acceptance tasks pass. Use normal `openspec archive specify-challenge-rate-budgets --yes`, never `--skip-specs`. Owner-control freshness and HTTP root-recovery gaps, full payment recovery and broader credential lifecycle remain separate work.
