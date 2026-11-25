> **2026-11-25 scope amendment:** The Strata launch selection adds the exact curated `engine:strata` profile described by `strata-curated-runtime`; llama.cpp/GGUF-specific constraints remain legacy-only. Common immutable agreement, accounting, signature, profile identity and evidence-renewal requirements remain unchanged. Changed engines cannot reuse evidence or silently migrate agreements. The author explicitly authorized revising the book; earlier read-only/book-line references record the prior baseline. See `adopt-strata-curated-launch` for supersession and current verification gates.

## Purpose

Define the complete signed inference-service terms and the boundaries for accepting an offer, issuing and verifying a quote, and retaining an existing agreement without silently changing its model, price or qualification terms.

## ADDED Requirements

### Requirement: Complete signed offer declaration

An offer SHALL use the signed envelope, complete network identity, canonical wallet/money representations and offer domain defined by `inference-wire-foundation`. Its payload SHALL allow exactly these fields and SHALL reject unknown fields, including within its model and serving-profile objects:

| Field | Contract |
|---|---|
| `network` | Complete selected network identity |
| `provider_wallet` | Canonical provider wallet and required offer signer |
| `peer_id` | Syntactically valid libp2p PeerId; authenticated manifest retrieval additionally binds it to the responding peer |
| `deployment_id` | Provider deployment namespace, 1–128 ASCII letters, digits, `_` or `-`, distinct from `network.deployment_id` |
| `endpoint` | Provider HTTP origin satisfying the origin requirement below |
| `model` | Complete model declaration below |
| `price` | Canonical `input_wei_per_1k` and `output_wei_per_1k` strings |
| `min_funding_wei` | Canonical minimum opening value for paid service; local publication emits `"0"` for genuinely free service |
| `platform_wallet` | Canonical provider-remitted platform-fee destination |
| `fee_bps` | Unsigned 16-bit integer, exactly `50` |
| `max_concurrency` | Unsigned 32-bit integer, 1–256; equals profile concurrency when a profile is present |
| `revision` | Positive unsigned 64-bit integer in the provider-wallet/deployment/model revision stream |
| `issued_at`, `expires_at` | Unsigned 64-bit Unix-second timestamps |

The receiver SHALL reject a serialized signed offer exceeding 16 KiB, a wrong network or signer, malformed fields, an invalid model/profile, zero revision, unsupported fee or invalid concurrency. Validation SHALL NOT accept a malformed offer merely because its signature is valid. A valid declaration SHALL NOT by itself prove health, slot availability, execution of the advertised artifact, truthful metering or completed qualification.

#### Scenario: Attributable but invalid commercial metadata

- **WHEN** a provider signs an offer with an invalid fee, concurrency, model, peer identity or provider deployment identifier
- **THEN** offer acceptance rejects it despite a valid provider signature

#### Scenario: Oversized or extended declaration

- **WHEN** a signed offer exceeds 16 KiB or includes an unknown offer/model/profile field
- **THEN** it is rejected rather than storing a truncated or projected commercial declaration

#### Scenario: Separate network and provider deployment namespaces

- **WHEN** two offers share a provider deployment string but differ in the selected network tuple
- **THEN** the foreign-network offer is not accepted as the selected network's offer

### Requirement: Model identity and capability limits

A model declaration SHALL have exactly the following allowed fields. `quantization` and `serving` SHALL be nullable optional values; their canonical signed representation includes null values as defined by the wire foundation. Other fields SHALL be required. Unknown fields SHALL be rejected.

| Field | Receiver constraint |
|---|---|
| `id` | Nonempty string, at most 256 UTF-8 bytes, no control characters |
| `revision`, `tokenizer_revision` | Immutable content revisions, each 40 or 64 hexadecimal characters, not floating names such as branches |
| `chat_template_sha256` | Exactly 64 hexadecimal characters |
| `quantization` | Null or nonempty string of at most 64 UTF-8 bytes |
| `context_length` | Unsigned 64-bit integer, 1–1,048,576 tokens |
| `max_output_tokens` | Unsigned 64-bit integer, at most the context length; positive with `chat` or `completions`, otherwise zero |
| `capabilities` | Array of 1–6 unique values from `chat`, `completions`, `embeddings`, `tools`, `structured_output`, `streaming`, including at least one of the first three |
| `serving` | Null or the complete serving profile below |

