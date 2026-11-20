## Why

The read-only whitepaper (`docs/chiral-book.md`) states the provider's public OpenAI-compatible API and its per-capability model qualification in *API compatibility and model qualification* (lines 579–603). The relevant parts are:

- ordinary bearer authentication, no custom version header for stock clients, and control-protocol compatibility enforced at onboarding (line 581);
- the implemented endpoint table (lines 583–589);
- "A route's existence does not establish capability for every model", readiness probes before publication, and honest failures instead of fake responses (line 591);
- GGUF capability derivation, the artifact identity and the engine's own tokenization (line 593);
- readiness that verifies the actual model, template and context and probes every advertised capability, including streaming, echo/logprobs and tools/structured output (line 595);
- completion `echo:true` and prompt log-probabilities that are never invented (line 597);
- embedding dimensions that match the request and are never silently substituted (line 599);
- separate qualification of tools and JSON-schema output, and the list of surfaces outside this release (line 601);
- stock SDK onboarding and output treated as untrusted data that the provider never executes (line 603).

Other lines add to it: line 337 (the model listing is account- and model-scoped, not the marketplace), line 530 (supported options are validated before execution), line 502 (request bodies stay bounded), line 520 (no rerouting to another provider), line 628 (model, template, tokenization and capability probes gate readiness; a process or health answer is not qualification), line 656 (engine tokenization, metrics and model loading are not public API) and line 77 (no cross-provider execution). `CLAUDE.md` adds: "Qualify each advertised capability with exact tokenizer/template identity and real inference/usage probes", "Generation and embeddings have separate task/capability/limit contracts", "Do not expose tokenization, template, metrics, slots, model management or arbitrary engine endpoints through the public gateway", "Public inference authenticates before body buffering, bounds simultaneous body readers and bytes, and separates body-read deadlines from generation/SSE deadlines" and "Model output is untrusted data, never HTML or executable tool instructions". `docs/provider-qualification.md` (lines 45–47 and 417–418) describes the provisioning and probing procedure.

The comparison pass of 2026-11-18 (`docs/apas-worker/comparison-2026-11-18.md`) lists this contract as missing specification item 13. Existing changes cover parts of it:

- `account-credentials` (pending `specify-account-credentials`) owns the bearer scope each route requires and refusal of revoked or expired credentials; its design deferred "the public API's body bounds and per-capability behaviour" to this item;
- `inference-request-accounting` (pending `specify-inference-request-accounting`) owns `Idempotency-Key`, exact tokenization before reservation, `n = 1` and bounded output, authoritative usage, unknown usage, request status and cancellation;
- `inference-commercial-terms` (pending `specify-inference-commercial-terms`) owns the signed model declaration (revisions, template digest, limits, capability syntax), the managed GGUF binding of model and tokenizer revision, separate generation and embedding tasks, and zero embedding output price;
- `inference-generation-performance` (pending `specify-generation-performance`) owns the token clock, the private engine stream behind both public modes, removal of engine instrumentation from public output, overload refusal and speed qualification;
- `managed-engine-runtime` (pending `specify-managed-engine-launch`) owns the private engine launch and the engine-reported revision, model identifier, slots and context at readiness, and states that "the model, template, tokenization and capability probes (book line 628)" are not specified there;
- `outbound-http-safeguards` (pending `specify-outbound-http-safeguards`) owns the private engine origin, the SDK base URL and the ingress and preflight checks of private routes;
- `client-version-policy` (pending `specify-version-policy`) owns the optional `X-Chiral-Client-Version` check;
- `desktop-gui-global-rules` (pending) owns rendering model output as plain text;
- the target-design `desktop-local-api-server`, `desktop-share-online` and `provider-launch-reputation` build on these routes and probes but are not implemented.

Nothing specifies the rest: the exact public endpoint set and the absence of engine and out-of-scope routes, both generation modes for stock clients, the scope of the model listing, admission only within advertised capabilities, honest failure reporting, the request-body bounds, GGUF capability derivation, the readiness template check and per-capability probes, completion echo and log-probabilities, embedding dimensions, tools as non-executed descriptions, and the whole-model text-inference scope. A later change could therefore relax any of these without a failing requirement.

## What Changes

