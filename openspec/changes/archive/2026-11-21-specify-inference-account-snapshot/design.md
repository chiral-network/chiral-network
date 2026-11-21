## Scope and authority

The read-only book's “Identity, wire types, and signed metadata,” request lifecycle, funding/accounting and performance-accounting chapters require exact accounting, immutable original terms and attributable bounded receipts. Existing `inference-wire-foundation` deliberately leaves individual payload fields to their own schemas. Pending `inference-request-accounting` defines transitions and receipt snapshots, not the complete shared account member schema. This change supplies that dependency, not a new account lifecycle or complete receipt protocol.

The selected independently useful slice is `AccountView` plus the shared accounting `TokenUsage` leaf. Full receipt/request/performance/credential/opening-result envelopes remain separate work. The initial commit was a specification step that left production and permanent test code unchanged; the dated sections below record the implementation and acceptance that followed, and "Requirement, source and test correspondence" maps every requirement and scenario to its evidence.

## Decisions

### One typed accounting object, not a projection-dependent signature

Keep the current producer's 17 account members and two usage counters. Require one closed object schema for each payload, wherever it occurs. Positional arrays and extra account members currently accepted by derived deserialization are not emitted wire formats; retaining them would make acceptance depend on parser projection rather than a portable statement schema. The explicit object visitor already used by network/model/offer/quote/opening/top-up types is the intended implementation convention. Do not introduce a permissive alias, `Value` buffering layer, guessed defaults or a second public serializer.

This closes a detail the book leaves to the implementation; it does not change author-owned commercial or monetary policy. Extra unsigned account members are refused too, so the same type does not have contradictory signed/unsigned decoding modes. Genuine stored schema-3 objects already use the named producer representation. Never rewrite old financial state to force decoding success; unsupported/corrupt records remain protected and fail closed.

### Usage is an exact accounting leaf, not all OpenAI usage

`TokenUsage` has required `u64` prompt/completion counts and currently denies unknown members. Retain that range, explicit zero and known/unknown semantics. A missing field is not the same as internally constructing `TokenUsage::default()` for a separately identified unknown outcome. A valid zero-cost waiver still preserves authoritative known counts.

OpenAI-compatible responses may include `total_tokens` and other endpoint-specific details. Their raw usage schema and validation are not replaced by this closed two-counter accounting object. Counter syntax is not evidence that supplied engine counts are authoritative or within a reservation. Any limited-precision client/export path must preserve signed integers rather than rounding them or shrinking the protocol range; actual boundary acceptance remains a task, not a claim from the Rust probe.

### Original account terms and mutable accounting have different authority

The quote supplies exact account/network/party/model/price/concurrency identity. Account totals and status are mutable provider statements. `account_matches` already checks original terms and all eight canonical money fields, then compares wide `U256` totals. Preserve that design rather than recalculating balances from the bounded recent-request page or treating per-request exposure as new account credit.

Funding reconciliation's current loss compensation is recomputed from retained funding and liabilities, not accumulated as an inference-loss tally. Reversed funding and fee charges can decrease on verified restoration; batched/remitted fee liability has distinct retention rules. These meanings must survive the schema work without importing the separate reorg/remittance transition matrix. `waived_wei` is recorded foregone nominal cost, not a refund or positive term in the available-balance identity.

Approved evidence-only renewal does not rewrite `AccountView.model`. The actual admitted report remains on the request, as covered by request-accounting task 3.2.1 and its completed native regression. The receipt identifies a provider claim; report bytes and qualification timestamps are not present and cannot be independently verified from it.

### Status is not an admission certificate

The inspected writers produce `free`, `active`, `settlement_pending` and `frozen`; do not accidentally omit the pending reconciliation state. Keep the existing string representation and permit an unfamiliar value to remain an opaque readable statement. Operation-specific allowlists still decide whether inference/top-up is permitted; an unknown string is not a successful default.

Even an active snapshot is not proof that the engine is available, paid capacity exists, a fee was remitted, the chain is irreversible or the provider is honest. Free work can occupy concurrency while reserved money is zero. Read authority and credential mutation authority remain separate. No credential secret belongs in the shared account object.

## Observed temporary probe

A disposable `src-tauri/examples/apas_account_snapshot_probe.rs` executable constructed a complete synthetic account and a genuinely provider-signed receipt using a public test scalar. It performed no network, filesystem-money-state or payment operation. `cargo run --locked --no-default-features --manifest-path src-tauri/Cargo.toml --example apas_account_snapshot_probe` succeeded and observed:

- The named-object control decoded and verified.
- An extra account member decoded, disappeared from typed reserialization and still verified against the original signature.
- A complete positional account array decoded to the original typed statement and still verified.
- A positional usage array inside that account likewise decoded and verified.
- An extra `total_tokens` member in accounting usage was rejected, confirming the existing leaf member restriction.

In all three accepted noncanonical projections, the reserialized typed receipt exactly equaled the original signed receipt. This demonstrates decoder/signature behavior, not native HTTP acceptance, a forged authorized operation or a financial-state mutation. The probe was removed after observation; no executable or permanent test was part of the initial specification-only commit `9ba4c56f`.

## Implemented decoder acceptance — 2026-11-15

Snapshot tasks 2.1–2.2 now use the shared direct object visitor for both `AccountView` and `TokenUsage`, retaining the derived closed-field checks without buffering through `Value`. `AccountView` no longer silently drops extra members. Both types reject positional containers; `TokenUsage::default()` remains an explicit internal constructor, never a missing-field decoding fallback. The public fields, serialization, monetary representation and nested model semantics are unchanged.

`cargo test --locked --no-default-features --manifest-path src-tauri/Cargo.toml --test inference_account_snapshot_test` passed all seven deterministic regressions. They exercise raw and pre-parsed shapes, all required-member omissions/nulls/wrong types, unknown members, raw duplicates, counter boundaries through `u64::MAX`, fractions/exponents/signs/overflow, reordered named objects and nullable model members. Independent declared account fields and explicit counter bytes retain their canonical representations; unchanged provider signatures verify reordered valid receipts but no longer admit account/usage projections.

