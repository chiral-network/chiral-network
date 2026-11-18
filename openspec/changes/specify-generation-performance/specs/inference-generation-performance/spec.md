## ADDED Requirements

### Requirement: A speed class rests only on qualification of the exact profile

The serving-profile fields, their validation, the allowed policy and classes, and the capacity-tier thresholds are governed by `inference-commercial-terms` (*Complete serving profile identity* and *Declared capacity and execution settings are internally consistent*). This capability governs when a declared speed class is a qualified claim (book lines 92, 99–100 and 544).

A declared speed class SHALL be a qualified claim only when a passing qualification under this capability measured the exact profile it is signed with. That profile binds the model, GGUF artifact and tokenizer, the native engine inventory hash and version, platform and backend, physical device identities, memory kind and headroom, layer, split, KV-cache, attention and batch settings, maximum concurrency, sustained arrival rate, region and qualification evidence. Capacity and speed SHALL be independent. None of the following SHALL establish or imply a speed class: a capacity tier, a model that loads successfully, CPU or system-memory execution, runtime support for a backend, or evidence measured on another operating system, backend or engine.

A change to any profile term other than its `qualification` field SHALL invalidate the existing evidence for that profile. Native readiness SHALL validate the profile's evidence under this capability before the model is admitted for profiled requests or published, and again on every readiness refresh (provider guide line 423). A profiled model whose evidence file is missing or fails validation SHALL NOT be admitted or published as qualified.

#### Scenario: Capacity tier alone is not a speed class

- **GIVEN** a profile on one dedicated CUDA device with 8,188 MiB, which meets the `8gb` tier, and a model that loads and answers its capability probes
- **WHEN** no evidence file for that exact profile passes validation
- **THEN** the model is not admitted for profiled requests and no offer for it is published as a qualified 20, 50 or 100 tokens/second class

#### Scenario: A changed term invalidates the evidence

- **GIVEN** evidence that passes validation for a profile declaring 20 output tokens/second and a model context of 4,096 tokens
- **WHEN** the profile instead declares 100 output tokens/second, or the model context changes to 8,192, while the same evidence file is kept
- **THEN** validation rejects the evidence
- **AND** changing only the `qualification.measured_at` value leaves the profile digest unchanged

#### Scenario: CPU execution cannot qualify

- **WHEN** the managed runtime observation reports CPU execution with system memory
- **THEN** the qualification runner refuses before any paid request, and no speed class is produced

### Requirement: Policy `warm-generation-v1` defines a provider-side warm-generation class

The only performance policy SHALL be `warm-generation-v1` (book line 546). A declared class SHALL be 20, 50 or 100 output tokens/second, with a time to first token (TTFT) of at most 2 seconds, which is 2,000,000 microseconds inclusive. Qualification SHALL require at least 95% joint attainment, as the qualification requirements of this capability define.

The class SHALL be a commitment about warm, admitted requests inside the runtime guarantee envelope, measured at the provider. It SHALL NOT be presented as a guarantee about cold model loading, Internet or transport-path latency, or end-to-end latency at the client. Latency added by hole-punched or relayed paths is outside the guarantee (book line 309), as `p2p-provider-transport` *The transport path does not change qualification or waiver terms* specifies.

#### Scenario: Thresholds are inclusive

- **GIVEN** a profile declaring 20 output tokens/second and an in-envelope request that completes with 256 output tokens
- **WHEN** its TTFT is 2,000,000 µs and its decode interval is 12,750,000 µs
- **THEN** the request attains its class
- **AND** a TTFT of 2,000,001 µs, or a decode interval of 12,750,001 µs, is a miss

#### Scenario: Client-side latency is outside the class

- **GIVEN** a client whose network path adds 300,000 µs before its complete request body reaches the provider
- **WHEN** the provider observes the first generated token 1,950,000 µs after the complete body arrived, and the client sees it 2,250,000 µs after it started sending
- **THEN** the request attains TTFT, and no waiver is granted for the client's 2,250,000 µs