Array order and all model identity fields SHALL remain part of the signed terms. Auxiliary capabilities alone SHALL NOT establish an inference endpoint. Declaration validation SHALL NOT substitute for real capability probes or silently convert a generation artifact into an embedding task. The stricter native task restrictions are defined separately below.

#### Scenario: Floating or malformed artifact identity

- **WHEN** a model uses a branch name, a non-hex revision, a wrong revision length, an invalid template digest, or a control character in its API identifier
- **THEN** model declaration validation rejects it

#### Scenario: Invalid capability set

- **WHEN** a declaration has duplicate/unknown capabilities, no capabilities, more than six entries, or only auxiliary capabilities
- **THEN** it is rejected rather than inferring an endpoint from a model name

#### Scenario: Generation and non-generation output bounds

- **WHEN** a declaration includes `chat` or `completions` with zero output budget, has output exceeding context, or declares only nongenerating endpoints with positive output budget
- **THEN** it is rejected

#### Scenario: No unearned performance claim

- **WHEN** a structurally valid model has no serving profile
- **THEN** declaration validation does not invent capacity, speed, memory or qualification metadata
- **AND** stricter local publication/admission requirements still apply

### Requirement: Complete serving profile identity

A present serving profile SHALL include exactly these fields, reject unknown fields and be bound in full by the offer and any quote copying it:

| Group | Fields and wire types |
|---|---|
| Artifact | String `hf_repo`, `tokenizer_repo`, `artifact_manifest_sha256` |
| Runtime | String `engine`, `engine_version`, `runtime_sha256`, `platform`, `backend` |
| Physical inventory | String `memory_kind`; array `devices`; unsigned 64-bit `memory_reserve_mib` |
| Execution | Unsigned 32-bit `gpu_layers`; string `split_mode`; unsigned 32-bit integer array `tensor_split`; string `cache_type_k`, `cache_type_v`; boolean `flash_attention`; unsigned 32-bit `batch_size`, `ubatch_size` |
| Admission/performance | String `capacity_tier`, `region`, `policy`; unsigned 32-bit `max_concurrency`, `arrival_milli_rps`, `output_tokens_per_second` |
| Evidence | `qualification` object with exactly string `evidence_sha256` and unsigned 64-bit `measured_at` |

Each device SHALL contain exactly string `id`, `name`, `driver_version`, `engine_device` and unsigned 64-bit `memory_mib`. Repository identifiers, engine version, region and device text fields SHALL be nonempty, at most 256 UTF-8 bytes, and contain no control characters. Artifact-manifest, runtime and evidence digests SHALL each be exactly 64 lowercase hexadecimal characters. Measurement time SHALL be positive; current offer acceptance SHALL reject a measurement in the future.

Profile validation SHALL require either explicit legacy `engine = "llama.cpp"` or curated `engine = "strata"` under `strata-curated-runtime`, `policy = "warm-generation-v1"`, concurrency 1–256 (exactly one for Strata), arrival rate 1–1,000,000 milli-requests/second, and an output class of 20, 50 or 100 tokens/second. A profile SHALL apply only to a generating model with context at least 4,096 and output allowance at least 256, without the `embeddings` capability. These are declaration constraints, not proof that the provider attained the class.

#### Scenario: Missing or altered profile identity

- **WHEN** a profile omits a required field, introduces an unknown field, uses a malformed digest or invalid text, or advertises another engine/policy
- **THEN** the declaration is rejected rather than filling in a local runtime default

#### Scenario: Inconsistent model or admission profile

- **WHEN** a profile includes embeddings, lacks a generation capability, has insufficient model limits, or disagrees with the offer's concurrency
- **THEN** the offer is rejected

#### Scenario: Future qualification claim

- **WHEN** profile measurement time exceeds the receiver's current Unix time
- **THEN** current offer acceptance rejects that profile

