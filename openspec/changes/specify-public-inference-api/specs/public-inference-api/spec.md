## ADDED Requirements

### Requirement: The public inference surface is the implemented endpoint set, with no engine or out-of-scope route

For bearer-authenticated inference, the provider's public listener SHALL serve exactly these endpoints (book lines 583–589):

- `GET /v1/models`, a credential-scoped model listing;
- `POST /v1/chat/completions`, for qualified text-chat models;
- `POST /v1/completions`, for qualified text-completion models;
- `POST /v1/embeddings`, for a separately configured embedding deployment only;
- `GET /v1/requests/:id` and `POST /v1/requests/:id/cancel`, for request status and explicit cancellation with account isolation.

The scope that each of these routes requires, and the refusal of revoked and expired credentials, are specified by `account-credentials` (*An account bearer authorizes the inference routes only within its credential's account, scope and lifetime*). Request identity, status, cancellation and settlement are specified by `inference-request-accounting`. The listener's other routes (owner control, health, readiness and version policy) belong to their own capabilities.

The public listener SHALL NOT serve or forward any endpoint of the private engine. That includes tokenization, template rendering, engine properties, metrics, slots, model loading or management, adapter loading and the engine's native completion route, and it SHALL NOT forward an arbitrary path to the engine (book lines 595 and 656; `CLAUDE.md`: "Do not expose tokenization, template, metrics, slots, model management or arbitrary engine endpoints through the public gateway"). It SHALL NOT serve the Responses API, image, audio or video endpoints, fine-tuning, batch jobs, or file or adapter uploads (book line 601). A request for any of these SHALL be answered as an unknown endpoint, SHALL reserve nothing and SHALL cause no engine request. That ingress and host exposure never reach the engine or administration listener directly is specified by `outbound-http-safeguards` (*TLS ingress fronts only the public listener, and public HTTPS reachability remains required*).

#### Scenario: Engine endpoints are not public

- **GIVEN** a provider whose private engine serves `/tokenize`, `/apply-template`, `/props`, `/metrics`, `/slots` and `/completion`
- **WHEN** each of those paths is requested from the provider's public listener, with and without a valid account bearer
- **THEN** each is answered as an unknown endpoint
- **AND** the engine receives no request

#### Scenario: Out-of-scope OpenAI surfaces are not served

- **WHEN** a valid account bearer sends `POST /v1/responses`, `POST /v1/images/generations`, `POST /v1/audio/transcriptions`, `POST /v1/files`, `POST /v1/batches` or `POST /v1/fine_tuning/jobs`
- **THEN** each is answered as an unknown endpoint
- **AND** no request is reserved and the engine receives no request

### Requirement: Generation routes serve stock OpenAI-compatible clients in non-streaming and SSE modes

`POST /v1/chat/completions` and `POST /v1/completions` SHALL serve the model of the requesting account in both modes that a stock OpenAI-compatible client uses: one JSON response, and a server-sent event stream when the request asks to stream (book lines 587 and 588). A stock client SHALL need only the exported account secret, sent in ordinary bearer authentication, and the selected origin plus `/v1` as its base URL (book lines 581 and 603).

Chiral control-protocol compatibility is enforced during onboarding, not on stock inference requests (book line 581). That a request without the `X-Chiral-Client-Version` header is not refused for lacking it, and the check of the header when present, are specified by `client-version-policy` (*Native clients state their version and providers refuse a stated version below the minimum*). The derivation of the SDK base URL from a validated origin is specified by `outbound-http-safeguards` (*Advertised endpoints and catalog URLs are origins, and the SDK base URL is derived after validation*). How the private engine executes and times both modes, and the removal of engine instrumentation from every public response, are specified by `inference-generation-performance` (*The token clock uses private per-token engine IDs*).

#### Scenario: A stock client uses both modes with only its bearer

- **GIVEN** an active account whose contracted model is ready and advertises `chat` and `completions`
- **WHEN** a client sends a non-streaming request and then a streaming request to each of the two routes, with only `Authorization: Bearer <exported secret>` and no Chiral header
- **THEN** each non-streaming request receives one JSON completion object, and each streaming request receives a server-sent event stream
- **AND** no response carries engine instrumentation

#### Scenario: The base URL is the origin plus /v1

- **WHEN** an OpenAI SDK is configured with the exported secret and the provider's selected origin plus `/v1`
- **THEN** its chat, completion and model-listing calls reach the routes of this capability without any other configuration

### Requirement: The model listing is scoped to the credential's agreement

`GET /v1/models` SHALL list only the model of the account that the bearer's credential belongs to, that is the deployment that serves that account's agreement (book lines 337 and 585). It SHALL NOT list another account's model, the provider's other deployments or the marketplace (book line 337).

#### Scenario: Each account sees only its own model

- **GIVEN** a provider with two ready models, A and B, and one account on each
- **WHEN** the bearer of the account on A lists models
- **THEN** the list contains model A only
- **AND** the bearer of the account on B sees model B only

### Requirement: Requests are admitted only within the contracted model's advertised capabilities

A route's existence SHALL NOT establish a capability for every model (book line 591). Before reservation and engine dispatch, the provider SHALL refuse:

- a request whose `model` is not the model of the credential's agreement (model mismatch, book line 591);
- a request to `POST /v1/chat/completions`, `POST /v1/completions` or `POST /v1/embeddings` when the agreement's model does not advertise `chat`, `completions` or `embeddings` respectively (book lines 167, 587, 588 and 591; `CLAUDE.md`: "Generation and embeddings have separate task/capability/limit contracts");
- a request that supplies tool definitions, a tool-role message or an assistant tool call when the model does not advertise `tools`, and a request that asks for JSON-object or JSON-schema output when the model does not advertise `structured_output` (book line 601);
- a request member outside the supported OpenAI subset, including a member that would select or alter the private engine's pinned template or tokenization, or return its private instrumentation (book lines 530, 591, 593 and 595).

Bounded output, `n = 1` and the refusal of unsupported expansion are specified by `inference-request-accounting` (*Maximum reservation and nominal usage cost round once*). A refused request SHALL create no reservation and SHALL send nothing to the engine.

#### Scenario: Model mismatch

- **WHEN** an account whose agreement is for model A sends a chat request naming model B
- **THEN** the request is refused as a model mismatch
- **AND** no request is reserved and the engine receives nothing

#### Scenario: A route the model does not advertise

- **GIVEN** an agreement whose model advertises only `completions`
- **WHEN** its bearer sends a request to `POST /v1/chat/completions` or `POST /v1/embeddings`
- **THEN** each request is refused before reservation, and the engine receives nothing

#### Scenario: Tools and JSON output need their capabilities

- **GIVEN** an agreement whose model advertises `chat` and `completions` but neither `tools` nor `structured_output`
- **WHEN** its bearer sends a chat request with a `tools` array, one with a `tool`-role message, one with `response_format` of type `json_schema` and one of type `json_object`
- **THEN** each request is refused before reservation, and the engine receives nothing

#### Scenario: Engine-only members are refused, not forwarded

- **WHEN** a chat or completion request carries `chat_template`, `chat_template_kwargs`, `n_probs`, `return_tokens`, `cache_prompt` or `response_fields`
- **THEN** the request is refused before reservation
- **AND** the member never reaches the engine

### Requirement: Public inference authenticates before reading a body and bounds body reading separately from generation

Each inference request SHALL be authenticated, and its credential's scope checked, before any of its body is read. A request that fails authentication SHALL be refused without waiting for its body (`CLAUDE.md`: "Public inference authenticates before body buffering").

The number of inference request bodies read at the same time SHALL be bounded across the provider. A request that arrives while that bound is reached SHALL be refused, not queued. A body SHALL be refused as soon as it exceeds the byte bound, whether or not its length is declared, before anything beyond the bound is buffered (book line 502: "request bodies ... must remain bounded"; `CLAUDE.md`: "bounds simultaneous body readers and bytes").

Reading a body SHALL have its own deadline, separate from the tokenization, generation and SSE deadlines (`CLAUDE.md`: "separates body-read deadlines from generation/SSE deadlines"). A slow upload SHALL be refused at the body deadline and SHALL NOT consume the request's generation time; the performance clock starts only after the complete bounded body arrives, as `inference-generation-performance` (*Timing is measured at the provider in integer microseconds*) specifies (book line 558). Every outcome, including a refused, malformed or abandoned body, SHALL release the body-reading capacity it held. Model admission is non-queuing as `inference-generation-performance` (*Overload is refused before paid dispatch, not queued*) specifies.

#### Scenario: An unauthenticated incomplete body is not read

- **WHEN** a request to any of the three inference routes, with no bearer or an invalid one, declares a large body and sends only its first byte
- **THEN** it is refused for authentication immediately
- **AND** its body is never read

#### Scenario: Body readers at the bound refuse rather than queue

- **GIVEN** as many authenticated requests as the body-reader bound, each still sending its body
- **WHEN** another authenticated request arrives
- **THEN** it is refused immediately, not queued
- **AND** once a reader finishes, fails or is abandoned, its capacity is available to a new request

#### Scenario: An oversized body without a declared length

- **WHEN** an authenticated request streams a body without `Content-Length` that exceeds the byte bound
- **THEN** it is refused as too large, and no reservation is made

#### Scenario: The body deadline is independent of the generation timeout

- **GIVEN** a model whose generation timeout is longer than the body deadline
- **WHEN** an authenticated request stops sending its body
- **THEN** it is refused when the body deadline passes, not when the generation timeout passes

### Requirement: Inference failures surface honestly

Authentication failure, invalid options, model mismatch, insufficient prepaid credit, saturation, context overflow, an unavailable engine and a failed stream SHALL each be reported to the client as an error that names its condition. None of them SHALL produce a successful response with fabricated, estimated or substituted content (book line 591).

A refusal that the provider can determine from the client's request and the agreement SHALL be reported as that refusal and SHALL NOT be reported as an engine failure; an engine failure SHALL NOT be reported as a client error. A condition found before dispatch SHALL leave no hold and SHALL dispatch no generation. An engine failure, an invalid engine response, or a stream that fails after its output began SHALL be reported as a failure, not as a completed response. A failed stream SHALL NOT end the way a completed stream ends: the client SHALL receive an error event, or the response SHALL end abnormally. The accounting outcome of a failed request is specified by `inference-request-accounting` (*Billable usage is authoritative and matches the reservation* and *Unknown usage releases the hold without inventing execution*). That no engine origin appears in a public error is specified by `outbound-http-safeguards` (*The private engine origin is operator configuration on a private address*).

#### Scenario: Insufficient prepaid credit

- **GIVEN** an account whose available credit is less than a request's maximum cost
- **WHEN** it sends that request
- **THEN** the request is refused as insufficient prepaid credit
- **AND** no hold remains and the engine generates nothing

#### Scenario: Context overflow

- **WHEN** a request's prepared prompt plus its output limit exceeds the contracted context
- **THEN** the request is refused as a context overflow before reservation

#### Scenario: An input that prepares to no tokens is not a context overflow

- **WHEN** a completion request's prompt is prepared by the engine's tokenization into no tokens
- **THEN** the request is refused before reservation as invalid input
- **AND** the refusal does not name a context overflow

#### Scenario: An unavailable engine is not a fabricated answer

- **GIVEN** a ready model whose private engine stops answering its health check
- **WHEN** an account sends a request for that model
- **THEN** the request fails with an error that reports the engine as unavailable or failed
- **AND** the response contains no generated text and no reservation remains

#### Scenario: A failed stream does not end like a completed one

- **GIVEN** a streaming request whose client has stopped reading after the provider's output buffer filled
- **WHEN** the request then fails, for example at its generation deadline
- **AND** the client resumes reading
- **THEN** the response carries an error event or ends abnormally, and it does not end with only output events
- **AND** the request is not recorded as completed

### Requirement: GGUF provisioning derives endpoint capabilities from the task and the selected template

Provisioning a GGUF artifact for the generation task SHALL derive the `completions` capability, and SHALL add `chat` only when an embedded chat template of that GGUF is selected (book line 593). Provisioning for the embedding task SHALL derive exactly `embeddings`, with no chat template and no generation capability (book lines 593 and 601). Provisioning SHALL NOT add `tools`, `structured_output` or `streaming` because of a model card, a repository or file name, or embedded metadata (book line 601: tools and JSON-schema output "are not added automatically by a model card"; line 595: "Model-card claims and successful downloads do not establish compatibility").

The selected full Hugging Face revision, the GGUF shard inventory, the quantization and the template hash SHALL define the artifact, and the tokenizer revision SHALL equal the GGUF revision (book line 593). How a managed deployment binds and verifies them is specified by `inference-commercial-terms` (*Native deployment constraints cannot be bypassed by declaration validity*) and `managed-engine-runtime` (*Runtime and artifact inventories are verified before every launch*).

Input preparation SHALL use the serving engine's own tokenization of the deployed GGUF and, for chat, its selected embedded template. No separate tokenizer, safetensors or other execution path SHALL prepare or execute a request (book line 593). That exact token IDs prepare the input and bound the reservation is specified by `inference-request-accounting` (*Maximum reservation and nominal usage cost round once*).

#### Scenario: A generation GGUF with and without an embedded template

- **WHEN** a generation GGUF with an embedded chat template is provisioned
- **THEN** its derived capabilities are `chat` and `completions`
- **AND** when a generation GGUF without any embedded chat template is provisioned, its derived capabilities are `completions` only

#### Scenario: An embedding GGUF never derives chat

- **WHEN** an embedding GGUF that also carries an embedded chat template is provisioned for the embedding task
- **THEN** its derived capabilities are `embeddings` only, and no template is selected

#### Scenario: A tool-use template does not add the tools capability

- **GIVEN** a generation GGUF whose embedded metadata includes a named tool-use chat template
- **WHEN** it is provisioned with that template selected
- **THEN** its derived capabilities are `chat` and `completions`, without `tools`

### Requirement: Readiness verifies the template identity and probes every advertised capability before admission and publication

A model SHALL NOT become ready, that is admitted for new inference or published, until readiness has verified from the serving engine itself, whether that engine is managed or operator-owned, that it serves the model under the model's API identifier with the model's context length and, when the model advertises `chat`, that it uses the model's selected chat template, whose digest is the signed `chat_template_sha256`. When the model also advertises `tools`, readiness SHALL verify that the engine uses that same template for tool calls (book line 595: readiness "must verify llama.cpp's actual model/template/context"). For a managed engine, the pinned source revision, slot count and per-slot context are verified as `managed-engine-runtime` (*The contract is never reduced to make a model fit*) specifies, and a qualified profile's evidence as `inference-generation-performance` (*A speed class rests only on qualification of the exact profile*) specifies.

Readiness SHALL run real inference, through the same request preparation and engine tokenization that public requests use and with authoritative usage validated against the prepared input and authorized output, for every capability the model advertises (book lines 591, 595 and 628; `CLAUDE.md`: "Qualify each advertised capability with exact tokenizer/template identity and real inference/usage probes"):

- for `chat` and for `completions`, a non-streaming request and a streaming request, the streaming one with log-probabilities of generated tokens;
- for `completions`, an `echo` request whose response SHALL return the exact prompt;
- for `embeddings`, an embedding request over a batch of inputs;
- for `streaming`, a streamed request on each advertised generation endpoint;
- for `tools`, a non-streaming and a streaming request, each of which SHALL produce a well-formed call of the offered function;
- for `structured_output`, a non-streaming and a streaming request, each of which SHALL produce output that conforms to the requested JSON schema.

Each probe SHALL require the expected response shape and authoritative usage consistent with its prepared input and authorized output. When an advertised capability has no probe that applies to the model's endpoints, or when any probe fails, the model SHALL NOT be ready. A later failed probe SHALL withdraw readiness: the model SHALL stop being admitted for new inference and SHALL NOT be published. An engine process, a health answer, a loaded model, a model-card claim, a successful download or a valid declaration SHALL NOT make a model ready (book lines 595 and 628), as `inference-commercial-terms` (*Native deployment constraints cannot be bypassed by declaration validity*) and `managed-engine-runtime` (*Startup is bounded, and a failed launch serves and advertises nothing*) also require. Readiness is not a speed class (`inference-generation-performance`).

#### Scenario: A different template is not ready

- **GIVEN** a chat model whose engine reports a chat template whose digest differs from the signed `chat_template_sha256`
- **WHEN** readiness runs
- **THEN** the model is not ready, it is not admitted for new inference, and no offer is published for it

#### Scenario: A healthy engine whose probe fails is not ready

- **GIVEN** an engine that answers its health check, reports the expected identity and template, and returns a stream without final usage
- **WHEN** readiness runs its streaming probe
- **THEN** the model is not ready

#### Scenario: A tool probe that produces no call

- **GIVEN** a model that advertises `chat` and `tools`
- **WHEN** the engine answers the tool probe with ordinary text instead of a call of the offered function
- **THEN** the model is not ready and no offer is published for it

#### Scenario: Streaming advertised without a generation endpoint

- **GIVEN** a deployment whose model advertises `embeddings` and `streaming` and whose embedding probe succeeds
- **WHEN** readiness runs
- **THEN** the model is not ready, because no probe exercises its advertised `streaming`
- **AND** no offer advertising that capability is published

#### Scenario: A structured-output probe that does not conform

- **GIVEN** a model that advertises `chat` and `structured_output`
- **WHEN** the engine answers the structured-output probe with text that is not JSON conforming to the requested schema
- **THEN** the model is not ready and no offer is published for it

### Requirement: Completion echo preserves the exact prompt and never invents input probabilities

With `echo: true`, `POST /v1/completions` SHALL return the exact prompt text, unchanged, before the generated text, in both modes (book line 597).

The engine supplies no prompt-token probabilities, so the provider SHALL NOT invent, estimate or zero-fill them (book line 597). In a stream, the event that carries only the echoed prompt SHALL have `logprobs: null`. In a non-streaming response, log-probabilities SHALL cover the generated tokens only, and their text offsets SHALL start after the echoed prompt. Requested log-probabilities of generated tokens, and authoritative usage, SHALL remain available with echo (book line 597). The echoed prompt SHALL NOT be counted, billed or timed as generated output (book line 536: output accounting "includes attributable generated tokens, not an estimate from visible text").

A completion that stops at the full context SHALL be accepted only with consistent exact usage (book line 597). Early truncation, a changed prompt count and excess output fail settlement, as `inference-request-accounting` (*Billable usage is authoritative and matches the reservation*) specifies.

#### Scenario: Streamed echo

- **WHEN** a streaming completion request has `echo: true` and asks for log-probabilities
- **THEN** one event carries exactly the prompt with `logprobs: null`
- **AND** the events that follow carry the generated text with the generated tokens' log-probabilities

#### Scenario: Non-streaming echo with log-probabilities

- **WHEN** a non-streaming completion request with the prompt `Hello` has `echo: true`, asks for log-probabilities, and generates the single token ` world`
- **THEN** the text is `Hello world`
- **AND** the log-probabilities list one token, ` world`, whose text offset is the length of `Hello`
- **AND** the usage reports one completion token

#### Scenario: A full-context stop

- **GIVEN** a model with a context of 20 tokens and a prepared prompt of 12 tokens
- **WHEN** the engine stops with 8 completion tokens and a total of 20
- **THEN** the completion is accepted with that usage
- **AND** a stop marked as truncated whose total is 19 (early truncation), or whose total of 20 disagrees with 12 prompt plus 7 completion tokens, is not accepted as a completed response

### Requirement: Embedding dimensions match the request and are never substituted

Every vector in an embedding response SHALL have the same number of dimensions, which are the response dimensions of book line 599. When the request names `dimensions`, that number SHALL equal it (book line 599). The provider SHALL NOT truncate, pad, project or otherwise transform vectors to reach a requested number of dimensions, and SHALL NOT advertise dimension reduction (book line 599).

A request that names a number of dimensions the deployment does not produce SHALL be refused as an unsupported option before reservation and engine dispatch (book line 530: "Before execution the gateway validates supported options"; line 591). It SHALL NOT be dispatched and then reported as an engine failure. An engine response whose vectors do not have the requested dimensions, or do not all have the same dimensions, SHALL be a failed response and SHALL NOT be returned as a success.

The input-only reservation of embeddings is specified by `inference-request-accounting` (*Maximum reservation and nominal usage cost round once*). The zero output limit and price, and the absence of a generation template, by `inference-commercial-terms` (*Native deployment constraints cannot be bypassed by declaration validity*). The absence of a generated-token speed class by `inference-generation-performance` (book line 601).

#### Scenario: The native dimension is served

- **GIVEN** an embedding deployment whose model produces 768-dimensional vectors
- **WHEN** a request names `dimensions` 768, or names no `dimensions`
- **THEN** every returned vector has 768 dimensions

#### Scenario: A reduced dimension is refused, not substituted

- **GIVEN** the same deployment
- **WHEN** a request names `dimensions` 256
- **THEN** it is refused as an unsupported option before reservation
- **AND** the engine receives nothing, no request is recorded for its key, and no vector of any length is returned

#### Scenario: An inconsistent engine response is not a success

- **WHEN** the engine returns a batch whose vectors have different lengths, or a vector whose length differs from the requested `dimensions`
- **THEN** the response is a failure, and no vector is returned to the client

### Requirement: Tool and structured-output requests are descriptions, and their results are data the provider never executes

The provider SHALL accept tools only as function descriptions. It SHALL NOT execute a tool, call a function, fetch a URL or run code on the consumer's behalf, including when the model returns a tool call (book lines 601 and 603: "the provider API does not run returned tools on a consumer's behalf"; line 77: no "customer-supplied code execution"). A JSON schema in a request SHALL NOT cause the provider to fetch a remote resource. Tools and JSON-schema output SHALL be offered only for a model that advertises them after the readiness probes of this capability pass, and SHALL NOT be added because of a model card (book line 601).

Generated text and tool calls, including their arguments, SHALL be returned to the client as data. The provider SHALL NOT treat them as instructions to itself (book line 603; `CLAUDE.md`: "Model output is untrusted data, never HTML or executable tool instructions"). How native clients display model output is specified by `desktop-gui-global-rules` (*Untrusted content renders as plain text*).

#### Scenario: A tool call is returned, not executed

- **GIVEN** a model that advertises `chat` and `tools`
- **WHEN** a request offers a function and the model calls it
- **THEN** the response carries the call's name and arguments as data
- **AND** the provider makes no request and runs no code because of the call

#### Scenario: A remote schema reference is not fetched

- **WHEN** a request for a model with `structured_output` gives a JSON schema containing `"$ref": "https://example.org/schema.json"`
- **THEN** the provider makes no request to that URL

### Requirement: Requests stay within whole-model text inference on the provider's own engine

Chat message content SHALL be text. Image, audio, video and file content SHALL be refused before reservation (book line 601). The provider SHALL NOT load adapters, such as LoRA weights, or any other weights on a request's behalf, and SHALL NOT accept file uploads (book line 601).

The provider SHALL execute an admitted request only on its own configured private engine for the contracted model. It SHALL NOT split a request across providers, forward it to another provider, or retry it on another provider (book lines 77, 520 and 601). Automatic paid retries are excluded as `inference-request-accounting` (*Request identity prevents automatic paid replay*) specifies.

#### Scenario: Non-text content is refused

- **WHEN** a chat message carries an `image_url`, `input_audio` or `file` content part
- **THEN** the request is refused before reservation, and the engine receives nothing

#### Scenario: Adapter loading is not a request option

- **WHEN** a chat or completion request carries a `lora` member
- **THEN** the request is refused before reservation, and the engine receives nothing

#### Scenario: An engine failure is not rerouted

- **GIVEN** a request admitted for a model whose engine then fails
- **WHEN** the provider handles the failure
- **THEN** the request fails on that provider and is not sent to any other provider or engine