### Requirement: The runtime guarantee envelope decides waiver entitlement

A request SHALL be inside the runtime guarantee envelope only when all of these hold (book lines 548–556):

- its endpoint is generation, `POST /v1/chat/completions` or `POST /v1/completions`, not embeddings;
- its exact prepared prompt, including the applicable chat template, is at most 2,048 tokens;
- its requested maximum output is at most 256 tokens. The requested maximum is the output limit the gateway reserves for the request: the client's `max_tokens` or `max_completion_tokens`, or the gateway's default when the client sets neither. It is never the eventual output;
- its prepared prompt plus requested maximum output is at most 4,096 tokens.

A request outside the envelope SHALL still be served under the ordinary model, context and admission limits, and SHALL NOT be refused for being outside it. It SHALL have no speed-waiver entitlement and SHALL NOT be counted in the withdrawal observation window. A request inside the envelope that ends normally before its requested maximum SHALL be evaluated on its actual output.

#### Scenario: The output budget, not the eventual output, decides

- **GIVEN** a chat request with a 100-token prepared prompt and `max_tokens` 512
- **WHEN** it ends after 100 output tokens with a TTFT of 3,000,000 µs
- **THEN** it is outside the envelope, receives no waiver, and adds no observation to the withdrawal window

#### Scenario: An in-envelope early stop is evaluated on its actual decode

- **GIVEN** a chat request with a 2,048-token prepared prompt and `max_tokens` 256, under a 20 tokens/second class
- **WHEN** it ends normally after 100 output tokens with a TTFT of 1,000,000 µs and a decode interval of 4,950,000 µs
- **THEN** it is evaluated on (100 − 1) / 4.95 s = 20 tokens/second and attains its class

#### Scenario: Template tokens count toward the prompt limit

- **WHEN** a chat request's message text is 2,040 tokens and its rendered chat template adds 12 tokens
- **THEN** its prepared prompt is 2,052 tokens and the request is outside the envelope

#### Scenario: Embeddings are never in the envelope

- **WHEN** a request uses `POST /v1/embeddings`
- **THEN** it has no speed class, no performance waiver and no withdrawal observation

### Requirement: Timing is measured at the provider in integer microseconds

TTFT SHALL start when the complete bounded request body has arrived at the provider's gateway. It SHALL include parsing, validation, admission and tokenization, and SHALL end when the gateway observes the first generated token ID (book line 558). Request-body upload time SHALL NOT be part of TTFT.

For N ≥ 2 output tokens, where N is the authoritative final completion-token count, the decode interval SHALL be the time from the first to the last generated token ID, the decode rate SHALL be (N − 1) / decode interval in tokens/second, and the time per output token (TPOT) SHALL be its inverse. Every timing SHALL be recorded and compared in integer microseconds, never as rounded or displayed throughput:

- TTFT is attained exactly when 0 < `ttft_us` ≤ 2,000,000.
- The decode rate is attained exactly when `decode_us` > 0 and `decode_us` × class ≤ (N − 1) × 1,000,000.

A nonpositive or missing TTFT or decode interval SHALL NOT be evidence of attainment. When no first token has been observed and more than 2,000,000 µs have elapsed since the complete body arrived, TTFT SHALL count as missed. Time the gateway spends waiting for a consumer that has stopped reading, while its bounded downstream buffer is full, SHALL be excluded from the decode interval and the elapsed time.

#### Scenario: A slow upload is not provider time

- **GIVEN** a request body that takes 5,000,000 µs to upload and completes within the body deadline
- **WHEN** the first generated token ID is observed 1,500,000 µs after the complete body arrived
- **THEN** the recorded TTFT is 1,500,000 µs and TTFT is attained

#### Scenario: Integer comparison at the decode boundary

