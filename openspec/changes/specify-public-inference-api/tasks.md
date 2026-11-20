## 1. Specify the public inference API

- [x] 1.1 Read the source material:
  - the read-only book section *API compatibility and model qualification* in full (lines 579–603, every line, including the long lines 591, 593, 595, 597, 601 and 603);
  - the lines it depends on, found by searching the book for "echo", "logprobs", "embeddings", "dimensions", "tools", "structured output", "capabilit", "Idempotency-Key" and "qualif": 37, 73, 77, 167, 337, 343, 353, 502, 520, 530, 536, 550, 558, 569, 609–619, 628 and 656;
  - `docs/provider-qualification.md` (lines 1–60, 150–175 and 395–430);
  - the `CLAUDE.md` rules on per-capability qualification, separate generation and embedding contracts, hidden engine endpoints, public body bounds and untrusted model output;
  - comparison item 13;
  - the pending `specify-account-credentials`, `specify-inference-request-accounting`, `specify-inference-commercial-terms`, `specify-generation-performance`, `specify-managed-engine-launch`, `specify-outbound-http-safeguards`, `specify-version-policy`, `specify-offer-discovery` and `specify-desktop-gui-global-rules`;
  - the target-design `specify-desktop-local-api-server`, `specify-desktop-share-online` and `specify-provider-launch-reputation`.

  Searched `openspec/changes` and `openspec/specs` for existing coverage of each rule, mapped each uncovered whitepaper rule to one requirement, and referenced the owners of the rest instead of restating them.
  Done 2026-11-19: the book's SHA-256 is `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`.