### Requirement: Declared capacity and execution settings are internally consistent

A serving profile SHALL use Linux or Windows with CUDA/Vulkan, or macOS with Metal. Memory kind SHALL be `dedicated` or `unified`; Metal SHALL require unified memory and CUDA SHALL require dedicated memory. The profile SHALL contain 1–64 devices with unique physical IDs and unique engine selectors. Unified memory SHALL be represented by exactly one device.

Physical memory per device SHALL exceed its reserve and be at most 1,073,741,824 MiB. Reserve SHALL be at least 4,096 MiB for unified memory and at least 256 MiB per dedicated device. Capacity SHALL be based on physical memory, not memory after reserve, without claiming that headroom checks are an exclusive OS reservation:

| Tier | Minimum physical MiB | Counting rule |
|---|---:|---|
| `8gb` | 7,500 | Each selected device |
| `16gb` | 15,000 | Each selected device |
| `32gb` | 30,000 | Each selected device |
| `higher` | 45,000 | Sum of selected devices inside this provider runtime |

GPU layers SHALL be 1–999. One device SHALL use split mode `none` with an empty tensor-split array. Multiple devices SHALL use `layer` or `row`, with one integer weight per device in 1–1,000,000. KV cache types SHALL each be one of `f32`, `f16`, `bf16`, `q8_0`, `q4_0`, `q4_1`, `iq4_nl`, `q5_0`, `q5_1`; a quantized value-cache type SHALL require flash attention. Batch size SHALL be 1–8,192, and microbatch size SHALL be positive and no greater than batch size. Invalid combinations SHALL be rejected rather than silently adjusted.

#### Scenario: Aggregating undersized devices into a per-device tier

- **WHEN** two 4,096 MiB devices claim the `8gb` tier
- **THEN** validation rejects the declaration even though their summed memory exceeds 7,500 MiB

#### Scenario: Double-counted or under-reserved unified memory

- **WHEN** unified memory is represented as multiple devices or reserves fewer than 4,096 MiB
- **THEN** validation rejects the declaration

#### Scenario: Unbound or incompatible execution settings

- **WHEN** device IDs/selectors repeat, tensor-split length or weights disagree with the inventory, a quantized value cache lacks flash attention, or microbatch exceeds batch size
- **THEN** validation rejects the declaration instead of choosing replacement settings

### Requirement: Offer freshness and advertised origin

Current offer acceptance SHALL require `expires_at > issued_at`, `expires_at > now`, issuance no more than 30 seconds in the future, and lifetime no greater than 3,600 seconds. Native publication SHALL normally issue 120-second offers; that shorter issuer lifetime SHALL NOT replace the receiver's 3,600-second bound. Offer expiry SHALL prevent fresh selection/proposal without erasing a previously acknowledged agreement.

An advertised remote origin SHALL use HTTPS on port 443, with no credentials, query, fragment or non-root path. Forbidden literal addresses, loopback/localhost names, single-label remote hostnames and non-public remote origins SHALL be rejected. A hostname SHALL be classified after URL normalization by its nonempty labels, so trailing DNS root dots and empty leading labels add no label: a root-dot spelling such as `https://intranet./`, `https://com./` or `https://intranet..`, and a leading-dot spelling such as `https://.intranet/`, is a single-label hostname; classification SHALL NOT rewrite the accepted origin, so `https://inference.example.org./` remains valid as given. Explicit development mode SHALL permit HTTP(S) and non-443 ports only for literal loopback or exact `localhost`, not arbitrary private LAN or metadata endpoints. Consumers SHALL not append an SDK path before validating the origin. Static declaration acceptance SHALL NOT be represented as successful DNS/reachability, health or qualification verification; outbound transport safeguards remain additionally required.

#### Scenario: Inclusive issuance and lifetime bounds

- **WHEN** an otherwise valid offer is issued at `now + 30` with a lifetime of exactly 3,600 seconds
- **THEN** it satisfies these time bounds
- **AND** issuance at `now + 31`, lifetime of 3,601 seconds, or expiry at `now` is rejected

