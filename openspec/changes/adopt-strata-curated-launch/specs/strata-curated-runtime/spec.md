## ADDED Requirements

### Requirement: Launch selection is a small immutable Strata catalog

Chiral SHALL select Strata for launch and SHALL install only the checked-in catalog's Inkling-2-Mini SQ-10/SQ-15 entries. Each entry SHALL bind repository/full revision, license, all files and exact sizes/SHA-256 values, quantization, runtime source, platform/backend support, context/output/concurrency and capabilities. Initial capabilities SHALL be text chat and streaming only, with context 4096, output maximum 512 and concurrency one. A catalog update SHALL require review and fresh compatibility evidence. Support SHALL NOT imply qualification or low-end hardware fit.

#### Scenario: Arbitrary import or floating revision

- **WHEN** an installation requests `latest`, an unknown catalog ID, altered revision or extra file
- **THEN** it is refused without an installed artifact or engine startup

#### Scenario: A valid download is not performance evidence

- **WHEN** a pinned entry is downloaded and its inventory verifies
- **THEN** its status remains unqualified and no speed or paid-capacity claim is created

### Requirement: Artifacts and the complete private runtime are verified before execution

Strata SHALL use source `b28ca95e3e6847f2d5e317994f253b66716deeaa`, app identity 0.2.22 and native component 0.1.69. The runtime inventory SHALL include the native engine, self-contained interpreter, pinned regex/Jinja2/MarkupSafe dependencies, reviewed tokenizer/template/dialect helpers, Chiral adapter and catalog. Rust SHALL verify exact files, sizes, hashes, platform/architecture and native entry points before launch. Dependencies SHALL import only from the packaged interpreter tree. Installation SHALL require license acknowledgement, refuse an existing destination, stage/verify before commit and never start inference. The request path SHALL NOT install packages, fetch models or execute model-supplied Python.

#### Scenario: Dependency or adapter alteration

- **WHEN** a runtime contains a changed helper, catalog, dependency or unlisted file
- **THEN** verification fails before engine execution

#### Scenario: System-linked Python

- **WHEN** the interpreter or dependency tree escapes the runtime package
- **THEN** packaging or startup refuses the runtime instead of importing the system environment

### Requirement: The managed process tree preserves the selected execution contract

`runtime.strata_catalog` SHALL select the exact entry. Rust SHALL own the private interpreter/native tree, use literal loopback HTTP with a protected key file and minimal environment, and refuse an occupied endpoint. The fixed recipe SHALL use DSpark beam 1, FP16 KV, KV offload off, chunk 512, anti-loop shortening disabled, context 4096 and one slot. Native INFO/READY SHALL confirm engine, architecture, backend, KV/draft mode, context and slots. The process SHALL NOT fall back, shrink context, truncate input, restart automatically or adopt an unrelated endpoint. Available host RAM SHALL be at least 4096 MiB before/after loading and during qualification; Metal unified reserve SHALL be at least 4096 MiB and CUDA reserve at least 256 MiB. Observed devices SHALL match qualified profiles. The process boundary SHALL NOT be described as an OS sandbox.

#### Scenario: Upstream fits a different context or selects CPU

- **WHEN** a CUDA configuration reports CPU or a smaller READY context
- **THEN** startup fails and no model is admitted or published

#### Scenario: Bridge or native failure

- **WHEN** the bridge or native engine exits
- **THEN** service is withdrawn, the owned process tree is contained and no replacement is automatically started

### Requirement: Exact prompt preparation and accepted tokens govern accounting

The private preparation and generation paths SHALL use the same pinned tokenizer/template rendering and protect literal user-written control tokens. Prompt plus authorized output SHALL fit the contracted context before reservation. The adapter SHALL expose accepted native T token IDs privately and SHALL emit authoritative usage only when DONE prompt/generated counts agree with prepared/observed tokens. Draft proposals/rejections, text length and SSE event counts SHALL NOT be used as billable token estimates. Private IDs SHALL be removed from public responses. Unsupported parameters, including logprobs, stop strings, penalties, tools and media, SHALL be rejected before reservation. Unknown usage SHALL retain existing provider-expense release semantics, with no automatic retry.

#### Scenario: Drafted tokens exceed accepted output

- **GIVEN** native output reports one accepted token and eight draft proposals
- **WHEN** its terminal prompt/generated counts reconcile
- **THEN** completion usage is one, not eight or nine

#### Scenario: Incorrect terminal usage

- **WHEN** native DONE counts differ from observed token IDs or exact prepared prompt length
- **THEN** no authoritative usage checkpoint is emitted and no invented charge settles

#### Scenario: Unsupported request

- **WHEN** a client requests token log probabilities or a stop string from the Strata launch model
- **THEN** the request fails before funds reservation or native generation

### Requirement: Private HTTP and cancellation are bounded

The adapter SHALL expose only authenticated loopback health, model listing, properties, prompt preparation and text-chat routes. It SHALL reject browser Origins and bound body bytes/readers, native line length, deadlines and one active generation. It SHALL expose no management, filesystem or download API. Cancellation/disconnection SHALL send STOP and drain that same generation before releasing its slot; a failed drain SHALL stop the native engine. A cancelled/failed response SHALL NOT fabricate terminal usage or replay generation.

#### Scenario: Cancel then reuse

- **WHEN** a client disconnects after an accepted token
- **THEN** the native request is stopped/drained before another request can use its slot, or the engine is stopped if draining fails

#### Scenario: Browser or unknown management route

- **WHEN** a caller supplies an Origin, omits the bearer or requests engine management
- **THEN** the private adapter refuses it without dispatching native generation

### Requirement: Strata evidence and legacy agreements remain distinct

Strata paid profiles SHALL name engine `strata`, version 0.2.22, exact catalog model and actual runtime/artifact inventory hashes. Only single-device CUDA/Metal profiles with the fixed catalog settings SHALL qualify; CPU SHALL remain development-only. Qualification SHALL measure the actual deployment under the existing speed policy, including host/device memory headroom. Legacy llama.cpp SHALL remain explicit development/recovery support with its original identity. Existing agreements, ledgers, keys and evidence SHALL NOT be converted, relabeled or renewed across changed engine terms. Fixture tests and native self-checks SHALL NOT be recorded as real-model, paid-performance or public-release qualification.

#### Scenario: Old evidence presented for Strata

- **WHEN** a Strata candidate carries a llama.cpp runtime identity or changed immutable serving settings
- **THEN** qualification/admission refuses it and existing agreements remain unchanged

#### Scenario: Desktop launch catalog

- **WHEN** the separate desktop acquisition milestone is implemented for launch
- **THEN** Discover selects these exact catalog entries and displays their pinned files, limits and qualification status; broad repository search/import remains deferred