- **GIVEN** a 50 tokens/second class and an in-envelope request with 256 output tokens
- **WHEN** its decode interval is 5,100,000 µs
- **THEN** the decode rate is attained
- **AND** a decode interval of 5,100,001 µs is a miss

#### Scenario: Nonpositive timing is not attainment

- **WHEN** a request with 2 output tokens records a decode interval of 0 µs, or a request records a TTFT of 0 µs
- **THEN** that timing is not evidence of attainment and the request is not recorded as attained

#### Scenario: No first token after two seconds

- **WHEN** no generated token ID has been observed 2,000,001 µs after the complete body arrived
- **THEN** TTFT counts as missed, whatever happens to the request afterwards

### Requirement: The token clock uses private per-token engine IDs

The gateway SHALL time generation from the private engine's sampled per-token IDs, not from SSE event counts, characters or retokenized text (book lines 560 and 595). Every generation request executed under a serving profile SHALL be executed by the engine as a stream, including a request whose client asked for a non-streaming response. For such a client the gateway SHALL reconstruct the ordinary non-streaming response from that stream.

Token IDs and other engine instrumentation SHALL be removed from all public output, streaming and non-streaming. Instrumentation includes `token_ids`, `timings`, verbose metadata and the sampled-token IDs inside log-probabilities. Log-probabilities that the client requested SHALL keep only their public fields.

A valid decode measurement SHALL require exactly one generated ID per output event, and a total of observed IDs equal to the final authoritative completion-token count. The decode measurement SHALL be invalid when an event carries more than one ID or an ID that is not a non-negative integer, when visible output arrives without an ID, or when the observed IDs disagree with final usage. An invalid decode measurement SHALL NOT be evidence of attainment. Settlement of authoritative usage is unaffected and remains governed by `inference-request-accounting`.

#### Scenario: A non-streaming client is timed by the same clock

- **GIVEN** a non-streaming chat request inside the envelope, under a serving profile
- **WHEN** the gateway serves it
- **THEN** the engine streams the generation, and the client receives one ordinary `chat.completion` JSON body with its content and usage
- **AND** the body contains no `token_ids`, `timings` or `__verbose` member, and the request's performance record contains a decode interval

#### Scenario: Batched token IDs establish no decode rate

- **WHEN** an engine output event carries the two token IDs `[10, 11]`
- **THEN** no decode interval is recorded, and the request is not recorded as attained

#### Scenario: IDs that disagree with usage establish no decode rate

- **GIVEN** a stream with one ID-bearing output event
- **WHEN** final authoritative usage reports 2 completion tokens
- **THEN** no decode interval is recorded, and the 2 completion tokens are still settled as usage

### Requirement: Overload is refused before paid dispatch, not queued

A request that would exceed the deployed model's maximum concurrency, or the signed sustained arrival rate over all accounts of that model, SHALL be refused with HTTP 429 before any reservation, private tokenization or engine dispatch (book line 560). It SHALL NOT wait in a queue. Private tokenization SHALL also hold a model concurrency slot. A refused request SHALL create no reservation, no hold, no performance observation and no waiver.

#### Scenario: Concurrency is full

- **GIVEN** a qualified model with maximum concurrency 2 and two requests generating
- **WHEN** a third request arrives
- **THEN** it is refused immediately with HTTP 429 `model_saturated`, and no reservation or hold is created for it

#### Scenario: The arrival rate is exceeded

- **GIVEN** a qualified model with maximum concurrency 1 and `arrival_milli_rps` 1,000, which is one request per second, and a first request that used the arrival allowance and has already completed
- **WHEN** another request arrives 100,000 µs after the first
- **THEN** it is refused with HTTP 429 `rate_limit` before reservation, and nothing is dispatched to the engine

### Requirement: Qualification evidence binds the exact profile and observed runtime

Qualification evidence SHALL use schema `chiral-performance-evidence-v2` (book line 564) and SHALL bind:

- the profile digest, which is the SHA-256 of the ModelSpec in canonical JSON (`inference-wire-foundation` *Deterministic integer-only canonical JSON*) with the serving profile's `qualification` field removed;
- the canonical runtime observation of the managed engine process, and its SHA-256. The observation contains the process ID, the literal-loopback HTTP engine origin, model ID, context length, maximum concurrency, artifact-manifest and runtime SHA-256, engine version, platform, backend, memory kind, devices, memory reserve and every execution setting;
- the measurement time, probe location and transport description;
- each sweep's physical-memory measurements.

The signed profile's `qualification.evidence_sha256` SHALL equal the SHA-256 of the exact evidence file bytes. Its `measured_at` SHALL equal the evidence's measurement time, which SHALL NOT be later than the validator's current time. The runtime observation SHALL equal the signed profile in every runtime, device and execution field, and SHALL equal the model's context length and the profile's concurrency.

Validation SHALL reject evidence for a profile whose model has the `embeddings` capability, a context below 4,096 tokens or an output allowance below 256 tokens (book line 569). It SHALL also reject evidence under another schema, such as an old vLLM or Docker report, and evidence from another engine, backend or operating system. The qualification runner SHALL inspect the managed runtime on its own host before and after measurement. It SHALL refuse when the host's operating system differs from the profile's platform, when the observation differs from the candidate profile, or when the observation changes during the run.

#### Scenario: An altered runtime observation is rejected

- **GIVEN** evidence that passes validation for a profile signing one 32,000 MiB device
- **WHEN** the evidence's runtime observation reports that device with 8,000 MiB
- **THEN** validation rejects the evidence

#### Scenario: The evidence file is bound byte for byte

- **WHEN** any byte of a validated evidence file changes
- **THEN** its SHA-256 no longer equals the signed `evidence_sha256`, and validation rejects it

#### Scenario: A future measurement is rejected

- **GIVEN** evidence and a signed profile with `measured_at` 100
- **WHEN** validation runs at current time 99
- **THEN** validation rejects the evidence

#### Scenario: Another operating system cannot qualify the profile

- **WHEN** the runner is started on a Linux host for a profile whose platform is `macos`
- **THEN** it refuses before any paid request, and no evidence is written

### Requirement: Qualification covers every concurrency, endpoint, mode and prompt boundary

Qualification SHALL be a deployment-specific experiment (book lines 562 and 565–566):

- **Sweeps.** For each path, the private engine and the paid gateway, evidence SHALL hold exactly one sweep at every concurrency from 1 through the advertised maximum. Each sweep's observed peak concurrency SHALL equal its swept level.
- **Endpoints and modes.** Every advertised generation endpoint SHALL be exercised. Engine samples SHALL stream. Gateway samples SHALL cover both streaming and non-streaming clients, through genuinely funded paid agreements.
- **Samples.** Each endpoint/mode SHALL have at least 100 samples in every sweep. The runner SHALL accept 100–1,000 samples per mode.
- **Prompts.** Every prepared prompt SHALL be unique within the evidence. Prompt sizes SHALL be measured with the pinned tokenizer and the actual template. They SHALL fall in the boundary buckets 120–128, 504–512, 1,016–1,024 and 2,040–2,048 tokens, with at least 20 samples per bucket for each endpoint/mode in every sweep.
- **Output.** A passing sample SHALL have exactly 256 output tokens.
- **Warmups.** Warmups SHALL be real requests with unique prompts, and SHALL be excluded from the evidence (provider guide line 419).

#### Scenario: A missing concurrency level is rejected

- **GIVEN** a profile with maximum concurrency 3
- **WHEN** the evidence holds engine and gateway sweeps at concurrency 1 and 3 but not at 2
- **THEN** validation rejects the evidence

#### Scenario: A thin prompt bucket is rejected

- **WHEN** a gateway sweep's non-streaming chat mode has 19 samples in the 2,040–2,048-token bucket
- **THEN** validation rejects the evidence, even if that mode's overall attainment is 100%