#### Scenario: Provider path or private-network origin

- **WHEN** an offer advertises a path-prefixed origin, remote cleartext HTTP, remote port 8443, a private address or a metadata endpoint
- **THEN** origin validation rejects it
- **AND** enabling development does not authorize the private LAN/metadata/remote cases

#### Scenario: Single-label remote hostname, including a root-dot spelling

- **WHEN** an offer advertises `https://intranet`, `https://intranet./`, `https://com./`, `https://intranet..` or `https://.intranet/` on port 443
- **THEN** origin validation rejects it, with or without development mode
- **AND** `https://inference.example.org./` is accepted and returned with its root dot

### Requirement: Complete signed quote and immutable copied terms

A quote SHALL use the quote domain and allow exactly these required fields, rejecting unknown fields:

| Fields | Contract |
|---|---|
| `id` | Quote/account identifier under the wire foundation |
| `network`, `offer_id`, `provider_wallet` | Selected full network, exact offer content ID and provider signer |
| `consumer_wallet` | Canonical intended payer |
| `model`, `price`, `platform_wallet`, `fee_bps`, `max_concurrency` | Exact selected model/profile, rates, fee destination/rate and concurrency; no implicit substitution |
| `funding_wei` | Canonical requested opening amount |
| `nonce` | Provider-supplied signed string; native issuance uses a fresh cryptographically random 32-byte `0x`-prefixed hexadecimal nonce |
| `issued_at`, `expires_at` | Unsigned 64-bit Unix-second timestamps; native issuance expires 300 seconds after issuance |

The native issuer SHALL create a fresh random quote/account identifier and nonce and sign and durably record the complete quote before returning it. A quote SHALL be an overbookable commercial agreement proposal, not an opening-capacity reservation, admission acknowledgement, guarantee of later admission, on-chain smart contract, spendable credit or proof of payment. Every copied field, including the complete optional serving profile, SHALL remain covered by the quote signature and opening commitment.

#### Scenario: Quote copies the selected commercial proposal

- **WHEN** a valid request selects an available local offer and an allowed funding amount
- **THEN** the provider returns a signed quote with the exact selected terms, named payer, requested amount, fresh identifier/nonce and native 300-second validity window
- **AND** the quote is recorded before the successful response

#### Scenario: Unknown or substituted quote term

- **WHEN** a quote contains an unknown field or changes a copied commercial term despite a valid provider signature
- **THEN** the native consumer does not accept it as the selected agreement

### Requirement: Bounded public quote proposal is not owner authorization

`POST /v1/contracts/propose` SHALL accept exactly `offer_id`, `consumer_wallet` and `funding_wei`. Proposal SHALL be public and SHALL NOT require an owner challenge or inference bearer. The selected offer SHALL be present, unexpired, local to this provider wallet and network, and subject to the provider's new-funding gates. Administrative drain or unavailable new-funding readiness SHALL refuse new proposals even when some existing settled accounts can still be served.

The public control boundary SHALL limit the request body to 96 KiB, allow at most five seconds to read it, and admit at most 64 concurrent control readers without queuing, before signing. Under the authoritative pending replacement `specify-non-reserving-quotes`, standalone unaccepted quote retention SHALL be bounded to 10,000 per provider ledger without named-wallet pending or rate allowances. Quote issuance SHALL NOT consume paid-intent leases, free-account capacity or inference capacity; otherwise eligible quoting SHALL remain possible when opening capacity is full. Ordinary offer/readiness gates remain necessary.

Signing work SHALL use a separate provider-ledger budget of 600 durable permits per effective minute, claimed after input validation and before signing; signing, insertion, persistence or response failure SHALL NOT refund a claimed permit. Under the aligned `specify-proposal-rate-budgets`, committed issuance SHALL independently consume at most 600 units per effective minute, charged atomically with durable quote insertion. Failed insertion SHALL roll back committed charges and quote mutations, not a prior work permit; successful insertion SHALL NOT be refunded by response loss, expiry, acknowledgement or opening. Each budget SHALL retain its own durable non-regressing clock across restart and cleanup, following its authoritative contract's initialization/adoption and fail-closed rules. Neither budget clock SHALL rewrite signed validity times. Optional source controls SHALL use transport or explicitly trusted proxy identity, never the named wallet or an untrusted forwarding header. Challenge authorization and per-inference admission/performance rules SHALL remain unchanged.

