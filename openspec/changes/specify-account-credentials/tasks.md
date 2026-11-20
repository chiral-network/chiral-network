## 1. Specify account credentials

- [x] 1.1 Read the source material:
  - the read-only book section *Consumer control, credentials, and request lifecycle* in full (lines 325–360);
  - the other credential lines: 56, 61, 65, 472, 474, 493, 495, 581, 585, 603, 671, 673, 677, 808, 815 and 930;
  - the `CLAUDE.md` rules on credential rotation and reachability, protected secret files and private keys outside Svelte;
  - comparison item 12;
  - the pending `specify-owner-control-authorization`, `specify-inference-request-accounting`, `specify-funding-settlement`, `specify-challenge-rate-budgets`, `specify-inference-account-snapshot`, `specify-outbound-http-safeguards`, `specify-desktop-gui-global-rules`, `specify-desktop-gui-pages`, `specify-version-policy` and `specify-profile-path-resolution`;
  - the normative `inference-wire-foundation` and `inference-account-opening-request`;
  - the target-design `specify-cli-loopback-gateway` and `specify-desktop-local-api-server`.

  Searched `openspec` for existing coverage of each rule, mapped each uncovered whitepaper rule to one requirement, and referenced the owners of the rest instead of restating them.
  Done 2026-11-19: the book's SHA-256 is `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`.
- [x] 1.2 Wrote the proposal, design and `account-credentials` spec: 9 requirements and 23 scenarios, each requirement citing its book lines. Inspected the code:
  - `contracts.rs`: router, `error`, `parse`, `owner`, `authorize_read`, `authorize_owner`, `accepting`, credential branch;
  - `billing_store.rs`: `CredentialIssueRequest`, `CredentialView`, `money`, credential table, `authenticate`, `open_account`, `issue_credential`, `credentials`, `revoke_credential`, `rotate_credential`, `reserve`;
  - `inference_gateway.rs`: `ledger_error`, `authenticate`, `models`, `request_status`, `cancel_request`, inference;
  - `auth.rs`: `bearer`;
  - `protocol.rs`: `deserialize_object`, `Credential`, `OpenResult`, the request decoders, `hash_hex`, `random_id`;
  - `consumer.rs`: `SavedAccount`, `private_read`, `private_write`, `account_directory`, opening, `store_credential`, the credential operations, `credential_export`, `credential`, `inference_response`, `request_control`, `credential_metadata`;
  - `wallet.rs`: the secret-file functions;
  - `desktop.rs` and `bin/chiral.rs`: dispatch and commands;
  - `src/pages/Usage.svelte`, `src/lib`, the SDK examples and the credential tests.

  Recorded in `design.md` the coverage table, two code differences, the implementation values (decision 4), eight observations outside this capability and three author questions.
- [x] 1.3 `openspec validate specify-account-credentials --strict --no-interactive` passes. The focused runs behind the coverage table passed on 2026-11-19: 14 library tests and 6 integration tests (listed in design *Coverage table*). No code or test was changed. The whitepaper is unchanged.
  Adversarial review, 2026-11-19: re-read every cited book line and verified both code differences and every cited test (all pass when rerun). Implementation values were moved out of the requirements into design decision 4: the Unix-seconds encoding of `expires_at`, the members of the creating response and of the export, the exact store modes and the wallet subdirectory. Hash disclosure is no longer forbidden, because the book does not address it. Owner-control and request-accounting rules are now referenced instead of restated with SHALL. The unowned book rule that the SDK examples never print the key (line 474) was added to requirement 8. Requirement 9 now states only the book's "does not silently replace". Validation still passes.