#### Scenario: A repeated prompt is rejected

- **WHEN** two samples anywhere in the evidence have the same prompt SHA-256
- **THEN** validation rejects the evidence

#### Scenario: Engine samples must stream

- **WHEN** an engine sweep contains a non-streaming sample
- **THEN** validation rejects the evidence

### Requirement: Qualification passes only with joint attainment everywhere at the declared arrival rate

A qualification sample SHALL pass only when all of these hold (book lines 567–568; provider guide lines 420–421): it has no error; its authoritative usage is known; its prompt is 1–2,048 tokens; it has exactly 256 output tokens; 0 < `ttft_us` ≤ 2,000,000; `decode_us` > 0 and `decode_us` × class ≤ 255,000,000; and its elapsed time is at least TTFT plus the decode interval. Errors SHALL count against attainment. So SHALL offered load that the runner could not dispatch at the declared rate, which it records as a failed sample rather than queueing.

In every sweep, at least 95% of samples SHALL pass in each endpoint/mode, in each prompt bucket of each endpoint/mode, and over the whole sweep. Average speed or a favorable aggregate percentile SHALL NOT substitute for any of these.

Each sweep's sustained offered rate, samples × 1,000,000 / `duration_ms`, SHALL be within ±5% of the signed `arrival_milli_rps`, where 1,000 milli-requests/second is one request per second. A run that fails any of these SHALL produce no qualified ModelSpec, and its evidence SHALL be retained, not relabeled (provider guide line 433).

#### Scenario: One failing bucket cannot hide in a passing sweep

- **GIVEN** a gateway sweep of 200 samples whose streaming chat mode has 100 samples, 25 of them in the 120–128-token bucket
- **WHEN** 2 of those 25 samples miss TTFT, so the bucket attains 92% while the mode attains 98% and the sweep 99%
- **THEN** qualification fails
- **AND** with only 1 miss in that bucket (96%), the evidence passes

#### Scenario: Errors count against attainment

- **WHEN** an endpoint/mode has 100 samples and 6 of them are errors
- **THEN** that mode attains at most 94%, and qualification fails

#### Scenario: The sustained rate must match within five percent

- **GIVEN** `arrival_milli_rps` 50 and a sweep of 200 samples
- **WHEN** the sweep lasted 4,200,000 ms, an offered rate of about 47.6 milli-requests/second
- **THEN** the rate check passes
- **AND** a duration of 4,300,000 ms, about 46.5 milli-requests/second, fails it

### Requirement: Qualification measures memory, records distributions and fails closed without trustworthy counters

Each sweep SHALL record its physical-memory metric, the peak memory used and the minimum memory available while it ran (book line 568; provider guide line 422). The metric SHALL be `device_used` for dedicated memory and `system_used` for unified memory. Validation SHALL require peak use of at most total physical memory minus the reserve, counted per dedicated device and once for unified memory. It SHALL also require a minimum available memory of at least the reserve.

A runtime without a trustworthy physical-memory counter for its memory kind SHALL fail closed before any paid request is dispatched. CPU execution and system RAM SHALL NOT certify a GPU capacity tier or speed class (book line 100). The `system_used` metric SHALL be accepted only for a unified-memory profile, whose shared physical memory is the device memory (book line 90).

Validation SHALL derive, for each sweep, its request and error counts, its jointly attained count, its attained goodput, and its TTFT, TPOT and latency distributions. Controlled external (client-side) timings SHALL be recorded separately from provider-side timings and SHALL NOT replace them (book line 560; provider guide line 425). Every streaming sample without an error SHALL carry external TTFT and decode timings.

#### Scenario: A backend without a qualifying counter fails closed

- **WHEN** the runner inspects a Vulkan runtime, for which it has no trustworthy per-device memory counter
- **THEN** it refuses before any paid request is dispatched

#### Scenario: Headroom below the reserve is rejected