Creating a quote SHALL NOT authorize spending or credential issuance, broadcast a transaction or establish a funded balance. Both old named-wallet quote gates SHALL be removed together during native cutover. These replacement/rate contracts supersede conflicting historical quote-quota wording while implementation is pending; old-wallet tests SHALL NOT be treated as compliance evidence.

#### Scenario: Unauthenticated proposal without account authority

- **WHEN** a caller proposes valid terms for a canonical payer without owner-proof headers or a bearer
- **THEN** the provider can issue the quote subject to normal proposal gates
- **AND** the caller gains no authority to fund, operate or obtain credentials for that payer's account merely by knowing the quote

#### Scenario: Absent offer or funding gate closed

- **WHEN** the selected offer is absent/expired/foreign, the provider is draining or new-funding readiness is unavailable
- **THEN** a new proposal is refused rather than silently selecting another offer or provider

#### Scenario: Oversized, slow or saturated control request

- **WHEN** a proposal exceeds the body bound, misses the body-read deadline or arrives when all control-reader slots are occupied
- **THEN** it is rejected without adding an unbounded waiting request or issuing a quote

#### Scenario: Unauthenticated names do not consume wallet allowances

- **GIVEN** unrelated callers have obtained eight pending quotes and twelve committed proposals naming one wallet while provider-wide resources remain available
- **WHEN** another valid public request names that wallet
- **THEN** neither former wallet ceiling refuses it or charges wallet-specific quote state
- **AND** the owner's authenticated opening remains governed by its own authorization and opening-capacity rules

#### Scenario: Quote issuance overbooks opening admission

- **GIVEN** paid-intent or free-opening capacity is full but quote resources and ordinary offer/readiness gates permit issuance
- **WHEN** a valid public proposal is issued
- **THEN** exact signed terms are retained without allocating an opening lease or account
- **AND** receipt of the quote does not authorize payment broadcast

#### Scenario: Failed insertion spends work but not committed issuance

- **WHEN** a valid proposal claims a signing-work permit but subsequent insertion fails
- **THEN** the work unit remains spent while tentative committed-rate and quote mutations roll back
- **AND** exhausted work allowance refuses further signing before signer invocation

### Requirement: Authenticated opening admission preserves original acceptance

Fresh paid admission SHALL occur at payer-authenticated funding-intent registration after exact quote/payment validation and before durable acknowledgement or client broadcast. The final capacity check and accepted-evidence insertion SHALL be atomic across competing writers. At most 10,000 original un-opened intent leases whose original quote-expiry deadlines exceed transaction time SHALL be active per ledger, independently of standalone quote retention; replay SHALL NOT extend those deadlines. Full admission SHALL return 429/`rate_limit` without acknowledgement, accepted intent, account, credit or submission. An honest provider SHALL NOT broadcast the refused original. The consumer SHALL validate and durably persist a valid acknowledgement before submitting the same prepared transaction bytes.

Once acknowledged, the exact original quote, acknowledgement, transaction and sender/nonce ownership SHALL survive later capacity pressure, quote/lease expiry and restart. Lease expiry SHALL release capacity only, never accepted evidence. Credit SHALL still require verified canonical successful execution at the healthy safe frontier. A lost reply or timeout SHALL be ambiguous, not proof of refusal: recovery SHALL use the original intent/acknowledgement/payment without automatic replacement. The provider receives broadcastable signed bytes; this residual trust boundary SHALL remain disclosed.

Free first-opening admission SHALL instead occur atomically after payer authentication, requiring fewer than eight accounts owned by that authenticated wallet and fewer than 10,000 total ledger accounts immediately before creation. It SHALL NOT fabricate a payment or paid intent. Cached accepted-original, existing-account, top-up and settlement recovery SHALL NOT acquire fresh quote storage/work/issuance or opening-capacity gates, rotate an existing exported credential, or rewrite accepted terms.