This is shared-decoder and signed-statement evidence, not native HTTP installation/recovery, complete persisted-record acceptance, client/export precision or chain/engine qualification. Those remaining tasks stay open.

## Native account-refresh acceptance — 2026-11-15

`native_account_refresh_preserves_original_terms_money_and_recovery_state` passes actual `Consumer::account` HTTP reads through the production contracts router and owner challenge/signature checks. Its paid, profiled ledger account and signed payment evidence are explicit post-verification prerequisites; the native account file and original journal are restored from those records. This is not a newly observed native funding workflow, engine execution or live safe-chain credit.

Response-only faults cover every immutable identity and nested model/profile/price/concurrency member, canonical-money aliases for each of the eight fields, and one-wei balance mismatches. The successful wide response conserves exactly `2*u128::MAX` with nonzero reversal/loss/fee/spend/hold/available terms and an excluded maximum waiver. It is an unsigned provider accounting claim for receiver validation, not a stored provider transition or proof that those amounts were funded.

The same native path refuses null/scalar/positional accounts; every required member missing, null, wrong-type or raw-duplicated; extra account members; and missing/extra/positional/raw-duplicated usage. Each response is changed only after successful production authorization. A delivered-response counter prevents unrelated authorization/rate errors from passing the rejection cases. Independent batches use at most the real twelve owner challenges, including four success controls; no rate budget, accepted timestamp or production limit is changed.

After every refusal, exact native saved bytes, the accepted receipt watermark, exported credential, original transaction/cancellation journal and provider financial rows remain intact through native reopening. Authentication/rate/clock bookkeeping is intentionally outside the financial comparison. Restoring the genuine response refreshes only the cached account while retaining all other saved evidence. The older renewed-evidence receipt test reuses the extracted prerequisite setup without changing its assertions.

Tasks 2.3.1 and 3.1.1 are complete. This slice does not stand in for the containing-response or remaining lifecycle/persistence/client acceptance below.

## Native signed-receipt snapshot acceptance — 2026-11-15

`native_signed_snapshots_preserve_terms_balances_and_receipt_watermarks` reuses the account-refresh mutation plan through actual `Consumer::receipt` HTTP reads, production owner authorization and genuine provider receipt issuance. Every identity/model/profile/price/concurrency change, canonical-money alias and one-wei mismatch is re-signed with the fixture provider key and verified before delivery. Signature rejection cannot mask original-agreement or monetary validation. The re-signed named wide snapshot conserves `2*u128::MAX`; a valid signature attributes the accounting claim, not chain funding or model execution.

The containing account/usage shape matrix retains the original valid signature and raw changed representation, including duplicate members, unknown additions and positional aliases. Fault construction is counted only after completion, so an unrelated authorization/rate failure or middleware panic cannot satisfy a refusal. Seven faults plus original/wide/corrected/replay/recovery requests fit each independent fixture's unchanged twelve-challenge budget.

Every fault preserves native saved bytes, the nonzero accepted receipt watermark, exported credential and original transaction/cancellation journal through native reopening. Provider financial rows remain unchanged except for the genuine receipt issuer sequence; authentication/rate/clock bookkeeping is excluded. A later genuine receipt accepts the resulting sequence gap. Replaying that accepted statement after reopening is refused without advancing the native watermark even though the provider has issued another sequence; a subsequent genuine receipt remains usable. Successful-read assertions permit retaining the previous cached account or adopting the validated snapshot, while preserving all other original evidence.

Tasks 2.3.2.1 and 3.1.2.1 are complete. This signed-receipt slice uses restored paid/profile/payment prerequisites and controlled response faults, not observed native opening, top-up, engine execution or live-chain credit. Opening and top-up evidence follows below; lifecycle/client precision and persistence work remain open. The existing renewed-request-evidence regression is retained without changing its assertions.

## Native paid opening/replay acceptance — 2026-11-15

`native_opening_snapshots_recover_committed_credit_without_installing_invalid_accounts` exercises the complete shared original-term, canonical-money, wide-balance and account/usage-shape plan against both fresh paid opening responses and their already-committed replays. Each fault has an independent account, ledger, native directory and RPC endpoints. No account or accepted credit is preloaded: signed synthetic terms, admission acknowledgement and original signed preparation are restored prerequisites. Native recovery obtains the cached acknowledgement through production owner authorization, and native/provider chain clients verify controlled canonical inclusion before actual provider HTTP opening commits one funding credit, fee obligation and root credential. This is not observed live-chain payment, fresh spendability admission, native broadcast or hardware/engine qualification.

The first invalid response retains its genuinely issued accompanying root, but native acceptance installs neither account nor credential. Every refusal preserves exact pending native bytes and original acknowledged payment evidence through reopening. Provider financial rows and accepted journal records remain unchanged across faulty replay and corrected recovery; original sender/nonce/raw bytes are retained. A genuine replay exposes the existing account without returning or rotating the root secret. Explicit native credential issuance then restores API access while leaving the original provider credential usable and the funding records unchanged. A later native reopening uses account refresh, preserves the exported credential and does not open or credit the account again.

Original and wide named success controls install the issued credential. The wide response conserves `2*u128::MAX`, while the actual ledger remains funded with 1000 wei, a 5-wei fee and 995 wei available: the changed response is an accounting claim, not funding proof. Raw duplicate and positional/extra-member faults are delivered without normalization. Completed-response observations prove each refusal reaches successful production opening/replay rather than unrelated authorization/rate failure.

The serial layout exceeded its check deadline while continuing to complete cases. Four independent current-thread runtime partitions now retain every case/assertion and keep each SQLite connection on its owning thread. A test-only gate serializes their provider control requests, including quote setup, so partitioning adds no control-slot pressure to the shared suite. Each case uses at most seven owner challenges; no production limit, rate budget, accepted timestamp or financial state is reset. Temporary tracing was removed.

Tasks 2.3.2.2, 3.1.2.2 and the paid-opening portion 3.2.1 are complete. The shared quote prerequisite was extracted from the existing accounting fixture without weakening its account/receipt assertions; free-opening and top-up evidence is below. Lifecycle reads, client/export precision and corrupt/unsupported persistence remain open.

