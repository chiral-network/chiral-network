## Why

The read-only whitepaper (`docs/chiral-book.md`) defines a complete generation performance contract in *Performance accounting and withdrawal* (lines 542–577), supported by *Hardware participation and platform status* (lines 79–102) and the transport-path rule at line 309. The contract covers these points:

- A signed profile binds the measured deployment.
- Policy `warm-generation-v1` declares a 20, 50 or 100 tokens/second class with TTFT at most 2 seconds.
- A runtime envelope decides waiver entitlement.
- TTFT and decode are timed at the provider in integer microseconds from private per-token engine IDs.
- Overload is refused rather than queued.
- Qualification is a deployment-specific sweep with ≥95% joint attainment everywhere.
- Each request is classified once, and eligible misses settle with `inference_waiver`.
- A durable latch withdraws a report after three misses in the last up-to-20 observations, or two once the window reaches 20.

The `CLAUDE.md` performance rules restate parts of it.

Existing specs cover only the edges. Pending changes define:

- serving-profile fields and evidence-only renewal (`specify-inference-commercial-terms`);
- waiver arithmetic and the binding of the admitted report (`specify-inference-request-accounting`), which defers "qualification, timing eligibility and withdrawal classification" to the whitepaper's performance contract;
- the rule that the transport path does not change qualification or waiver terms (`specify-p2p-provider-transport`);
- the in-app qualification run (`specify-desktop-share-online`).

None of them specifies the contract itself. Its rules exist only as prose and code, so a later change could weaken them without a failing requirement.

## What Changes

- Add the `inference-generation-performance` capability with 12 requirements and 44 scenarios. Each requirement cites its whitepaper lines and comes from the whitepaper, not from current code. Numeric values that the book does not state are recorded in `design.md` as current implementation values.
- Specify that a speed class is a qualified claim only for the exact profile it was measured with, and that capacity, a loaded model, CPU execution or runtime support is not speed.
- Specify what `warm-generation-v1` promises: a provider-side, warm, admitted-workload class, not cold-load or transport-path latency.
- Specify the runtime guarantee envelope: generation only; a prepared prompt of at most 2,048 tokens including the template; a requested maximum output of at most 256 tokens; at most 4,096 tokens in total. The requested maximum is the output limit the gateway reserves.
- Specify the timing points and integer-microsecond comparisons. Nonpositive or missing timing is never attainment.
- Specify the private per-token token clock, internal streaming for non-streaming clients and the stripping of instrumentation.
- Specify non-queuing overload refusal at the signed concurrency and arrival rate.
- Specify qualification evidence binding, coverage of every concurrency, endpoint, mode and prompt boundary, joint attainment, the ±5% arrival check, memory checks and fail-closed counters.
- Specify the ordered per-request classification: outside the envelope, measured miss, client interruption, provider-incomplete, short-output success, attained.
- Specify the observation window and the durable latch, with the book's rule stated exactly, and what a latch closes and leaves open.
- Record a coverage table in `design.md` (requirement → code → existing tests, or the missing case). Record code differences and missing acceptance evidence as unchecked tasks.

## Capabilities

### New Capabilities

- `inference-generation-performance`: Qualification of generation serving profiles, the runtime guarantee envelope, provider-side timing and the private token clock, non-queuing overload refusal, per-request performance classification and the durable withdrawal latch.

### Modified Capabilities

None. This capability references `inference-wire-foundation` and the pending `inference-commercial-terms`, `inference-request-accounting` and `p2p-provider-transport` capabilities. It adds no obligation to them and does not change their requirements. The in-app qualification run of `desktop-share-online` must satisfy the qualification requirements here. Its exclusion of pre-promotion free agreements from performance accounting is consistent with the latch requirement.

## Impact

- Authority: `docs/chiral-book.md` stays read-only and unchanged (SHA-256 `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`).
- Code that corresponds to these requirements:
  - `src-tauri/src/performance.rs`: profile validation, evidence validation and classification;
  - `inference_gateway.rs`: envelope, token clock, internal streaming and overload;
  - `inference_backend.rs`: engine instrumentation and readiness revalidation;
  - `billing_store.rs`: outcome checks, observation window and latch;
  - `app.rs` and `contracts.rs`: admission, publication and funding closed by a latch;
  - `engine_runtime.rs`: launch binding;
  - `scripts/qualify-provider.py`: the qualification runner.
- Three code differences were found. Tasks 2.1–2.3 track them:
  - a client disconnect that the gateway does not attribute is waived and counted as a provider miss, so three disconnects can latch withdrawal;
  - a 0 µs TTFT counts as attainment;
  - native validation accepts qualification samples outside every prompt boundary.
- Missing acceptance evidence is tracked as unchecked tasks 3.1–3.12, mainly gateway-level tests of the envelope, TTFT start point, batched IDs, overload, interruption classes and latch persistence across resume and refresh, plus negative evidence-validation cases.
- Live qualification and live token-clock evidence are tasks 4.1 and 4.2. They need an authorized private deployment and are never satisfied by fixtures.
- One question for the whitepaper author is recorded in `design.md` and `docs/apas-worker/report.md`: whether failures caused by the request itself count as provider misses.
- No ledger schema, wire format, deployment or whitepaper change is part of this specification step. This change grants no speed class, qualification or public-release approval.
