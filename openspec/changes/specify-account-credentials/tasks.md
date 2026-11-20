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

- [ ] 2.1 Accept the options body only as a JSON object (requirement 1, design code difference 1).
  - Change: in `src-tauri/src/billing_store.rs`, give `CredentialIssueRequest` a `Deserialize` implementation that decodes a private `#[serde(deny_unknown_fields)]` field struct through `protocol::deserialize_object`, exactly as `OpenRequest` and `TopupRequest` do (protocol.rs:692–725). Make `deserialize_object` `pub(crate)`. Keep `Serialize` and the field types unchanged. The issuance and rotation routes then refuse a non-object body as 400 `invalid_request`, and native `credential_issue` and `credential_rotate` refuse non-object options with "invalid credential options" before any provider request.
  - Unit regression: `serde_json::from_slice::<CredentialIssueRequest>` refuses `[1000,null,1,["read"]]`, `"x"`, `null` and an object with a repeated `scopes` member, and accepts the object form.
  - Router regression in `inference_owner_control_test`, reusing the credential-mutation fixture:
    - with a valid owner proof bound to `[<now+3600>, null, 1, ["read"]]`, issuance and rotation answer 400 `invalid_request`;
    - the credential rows are unchanged, checked through an independent ledger connection;
    - the rotation's named credential still authenticates;
    - the object form with a fresh proof then succeeds.
  - Native regression in `consumer::tests` with the funding fixture: `credential_issue(id, json!([...]))` fails and the fixture records no control request.
  - All three regressions fail without the change.
- [ ] 2.2 Keep a valid native default when explicit issuance cannot see it in the bounded list (`inference-funding-settlement` *Fund and resume recover the saved agreement without rotating credentials*; design code difference 2).
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

## 3. Acceptance evidence per requirement

Task 3.N corresponds to requirement N in the spec and to row N of the design coverage table. A task is checked only when existing passing tests prove every statement and scenario of its requirement. An unchecked task names the missing case.

- [ ] 3.1 Credential issuance and rotation accept exactly the four-member options object.
  - Existing evidence: `credential_business_refusals_consume_proof_without_partial_mutations`, `credential_mutations_accept_only_the_account_owner_not_any_bearer` and `caps_concurrency_revocation_and_scope_are_atomic`.
  - Missing, besides task 2.1: a router test in which issuance and rotation are refused, with the credential rows unchanged, for each of the following:
    - `scopes` `[]` and `["admin"]`;
    - `max_concurrency` one above the account's limit;
    - `spend_cap_wei` `"01"`, `"1.5"`, `"-1"`, `""` and the number `1`;
    - an unknown member.
  - Also missing: a positive case issuing `{"spend_cap_wei":"900","max_concurrency":<account limit>,"scopes":["infer","read"]}` whose list entry shows exactly that expiry, cap, concurrency and both scopes.
- [ ] 3.2 The provider keeps only a hash of each secret, separate from the credential's public identifier.
  - Existing evidence: `every_control_read_requires_its_owner_or_an_account_scoped_read_bearer`, `authenticated_free_and_paid_reopening_preserves_credentials_and_funding_without_readiness`, `native_lost_free_opening_recovers_without_issuing_or_rotating_credentials` and `open_replay_never_revives_revoked_or_expired_roots`.
  - Missing: a ledger test that creates credentials by first opening, issuance and rotation on a file-backed ledger, then:
    - reads the bytes of `ledger.sqlite` and of any `-wal` and `-journal` files and finds none of the three secrets;
    - checks that each `token_hash` equals `protocol::hash_hex` of its secret;
    - checks that each list entry has no `token_hash` or secret member;
    - checks that each identifier differs from its secret and neither contains the other.
- [ ] 3.3 An account bearer authorizes the inference routes only within its credential's account, scope and lifetime.
  - Existing evidence: `caps_concurrency_revocation_and_scope_are_atomic`, `unauthenticated_incomplete_inference_bodies_are_not_polled` and `llama_streams_preserve_public_modes_hide_metadata_and_settle_only_valid_usage`.
  - Missing: `inference_gateway` router tests, which can extend `ingress_fixture` with `read`-only and `infer`-only credentials:
    - a `read`-only bearer gets 403 `credential_scope` on `GET /v1/models`, the three inference routes and `POST /v1/requests/:id/cancel`, with no request reserved and no cancellation signalled;
    - an `infer`-only bearer gets 403 on `GET /v1/requests/:id`;
    - a revoked bearer gets 401 `invalid_api_key` on each route, and so does a bearer at exactly its `expires_at`, using a ledger time seam or a restored record rather than sleeping.