- **GIVEN** a profile with one dedicated 32,000 MiB device and a 256 MiB reserve
- **WHEN** a sweep records a minimum available memory of 200 MiB
- **THEN** validation rejects the evidence

#### Scenario: Missing external timing is rejected

- **WHEN** a streaming sample without an error has gateway timings but no external TTFT
- **THEN** validation rejects the evidence

### Requirement: Each profiled request receives one performance classification

Every settled request of an agreement with a serving profile SHALL record one performance outcome. The outcome SHALL hold the policy, the classification, TTFT, decode interval and elapsed time in microseconds where measured, the authoritative output-token count where known, the attainment (attained, missed or not applicable) and the remedy. The outcome SHALL be bound to the report admitted at reservation, as `inference-request-accounting` *Performance remedies preserve usage and admitted evidence* requires.

The first matching rule in this order SHALL decide the classification (book lines 554, 556 and 571):

1. **Outside the envelope** (`outside_workload`): not applicable, no remedy, not counted.
2. **Measured miss.** TTFT was missed, or N ≥ 2 with a valid token clock and a decode interval that is not positive or whose rate is below the class. The outcome is missed, with remedy `inference_waiver`. This SHALL hold even when the client afterwards cancelled, disconnected or stopped reading: client interruption does not erase an already measured miss. With fewer than two output tokens this is a short-output TTFT miss.
3. **Client interruption** (`client_interrupted`). The client cancelled the request, closed its connection, or stopped reading, before any measured miss. The outcome is not applicable, with no waiver, and is not counted. The gateway SHALL record a client interruption whenever it can observe one, including a non-streaming client that closes its connection, and a client that disconnects before the engine starts responding.
4. **Provider-incomplete.** The request did not complete normally for a reason other than client interruption. Examples are an engine failure, a timeout, an interrupted gateway task and restart recovery (`restart_unknown_usage`). The same applies when the request completed without authoritative usage, without a positive TTFT, or with N ≥ 2 and no valid decode measurement. The outcome is missed, with remedy `inference_waiver`. Provider-incomplete execution SHALL NOT be recorded as attained.
5. **Short-output success.** The request completed with known usage, attained TTFT and N < 2. It is attained, with no remedy, and SHALL NOT be counted in the withdrawal window (book line 571).
6. **Otherwise** the request is attained, with no remedy.

The remedy SHALL be `inference_waiver` exactly when the attainment is missed, and `none` otherwise. The outcome's output-token count SHALL equal the settled authoritative usage, or be absent when usage is unknown. An outcome with remedy `inference_waiver` SHALL settle once, as `inference-request-accounting` *Performance remedies preserve usage and admitted evidence* specifies: inference cost is zero, known usage stays recorded, missing usage keeps the unknown-usage policy, and no funding, platform fee or gas is refunded.

#### Scenario: A measured miss survives cancellation

- **GIVEN** an in-envelope request under a 20 tokens/second class
- **WHEN** its first token ID is observed at 2,400,000 µs and the client then cancels the request
- **THEN** the outcome is a measured miss with remedy `inference_waiver`, and it is counted in the withdrawal window

#### Scenario: Cancellation before a miss is not a waiver

- **WHEN** the client cancels an in-envelope request 1,500,000 µs after its body arrived, before any token
- **THEN** the outcome is `client_interrupted`, with no waiver, and it is not counted

#### Scenario: A non-streaming client that disconnects is a client interruption

- **GIVEN** an in-envelope request from a non-streaming client
- **WHEN** the client closes its connection 500,000 µs after its body arrived, before any token
- **THEN** the outcome is `client_interrupted`, with no waiver, and it adds no observation to the withdrawal window

#### Scenario: An engine failure is provider-incomplete

- **WHEN** the engine fails an in-envelope request 300,000 µs after dispatch, without authoritative usage
- **THEN** the outcome is missed with remedy `inference_waiver`, it is counted in the withdrawal window, and the request keeps the unknown-usage settlement