- [x] 1.4 Append report entry and mark comparison item 12. For the coordinator: append an entry to `docs/apas-worker/report.md` with the two code differences and the three author questions, and mark comparison item 12 in `docs/apas-worker/comparison-2026-11-18.md` as specified by this change. Done 2026-11-19 by the coordinator: report entry "Account credentials: omitted spend cap, rotation restoring a missing default, and listing completeness" (it also notes the review's strict-object reading of requirement 1); item 12 marked "Specified in round 50".

## 2. Close code differences

- [x] 2.1 Accept the options body only as a JSON object (requirement 1, design code difference 1).
  - Change: in `src-tauri/src/billing_store.rs`, give `CredentialIssueRequest` a `Deserialize` implementation that decodes a private `#[serde(deny_unknown_fields)]` field struct through `protocol::deserialize_object`, exactly as `OpenRequest` and `TopupRequest` do (protocol.rs:692–725). Make `deserialize_object` `pub(crate)`. Keep `Serialize` and the field types unchanged. The issuance and rotation routes then refuse a non-object body as 400 `invalid_request`, and native `credential_issue` and `credential_rotate` refuse non-object options with "invalid credential options" before any provider request.
  - Unit regression: `serde_json::from_slice::<CredentialIssueRequest>` refuses `[1000,null,1,["read"]]`, `"x"`, `null` and an object with a repeated `scopes` member, and accepts the object form.
  - Router regression in `inference_owner_control_test`, reusing the credential-mutation fixture:
    - with a valid owner proof bound to `[<now+3600>, null, 1, ["read"]]`, issuance and rotation answer 400 `invalid_request`;
    - the credential rows are unchanged, checked through an independent ledger connection;
    - the rotation's named credential still authenticates;
    - the object form with a fresh proof then succeeds.
  - Native regression in `consumer::tests` with the funding fixture: `credential_issue(id, json!([...]))` fails and the fixture records no control request.
  - All three regressions fail without the change.

  Done 2026-11-19: `CredentialIssueRequest` (`billing_store.rs:43–72`) decodes a private `deny_unknown_fields` field struct through `protocol::deserialize_object`, now `pub(crate)` (`protocol.rs:12`). `Serialize`, the field types and the decoding of an omitted `spend_cap_wei` as `null` (open question 1) are unchanged. Regressions:
  - unit `billing_store::tests::credential_options_decode_only_from_one_object_without_repeated_members`: the array, `"x"`, `null` and a repeated `scopes` are refused, also from a `serde_json::Value`; the object form decodes and re-encodes unchanged;
  - router `credential_mutations_refuse_positional_options_after_consuming_owner_proof` (`inference_owner_control_test`): issuance and rotation answer 400 `invalid_request` and consume the proof. The complete credential rows are unchanged through an independent connection, and the rotation's named credential and the account's other credentials still authenticate. The object form with a fresh proof then succeeds;
  - native `consumer::tests::native_credential_options_refuse_a_positional_array_before_any_provider_request`: `credential_issue` and `credential_rotate` fail with "invalid credential options". The funding fixture's new `controls` recorder sees no request, and the account file, the provider's credentials and the default are unchanged. The object form then sends exactly challenge, list, challenge and issuance.

  With the derived `Deserialize` restored, all three fail. The unit test fails at its first assertion, because the array decodes. The native test fails because the issuance succeeds, and the router test because the issuance answers 200 instead of `invalid_request`. The repeated-member case already passed before the change, because serde's derived decoder refuses duplicate members; it stays as a guard. Restored file compared byte-identical (`cmp`).
- [x] 2.2 Keep a valid native default when explicit issuance cannot see it in the bounded list (`inference-funding-settlement` *Fund and resume recover the saved agreement without rotating credentials*; design code difference 2).
  - Change: in `src-tauri/src/consumer.rs` `credential_issue`, decide whether the stored default is valid from evidence that the 100-entry list bound cannot hide, for example:
    - when the default is absent from the list, check that credential directly at its provider: send the stored bearer to an `infer`-scoped route, where 401 means revoked or expired and 403 `credential_scope` means not inference-capable;
    - or, after open question 3, have the provider list every credential that can still authenticate.
  - A transport failure during that check must fail the issuance, not clear the default.
  - Regression in `inference_owner_consumer_test`, with a native consumer against the production control router:
    - open a free account natively, so that its default is the first-opening credential;
    - through the ledger, issue and revoke 100 further credentials for that account;
    - run native `credential_issue` with `infer` and `read`;
    - then `credential_export(id, None)` returns the original credential identifier and secret, the original still authenticates, and the new credential is stored but not the default.
  - Control case: after the default is revoked at the provider, the same issuance makes the new credential the default.
  - The regression fails without the change.

  Done 2026-11-19: when the stored, locally unexpired default is in the provider's list, native `credential_issue` still decides from that entry. When it is absent, `default_still_infers` (`consumer.rs:2162–2197`) sends the stored bearer to the account's own provider, over the same validated transport as inference, on `GET /v1/models`. Then:
  - a 200 model list keeps the default;
  - 401 `invalid_api_key` (unknown, revoked or expired) or 403 `credential_scope` (no `infer`) clears it, and an `infer` issuance becomes the default;
  - a transport or body failure, and any other answer (a 401 or 403 with another code included), fails the issuance before any credential is created. The default and the account file are unchanged.

  Why this approach: it needs no answer to open question 3 and changes no provider route or wire contract. No owner-authenticated read of one credential exists, and the credential list is the only credential read. The probe also keeps requirement 6. `/v1/models` (`inference_gateway.rs:242–259`) authenticates the bearer and its `infer` scope before anything else. It then answers 200, with an empty `data`, when the provider is not accepting, the account is not active or no model is qualified, and it reads no funding readiness, offer, quote budget or engine. So "credential invalid" and "provider not serving" never share an answer. An answer outside the three definite ones is no evidence, so it neither clears the default nor issues on a guess. Regressions in `inference_owner_consumer_test`, with a native consumer against the production control and inference routers. In each, a native free opening makes the root the default, and 100 ledger issue-and-revoke records then push the root out of the list while it still authenticates:
  - `native_issue_keeps_a_valid_default_that_the_bounded_credential_list_omits`: after an `infer`+`read` issuance, `credential_export(id, None)` returns the original identifier and secret, and the original still authenticates. The new credential is stored and authenticates with both scopes, but it is not the default. The exchange is list, probe (200, no owner proof) and issuance;
  - `native_issue_keeps_a_hidden_valid_default_while_the_provider_admits_nothing` (requirement 6): quote budgets are exhausted (`propose` answers 429 `rate_limit`). `accepting` and `funding_ready` are false, and no model is qualified. The production `/v1/models` then answers `{"object":"list","data":[]}` for the stored bearer. Issuance succeeds and keeps the default, and the account view is unchanged;
  - control `native_issue_restores_a_hidden_default_revoked_at_the_provider`: after a provider-side revocation, the probe gets 401, and the same issuance makes the new credential the default;
  - control `native_issue_restores_a_hidden_default_its_provider_reports_without_infer`: a probe answered 403 `credential_scope` makes the same issuance's credential the default, and the old one stays exportable by its identifier;
  - `native_issue_fails_unchanged_when_a_hidden_default_cannot_be_checked`: a lost probe body, a 200 whose body is not a model list, 429 `rate_limit_exceeded`, 426 `upgrade_required`, 503 `provider_draining`, 500 `ledger_unavailable`, 403 `account_forbidden`, 401 `upgrade_required` and 404 each fail the issuance. No issuance request is sent, no credential row is added, and the account file and the default are unchanged. An honest probe then issues and keeps the default.

  With the previous rule restored (absence from the list means invalid), all five tests fail. The regression and the requirement-6 case fail at `credential_export(id, None)`, which returns the new credential. In the probe-failure case, the issuance succeeds. The two control cases pass every behavioural assertion and fail only at their last assertion, which requires the provider's 401 or 403 to the probe. Restored file compared byte-identical (`cmp`). Rotation is unchanged: it selects its default from the rotated identifier, not from the list.

  Adversarial review, 2026-11-19: the product change was confirmed and not changed. The four original tests still passed when the probe's verdict was weakened to "any 200 keeps the default" and "403 `credential_scope` is no evidence". The 403 control case and the 200-not-a-list, 429 and 426 answers were added, and both weakened rules now fail (the control case at the issuance, the probe-failure case because the 200 issues). Restored file compared byte-identical (`cmp`). `/v1/models` calls no rate limit, body-reader slot or ledger write and creates no request record. Neither the 120-per-minute window nor a paused or frozen account affects it, and the stored bearer goes only to the saved offer's origin through the pinned client, which follows no redirect and uses no proxy.

## 3. Acceptance evidence per requirement

Task 3.N corresponds to requirement N in the spec and to row N of the design coverage table. A task is checked only when existing passing tests prove every statement and scenario of its requirement. An unchecked task names the missing case.

- [x] 3.1 Credential issuance and rotation accept exactly the four-member options object.
  - Existing evidence: `credential_business_refusals_consume_proof_without_partial_mutations`, `credential_mutations_accept_only_the_account_owner_not_any_bearer` and `caps_concurrency_revocation_and_scope_are_atomic`.
  - Missing, besides task 2.1: a router test in which issuance and rotation are refused, with the credential rows unchanged, for each of the following:
    - `scopes` `[]` and `["admin"]`;
    - `max_concurrency` one above the account's limit;
    - `spend_cap_wei` `"01"`, `"1.5"`, `"-1"`, `""` and the number `1`;
    - an unknown member.
  - Also missing: a positive case issuing `{"spend_cap_wei":"900","max_concurrency":<account limit>,"scopes":["infer","read"]}` whose list entry shows exactly that expiry, cap, concurrency and both scopes.

  Done 2026-11-20: new library tests in `src-tauri/src/billing_store_tests.rs`. They drive the production control router (`contracts::router`) in process with owner proofs recorded through `BillingStore::create_challenge` (as `owner_post` does), over a file-backed ledger with a paid account whose signed concurrency limit is 2 (fixture `CredentialRoutes`, `paid_now`). No case was added to `inference_owner_control_test`.
  - `credential_options_outside_the_contract_change_no_credential`: sends each of `scopes` `[]` and `["admin"]`, `max_concurrency` 3, `spend_cap_wei` `"01"`, `"1.5"`, `"-1"`, `""` and the number `1`, the extra member `"account":"other"` and a repeated `scopes` member, once as an issuance and once as a rotation of a valid credential A, each with a valid proof for exactly that body. Each answers 400 and consumes the proof; the complete credential rows (read through an independent connection) and the account view are unchanged, and A and the opening credential still authenticate. This is also requirement 4's *A refused rotation keeps its predecessor* for the concurrency case. Codes pinned as current implementation values (design decision 4): `invalid_credential` for scopes and concurrency, `invalid_amount` for the four non-canonical cap strings, `invalid_request` for the numeric cap and the unknown or repeated member. It would catch a dropped scope, concurrency or canonical-cap check, a rotation that keeps its revocation of A after the replacement is refused, and an accepted unknown or repeated member.
  - `valid_options_issue_and_rotate_credentials_with_exactly_those_limits`: issuance and rotation with `{"expires_at":<now+3600>,"spend_cap_wei":"900","max_concurrency":2,"scopes":["infer","read"]}` answer 200. The owner-proved list entry of each new credential equals exactly that expiry, cap `"900"`, concurrency 2, both scopes, `revoked:false`, zero spent and reserved and `root:false`. Each authenticates with both scopes at `expires_at − 1` and not at `expires_at`, the account view is unchanged, and the rotated credential is listed revoked. It would catch a stored cap, concurrency, scope or expiry other than the one requested, and an off-by-one expiry boundary.

  Mutations, each restored by copy with a fresh mtime and compared byte-identical (`cmp`): removing `request.max_concurrency > account.max_concurrency` from `issue_credential` fails the first test (the case answers 200); removing the `money(cap)` check fails it at the `"01"` case; storing `spend_cap_wei: None` fails the second test at the list entry. The positional array, non-object bodies and the native path remain task 2.1's evidence.

  Adversarial review, 2026-11-20: the tests were confirmed, and these further mutations each fail the first test (restored by copy with a fresh mtime, compared byte-identical):
  - no `deny_unknown_fields` on the options decoder (the unknown member answers 200);
  - no empty-scopes check (200);
  - a decoder that buffers through `serde_json::Value`, so the last repeated member wins (the repeated member answers 200);
  - rotation that commits the predecessor's revocation in its own transaction before the replacement is checked (a credential row changed after the refused rotation);
  - issuance outside a transaction that inserts its row before checking the cap (a row changed after the refused `"01"` issuance).

  One plausible regression passed: accepting a scope list when any element, rather than every element, is `infer` or `read`, because `[]` and `["admin"]` are refused either way. The case `["infer","admin"]` was added, and that regression now fails at it (200). The positional array `[<now+3600>, null, 1, ["read"]]` was also added to the first test (400 `invalid_request`, no credential row changed, the rotated credential still authenticates), so all three scenarios of requirement 1 now run in the library suite.
- [x] 3.2 The provider keeps only a hash of each secret, separate from the credential's public identifier.
  - Existing evidence: `every_control_read_requires_its_owner_or_an_account_scoped_read_bearer`, `authenticated_free_and_paid_reopening_preserves_credentials_and_funding_without_readiness`, `native_lost_free_opening_recovers_without_issuing_or_rotating_credentials` and `open_replay_never_revives_revoked_or_expired_roots`.
  - Missing: a ledger test that creates credentials by first opening, issuance and rotation on a file-backed ledger, then:
    - reads the bytes of `ledger.sqlite` and of any `-wal` and `-journal` files and finds none of the three secrets;
    - checks that each `token_hash` equals `protocol::hash_hex` of its secret;
    - checks that each list entry has no `token_hash` or secret member;
    - checks that each identifier differs from its secret and neither contains the other.

  Done 2026-11-20: library test `billing_store::tests::the_ledger_keeps_only_a_hash_of_each_secret_and_an_independent_identifier` on a file-backed ledger. Credentials are created by a paid first opening, an issuance and a rotation of the issued one, and a request is held. Then:
  - every file in the ledger directory (`ledger.sqlite`, `-wal`, `-shm` and any `-journal`) is read while the ledger is open, which the test requires to include the `-wal` file, and again after it is closed. None holds any of the three secrets, whole, as the 64 hex digits after `chiral_` in lower or upper case, or as the 32 bytes those digits encode;
  - each row's `token_hash` equals `protocol::hash_hex(secret)` (Keccak-256, a current implementation value);
  - each list entry has exactly the eleven `CredentialView` members, so no `token_hash`, `bearer` or other secret member, and contains no secret. The account view, the request list and request view, a signed receipt and an opening replay contain no secret either;
  - each identifier differs from its secret, from the secret's random part and from its hash, and neither the identifier nor the secret contains the other (also checked between the random hex parts).

  It would catch a secret anywhere in the ledger files in the forms listed above, a hash of something other than the returned secret, a secret or hash member in the list, and an identifier that is, contains or is contained in the secret, its random part or its hash. It does not search for other encodings of a secret, such as base64. Mutation: writing the bearer itself into `token_hash` fails the test (the WAL search is what catches it first); restored and compared byte-identical.

  Adversarial review, 2026-11-20: these mutations each fail the test, restored and compared byte-identical: an identifier made of the secret's random part, an identifier equal to the secret's hash, and a plaintext copy of each new secret in the `metadata` table. A copy of each secret's 32 random bytes as a blob in `metadata` passed. The search was therefore extended to those bytes and to the upper-case hex digits, and that mutation now fails ("open: ledger.sqlite-wal holds a secret, as text or as its random bytes"). "No secret in the provider's logs" still holds by inspection only: the gateway, control router, ledger and authentication modules make no logging call (design, adversarial review).
- [x] 3.3 An account bearer authorizes the inference routes only within its credential's account, scope and lifetime.
  - Existing evidence: `caps_concurrency_revocation_and_scope_are_atomic`, `unauthenticated_incomplete_inference_bodies_are_not_polled` and `llama_streams_preserve_public_modes_hide_metadata_and_settle_only_valid_usage`.
  - Missing: `inference_gateway` router tests, which can extend `ingress_fixture` with `read`-only and `infer`-only credentials:
    - a `read`-only bearer gets 403 `credential_scope` on `GET /v1/models`, the three inference routes and `POST /v1/requests/:id/cancel`, with no request reserved and no cancellation signalled;
    - an `infer`-only bearer gets 403 on `GET /v1/requests/:id`;
    - a revoked bearer gets 401 `invalid_api_key` on each route, and so does a bearer at exactly its `expires_at`, using a ledger time seam or a restored record rather than sleeping.

  Done 2026-11-20: library test `inference_gateway::tests::bearers_outside_their_scope_or_lifetime_change_nothing_on_any_route` (helper `bearer_route`). It extends `ingress_fixture` with a qualified model, an engine that counts every request it receives and holds a `token:0` chat open after one token (`delayed_engine`), and `read`-only, `infer`-only, revoked and expired credentials of the account. Its state snapshot is the account view, the credential list, the request list and the engine request count.
  - The `read`-only bearer gets 403 `credential_scope` on `GET /v1/models` and the three inference routes, with the snapshot unchanged: no request is reserved and the engine receives nothing.
  - The revoked bearer, and a bearer whose restored record has `expires_at` 60 s in the past, get 401 `invalid_api_key` on all six routes with the snapshot unchanged.
  - At exactly `expires_at`, on all six routes: the record is restored with `expires_at` = the current second, and a call counts only when the clock reads that second both before and after it. The provider's own reading was therefore `expires_at`. The test requires one such call per route (it retries when a second boundary intervenes, up to 20 times) and each answers 401 with the snapshot unchanged. There is no sleep and no time seam.
  - With a request of the same account in flight (reserved, the engine stream open), the `read`-only bearer gets 403 `credential_scope` on its cancellation, and the revoked and expired bearers get 401. After each refusal no engine drop arrives within 200 ms, the request is still `reserved` and the snapshot is unchanged: no cancellation was signalled. The `infer`-only bearer gets 403 `credential_scope` on `GET /v1/requests/:id`, and the revoked and expired bearers get 401 there.
  - Positive controls in the same fixture: the `infer`-only bearer completes a chat request; the `read`-only bearer reads its status and the in-flight request's; and the `infer`-only bearer's cancellation answers 202, drops the engine request and settles the request as `cancelled_unknown_usage` (the held call answers 409 `request_cancelled`).

  403 `credential_scope` and 401 `invalid_api_key` are current implementation values (design decision 4). Mutations, each failing the test, restored and compared byte-identical:
  - ignoring the scope in `inference_gateway::authenticate` (the `read`-only `/v1/models` answers 200);
  - `expires_at < now` instead of `<= now` in `BillingStore::authenticate` (the exact-expiry `/v1/models` answers 200; the past-expiry cases still pass, so the boundary loop is what catches it);
  - not refusing revoked credentials there (200);
  - cancellation requiring `read` instead of `infer` (the `read`-only cancel answers 202).

  Account isolation of request reads stays with `inference-request-accounting`, as requirement 3 says.

  Adversarial review, 2026-11-20: requiring `read` instead of `infer` on `POST /v1/completions` alone fails the test at the `read`-only completions call (400, not 403), so a scope check that is missing or wrong on one route is caught for that route. Restored and compared byte-identical. No test was changed.
- [x] 3.4 Revocation and rotation act on one named credential of the signed account.
  - Existing evidence: `credential_mutations_accept_only_the_account_owner_not_any_bearer`, `credential_identifiers_cannot_escape_the_signed_account_even_for_the_same_owner`, `credential_business_refusals_consume_proof_without_partial_mutations` and `application_concurrency_rotation_and_revocation_do_not_release_running_holds`.
  - Missing: a router test that rotates an expired credential (a restored record whose `expires_at` has passed) and a revoked credential with valid `infer` options. The replacement must authenticate with the requested scopes, and the predecessor must still be refused.

  Done 2026-11-20: library test `billing_store::tests::an_expired_or_revoked_credential_rotates_into_a_working_replacement`, through the production control router with owner proofs (fixture `CredentialRoutes`). Credential A is either expired, through a restored record whose `expires_at` is one second in the past (no sleep), or revoked by an owner-proved `DELETE` that answers `{"revoked":true}`. A then gets 401 `invalid_api_key` on `GET /v1/models`. The owner rotates A with `infer` (expired case) or `infer`+`read` (revoked case), concurrency 1, no cap and a one-hour expiry. In both cases:
  - the rotation answers 200;
  - the replacement belongs to the account, has a new identifier and secret, and authenticates with exactly the requested scopes, also in its list entry (unrevoked, concurrency 1);
  - on the public router the replacement lists the qualified model on `/v1/models`. On `/v1/requests/:id` it gets 403 `credential_scope` when it has only `infer`, and 404 `request_not_found` when it also has `read`;
  - A still does not authenticate (401 on `/v1/models`) and is listed revoked;
  - the account's other credential and the opening credential still authenticate, and the account view is unchanged.

  It would catch rotation refusing an unusable predecessor, and rotation leaving its predecessor unrevoked. Mutations, each failing the test, restored and compared byte-identical:
  - refusing rotation when the predecessor is revoked or expired (400 instead of 200, at the expired case);
  - refusing it only when revoked (400 at the revoked case);
  - not revoking the predecessor (the expired A is listed unrevoked).

  *A refused rotation keeps its predecessor* with `max_concurrency` above the account's limit is covered by task 3.1's first test.

  Adversarial review, 2026-11-20: removing the account check from `rotate_credential`, or from `revoke_credential`, passed every library test. Only `credential_identifiers_cannot_escape_the_signed_account_even_for_the_same_owner` in `inference_owner_control_test` covered *An identifier from another account is refused*, and that target was not run in this round. A library test was added: `billing_store::tests::an_identifier_of_another_account_of_the_same_owner_is_refused`. It uses the production control router and owner proofs. Accounts X (paid) and Y (free) have the same owner. An owner-proved revocation and rotation for X naming Y's credential C each answer 404 `not_found` (a current implementation value) and consume the proof. The credential rows of both accounts and both account views are unchanged, and C still authenticates for Y. As a control, the same rotation addressed to Y answers 200. Both mutations now fail it (200 instead of 404), restored and compared byte-identical.
- [x] 3.5 Credential limits restrict authority over the same prepaid balance.
  - Existing evidence: `caps_concurrency_revocation_and_scope_are_atomic` and `authenticated_free_and_paid_reopening_preserves_credentials_and_funding_without_readiness`.
  - Missing: a ledger test on a paid account with 1,000 wei available. Issuing a credential with cap `1000000000000000000` leaves the account view unchanged. A reservation with maximum cost 2,000 wei fails with `insufficient_funds`, not `spend_cap`. Revoking a credential with cap 900 that has spent 100 leaves the account view, the funding history and the fee batches unchanged.

  Done 2026-11-20: library test `billing_store::tests::credential_caps_restrict_authority_and_never_create_or_refund_money` on a file-backed ledger with a paid account: 1,005 wei funded and a 5-wei platform fee leave exactly 1,000 wei available. The money state compared throughout is:
  - the account view and the signed agreement;
  - the funding history;
  - the fee batches and the operator fee summary;
  - the row counts of `funding`, `funding_attempts`, `fee_obligations` and `fee_batches`.

  Then:
  - issuing a credential with cap `1000000000000000000` leaves the money state unchanged;
  - that credential's reservation of 1,000 + 1,000 tokens (maximum cost 2,000 wei) fails with `insufficient_funds`, not `spend_cap`, records no request and reserves nothing on the credential;
  - a credential with cap 900 spends exactly 100 wei (account: spent 100, available 900, the list entry spent 100, reserved 0). Revoking it leaves the money state unchanged, so the 800 wei of unused cap is credited, returned or refunded nowhere and no funding, refund or fee record appears;
  - a rotation of the large-cap credential leaves the money state unchanged;
  - a credential that expires (refused at its `expires_at`, and its reservation refused) leaves the money state unchanged.

  It would catch a cap treated as credit, a missing balance check, and any revocation, rotation or expiry that moves money. Mutation: removing the `insufficient_funds` check from `reserve` fails the test (the reservation fails with "ledger invariant: monetary underflow"); restored and compared byte-identical. "No native surface presents a cap as a balance or refund guarantee" holds by inspection: the GUI issues credentials without a cap (`Usage.svelte:87`), the CLI prints the provider's credential metadata, and the GUI's presentation belongs to `desktop-gui-pages`.

  Adversarial review, 2026-11-20: two further mutations each fail the test, restored and compared byte-identical. The first is a balanced one-wei refund on revocation (spent −1, available +1, so the account still balances): the money state differs after the revocation. The second reports `spend_cap` instead of `insufficient_funds` when the credential has a cap: the large-cap reservation fails with `spend_cap`. No test was changed.
- [x] 3.6 Credential management stays reachable without new admission.
  - Existing evidence: `authenticated_free_and_paid_reopening_preserves_credentials_and_funding_without_readiness` (issuance and rotation).
  - Missing: in the same closed-gate state (`accepting` and `funding_ready` false, no qualified model, exhausted quote budgets), owner-authorized revocation and an owner-proof `GET /v1/contracts/:id/credentials` both succeed, and the account view is unchanged.

  Done 2026-11-20: library test `billing_store::tests::credential_management_stays_reachable_while_every_admission_gate_is_closed`, through the production control router with owner proofs, on a paid account with a settled spend and a held reservation (funded 1,000, fee 5, spent 15, reserved 20, available 960). The test closes every gate:
  - `accepting` and `funding_ready` are false and no model is qualified;
  - both quote budgets (`quote:global` and `quote-work:global`) are exhausted through restored rate state and watermarks.

  It first shows each gate closed: `POST /v1/contracts/propose` answers 503 `provider_draining`, `qualified_model` is none, `/v1/models` answers `{"object":"list","data":[]}` and quote signing is refused with `rate_limit`. Then each operation answers 200: an owner-proved `GET /v1/contracts/:id/credentials`, an issuance, a rotation and an owner-proved revocation (`{"revoked":true}`), and a final list shows each credential's revocation state. Before and after each operation, the following are unchanged:
  - the account view (including model and price), so nothing is renegotiated;
  - the signed agreement;
  - the funding, funding-intent, fee-obligation, fee-batch and request rows;
  - the exhausted budgets.

  This also repeats issuance and rotation from the existing `authenticated_free_and_paid_reopening_preserves_credentials_and_funding_without_readiness`. Mutations, each failing the test, restored and compared byte-identical:
  - requiring `accepting` before credential mutations (the issuance answers 503);
  - requiring it before revocation only (the revocation answers 503);
  - requiring a qualified model for the credential list (503).

  Adversarial review, 2026-11-20: requiring new-funding readiness before revocation, or before the credential list, fails the test (503). Requirement 6 also names opening capacity and a live or current offer, which the test did not close or assert. Gating issuance on the active funding-intent capacity passed the test. The test now also:
  - restores `MAX_PENDING` (10,000) unopened funding intents whose admission is a day ahead;
  - shows a new opening refused with `rate_limit: active funding intent capacity reached`, with `accepting` and the model passed as available (its quote is restored directly because the quote budgets are exhausted);
  - asserts that no offer is registered;
  - keeps the count and admission times of those intents in the compared state.

  That mutation now fails the test: the issuance answers 429. Restored and compared byte-identical.
- [ ] 3.7 The native consumer keeps credential secrets only in its protected account store.
  - Existing evidence: `persisted_credentials_are_private_and_symlinks_rejected` and `agreement_exposes_only_public_disclosure_terms`.
  - Missing: a Unix consumer test that, after native opening, issuance and one inference dispatch, checks:
    - the account directory and the account file are owned by the user with no group or other permission (currently 0700 and 0600), and the account file is a regular file;
    - no other file under the profile's `data_dir` contains either secret, including the request-key file.
  - Missing: a test in which an account file of mode 0640, and one replaced by a symbolic link, makes `credential_export`, `credential_issue`, `credential_rotate`, `credential_revoke` and `infer` fail without contacting the provider, with the file mode, the link and its target unchanged.
  - Missing: a native Windows run in which an account file readable by another account is refused.

  Portable part done 2026-11-20. Only the native Windows run remains. It cannot run on this macOS host, and its code path (`wallet_windows.rs`: `validate`, `create` and `directory`) is not exercised here. Two new Unix library tests in `consumer::tests` use the funding fixture. The fixture now also records each inference request: the identifier of the credential whose secret it presented, and the body.
  - `native_credential_secrets_live_only_in_the_owner_only_account_file` runs a paid native opening, an `infer`+`read` issuance, a `read` issuance rotated with `read`, and one chat dispatch, which presents the default and records one request key. Then:
    - the consumer root and the account directory are directories, not links, owned by this user with no group or other permission. The account file is a regular file with the same protection, under the network-scoped `<data_dir>/<network-hash>/consumer/`;
    - each stored secret (the opening's, the issuance's and the rotation's replacement) equals the provider's, is in the account file and is exportable;
    - no other file under `data_dir` holds any of the four secrets the provider issued, including the rotated-away one, whole or in upper case. The test requires that the search covered the request-key file, the wallet journal, the wallet key and the lock file. No symbolic link exists under `data_dir`, and the inference body holds no secret.

    It would catch a secret written to any other file (a request-key file, a leftover temporary file, the wallet journal), a secret in the inference body, a store directory or account file with group or other permission, and a store outside the network-scoped directory.
  - `an_unprotected_or_linked_account_store_fails_closed_without_contacting_the_provider` covers six cases (the 0604 and 0705 cases were added by the adversarial review below):
    - an account file of mode 0640;
    - an account file of mode 0604 (other permission only);
    - the account file replaced by a symbolic link to a protected (0600) copy in another protected directory;
    - the account directory at mode 0750;
    - the account directory at mode 0705 (other permission only);
    - the account directory replaced by a symbolic link to a protected copy.

    In each case, `credential_export` (default and named), `credentials` (the list), `credential_issue`, `credential_rotate`, `credential_revoke` and `infer` fail with the store's protection error. These errors are current implementation values: "secret file must be owned by this user with no group/other permissions", "cannot open protected secret file", "private directory must belong to this user and have mode 0700" and "private directory cannot be a symlink". No error holds the secret. The fixture records no control or inference request, and the provider's credentials are unchanged. Every entry under the consumer root and under the link targets' directory is unchanged: its kind, permission bits, contents and link target. So nothing was widened, repaired or replaced. After a manual repair, the same store exports its default again, so each refusal came from the protection alone.

  Mutations, each failing its test, restored by copy with a fresh mtime and compared byte-identical (`cmp`):
  - a protected copy of the bearer written beside the account file at dispatch fails the first test ("… holds a credential secret"). Adding the bearer to the request-key record also fails it, earlier, because `local_requests` can no longer decode the record;
  - `private_write` copying instead of renaming, so its temporary file remains, fails the first test at the leftover `.tmp` file;
  - `open_secret_file` without `O_NOFOLLOW` fails the second test ("linked file: export succeeded");
  - `validate_secret_file` without the group/other check fails the second test ("group-readable file: export succeeded");
  - `private_read` narrowing an unprotected file to 0600 and retrying, which is a repair, fails the second test (the same message).

  "No secret on a command line, in a log or in a diagnostic" holds by inspection and by the task 3.8 tests. `consumer.rs` starts no process and makes no print or logging call, no CLI argument takes a secret, and no `chiral credentials` command prints a secret on stderr. Protecting an operator's backup is the operator's responsibility (book lines 474 and 673) and has no code path. Network scoping across identities is also shown by `network_scoping_and_process_mutation_locks`.

  Adversarial review, 2026-11-20. Three further mutations passed both tests:
  - `validate_secret_file` checking only group permission (`mode & 0o070`), so an account file readable by others is accepted;
  - `secure_directory` checking the same way, so an account directory open to others is accepted;
  - the request-key file named after the presented bearer (`<bearer>-<request_key>.json`), so a secret is written into a file name under `data_dir`.

  The tests now cover these. The fail-closed test adds the 0604 file and 0705 directory cases, and also runs the credential list in every case. The store test now requires that no file or directory name under `data_dir` holds a secret. That check runs before any assertion message names a path, and its own message names none. With the same mutations, the tests now fail with "other-readable file: export succeeded", "other-accessible directory: export succeeded" and "a file or directory name under data_dir holds a credential secret". A fourth mutation, `secure_directory` following links (`fs::metadata` instead of `fs::symlink_metadata`), already failed the second test ("linked directory: export succeeded"). Each was restored by copy with a fresh mtime and compared byte-identical. Not exercised: refusal of a file or directory owned by another user. Creating one needs another account's privileges, so this holds by inspection of the `uid` checks in `validate_secret_file` and `secure_directory` (wallet.rs).
- [ ] 3.8 Native credential commands output metadata, and only an explicit export reveals a secret.
  - Existing evidence: `native_lost_free_opening_recovers_without_issuing_or_rotating_credentials` (issuance result), the two `credential_export_*` tests and `agreement_exposes_only_public_disclosure_terms`.
  - Missing: a CLI process test, in the style of `inference_cli_fund_disclosure_test`, against a loopback development provider. `chiral credentials <contract> list`, `issue`, `rotate` and `revoke`, plus a failing `revoke` of an unknown identifier, print one JSON document or an error, and no secret appears on stdout or stderr. `export --id <id>` prints exactly one JSON document with `api_key`, `base_url`, `model`, `contract_id` and `credential_id`, and its stderr contains no secret.
  - Missing: a consumer test that the result of `credential_rotate` carries no `api_key`, `bearer` or other member holding the replacement's secret, while the secret is stored and exportable.
  - Missing: an automated run of `examples/openai_client.py` and `examples/openai-client.mjs` against a loopback development provider, once with a successful request and once with a failing or interrupted one. It must check that the exported secret appears on neither stdout nor stderr. Today this holds only by inspection (design coverage row 8).

  Portable part done 2026-11-20. Only the automated SDK-example run remains. It needs the Python and Node `openai` SDK packages, and neither is installed on this host: `python3 -c 'import openai'` fails with `ModuleNotFoundError`, and Node's `import('openai')` fails with `ERR_MODULE_NOT_FOUND`. Nothing was installed, so the examples' rule still holds by inspection only (design coverage row 8).
  - New CLI process test `src-tauri/tests/inference_cli_credentials_test.rs`, `credential_commands_print_metadata_and_only_export_prints_a_secret`. The production control and inference routers (`PreparedProvider`) run on isolated loopback. A native free opening runs in process from the same development profile, and then every command runs as its own `chiral` process. Failure messages never print a secret: command output is shown only masked (hex runs of 64 or more digits replaced), and other values only after they are shown to hold no secret.
    - Each of these exits 0 with exactly one JSON document on stdout and nothing on stderr: `list`, `issue --options <file>` (cap 900, concurrency 2, both scopes), `rotate` with the CLI defaults, a read-only `issue`, `revoke` (`{"revoked":true}`), `rotate` of the native default and a final `list`. `issue` and `rotate` print exactly `{contract_id, expires_at, id}`. List entries carry no `api_key`, `bearer`, `secret` or `token_hash` member, and the final list shows each credential's revocation state.
    - `export`, both default and with `--id`, prints exactly one JSON document with exactly `api_key`, `base_url` (the offer origin plus `/v1`), `contract_id`, `credential_id` and `model`, and stderr is empty. Each exported secret has the `chiral_` shape and authenticates at the provider's ledger as exactly that account, credential and scopes. It appears exactly once on its own export's stdout, and no other secret does.
    - Failing runs while the provider answers: `revoke` and `rotate` of an unknown identifier, `issue` with a positional options file, and `export --id` of an unknown identifier. Failing runs at a lost provider: `list`, `issue`, `rotate` and `revoke`. Each exits non-zero with an empty stdout and one `chiral:` line on stderr, and the account file is unchanged. The lines are pinned as current implementation values: `invalid_credential: … (HTTP 401)` twice, `invalid credential options`, `credential is not stored locally`, then `owner challenge unavailable` four times.
    - No management output, successful or failed, holds any of the five secrets on stdout or stderr: not whole, not as its 64 hex digits in either case, and no `chiral_` plus 64-hex string at all.
    - Rotating the default makes the replacement the export without `--id`, and the old default stops authenticating (requirement 9 at the CLI).

    It would catch a secret printed by any management command or in any error line, a second document or diagnostic on stdout, an export with missing or extra members, and an export whose secret is not the named credential's.
  - Library test `consumer::tests::native_credential_management_returns_metadata_and_only_export_reveals_a_secret` covers the functions behind both the CLI commands and the desktop operations `credentials`, `credential_issue`, `credential_rotate` and `credential_revoke`, which desktop.rs passes through unchanged (desktop.rs:100–108 and 170–182, by inspection; the desktop module is not built headless):
    - `credential_issue` and `credential_rotate` return exactly `{contract_id, expires_at, id}`, revocation returns `{"revoked":true}`, and list entries carry no secret member;
    - the rotation's replacement secret is in the account file, and only `credential_export` returns it, with the base URL and the model, equal to the provider's secret;
    - eight failures hold no secret: positional options for issuance and rotation, an unknown credential's export, an unknown account's list, and list, issue, rotate and revoke at a lost provider;
    - no result holds any of the four secrets the provider issued, and the stored default and replacement survive the failures.

    It would catch a rotation, issuance, revocation or list result that carries a secret, an error that carries one, and an export that is not the provider's secret.

  Mutations, each failing its test, restored by copy with a fresh mtime and compared byte-identical (`cmp`):
  - `credential_rotate` returning the whole credential: the library test fails at the member set (`bearer` present);
  - a revocation error that appends a stored secret: the library test fails ("the failed revoke holds a secret");
  - `chiral credentials issue` printing the new credential's export: the CLI test fails at the issue's member set;
  - `chiral credentials revoke` appending the default's exported secret to its error: the CLI test fails ("revoke unknown: stderr holds a secret-shaped string").

  "No CLI argument accepts a secret" still holds by inspection of the clap definitions (bin/chiral.rs). The credential commands take the contract, a credential identifier and `--options <file>`, and the wallet commands take files only.

  Adversarial review, 2026-11-20. A further mutation fails the CLI test: `chiral credentials export` also writing one diagnostic line to stderr, which holds only the key's length. It fails at the export's empty-stderr check. Restored and compared byte-identical. No test was changed for this task.
- [x] 3.9 Rotating the native default keeps a usable default.
  - Existing evidence: `issuance_recovers_remote_revocation_and_expiry_but_read_only_rotation_never_becomes_default` and `explicit_credential_issue_recovers_default_without_rotating_on_resume`.
  - Missing: a consumer test in which the native default is expired, in a second case revoked at the provider, and is rotated with `infer` and `read`. Native `infer` must then succeed with the replacement, and `credential_export(id, None)` must return the replacement's identifier.

  Done 2026-11-20: library test `consumer::tests::rotating_the_native_default_keeps_a_usable_default`. It uses the funding fixture with a free opening, plus a valid `infer`+`read` spare credential that is not the default. There are three cases, plus the `infer`-only case added by the adversarial review below:
  - **Expired.** The default's `expires_at` is one second past, both at the provider and in the stored record. Before the rotation, `infer` fails with "credential expired" and sends no inference request, and `credential_export(id, None)` fails the same way.
  - **Revoked at the provider only.** `infer` presents only the default and fails with `invalid_credential`. It is not retried with the spare, and `credential_export(id, None)` still returns the default.
  - In both of these cases, `credential_rotate` with `infer` and `read` then has these effects:
    - the provider lists the old default revoked and the replacement with both scopes;
    - `credential_export(id, None)` returns the replacement's identifier and the provider's secret for it;
    - native `infer` succeeds and presents exactly the replacement;
    - after a restart, the export without an identifier still returns the replacement.
  - **Read-only replacement.** Rotating a valid default with only `read` leaves no default. `credential_export(id, None)` and `infer` fail with "no active inference credential" and send no inference request, and after a restart the export without an identifier still fails. An export naming the replacement returns its provider secret.
  - In every case the valid spare stays stored and exportable by its identifier, and is never selected.

  The task 3.8 CLI process test adds the same rule through the production routers: `chiral credentials <id> rotate <default>` makes the export without `--id` return the replacement, which authenticates with both scopes, and the old default no longer authenticates. Refusals in this test go through a check that panics with fixed text, never with a returned value that could hold a secret.

  Mutations, each failing the test, restored by copy with a fresh mtime and compared byte-identical (`cmp`):
  - rotation never activating its replacement: fails at the expired case's export without an identifier;
  - activating the replacement whatever its scopes: fails at the read-only case's export, which succeeds;
  - `credential()` falling back to the newest unexpired stored credential when there is no default: fails at the read-only case's export.

  Adversarial review, 2026-11-20. One mutation passed the test: a rotation's replacement becomes the default only if it has both `infer` and `read`. Requirement 9 makes any replacement with `infer` the default. The test now has a fourth case, **`infer`-only replacement**: a valid default is rotated with `infer` alone. The provider then lists the old default revoked and the replacement with `["infer"]`. The export without an identifier returns the replacement and its provider secret, native `infer` presents exactly the replacement, and after a restart the export still returns it. With the mutation, the test now fails at that case's export without an identifier ("no active inference credential"). Restored by copy with a fresh mtime and compared byte-identical.

## 4. Qualify and integrate

- [ ] 4.1 After tasks 2.x and 3.x are complete, run the following:
  - the full headless test suite under `caffeinate`;
  - `npm test` and the frontend checks;
  - strict validation.

  Confirm the whitepaper checksum is unchanged.
- [ ] 4.2 Archive with `openspec archive specify-account-credentials --yes` only after all tasks pass and after the referenced pending changes are archived, so that the capabilities the spec text names exist in `openspec/specs`. Those changes are `specify-owner-control-authorization`, `specify-inference-request-accounting`, `specify-funding-settlement`, `specify-outbound-http-safeguards`, `specify-desktop-gui-global-rules`, `specify-desktop-gui-pages`, `specify-version-policy` and `specify-profile-path-resolution`. If one of them is abandoned instead, replace the reference before archiving. Never use `--skip-specs`.