- [ ] 3.4 Revocation and rotation act on one named credential of the signed account.
  - Existing evidence: `credential_mutations_accept_only_the_account_owner_not_any_bearer`, `credential_identifiers_cannot_escape_the_signed_account_even_for_the_same_owner`, `credential_business_refusals_consume_proof_without_partial_mutations` and `application_concurrency_rotation_and_revocation_do_not_release_running_holds`.
  - Missing: a router test that rotates an expired credential (a restored record whose `expires_at` has passed) and a revoked credential with valid `infer` options. The replacement must authenticate with the requested scopes, and the predecessor must still be refused.
- [ ] 3.5 Credential limits restrict authority over the same prepaid balance.
  - Existing evidence: `caps_concurrency_revocation_and_scope_are_atomic` and `authenticated_free_and_paid_reopening_preserves_credentials_and_funding_without_readiness`.
  - Missing: a ledger test on a paid account with 1,000 wei available. Issuing a credential with cap `1000000000000000000` leaves the account view unchanged. A reservation with maximum cost 2,000 wei fails with `insufficient_funds`, not `spend_cap`. Revoking a credential with cap 900 that has spent 100 leaves the account view, the funding history and the fee batches unchanged.
- [ ] 3.6 Credential management stays reachable without new admission.
  - Existing evidence: `authenticated_free_and_paid_reopening_preserves_credentials_and_funding_without_readiness` (issuance and rotation).
  - Missing: in the same closed-gate state (`accepting` and `funding_ready` false, no qualified model, exhausted quote budgets), owner-authorized revocation and an owner-proof `GET /v1/contracts/:id/credentials` both succeed, and the account view is unchanged.
- [ ] 3.7 The native consumer keeps credential secrets only in its protected account store.
  - Existing evidence: `persisted_credentials_are_private_and_symlinks_rejected` and `agreement_exposes_only_public_disclosure_terms`.
  - Missing: a Unix consumer test that, after native opening, issuance and one inference dispatch, checks:
    - the account directory and the account file are owned by the user with no group or other permission (currently 0700 and 0600), and the account file is a regular file;
    - no other file under the profile's `data_dir` contains either secret, including the request-key file.
  - Missing: a test in which an account file of mode 0640, and one replaced by a symbolic link, makes `credential_export`, `credential_issue`, `credential_rotate`, `credential_revoke` and `infer` fail without contacting the provider, with the file mode, the link and its target unchanged.
  - Missing: a native Windows run in which an account file readable by another account is refused.
- [ ] 3.8 Native credential commands output metadata, and only an explicit export reveals a secret.
  - Existing evidence: `native_lost_free_opening_recovers_without_issuing_or_rotating_credentials` (issuance result), the two `credential_export_*` tests and `agreement_exposes_only_public_disclosure_terms`.
  - Missing: a CLI process test, in the style of `inference_cli_fund_disclosure_test`, against a loopback development provider. `chiral credentials <contract> list`, `issue`, `rotate` and `revoke`, plus a failing `revoke` of an unknown identifier, print one JSON document or an error, and no secret appears on stdout or stderr. `export --id <id>` prints exactly one JSON document with `api_key`, `base_url`, `model`, `contract_id` and `credential_id`, and its stderr contains no secret.
  - Missing: a consumer test that the result of `credential_rotate` carries no `api_key`, `bearer` or other member holding the replacement's secret, while the secret is stored and exportable.
  - Missing: an automated run of `examples/openai_client.py` and `examples/openai-client.mjs` against a loopback development provider, once with a successful request and once with a failing or interrupted one. It must check that the exported secret appears on neither stdout nor stderr. Today this holds only by inspection (design coverage row 8).
- [ ] 3.9 Rotating the native default keeps a usable default.
  - Existing evidence: `issuance_recovers_remote_revocation_and_expiry_but_read_only_rotation_never_becomes_default` and `explicit_credential_issue_recovers_default_without_rotating_on_resume`.
  - Missing: a consumer test in which the native default is expired, in a second case revoked at the provider, and is rotated with `infer` and `read`. Native `infer` must then succeed with the replacement, and `credential_export(id, None)` must return the replacement's identifier.

## 4. Qualify and integrate

- [ ] 4.1 After tasks 2.x and 3.x are complete, run the following:
  - the full headless test suite under `caffeinate`;
  - `npm test` and the frontend checks;
  - strict validation.

  Confirm the whitepaper checksum is unchanged.
- [ ] 4.2 Archive with `openspec archive specify-account-credentials --yes` only after all tasks pass and after the referenced pending changes are archived, so that the capabilities the spec text names exist in `openspec/specs`. Those changes are `specify-owner-control-authorization`, `specify-inference-request-accounting`, `specify-funding-settlement`, `specify-outbound-http-safeguards`, `specify-desktop-gui-global-rules`, `specify-desktop-gui-pages`, `specify-version-policy` and `specify-profile-path-resolution`. If one of them is abandoned instead, replace the reference before archiving. Never use `--skip-specs`.