#### Scenario: Paid writers compete for final admission

- **GIVEN** one paid lease remains and two authenticated original payments otherwise qualify
- **WHEN** their registrations compete
- **THEN** at most one durable acknowledgement and lease are created
- **AND** the refused original creates no accepted intent, account, credit or broadcast

#### Scenario: Lost acknowledgement survives later pressure

- **GIVEN** an original acknowledgement committed but its reply was lost
- **WHEN** restart, quote/lease expiry and later capacity exhaustion precede authenticated recovery
- **THEN** the exact original acknowledgement and transaction remain recoverable without a new quote or lease
- **AND** the valid original remains eligible for safe-frontier credit without replacement payment

#### Scenario: Free opening capacity is authenticated

- **GIVEN** two authorized first free openings compete for the final applicable account slot
- **WHEN** their creation transactions execute
- **THEN** at most one account is created and the other is refused without partial account creation or quote removal
- **AND** neither path fabricates payment, while existing-account recovery remains usable without fresh capacity admission

### Requirement: Free and paid opening terms are distinguished exactly

A service SHALL be genuinely free only when both quoted token rates are exactly zero. A free opening SHALL have zero funding and SHALL NOT fabricate a zero-value chain transfer or payment-backed reputation eligibility. A paid opening SHALL have positive funding at least the selected offer's minimum, including when only one token rate is nonzero. An offer's minimum SHALL NOT override the zero-funding rule for free service.

The issuer and native consumer SHALL validate canonical amounts without rounding or normalization. The native consumer SHALL require the returned quote's funding amount to equal the amount it requested, not merely to exceed the offer minimum. These proposal checks SHALL NOT credit an account before verified settlement.

#### Scenario: Minimum and free-session boundary

- **WHEN** either rate is nonzero and funding is zero or below the minimum, or both rates are zero and funding is nonzero
- **THEN** quote issuance/acceptance rejects the proposed amount

#### Scenario: Provider changes the requested deposit

- **WHEN** a provider returns a signed quote for a different funding amount that still meets its minimum
- **THEN** the consumer rejects it rather than funding the changed amount automatically

### Requirement: Native quote verification binds the chosen provider and payer

Before new onboarding, the native consumer SHALL validate the selected offer and verify both offer and quote signatures against that offer's canonical provider wallet. It SHALL require the selected network to match the offer and quote, the quote's offer ID to equal the exact selected offer content ID, the quote payer to equal the active wallet, and exact equality of model/profile, rates, fee wallet/rate and concurrency. It SHALL validate the quote/account identifier and the advertised origin, rejecting unsafe outbound destinations before a payment can be prepared/broadcast.

Quote acceptance SHALL reject issuance more than 30 seconds in the future or expiry not later than issuance. Fresh onboarding SHALL additionally require both the offer and quote to remain unexpired. The native issuer's 300-second policy SHALL not be confused with these receiver checks or used to destroy accepted historical terms. Historical verification SHALL retain network/signature/party/term checks while allowing expiry to be evaluated by the separate durable funding/recovery state machine.

A locally saved agreement SHALL NOT be overwritten by a different signed quote under the same account identifier. Offer refresh, endpoint observations and current model availability SHALL NOT renegotiate an accepted agreement or transfer a provider-specific balance.

#### Scenario: Re-signed quote for another payer or terms

- **WHEN** the provider validly signs a quote for another payer/network/offer, different prices, another fee destination, or changed model/profile/concurrency
- **THEN** the native consumer rejects it for the original selection

#### Scenario: Fresh onboarding versus historical recovery

- **WHEN** offer or quote expiry has been reached
- **THEN** fresh onboarding is refused
- **AND** expiry alone does not discard an acknowledged agreement or bypass the signature/network/party checks during recovery

#### Scenario: Reusing an account identifier with changed terms

- **WHEN** a local saved agreement exists and a subsequent funding call supplies different signed quote data under that identifier
- **THEN** the call rejects the conflict and preserves the saved agreement