#### Scenario: A one-token success is not counted

- **WHEN** an in-envelope request completes with 1 output token, known usage and a TTFT of 1,200,000 µs
- **THEN** the outcome is a short-output success, with no remedy, and it adds no observation to the withdrawal window

### Requirement: Withdrawal latches durably per model and evidence digest

Each settled outcome whose attainment is attained or missed SHALL be one observation, except a short-output success. The observation SHALL be recorded under the key (model ID, evidence digest of the report admitted at reservation), in settlement order, not request creation order or clock time. The withdrawal window for a key SHALL be its most recent min(n, 20) observations, where n is the number of observations recorded for that key.

When an observation is recorded, the latch SHALL be set for its key when either of these holds (book line 573):

- the window contains at least three misses; or
- the window contains exactly 20 observations and at least two misses.

The latch SHALL be evaluated and written in the same durable transaction that settles the observed request. If that transaction fails, the settlement, the observation and the latch SHALL all be absent. A latch SHALL be permanent for its key. Later successes, restart, operator resume (`POST /admin/resume`), offer refresh (`POST /admin/offers/refresh`) and periodic refresh SHALL NOT clear it (book lines 573 and 633).

While its key is latched, the provider SHALL NOT admit a new request executed under that report, SHALL NOT acknowledge new funding for a quote executed under it, and SHALL NOT publish offers for it. Offers already published SHALL be withdrawn at the next offer refresh. Settlement of requests already admitted, request status, receipts, unknown-usage recovery and existing balances SHALL stay available, as `inference-request-accounting` *Quote pressure and new-admission gates cannot strand settlement* requires.

A replacement report admitted under evidence-only renewal (`inference-commercial-terms` *Existing agreements permit only evidence-only renewal of identical terms*) has a different evidence digest. It SHALL therefore start its own empty window, and the old report's latch SHALL remain (book line 575). The same evidence digest under another model ID is another key.

#### Scenario: Three early misses latch

- **GIVEN** a key with no observations
- **WHEN** its first three observations are misses
- **THEN** the latch is set in the transaction that settles the third

#### Scenario: Two misses latch once the window reaches 20

- **GIVEN** a key whose first 19 observations are attained
- **WHEN** the 20th observation is a miss
- **THEN** no latch is set, because the window of 20 holds one miss
- **AND** when the 21st observation is also a miss, the window (observations 2–21) holds two misses and the latch is set

#### Scenario: A success can complete the window

- **GIVEN** a key whose first two observations are misses and whose next 17 are attained
- **WHEN** the 20th observation settles as attained
- **THEN** the window holds 20 observations with two misses, and the latch is set

#### Scenario: Short-output successes do not dilute the window

- **GIVEN** a key whose first two observations are misses
- **WHEN** 32 short-output successes settle and then a third full-length request misses
- **THEN** the window holds three observations, all misses, and the latch is set when the third miss settles

#### Scenario: Nothing clears the latch

- **GIVEN** a latched key
- **WHEN** the provider restarts, the operator calls `POST /admin/resume` or `POST /admin/offers/refresh`, requests admitted under that report before the latch later settle as attained, or requests under other reports succeed
- **THEN** the key stays latched, no offer is published for that report, and a new request under it is refused with HTTP 503 `model_unqualified` before reservation
- **AND** requests admitted before the latch still settle, and their attained observations do not clear the latch

#### Scenario: A failed settlement leaves no latch

- **WHEN** writing the observation or the latch fails inside the settlement transaction of the observation that would latch
- **THEN** neither the settlement, the observation nor the latch is committed, and the request stays reserved for recovery

#### Scenario: A renewed report starts its own window

- **GIVEN** a latched report with evidence digest `c…c`
- **WHEN** a strictly newer report with digest `e…e` for otherwise identical terms is admitted under evidence-only renewal and its first observation is a miss
- **THEN** the `e…e` key is not latched, and the `c…c` key stays latched
