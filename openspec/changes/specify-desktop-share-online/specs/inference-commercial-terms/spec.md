> **2026-11-25 scope amendment:** Launch shares must use an exact Strata catalog entry and separately measured configuration as specified by `strata-curated-runtime`. GGUF/embedded-template wording below is legacy-only. The same untrusted-model/process-boundary disclosure applies to Strata weights/tokenizer/templates. Common consent, free/paid qualification, immutable agreements, accounting and transport requirements remain target design; this migration does not implement desktop sharing. The author explicitly authorized revising the book; earlier read-only/book-line references record the prior baseline. See `adopt-strata-curated-launch` for supersession and current verification gates.

## MODIFIED Requirements

### Requirement: Native deployment constraints cannot be bypassed by declaration validity

Native provider configuration SHALL reject generation without a qualified serving profile outside explicit loopback-only development. The one exception is a free generation model: a model that advertises `chat` or `completions` and whose input and output token rates are both exactly zero. Outside development, a free generation model MAY be configured and published with no serving profile (`serving: null`); its offers SHALL carry a minimum funding of exactly `"0"`, and it SHALL claim no speed class, capacity tier or qualification identity. The exception rests on the free terms and the readiness probes below, never on private calibration. A generation model without a serving profile whose input or output rate is nonzero SHALL still be rejected outside explicit loopback-only development. Private calibration SHALL NOT authorize unqualified public publication. Profile concurrency SHALL match model admission concurrency, and a local evidence file SHALL be required when a profile is present. Simultaneous qualified models SHALL use disjoint physical device IDs; qualified and unprofiled models SHALL not share one provider process. A model API identifier SHALL be unique within its provider configuration.

A managed GGUF deployment SHALL bind its model/tokenizer to the same immutable GGUF revision and its selected embedded template/artifact inventory. Generation and embeddings SHALL be separate native tasks. An embedding deployment SHALL have zero output allowance, zero output price, no generation serving profile/speed class and no generation chat template. The zero embedding output-price rule SHALL be enforced at native configuration/admission boundaries, not solely in an optional provisioning script. These stricter deployment constraints SHALL NOT be inferred merely from the broad syntax accepted by a remote model declaration parser.

Actual runtime/artifact verification, readiness/capability probes and deployment-specific qualification SHALL remain necessary before publication/admission, except that a free generation model needs no deployment-specific qualification; a valid JSON declaration, model card or downloaded file SHALL NOT substitute for them. A free generation model SHALL be published and admitted only after its readiness probes pass: exact tokenizer and chat-template identity, the capability probe of every advertised endpoint, and a real inference whose usage the engine reports.

#### Scenario: Direct native embedding configuration bypasses provisioning

- **WHEN** a native embedding configuration specifies a nonzero output price without using the provisioning script
- **THEN** configuration validation rejects it rather than advertising non-input-only embedding terms

#### Scenario: Qualified and unqualified capacity is conflated

- **WHEN** a provider mixes profiled and unprofiled models or assigns the same physical device ID to simultaneous qualified models
- **THEN** configuration validation rejects the deployment instead of borrowing another model's qualification

#### Scenario: Managed task or artifact substitution

- **WHEN** a managed model changes its GGUF/tokenizer revision relationship, combines generation with embeddings in one task, or uses an inconsistent selected template/inventory
- **THEN** managed validation rejects it rather than silently substituting artifacts or capabilities

#### Scenario: Free generation model without a serving profile

- **GIVEN** a native provider configuration outside development whose only model advertises `chat`, has no serving profile, and has input and output rates of `"0"`
- **WHEN** configuration validation runs and the model's readiness probes pass
- **THEN** the configuration is accepted, and the model's offers carry `serving: null` and a `min_funding_wei` of `"0"` with no speed class, capacity tier or qualification identity
- **AND** while the readiness probes have not passed, no offer for the model is published and no request for it is admitted

#### Scenario: Priced generation model without a serving profile

- **GIVEN** a native provider configuration outside development with a generation model that has no serving profile
- **WHEN** its input or output rate is nonzero
- **THEN** configuration validation rejects it as generation without a qualified serving profile

### Requirement: Existing agreements permit only evidence-only renewal of identical terms

Execution under an existing agreement SHALL require the original model identity, artifact/tokenizer/template, capabilities, quantization, context/output limits and every serving term to remain identical. The same qualification identity SHALL remain compatible. The only supported replacement SHALL be a genuinely qualified report with a strictly newer measurement time for otherwise identical terms. Changing only an evidence digest without a newer measurement SHALL not authorize renewal; adding/removing a serving profile SHALL not count as evidence-only renewal.

Renewal SHALL NOT rewrite the agreement, prices, fees or balances, clear the old report's withdrawal latch, or imply independent hardware attestation. Admission SHALL still require the replacement's actual qualification and readiness; the durable request/report binding and performance accounting remain additional requirements of that lifecycle.

One exception is not renewal: a *pre-promotion free agreement*, meaning an agreement opened in a deployment under a free generation offer without a serving profile (`serving: null`, input and output token rates and minimum funding exactly zero, and a signed free allowance), SHALL remain executable by the same deployment's model after a serving profile is added to it, provided the model identity, artifact/tokenizer/template, capabilities, quantization, context/output limits and every execution setting other than the added profile remain identical. Such an agreement SHALL keep its own agreed terms: it SHALL be served only within its signed free allowance, at zero cost, under the free-opening and allowance rules that apply to it before the profile was added. It SHALL NOT acquire the added profile's speed class, qualification identity, performance remedy or waiver; its requests SHALL NOT be performance-accounted and SHALL NOT count toward the profile's withdrawal latch. The exception SHALL NOT apply in the other direction: an agreement made under a serving profile SHALL still not be executed by a model without one, and an unprofiled agreement with a nonzero token rate or minimum funding SHALL NOT be executed by a profiled model.

#### Scenario: Strictly newer evidence without changed service

- **WHEN** the deployment has a newly qualified report with strictly later measurement time and all other model/profile terms are identical
- **THEN** existing-agreement execution can use that report without rewriting the signed commercial agreement

#### Scenario: Changed deployment or stale replacement evidence

- **WHEN** hardware, artifact, runtime, context, capacity/admission/speed/policy terms change, a profile is added/removed, or replacement evidence is not strictly newer
- **THEN** it is not accepted as evidence-only renewal of the original agreement

#### Scenario: A pre-promotion free agreement stays servable after a profile is added

- **GIVEN** a free account opened in a deployment under a free generation offer with `serving: null` and a free allowance of 20,000 tokens and 86,400 seconds, with 5,000 tokens used and its lifetime not passed
- **WHEN** the same deployment's model gains a serving profile with nothing else in its execution settings changed, and the account's owner sends a request whose worst case fits the remaining allowance
- **THEN** the request is admitted and settled at zero cost against the account's allowance, without any speed promise, performance accounting, waiver or withdrawal-latch count
- **AND** a request from that account after its allowance is used or its lifetime has passed is refused as it would have been before the profile was added

#### Scenario: The exception does not cover changed settings or the reverse direction

- **GIVEN** a deployment with a pre-promotion free agreement and a funded agreement made under a serving profile
- **WHEN** the model is reconfigured with a different context limit and a serving profile, or the serving profile is removed
- **THEN** the free agreement is not executed by the reconfigured model with the different context limit, and the funded agreement is not executed by the model without a serving profile