- Add the `public-inference-api` capability: 12 requirements and 37 scenarios for the current runtime. Each requirement cites its whitepaper lines and comes from the whitepaper or `CLAUDE.md`, not from current code. Values that the book does not state are recorded in `design.md` as current implementation values: status and error codes, byte, reader, batch and parameter bounds, deadlines, the readiness probe bodies and cadence, the listing's provider, account and readiness filter, and the response extensions.
- Specify the public endpoint set, and that engine endpoints and out-of-scope OpenAI surfaces are not served.
- Specify both generation modes for stock clients with only an exported bearer, and a model listing scoped to the credential's agreement.
- Specify admission only within the agreement's advertised capabilities, and honest reporting of each failure the book lists.
- Specify authentication before body reading, bounded concurrent body readers and bytes, and a body deadline separate from generation.
- Specify GGUF capability derivation, the readiness template check and real probes of every advertised capability, completion echo and log-probabilities, embedding dimensions, tools and structured output as non-executed descriptions, and the whole-model text-inference scope.
- In `design.md`, record a coverage table (requirement, code, existing tests, gaps), four code differences, observations, four questions for the whitepaper author and the risks. The code differences and the missing acceptance evidence are unchecked tasks.

## Capabilities

### New Capabilities

- `public-inference-api`: The provider's public OpenAI-compatible inference API in the current runtime: the endpoint set and the routes it never serves, stock-client generation in both modes, the credential-scoped model listing, capability-scoped admission, honest failures, request-body bounds, GGUF capability derivation, readiness template identity and per-capability probes, completion echo and log-probabilities, embedding dimensions, tools and structured output as data, and the whole-model text-inference scope.

### Modified Capabilities

None. The spec references the pending `account-credentials`, `inference-request-accounting`, `inference-commercial-terms`, `inference-generation-performance`, `managed-engine-runtime`, `outbound-http-safeguards`, `client-version-policy` and `desktop-gui-global-rules`. It adds no obligation to them and does not change their requirements. The target-design changes `specify-desktop-local-api-server`, `specify-desktop-share-online` and `specify-provider-launch-reputation` build on these routes and probes; this change does not modify them and does not present them as current behaviour.

## Impact

- Authority: `docs/chiral-book.md` stays read-only and unchanged (SHA-256 `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`).
- Code that corresponds to these requirements:
  - `src-tauri/src/inference_gateway.rs`: `router`, `models`, `infer`, `execute`, `event_stream`, `stream_worker`, `collect_event`, `stream_inner`;
  - `src-tauri/src/inference_backend.rs`: `prepare`, `validate_messages`, `Backend::ready`, `tokenize`, `dispatch`, `qualification_probe`, `probe`, `validate_response`, `StreamNormalization`, `StreamValidation`, `model_health`, `probe_model`;
  - `src-tauri/src/app.rs`: `ProviderState::qualified_model`, `refresh_inner`, `public_router`, `public_headers`;
  - `src-tauri/src/provider_config.rs` and `src-tauri/src/offer_registry.rs`: `validate`, `validate_model`;
  - `src-tauri/src/engine_runtime.rs`: `validate_inventory`, `template_digest`;
  - `scripts/provision-model.py` (`inspect_gguf_set`, `template_source`, `model_spec`) and `scripts/qualify-provider.py` (`inspect_runtime` capability check, `probe_completion_echo`).
- Four code differences were found. Tasks 2.1–2.4 track them:
  - readiness passes, and the provider publishes, a `streaming` capability that it never exercises when the deployment has no generation endpoint;
  - a requested embedding `dimensions` is forwarded to the engine without comparison with the deployment, so a request for other dimensions is reserved, dispatched, reported as an engine failure and released as unknown usage;
  - a failed stream whose error event cannot be queued ends like a normal stream;
  - a completion prompt that the engine prepares into no tokens is refused as a context overflow.
- Missing acceptance evidence is tracked as unchecked tasks 3.1–3.12. The main gaps are router-level tests of the full public router, of `/v1/completions` in both modes, of capability refusals, of the listed failure conditions, of the readiness template and probe failures, of non-streaming echo end to end, of mixed embedding dimensions, of tool calls returned as data, and of non-text content and adapter members; and real pinned-engine evidence for the template digest and echo offsets.
- Four questions for the whitepaper author are recorded in `design.md`:
  - whether SSE on a generation route requires the `streaming` capability;
  - how `tools` or `structured_output` may be offered on a managed deployment;
  - whether the native embedding dimension is signed or listed;
  - whether readiness must repeat its inference probes while a model stays busy.

  All four are nonblocking.
- No wire format, ledger schema, deployment or whitepaper change is part of this specification step. This change grants no live, public-testnet or release qualification.