- [x] 1.2 Wrote the proposal, design and `public-inference-api` spec: 12 requirements and 37 scenarios, each requirement citing its book lines. Inspected the code:
  - `inference_gateway.rs`: `router`, `ledger_error`, `authenticate`, `models`, `request_status`, `cancel_request`, `infer`, `execute`, `event_stream`, `stream_worker`, `terminal_status`, `send_chunk`, `collect_event`, `stream_inner` and the tests;
  - `inference_backend.rs`: constants, `prepare`, `validate_messages`, `schema`, `Backend::new`, `ready`, `tokenize`, `dispatch`, `qualification_probe`, `probe`, `read_json`, `normalize_embedding_usage`, `StreamNormalization`, `validate_response`, `StreamValidation`, `model_health`, `probe_model` and the tests;
  - `app.rs`: `qualified_model`, the engine watcher, `refresh_inner`, `public_router`, `ready`, `public_headers` and the readiness fixture;
  - `provider_config.rs` (`validate`), `offer_registry.rs` (`validate_model`), `engine_runtime.rs` (`template_digest`, `validate_inventory`), `performance.rs` (`validate_profile`), `billing_store.rs` (the reservation's unknown-usage bound), `error.rs`;
  - `scripts/provision-model.py`, `scripts/qualify-provider.py` and their tests, and `inference_commercial_terms_test.rs`.

  Recorded in `design.md` the coverage table, four code differences, the implementation values (decision 4), ten observations and four author questions.
- [x] 1.3 `openspec validate specify-public-inference-api --strict --no-interactive` passes. The focused runs behind the coverage table passed on 2026-11-19: 43 library tests, 3 integration tests and 46 Python tests (listed in design *Coverage table*). No code or test was changed. The whitepaper is unchanged.

  Adversarial review, 2026-11-19: every requirement was re-read against book lines 579–603 and the lines it cites, and every code difference against the cited code. Code differences 1–3 were confirmed. Changed:
  - requirement 3 no longer makes the listing conditional on provider, account and readiness state, which the book does not state; that behaviour moved to design decisions 4 and 10, and its scenario was replaced by a second-account check;
  - requirement 8 now verifies the engine-reported model identifier and context for operator-owned engines too, since line 595 does not limit them to managed engines (decision 11);
  - requirement 4 no longer names "sampling metadata or prompt caching" as book-derived classes; requirement 9 cites line 536, and requirement 11 cites lines 77 and 603 (decision 12); requirement 1 references `outbound-http-safeguards` for ingress exposure;
  - code difference 4 (an empty prepared prompt refused as a context overflow) and task 2.4 were added, with a scenario under requirement 6;
  - task 2.3's regression was corrected: a few small events over HTTP/1 drain into the socket buffers, the queue is empty when the deadline fires, and the error event is delivered, so the original regression would pass without the change.

  The cited tests were re-run and passed: 42 library tests (`inference_backend`, `inference_gateway`, `engine_runtime`, `app::tests::failed_refresh_withdraws_the_offers_it_published`), the 3 integration tests and the 46 Python tests.
- [x] 1.4 Append report entry and mark comparison item 13. For the coordinator: append an entry to `docs/apas-worker/report.md` with the four code differences and the four author questions, and mark comparison item 13 in `docs/apas-worker/comparison-2026-11-18.md` as specified by this change.
  Done 2026-11-19 by the coordinator: report entry "Public inference API: the `streaming` capability, tools on managed deployments, embedding dimensions and busy-model probes"; item 13 marked "Specified in round 52".

## 2. Close code differences

- [ ] 2.1 Keep a model not ready when an advertised capability has no probe for its endpoints (requirement 8, design code difference 1).
  - Change: in `src-tauri/src/inference_backend.rs` `qualification_probe`, before the inference probes, fail when the model advertises `streaming` without `chat` or `completions`, or `tools` or `structured_output` without `chat`. The last two already fail through `prepare`; make the rule explicit so that it does not depend on the probe order. Keep the streamed probes of chat and completions.
  - Regression in `app::tests`, reusing `unavailable_provider`, `ready_chain` and `embeddings_engine` as `failed_refresh_withdraws_the_offers_it_published` does: a non-development provider whose only model is an external embedding deployment with capabilities `["embeddings","streaming"]` starts; its health entry is not ready, `qualified_model` is `None`, `funding_ready` is false, no offer is published (`/admin/offers` lists none and the DHT reports no published offer), and `GET /v1/models` with an account bearer lists no model.
  - Control: the same provider with `["embeddings"]` publishes one offer.
  - The regression fails without the change, because the embedding probes alone make the model ready.
- [ ] 2.2 Refuse embedding dimensions the deployment does not produce, before reservation (requirements 6 and 10, design code difference 2).
  - Change: record the deployment's native embedding dimension from readiness's embedding probe (for example in `ProviderState`, keyed by model identifier, replaced on each successful probe and cleared when the model stops being ready). In `infer`, before tokenization and reservation, refuse an embedding request whose `dimensions` differs from the recorded value as an unsupported option (400 `unsupported_parameter` today), and refuse any `dimensions` while no value is recorded. Keep `validate_response`'s length checks as the defence against an engine that disagrees.
  - Router regression in `inference_gateway::tests`: an embedding agreement (the `ingress_fixture` with the offer model's capabilities set to `["embeddings"]` and its output limit to zero before the quote is signed) and a stub engine whose `/v1/embeddings` always returns 2-dimensional vectors and counts its calls. After the recorded dimension is 2:
    - a request with `dimensions: 1` is refused as an unsupported option; the stub receives no `/tokenize` or `/v1/embeddings` call; no request exists for its `Idempotency-Key`; and the account's unknown-usage requests and available balance are unchanged;
    - requests with `dimensions: 2` and without `dimensions` both succeed with 2-dimensional vectors and are settled as completed.
  - The regression fails without the change: the `dimensions: 1` request reaches the engine, answers 502 `engine_error` and is settled as `failed_unknown_usage`.
- [ ] 2.3 End a failed stream with an error the client can see, even when the client has stopped reading (requirement 6, design code difference 3).
  - Change: in `src-tauri/src/inference_gateway.rs`, when `stream_worker` cannot queue its error event, make the response body end abnormally instead of cleanly. For example, give `event_stream` a shared failure flag that `stream_worker` sets before dropping its sender, and have the body stream yield an `Err` once the queue is drained with the flag set, so that the HTTP/1 chunked body is not terminated normally. A successful stream must still end with `data: [DONE]` and a clean end.
  - Regression in `inference_gateway::tests`, driving the router in-process (`tower::Service`, as `llama_streams_preserve_public_modes_hide_metadata_and_settle_only_valid_usage` does) so that no socket buffer drains the queue: a stub engine streams at least six output events, each with one token ID, and then stays open; the model's `request_timeout_secs` is 1. The test sends a streaming chat request, takes the response without polling its body, waits well past the deadline (until `GET /v1/requests/:id` shows `timed_out_unknown_usage`), then polls the body to its end. The body either yields an error event with `engine_timeout` or ends with a body error; it never ends cleanly with only output events.
    - A real HTTP/1 variant is acceptable only if the stub streams more output than the client and server socket buffers hold (for example large-text events within the 64 KiB event bound), so that the queue is still full when the deadline fires.
  - Control: the same request polled promptly against a stub that finishes ends with `data: [DONE]` and a clean end.
  - The regression fails without the change: the body yields the four queued output events and then ends cleanly. A regression with a few small events over real HTTP/1 passes without the change and does not count.
- [ ] 2.4 Refuse an input that the engine prepares into no tokens as invalid input, not as a context overflow (requirement 6, design code difference 4).
  - Change: in `src-tauri/src/inference_backend.rs` `tokenize_one`, answer an empty token list with an invalid-input refusal (400 `unsupported_parameter` or a dedicated invalid-input code), and keep `context_length_exceeded` for a list longer than the context. Optionally refuse an empty completion `prompt` string in `prepare`; do not refuse it when the engine prepares it into at least one token unless the requirement is changed.
  - Regression in `inference_backend::tests`, beside `tokenized_input_must_leave_room_for_authorized_output`: a stub `/tokenize` returning `{"tokens":[]}` makes `Backend::tokenize` of a completion request with `prompt: ""` fail with a code other than `context_length_exceeded`; the existing 128-token case asserts `context_length_exceeded` by code, not only `is_err()`.
  - Router regression: the same stub through `/v1/completions` is refused before reservation, with no request record for its `Idempotency-Key`.
  - The regression fails without the change: the empty list answers `context_length_exceeded`.

## 3. Acceptance evidence per requirement

Task 3.N corresponds to requirement N in the spec and to row N of the design coverage table. A task is checked only when existing passing tests prove every statement and scenario of its requirement. An unchecked task names the missing case.

- [ ] 3.1 The public inference surface is the implemented endpoint set, with no engine or out-of-scope route.
  - Existing evidence: `inference_gateway::tests::llama_streams_preserve_public_modes_hide_metadata_and_settle_only_valid_usage` (unauthenticated `GET` of five engine paths answers 404 from the inference router).
  - Missing: a test of the merged `ProviderRuntime::public_router` in which `/tokenize`, `/apply-template`, `/props`, `/metrics`, `/slots`, `/completion`, `/lora-adapters` and `/infill`, with and without a valid bearer and with `GET` and `POST`, and `POST /v1/responses`, `/v1/images/generations`, `/v1/audio/transcriptions`, `/v1/files`, `/v1/batches` and `/v1/fine_tuning/jobs` with a valid bearer, each answer as an unknown endpoint, while a counting engine fixture receives no request and no request record is created.
- [ ] 3.2 Generation routes serve stock OpenAI-compatible clients in non-streaming and SSE modes.
  - Existing evidence: `llama_streams_preserve_public_modes_hide_metadata_and_settle_only_valid_usage`, `internal_stream_preserves_nonstream_text_and_authoritative_logprobs` and the three disconnect tests (chat over real HTTP/1).
  - Missing: a router test of `/v1/completions` in both modes through a stub engine, with only a bearer and no Chiral header; a streaming test with `stream_options.include_usage` that receives exactly one usage event; and an automated run of `examples/openai_client.py` and `examples/openai-client.mjs` against a loopback development provider (shared with `account-credentials` task 3.8).
- [ ] 3.3 The model listing is scoped to the credential's agreement.
  - Existing evidence: `renewed_qualification_keeps_existing_model_visible_without_rewriting_agreement` (one account lists its own current model). `inference_owner_consumer_test::native_issue_keeps_a_hidden_valid_default_while_the_provider_admits_nothing` covers the empty answer while nothing is admitted, which is implementation behaviour (design decisions 4 and 10), not this requirement.
  - Missing: a router test with two models and one account on each, in which each bearer lists only its own model and never the other.
- [ ] 3.4 Requests are admitted only within the contracted model's advertised capabilities.
  - Existing evidence: `inference_backend::tests::rejects_expansion_and_media_before_tokenization` and `embedding_batch_and_schema_boundaries` (through `prepare` only).
  - Missing: router tests with a counting stub engine, each asserting refusal, no request record for the key and no engine call:
    - a chat request naming another model;
    - a completions-only agreement sending chat and embedding requests;
    - an agreement without `tools` or `structured_output` sending `tools`, a `tool`-role message, an assistant `tool_calls` message, and `response_format` of types `json_schema` and `json_object`;
    - each engine-only member of the unit test sent to `/v1/completions` as well as to chat.
- [ ] 3.5 Public inference authenticates before reading a body and bounds body reading separately from generation.
  - Existing evidence: `unauthenticated_incomplete_inference_bodies_are_not_polled`, `authorized_body_deadline_is_independent_of_generation_timeout`, `body_reader_saturation_never_queues_and_reclaims_slots_on_all_errors` and `sse_rejects_oversized_and_non_sse_events`.
  - Missing: a test that an authenticated request with `Content-Type: text/plain` and an incomplete body is refused without its body being polled.
- [ ] 3.6 Inference failures surface honestly.
  - Existing evidence: `unauthenticated_incomplete_inference_bodies_are_not_polled`, `credential_cap_is_payment_error_not_ledger_failure`, `tokenized_input_must_leave_room_for_authorized_output`, `llama_streams_preserve_public_modes_hide_metadata_and_settle_only_valid_usage` (overshoot) and `body_reader_saturation_never_queues_and_reclaims_slots_on_all_errors`.
  - Missing, besides tasks 2.2, 2.3 and 2.4, router tests with a stub engine:
    - a paid account whose available balance is below the request's maximum cost is refused as insufficient credit, with no hold and no generation call;
    - a second request while the model's only slot is busy is refused as saturation without queueing;
    - an engine whose `/health` fails makes a request fail as an unavailable engine before reservation, with no generated text;
    - a prompt whose tokens plus the output limit exceed the context is refused before reservation;
    - an engine stream that emits output and then an `error` event yields an error event to a reading client, and the request is not recorded as completed.
- [ ] 3.7 GGUF provisioning derives endpoint capabilities from the task and the selected template.
  - Existing evidence: `test_generation_identity_and_embedded_template`, `test_embeddings_do_not_publish_chat_even_with_embedded_template`, `test_named_template_is_explicit_and_bound`, `test_template_identity_uses_effective_source_without_trimming_content`, `test_generation_rejects_embedding_model`, `engine_runtime::tests::embedding_inventory_rejects_selected_templates_even_when_hashes_match` and `immutable_template_and_base_model_must_match_configuration`.
  - Missing: a `scripts/test_provision_artifacts.py` test that a generation GGUF without any embedded template validates with capabilities `["completions"]` and no template file; and one that provisioning with the named `tool_use` template selected yields `["chat","completions"]`, without `tools`. Engine-side tokenization (no separate tokenizer) holds by inspection of `Backend::tokenize`.
- [ ] 3.8 Readiness verifies the template identity and probes every advertised capability before admission and publication.
  - Existing evidence: `app::tests::failed_refresh_withdraws_the_offers_it_published` (embedding probes through a stub make models ready and published).
  - Missing, besides task 2.1, stub-engine tests of `qualification_probe` or `model_health` in which the model is not ready when:
    - an operator-owned (unmanaged) engine reports a different `model_alias`, a different `n_ctx`, or no entry for the model in `/v1/models`;
    - the engine reports a chat template whose SHA-256 differs from `chat_template_sha256`;
    - a `tools` model's engine reports a different `chat_template_tool_use`;
    - a streamed probe ends with `[DONE]` but no final usage;
    - the tool probe returns text instead of a `ping` call;
    - the structured-output probe returns text that is not `{}`.
  - Also missing: a refresh test in which a ready, idle model whose probe starts failing loses readiness and its offer is withdrawn; and a router test that a non-streaming chat request is collected from the engine stream into one `chat.completion` (design observation 1).
- [ ] 3.9 Completion echo preserves the exact prompt and never invents input probabilities.
  - Existing evidence: `echo_is_not_sampled_output_and_completion_logprobs_use_public_shape`, `internal_stream_preserves_nonstream_text_and_authoritative_logprobs`, `context_limit_stop_preserves_valid_usage_but_rejects_early_truncation_and_false_totals`, `chat_sample_ids_are_private_and_requested_logprobs_are_preserved` and `completion_finish_is_split_into_one_authoritative_checkpoint`.
  - Missing: router tests through a stub completion engine with `echo: true` and `logprobs: 1`:
    - streaming: one event carries exactly the prompt with `logprobs: null`, and the following events carry the generated tokens' log-probabilities;
    - non-streaming: the text is the prompt followed by the generated text, the log-probability arrays have one entry per completion token, the first text offset equals the prompt's length, and usage counts only generated tokens;
    - the same with a multibyte prompt such as `héllo`.
  - Also missing: a stronger `probe_completion_echo` check of the offsets and of generated-only coverage (design observation 7).
- [ ] 3.10 Embedding dimensions match the request and are never substituted.
  - Existing evidence: `invalid_embedding_payloads_cannot_be_settled_as_success`.
  - Missing, besides task 2.2: a `validate_response` test in which a two-input batch returns vectors of lengths 2 and 3 without `dimensions` and is refused.
- [ ] 3.11 Tool and structured-output requests are descriptions, and their results are data the provider never executes.
  - Existing evidence: `embedding_batch_and_schema_boundaries` (remote `$ref` refused).
  - Missing: a router test with a `tools` model and a stub engine streaming a `ping` call in fragments, in which a non-streaming response carries `message.tool_calls` with the reassembled name and arguments; a `prepare` test refusing a remote `$ref` inside a tool's `parameters` and inside `response_format.json_schema.schema`. That the provider executes nothing holds by inspection: no code path contacts anything but the configured engine.
- [ ] 3.12 Requests stay within whole-model text inference on the provider's own engine.
  - Existing evidence: `rejects_expansion_and_media_before_tokenization` (an `image_url` part).
  - Missing: `prepare` or router tests refusing `input_audio` and `file` content parts and a `lora` member before reservation. That a failed request is never sent to another provider or engine holds by inspection of `Backend::dispatch`.

## 4. Qualify and integrate

- [ ] 4.1 Record pinned-engine evidence on an isolated, authorized private deployment: readiness passes for a chat model whose embedded template ends with a newline (design observation 4); a `/v1/completions` echo request with log-probabilities in both modes shows the prompt event with `logprobs: null` and offsets after the prompt; an embedding request with `dimensions` equal to and different from the native value behaves as requirement 10 states after task 2.2. This is not public-testnet or release qualification.
- [ ] 4.2 After tasks 2.x and 3.x are complete, run the following:
  - the full headless test suite under `caffeinate`;
  - `python3 -m unittest discover -s scripts -p 'test_*.py'`;
  - `npm test` and the frontend checks;
  - strict validation.

  Confirm the whitepaper checksum is unchanged.
- [ ] 4.3 Archive with `openspec archive specify-public-inference-api --yes` only after all tasks pass and after the referenced pending changes are archived, so that the capabilities the spec text names exist in `openspec/specs`. Those changes are `specify-account-credentials`, `specify-inference-request-accounting`, `specify-inference-commercial-terms`, `specify-generation-performance`, `specify-managed-engine-launch`, `specify-outbound-http-safeguards`, `specify-version-policy` and `specify-desktop-gui-global-rules`. If one of them is abandoned instead, replace the reference before archiving. Never use `--skip-specs`.