## Native free opening/replay acceptance — 2026-11-15

`native_free_opening_snapshots_recover_without_payment_or_credential_rotation` runs the same complete term/money/shape matrix as paid opening, preserving the existing paid test and assertions. The shared fixture selects paid or free terms explicitly; free profiled quotes come from the actual public proposal route. Each case starts without an account, payment, funding acknowledgement or prepared transaction. Synthetic signed model/profile metadata is not engine or hardware qualification.

Actual owner-authorized public opening creates the free account and its root before each response fault. Native refusal installs neither account nor credential and preserves exact pending bytes across reopening and faulty replay. Corrected replay recovers the original account without returning or rotating the root. Explicit native issuance restores an exportable credential while the original provider root remains usable. Later reopening refreshes the account rather than opening again, retaining the exported credential.

Genuine and wide named controls install the actual issued root. The wide response is a mathematical accounting claim, not payment evidence: the real free ledger retains zero money and no funding, funding attempts, intents or fee obligations/batches. Native and provider RPC listeners are separate request-recording tripwires; neither is called. Both wallet journals remain empty across creation, refusal, recovery and credential issuance. Raw duplicates remain raw, and completed-response observations exclude unrelated auth/rate failures. The existing isolated partitions and control gate retain all cases without resetting rate budgets, modifying accepted timestamps or changing production limits.

Tasks 2.3.2.3, 3.1.2.3 and 3.2.2 are complete. This is HTTP, SQLite, protected-file and external-crate integration coverage, not a Verus proof or live-chain/runtime qualification.

## Native top-up snapshot recovery — 2026-11-15

`native_topup_snapshots_preserve_pending_originals_after_committed_credit` applies the complete shared original-term/money/account/usage fault plan to successful top-up responses, with genuine and wide named controls. Each case has isolated state and retains a real receipt watermark, prior settled usage/spend, a live reservation and its exported credential. The shared reply selector now serves opening and top-up tests without changing existing opening assertions.

Original paid opening, acknowledged payment, signed synthetic serving metadata and supplied prior usage are disclosed prerequisites. The top-up is not preloaded: native code creates its intent, signs and journals the original, and submits it exactly once to the controlled native RPC. Independent provider RPC observations precede actual owner-authorized HTTP credit. Only then does middleware change the account bytes, preserving raw duplicate-member faults. Completed-response observations distinguish snapshot refusal from authorization, rate, RPC or fault-construction failure.

The test requires exact pending native bytes and unchanged cached account, receipt watermark and credential after invalid first and replayed replies. It reopens the actual provider ledger/chain/router and native consumer between fresh `topup`, faulty `resume` and corrected `fund` recovery, with serving/new-funding readiness closed. The legitimate native journal transition from submitted to accepted evidence is checked separately from immutable sender/operation/nonce/raw identity; it is not an extra payment or permission to clear the pending intent.

Only the top-up's gross funding, funding fee, available credit and corresponding funding/fee rows may change. Prior funding/fees, requests, held/spent amounts, credential authority and original agreement remain intact. Corrected recovery clears only the confirmed pending operation and installs its valid account without duplicate credit or rotation. A later native reopening and authorized account refresh use no RPC or additional top-up. Wide response-only totals are accounting claims, not additional ledger funding.

The focused matrix passed all cases in four isolated current-thread partitions with one control-request gate. Each case stays within the unchanged challenge allowance; no accepted timestamp, budget or production limit is reset. Tasks 2.3.2.4, 3.1.2.4 and 3.2.3, and parents 2.3/3.1/3.2, are complete. This is I/O/external-crate acceptance, not a Verus proof, live payment, actual engine usage, hardware qualification or power-loss evidence.

## Lifecycle snapshot acceptance — 2026-11-15

`account_snapshots_preserve_lifecycle_reads_without_granting_spend_authority` completes task 2.4 through production owner/read-bearer account and signed-receipt routes. Free opening is genuine HTTP admission; paid opening, signed terms and accepted payment are post-verification prerequisites. Known usage and live reservations are supplied ledger operations, not engine evidence. The real reconciliation writer produces pending/frozen states from explicitly supplied observations; a hypothetical `provider_review` stored status tests forward-readable strings without defining a new transition.

Every state retains its complete original identity/terms and independently expected account values, including prior usage/spend, live holds, reversal and funding-loss compensation. In the frozen case, the former hold becomes a three-wei unknown-request exposure while account compensation is one wei for earlier spend; they are not interchangeable balances. Unavailable recheck observations neither create nor erase a lifecycle transition. Two live free reservations occupy both slots despite zero reserved money, and a third request is refused.

After provider ledger/chain/router reopening with admission and funding readiness closed and qualification cleared, owner and account-scoped read bearers still obtain exact accounts and authentically signed receipts. Infer-only and other-account bearers remain forbidden, consumed owner proofs cannot replay, and only legitimate receipt sequence increments change financial snapshots. Existing requests, agreements, credentials, funding/fees and unrelated accounts remain intact.

Actual native paid-account refresh retains each status, exact counters/money and original payment/credential state through file reopening. Fresh native top-up refuses pending, frozen and unfamiliar states without changing saved bytes or preparing/submitting payment; provider top-up also refuses free accounts and the same inactive states before RPC, and the ledger refuses new inference reservations for inactive states. A separate shared native/provider RPC tripwire remains untouched and both payment journals remain unchanged. No native free-account refresh is newly claimed here; the earlier free-opening matrix retains that coverage.

The focused regression passed. This is HTTP, SQLite, protected-file and external parsing/signature acceptance, not a Verus proof, live-chain regression, independent metering attestation, engine readiness or power-loss qualification. Client/export precision, fuller usage/loss reporting and corrupt/unsupported persistence remain open.

## Client and export precision acceptance — 2026-11-21

Task 2.5 inspected every surface that presents or exports an account, signed receipt, request record or usage, with values at 2^53 + 1, 2^64 − 1 and u128::MAX wei:

| Surface | Finding | Executable evidence and what it catches |
|---|---|---|
| CLI stdout (`chiral account`, `accounts`, `receipt`, `usage`) | Exact: typed results and pass-through pages are printed by `serde_json` as integer literals; wei stays decimal strings | `cli_and_native_files_keep_large_counters_wei_and_receipt_watermarks_exact` (`inference_account_presentation_test.rs`) parses stdout and requires the literal digits. A CLI that reformats numbers through doubles fails it |
| Native protected files (saved account, receipt watermark) | Exact: `u64` counters/sequence and wei strings are written as received after native acceptance | The same test reads the saved account bytes and re-reads `accounts` with the provider stopped. Receipts are claimed first at sequence 2^53 + 1, then at 2^64 − 1, and `receipt_sequence` must equal each exactly. A watermark kept at double precision fails at 2^53 + 1, holding 2^53. At 2^64 − 1 alone an `f64` round trip is invisible, because Rust's float-to-integer cast saturates back to `u64::MAX`. Each exported receipt still verifies against the provider wallet |
| Desktop IPC to the webview (command replies, stream channel) | **Lossy, fixed** (see Code differences): the webview parses IPC JSON into doubles | Five `desktop::tests` below |
| Frontend rendering (Usage, Account, Playground, raw JSON panels) | **Lossy, fixed**: token counters went through `toLocaleString` or plain interpolation of already-rounded Numbers. Wei was already exact (`formatCfx` uses `BigInt`), and the GUI sums no money | Eight `tests/exactStatements.test.ts` cases below |