### Requirement: Native deployment constraints cannot be bypassed by declaration validity

Native provider configuration SHALL reject generation without a qualified serving profile outside explicit loopback-only development. Private calibration SHALL NOT authorize unqualified public publication. Profile concurrency SHALL match model admission concurrency, and a local evidence file SHALL be required when a profile is present. Simultaneous qualified models SHALL use disjoint physical device IDs; qualified and unprofiled models SHALL not share one provider process. A model API identifier SHALL be unique within its provider configuration.

A managed GGUF deployment SHALL bind its model/tokenizer to the same immutable GGUF revision and its selected embedded template/artifact inventory. Generation and embeddings SHALL be separate native tasks. An embedding deployment SHALL have zero output allowance, zero output price, no generation serving profile/speed class and no generation chat template. The zero embedding output-price rule SHALL be enforced at native configuration/admission boundaries, not solely in an optional provisioning script. These stricter deployment constraints SHALL NOT be inferred merely from the broad syntax accepted by a remote model declaration parser.

Actual runtime/artifact verification, readiness/capability probes and deployment-specific qualification SHALL remain necessary before publication/admission; a valid JSON declaration, model card or downloaded file SHALL NOT substitute for them.

Outside explicit development, a model with a serving profile SHALL be served only by a Rust-managed runtime, whose launch verifies the runtime and artifact inventory and the selected devices; native configuration SHALL refuse a serving profile whose deployment has no managed runtime. An operator-owned external engine is for calibration only and SHALL NOT carry a qualified profile into publication (book lines 73, 609 and 615–617).

#### Scenario: Direct native embedding configuration bypasses provisioning

- **WHEN** a native embedding configuration specifies a nonzero output price without using the provisioning script
- **THEN** configuration validation rejects it rather than advertising non-input-only embedding terms

#### Scenario: Qualified and unqualified capacity is conflated

- **WHEN** a provider mixes profiled and unprofiled models or assigns the same physical device ID to simultaneous qualified models
- **THEN** configuration validation rejects the deployment instead of borrowing another model's qualification

#### Scenario: A qualified profile on an external engine is refused

- **GIVEN** a non-development provider configuration whose generation model carries a serving profile and an evidence file
- **WHEN** the model has no managed `runtime` and names an external engine, on loopback or a private address
- **THEN** native configuration validation refuses it, and no offer is signed or published
- **AND** the same profile under a managed runtime is accepted, and explicit development calibration may still use an external engine

#### Scenario: Managed task or artifact substitution

- **WHEN** a managed model changes its GGUF/tokenizer revision relationship, combines generation with embeddings in one task, or uses an inconsistent selected template/inventory
- **THEN** managed validation rejects it rather than silently substituting artifacts or capabilities

### Requirement: Existing agreements permit only evidence-only renewal of identical terms

Execution under an existing agreement SHALL require the original model identity, artifact/tokenizer/template, capabilities, quantization, context/output limits and every serving term to remain identical. The same qualification identity SHALL remain compatible. The only supported replacement SHALL be a genuinely qualified report with a strictly newer measurement time for otherwise identical terms. Changing only an evidence digest without a newer measurement SHALL not authorize renewal; adding/removing a serving profile SHALL not count as evidence-only renewal.

Renewal SHALL NOT rewrite the agreement, prices, fees or balances, clear the old report's withdrawal latch, or imply independent hardware attestation. Admission SHALL still require the replacement's actual qualification and readiness; the durable request/report binding and performance accounting remain additional requirements of that lifecycle.

#### Scenario: Strictly newer evidence without changed service

- **WHEN** the deployment has a newly qualified report with strictly later measurement time and all other model/profile terms are identical
- **THEN** existing-agreement execution can use that report without rewriting the signed commercial agreement

#### Scenario: Changed deployment or stale replacement evidence

- **WHEN** hardware, artifact, runtime, context, capacity/admission/speed/policy terms change, a profile is added/removed, or replacement evidence is not strictly newer
- **THEN** it is not accepted as evidence-only renewal of the original agreement