The desktop tests are `webview_integers_beyond_double_precision_become_exact_decimal_strings` (2^53 − 1 stays a number; 2^53, 2^53 + 1, 2^64 − 1 and i64::MIN become strings; floats and wei are unchanged), `account_and_receipt_statements_reach_the_webview_as_the_shared_exact_fixture` (every account-, receipt-, request- and usage-bearing operation delivers `tests/fixtures/desktop-exact-statements.json` exactly; offers and quotes are not converted), `stream_usage_reaches_the_webview_channel_as_exact_text` (the actual Tauri channel body text of a `webview_event` copy), `inference_stream_forwards_provider_usage_to_the_channel_exactly` (the actual `inference_stream` forwarding loop, `forward_stream`, delivering the fixture's `stream` events in order, uncancelled) and `native_accounts_and_request_keys_reach_the_webview_exactly_through_dispatch` (actual `consumer_command` dispatch over protected native files that keep their exact integers and are not modified). The Vitest cases render that same fixture through `JSON.parse`, as the webview does. They require exact grouped counters, exact CFX for u128::MAX wei, the opaque `provider_review` status as stated, raw receipt/request panels whose JSON equals the fixture, and the Playground's exact output-token limit with the requested count unchanged. A Playground case uses the real `stream()` and its Tauri `Channel`. It delivers the fixture's `stream` events by evaluating the exact script tauri 2.9 builds for a channel message, then requires the streamed and the reconciled `request_status` usage panels to carry the exact digits; the provider's unconverted text evaluated the same way yields 18446744073709552000. The cases also show that a counter already rounded by JSON parsing appears as unavailable, never as 18,446,744,073,709,552,000, and that `formatCount` refuses out-of-range, signed, fractional and non-canonical values.

Each new test failed under at least one temporary mutation, and every mutated file was restored byte for byte: neutralised `exact_integers` (all five desktop tests); a `>=` safe-integer threshold; `receipt` dropped from the presented operations; dispatch without presentation; `webview_event` neutralised; the forwarding loop sending the provider event instead of its `webview_event` copy (only `inference_stream_forwards_provider_usage_to_the_channel_exactly` fails; all nine earlier `desktop::tests` passed this mutation); CLI numbers printed through doubles; the receipt watermark cast through `f64` (fails at 2^53 + 1; at 2^64 − 1 alone the test had passed it); `formatCount` without its 2^64 − 1 bound, accepting unsafe Numbers or grouping through a double; Usage and Playground reverted to `toLocaleString`, and the Usage detail row to plain interpolation; a `stream()` handler coercing long numeric strings back to Numbers (only the Playground stream case fails); and `formatCfx` routed through `Number`.

The saved native account and receipt fixtures are response-only claims layered on a genuine free opening (re-signed with the fixture provider key) or written as restored prerequisites. They are not ledger transitions, funding, live-chain or engine evidence; the provider ledger keeps the genuine free account. No bearer or wallet key appears in these outputs, and private keys stay native.

Remaining limits, unchanged by this task:
- Discovered signed offers and quotes travel to the webview as JSON numbers and are handed back verbatim to `quote`/`fund`. An integer beyond 2^53 − 1 there cannot round-trip exactly, and native signature verification refuses the rounded copy. The Marketplace likewise shows such an offer integer, for example the context length, at double precision. This belongs to offer/quote schema work.
- The desktop's raw receipt and request panels are presentation copies. An integer beyond 2^53 − 1 appears there as a quoted decimal string, so a copied panel is not the canonical signed JSON. `chiral receipt` is the export that re-verifies.
- Credential metadata, funding history, the pending funding journal, wallet funding-recovery history and agreement terms are account-scoped but are not account, receipt, request or usage statements. They carry wei as strings, and their only `u64` values are timestamps and nonces. They are not converted, so a value beyond 2^53 − 1 (about 285 million years in seconds) would display rounded.
- The Usage feedback revision is a JavaScript number input. A revision beyond 2^53 − 1 would be rounded before native signing. This is reputation-feedback input, outside this account/receipt task, and is recorded for follow-up.
- The `usage`, `request_status` and `infer` pages pass through as untyped `serde_json::Value`. An integer above 2^64 − 1, which is outside the wire range, is already a double natively on both the CLI and desktop. Typed `RequestView` and OpenAI usage validation remain separate schema work.
- Wallet, network, version-policy and provider-administration results are not account statements and are not converted.

## Distinct accounting quantities in signed receipts — 2026-11-21

Task 3.3 keeps request-accounting 3.2.1's regression, `native_receipts_preserve_renewed_evidence_through_settlement_and_reopening` in `inference_owner_control_test.rs`, unchanged, and its report binding was not rewritten. It passed in the full suite run after this step's read checks were in place. That regression already covers original terms with renewed request reports, known waivers and a recovered unknown request. It never pairs them with account funding-loss compensation in the same statement. The 2.4 lifecycle test separates a frozen account's 1-wei compensation from its 3-wei request exposure, but without renewal or waivers.

`signed_receipts_keep_original_terms_waived_usage_unknown_exposure_and_funding_loss_distinct` (`inference_account_records_test.rs`) produces all four quantities in one genuine provider-signed receipt and accepts it through actual native `Consumer::receipt`.

How the fixture is built:
- The production ledger opens a profiled paid account (1000 wei, 5-wei fee) from restored post-verification prerequisites: provider-signed terms, an independently signed EIP-155 opening payment, its acknowledgement, and the native saved account and journal row.
- Price is 1000/2000 wei per 1k tokens, so each request costs prompt + 2 × completion wei.
- At one explicit logical time it settles: an original-report charge; a renewed-report charge; a renewed-report waiver; a renewed-report request recovered as `restart_unknown_usage` (29-wei hold); and a hold released as `funding_orphan_unknown_usage` (11 wei) by a supplied orphan observation of the opening payment.

| Quantity | What the accepted receipt shows |
|---|---|
| Original account terms | `account.model` is the original report (`c…`), with the original price and concurrency, never the renewed model. The three renewed requests carry the `d…` report; the two original-model requests keep `c…` |
| Known waived usage | The waived request keeps usage (1, 2), output tokens 2, cost 0, `waived_wei` 5 and remedy `inference_waiver`. Account usage (3, 6) includes it, and account `waived_wei` is 5. The identity 1000 + 10 = 0 + 1000 + 10 + 0 + 0 holds without the waiver and fails with it |
| Unknown request exposure | Both unknown requests keep their statuses, counters (0, 0), zero cost and waiver, and no output-token count. Request exposure equals each hold (29 and 11, total 40). Account `reserved_wei` is 0, and account usage excludes both |
| Account funding-loss compensation | Status `frozen`, `reversed_wei` 1000 = `funded_wei`, fee liability 0 (the unbatched obligation is cancelled), available 0. `provider_loss_wei` is 10: the spend consumed before the reversal, not the 40 wei of request exposure |

Four conflations are refused after reaching a successfully authorized receipt, each authentically re-signed: the account claims the renewed model ("does not match signed terms"); the waiver is credited to available; request exposure is added to loss compensation; loss compensation is erased (all three "monetary invariant"). Each refusal leaves unchanged the saved bytes and receipt watermark, the exported credential, the native journal and the provider's stored account. A genuine receipt then accepts sequence 6, and a later refresh installs the frozen statement without credential rotation.

The same test then checks that reversal and loss compensation use wide sums, with re-signed response-only claims on the same frozen agreement:
- Funded, loss, reversed and spent all at u128::MAX (2 × u128::MAX on each side) are accepted at sequence 7. Only the watermark changes; the cached statement stays the genuine one.
- A one-wei shortfall (loss u128::MAX − 1) is refused ("monetary invariant"). The two sides are equal in `f64`.
- A 2^128 shortfall (reversed u128::MAX − 1, spent 0) is refused. The two sides are equal in wrapping `u128`.
- Both refusals preserve the same native and provider state as above, and a genuine receipt then accepts sequence 10.

Mutations, each restored byte for byte, all fail this test:
- the receiver counting `waived_wei` as a balance term (the genuine receipt is refused);
- the receiver ignoring `model` (the renewed-model conflation is accepted);
- the ledger inventing reserved counters for unknown usage (account usage becomes (13, 21));
- the receiver identity in wrapping `u128` (the 2^128 shortfall is accepted), in `f64` (the one-wei shortfall is accepted) or in checked `u128` (the conserved wide claim overflows). Before the wide cases were added, the wrapping version passed this test, the presentation test and the snapshot test, and the `f64` version passed this test and the presentation test.

The renewed report, usage and outcomes are provider-attributed synthetic claims, and the orphan is a supplied observation. They are not qualification, engine or live-chain evidence. The genuine ledger cannot fund beyond the payment it verifies, so the wide receipts are accounting claims re-signed with the fixture provider key, not funding.

## Persisted schema-3 and native records — 2026-11-21

`persisted_schema3_and_native_records_read_exactly_and_corruption_fails_closed` (`inference_account_records_test.rs`) reads records written by the current code paths:
- a schema-3 provider ledger holding the genuine free opening, the frozen account above and an active paid account with a known charge and a live hold;
- the native saved account files: the free one from an actual native opening, the paid ones rewritten by the native writer after an accepted receipt and refresh;
- the receipt watermark;
- the native wallet journal.

Genuine records:
- Each stored account decodes through the strict `AccountView`/`TokenUsage` visitors and re-encodes to the identical stored bytes, with 17 members and the exact money strings. All seven request records do the same through `RequestView`.
- A reopened `BillingStore` returns those bytes from `account`, `requests` and the operator `list_accounts`, and signs them into a receipt.
- The production control router serves an account to its read bearer as exactly the stored bytes.
- Each native file contains the provider's stored account bytes verbatim, and the frozen file holds the receipt watermark 1. `accounts` and `credential_export` read them without rewriting a file or the journal.
- Across the compared tables (accounts, requests, credentials, funding, intents, fee obligations and batches, performance rows, metadata and the schema catalogue), only receipt issuance changed anything, and only an account's sequence.

Corrupt and unsupported records:
- **Provider faults.** Each corruption is applied to a byte-for-byte copy of the checkpointed ledger:
  - account members: an extra member, a positional account, a missing `waived_wei`, and a raw duplicate `spent_wei` member with the same value;
  - account values: a 2^64 counter, `"spent_wei":"010"`, `"waived_wei":"05"` and an unbalanced available amount;
  - wide account sums: one wei short (equal in `f64`) and 2^128 short (equal in wrapping `u128`);
  - positional usage in one request record.
- **Provider results.**
  - For a corrupt account, `account`, bearer authentication, receipt issuance and the operator listing all fail as `ledger corruption`. The control routes answer 500 without serving the money.
  - A corrupt request record blocks only that account's request page and receipts. Its account read stays exact.
  - Other accounts stay readable, served and signable.
  - Every compared ledger row and the schema catalogue are unchanged, except the sequence of the other account's receipt.
- **Wide control.** A hand-written stored statement conserving 2 × u128::MAX is not corruption. It is read, served to its bearer, listed and signed exactly as stored. This is an acceptance control for the read check, not a genuine record.
- **Unsupported schemas.** Schema `2` and `4` copies are refused at open with the preserve-without-conversion message. The file bytes and rows are unchanged.
- **Native files.** The same account corruptions, plus cached renewed terms and an unsupported top-level member, are written in place.
  - `account`, `receipt`, `credential_export` and both aggregate listings (`accounts` and `pending_accounts`) refuse without contacting the provider.
  - The file, directory listing and journal are unchanged.
  - Other accounts still export in every case, and a provider refresh of another account succeeds (checked once).
  - Restoring the original bytes restores service.

Structural faults already failed closed through the task 2.1 decoder. Semantic corruption did not; this is fixed (see Code differences). Mutations that each fail this test:
- the native load without its new check (`account` accepted `"010"`);
- the provider read without its new check (`"010"` was signed into a receipt);
- the operator listing without the check (the listing presented the corrupt account);
- `AccountView` without `deny_unknown_fields` (an extra member was accepted).

Each was restored byte for byte. The 3.3 test still passed under all four, so these failures are specific to persisted-record handling.

An adversarial review on 2026-11-21 added the duplicate-member, non-canonical-waiver, wide-shortfall and wide-control cases, the pending-listing assertion and the 3.3 wide claims. Before these were added, the two waiver mutations and the two `Value` mutations below passed this test, and the three provider-identity mutations had no stored wide case to fail on. Each of these mutations now fails the test, and each was restored byte for byte:
- the provider read skipping waiver validation (`"05"` was signed);
- the native check skipping waiver validation (`"05"` was accepted);
- ledger decoding buffered through `serde_json::Value` (a duplicate member was signed);
- native decoding buffered through `Value` (a duplicate member was accepted);
- the provider identity in wrapping `u128` (the 2^128 shortfall was signed);
- the provider identity in `f64` (the one-wei shortfall was signed);
- the provider identity in checked `u128` (the wide control is refused as `amount_overflow`).

Older records: `AccountView` has had the same 17 required members since baseline `7536493a`, and task 2.1 only refused additions and positional forms no producer emitted. No live-deployment ledger was examined. The paid accounts' payment acceptance and journal rows are restored prerequisites, not an observed native paid opening (owner-control 3.2.1–3.2.3 cover that path). SQLite page-level damage, power loss and file-permission faults are outside this test; the last is covered by `an_unprotected_or_linked_account_store_fails_closed_without_contacting_the_provider`.

## Code differences

- **Desktop webview integer precision. Fixed 2026-11-21 by task 2.5.**
  - **What was lossy.** `consumer_command` replies and `inference_stream` channel messages reached the webview as JSON. Command replies go through `JSON.parse`, and channel messages are evaluated as JavaScript literals, so both became IEEE-754 doubles. Every integer beyond 2^53 − 1 was therefore displayed rounded: 2^53 + 1 as 9007199254740992 and 2^64 − 1 as 18,446,744,073,709,552,000. This affected accounting `TokenUsage` counters, model token limits, the receipt `sequence`, request-record counters, timestamps and performance microseconds. It showed on the Usage token tiles and rows, the Playground limit and usage panel, and the raw receipt and request panels.
  - **Fix in `desktop.rs`.** `presented`/`exact_integers` replace every integer beyond ±(2^53 − 1) with its exact decimal string. This applies to the results of `accounts`, `account`, `fund`, `topup`, `resume`, `receipt`, `usage`, `request_status`, `cancel_request`, `local_requests` and `infer`, and to forwarded stream events through `webview_event`. Safe integers, floats and strings, canonical wei included, are unchanged. The `inference_stream` forwarding loop is moved unchanged into `forward_stream`, so the loop that applies `webview_event` is itself tested; its event order, cancellation and missing-completion behaviour are unchanged.
  - **Fix in the frontend.** `native.ts` types these `u64` fields as `ExactInteger` (`number | string`). The new `formatCount` renders a safe Number, or a canonical decimal string up to 2^64 − 1, with exact `BigInt` grouping. It shows anything else, including an already-rounded Number, as unavailable. Usage and Playground use it. `ServingProfile`'s approximate GiB and date figures now convert explicitly.
  - **What did not change.** The wire, signatures, persisted native state, the provider `u64`/`u128` range and the CLI's JSON integers. Discovered offers and quotes are not converted, because the webview returns them verbatim for native verification.
- **Semantically corrupt stored and cached accounts were served. Fixed 2026-11-21 by task 3.4.**
  - **What failed open.** Since task 2.1, structurally invalid account JSON fails to decode. But money fields are strings, so an account record with non-canonical wei (`"010"`) or an unbalanced identity still decoded.
    - Provider: `get_account` returned such a record. Observed: `receipt` signed `"spent_wei":"010"` into a provider-attributed statement, and the operator `list_accounts` presented it. By the same read, the account route would serve it and bearer authentication would accept it.
    - Native: `load_account` never re-validated its cached account. Observed: `account(id)` accepted a cached `"010"` and replaced it from the provider, a reset of corrupt state rather than a refusal. By the same load, `accounts` would list it and `credential_export` would proceed, and a cached account whose terms no longer matched its signed quote would behave the same.
  - **Fix in `billing_store.rs`.** The writer's existing identity check is now `balanced`: canonical wei for all eight fields and the wide `U256` identity, with the waiver validated but excluded. `get_account` and `list_accounts` pass each decoded record through `stored_account`, which applies it and reports `ledger corruption: …`. The control routes map that to an internal error. `save_account` applies the same check as before, plus `waived_wei` canonicality, which every writer already produces.
  - **Fix in `consumer.rs`.** `load_account` applies the existing `account_matches` to a cached account: original terms, canonical money and the wide identity. On failure it reports `saved account snapshot is corrupt; it is preserved unchanged` before any provider request.
  - **What did not change.** Serialization, stored representation, schema, writers, accepted range and every valid record. Nothing migrates, normalizes, repairs or resets a record. Request-record money is still not validated on provider read (see limits).

## Source and retained acceptance map

| Boundary | Inspected implementation or retained evidence | Required work |
|---|---|---|
| Payload and leaf | `protocol.rs::AccountView` and `TokenUsage` now use the direct object visitor with closed members; seven `inference_account_snapshot_test.rs` regressions pass | Tasks 2.1–2.2 complete; actual containing native replies and durable recovery remain below |
| Native account installation | `consumer.rs::account_matches`, `resume_inner`, `topup`, `accept_topup`, `refresh_account`, `receipt`; matching precedes credential/account installation, and `load_account` now re-applies it to a cached account | GET, signed receipts, paid/free opening/replay and top-up term/money/shape/state preservation pass; original-operation post-commit recovery passes in 3.2.1–3.2.3. Lifecycle reads/opaque native status retention pass in 2.4; client precision passes in 2.5; corrupt cached accounts fail closed in 3.4 |
| Producer and local records | `billing_store.rs::open_account`, `account`, `receipt`, `get_account`/`stored_account`/`balanced`, `save_account`, `list_accounts`; genuine account construction and signed snapshots | Task 3.4 complete: genuine schema-3 rows and native files decode strictly and re-encode to identical bytes; corrupt and unsupported records fail closed unchanged, with no migration, repair or reset |
| Funding meanings/status | `credit_funding`, `balance_funding`, `transition_funding` preserve gross funding, current reversals/loss compensation, retained fee liability and pending/frozen status | Task 2.4 observes real ledger transitions from supplied recheck inputs, owner/read-bearer snapshots, opaque status retention and closed spend/top-up guards; not full reorg/fee lifecycle or live-chain qualification |
| Original versus renewed evidence | `native_receipts_preserve_renewed_evidence_through_settlement_and_reopening` (request-accounting 3.2.1, retained unchanged) and `signed_receipts_keep_original_terms_waived_usage_unknown_exposure_and_funding_loss_distinct` pass actual native/HTTP receipts with original account terms and renewed request reports | Task 3.3 complete; this is not qualification attestation or complete receipt schema acceptance |
| Request exposure | `signed_receipt_rejects_noncanonical_request_exposure_without_advancing_sequence` preserves native state on malformed request exposure; the 3.3 test keeps 40 wei of exposure apart from 10 wei of loss compensation in one signed statement | Task 3.3 complete; typed `RequestView` validation remains separate schema work |
| Client precision/export | Native typed fields, signing, protected files and CLI `serde_json` output are exact. The desktop webview copy is fixed by `desktop.rs::presented`/`exact_integers`/`webview_event`/`forward_stream` and rendered with `native.ts::formatCount` | Task 2.5 is complete: CLI/native exactness, the desktop boundary and frontend rendering pass at 2^53 + 1, 2^64 − 1 and u128::MAX wei. The offer/quote round trip, feedback-revision input and untyped pages above the wire range are recorded limits |

## Requirement, source and test correspondence

Test targets are `src-tauri/tests/<target>.rs` unless marked `lib`. Abbreviations: **snapshot** = `inference_account_snapshot_test`, **presentation** = `inference_account_presentation_test`, **records** = `inference_account_records_test`, **owner-control** = `inference_owner_control_test`.

The 2026-11-21 step ran snapshot, presentation, records and the library suite (429 passed, 12 ignored). Its adversarial review reran them after adding the cases above. Because the task 3.4 read checks apply to every ledger and native account read, it also ran the 13 other integration targets that open a ledger or native consumer directory. All 16 passed: consumer profile paths, CLI credentials, control readers, funding acknowledgement and admission, owner authentication, challenge and consumer, proposal, proposal quota, quote acceptance, wire foundation and outbound proxy, plus the three account targets. The step itself did not run owner-control. The full headless suite run afterwards, with the task 3.4 read checks in place, passed all 32 targets, including owner-control 61/61. Its fixtures write only genuine ledger accounts into native files and ledger rows, which those checks accept.

| Requirement and scenario | Source | Tests |
|---|---|---|
| **Named-field schema**: a complete producer snapshot survives representation changes | `protocol.rs::AccountView`, `deserialize_object` | snapshot `named_account_snapshots_preserve_every_field_and_canonical_bytes`, `receipts_preserve_named_claims_but_reject_account_and_usage_projections`; records: genuine stored rows and native files re-encode to identical bytes |
| Aliases and omitted members are refused, including inside openings and receipts | same; `consumer.rs::refresh_account`, `receipt`, opening and top-up acceptance | snapshot `account_snapshots_reject_positional_arrays_and_nonobjects`, `account_snapshots_require_all_named_members_and_refuse_additions`, `account_and_usage_decoders_reject_raw_duplicate_members`; owner-control 3.1.1–3.1.2.4 matrices; records: stored and native extra, positional, missing and raw duplicate members |
| A signature does not authenticate discarded additions | same; `protocol::verify` after decoding | snapshot `receipts_preserve_named_claims_but_reject_account_and_usage_projections`; owner-control `native_signed_snapshots_preserve_terms_balances_and_receipt_watermarks` |
| **Two exact counters**: boundaries | `protocol.rs::TokenUsage`; `desktop.rs::exact_integers`; `native.ts::formatCount` | snapshot `accounting_usage_requires_two_exact_unsigned_counters_without_defaults`; presentation `cli_and_native_files_keep_large_counters_wei_and_receipt_watermarks_exact`; five `desktop::tests` (lib); records: 2^64 counter refused in stored and native records |
| A usage array is not an object | same | snapshot `accounting_usage_rejects_positional_arrays_and_nonobjects`; records: positional usage in a stored request record |
| Waived or unavailable work is not erased or invented | `billing_store.rs::settle_request`, `recover_requests`, `transition_funding` | records `signed_receipts_keep_original_terms_waived_usage_unknown_exposure_and_funding_loss_distinct`; lib `performance_waiver_preserves_usage_and_releases_reservation_exactly_once`, `restart_performance_waiver_does_not_invent_usage_or_credit`; owner-control 3.2.1 |
| **Original identity and wide conservation**: re-signed commercial substitution is refused | `consumer.rs::account_matches` | owner-control 2.3.1–2.3.2.4 term matrices; records renewed-model conflation |
| Reversal and loss balances use wide sums | `account_matches`; `billing_store.rs::balanced`, `balance_funding` | owner-control wide controls and one-wei mismatches; presentation 2 × u128::MAX control; records: genuine reversal and loss with three money conflations refused; re-signed 2 × u128::MAX receipts accepted, with one-wei and 2^128 shortfalls refused; unbalanced, one-wei-short and 2^128-short stored and cached records refused; a wide balanced stored record served and signed |
| Renewal changes a request report, not the agreement | `billing_store.rs::reserve`, `settle_request`; `consumer.rs::receipt` | owner-control `native_receipts_preserve_renewed_evidence_through_settlement_and_reopening`; records 3.3 test; lib `evidence_renewal_preserves_existing_credit_and_request_identity` |
| A malformed post-commit response leaves recovery reachable | `consumer.rs::resume_inner`, `accept_topup`, `issue_credential` | owner-control `native_opening_snapshots_recover_committed_credit_without_installing_invalid_accounts`, `native_free_opening_snapshots_recover_without_payment_or_credential_rotation`, `native_topup_snapshots_preserve_pending_originals_after_committed_credit` |
| **Status and attribution**: pending or frozen balances remain inspectable | `contracts.rs` read routes; `billing_store.rs::transition_funding` | owner-control `account_snapshots_preserve_lifecycle_reads_without_granting_spend_authority`; records: a frozen account read through native receipt and refresh and provider bearer reads after reopening |
| An unfamiliar status is not an active account | `reserve` and top-up status allowlists | owner-control 2.4 test; presentation (`provider_review` kept as stated) |
| Zero monetary holds do not certify idle capacity | `reserve` concurrency count | owner-control 2.4 test (two free holds fill both slots at zero reserved money) |
| Persisted records (task 3.4; design decision, not a separate scenario) | `get_account`, `list_accounts`, `load_account`, `BillingStore::open` schema check | records `persisted_schema3_and_native_records_read_exactly_and_corruption_fails_closed`; lib `network_scoped_performance_cutover_refuses_old_ledgers_without_rewriting_money` |

## Operator and client notes

- **Provider operators.** A ledger account record that no longer decodes, or whose money is non-canonical or unbalanced, makes that account fail closed.
  - That account's control reads, bearer authentication and inference admission, and receipt issuance fail. Each control route answers `internal_error` without detail and nothing is logged; the ledger API itself reports `ledger corruption: …`.
  - The loopback operator account listing refuses as a whole, as it already did for malformed JSON. A corrupt request record blocks only that account's request page and receipts.
  - Other accounts keep their reads, authentication and receipts (tested).
  - By inspection, not exercised by a test: two ledger-wide effects predate this change for structural and most money corruption. Daemon startup recovers every reserved request in one transaction, so a corrupt account holding an unsettled reservation stops the daemon from starting. A maintenance recheck of that account's funding is quarantined, which closes new funding and offer publication provider-wide until operator review.
  - Nothing repairs, rewrites, migrates or resets the row. Preserve the ledger and investigate from a backup; do not edit money fields or delete rows to make reads succeed. A ledger whose schema is not `3` is refused at open, as before.
- **Consumers (CLI and desktop).** A local saved account whose cached statement is malformed, unbalanced or no longer matches its signed terms fails as `saved account snapshot is corrupt; it is preserved unchanged`.
  - `account`, `receipt` and `credential export` for that agreement fail without contacting the provider (tested). Every other native operation on that agreement loads the same file and refuses too, by inspection, including inference, top-up, funding recovery and cancellation.
  - Both aggregate listings refuse as a whole rather than silently omitting an agreement (tested): `chiral accounts` and `chiral pending`. On the desktop these feed the Account, Usage and Playground account lists and the Marketplace pending-funding list.
  - Other agreements stay readable and exportable by id, and per-agreement commands such as `resume` remain reachable.
  - The file and wallet journal are left unchanged; recovery is an explicit owner action, for example from a backup.
- **Reading statements.** `waived_wei` is foregone nominal cost for known usage, not credit. A request's `provider_loss_wei` is that request's unknown-usage exposure. The account's `provider_loss_wei` is current funding-loss compensation after reversals; the two are never summed. Zero counters on an `*_unknown_usage` request mean the usage is unknown, not that no tokens ran.

## Integration and release limits

Remaining limits after tasks 3.3, 3.4 and 4.1:
- The provider does not validate request-record money on read. A request record with non-canonical wei but valid JSON would be served in request pages and signed into a receipt; the native receiver refuses such a receipt (`validate_money`). The native receiver also does not require zero counters on unknown-status requests, which the producer guarantees. Typed `RequestView` and full receipt-envelope validation remain separate schema work.
- One corrupt local file makes both aggregate native listings fail as a whole. One corrupt ledger account makes the operator account listing fail, can stop daemon startup recovery and quarantines its funding rechecks (by inspection). These are deliberate fail-closed choices, not partial results; per-entry error reporting would be separate product work.
- Persisted-record evidence uses records written by current code. No live-deployment ledger or historical user file was examined. Paid payment acceptance and native journal rows are restored prerequisites. SQLite page damage, power loss and crash durability are not exercised.
- Wide-sum receiver acceptance rests on response-only claims; no ledger was funded beyond a verified payment.

Implement and verify the shared payload before declaring the containing responses fully specified. Every new regression must catch an observable consumer contract violation and preserve an unmodified success control; existing signature/financial/credential tests are retained, not weakened. Invalid replies after a real remote commit remain ambiguous and recoverable with the original operation; local refusal cannot roll back the provider or manufacture a new payment.

Run strict validation and the available README/CI checks. Leave this change open until its listed implementation and acceptance tasks are complete, then archive normally without `--skip-specs`. Native desktop dependencies, actual policy authority and external chain/engine/platform/hardware/public-release qualification remain separate blockers. Do not install prerequisites, use a fake authority, operate on a server/account or edit the book.
