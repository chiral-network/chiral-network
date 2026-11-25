## Context

The launch decision supersedes prior book quotations selecting llama.cpp exclusively. Historical quotations and checklists elsewhere remain evidence of their original scope, not instructions to restore that selection. The implementation uses source `b28ca95e3e6847f2d5e317994f253b66716deeaa` (Strata app 0.2.22, native Inkling 0.1.69) and the exact model pins in `dev/strata/catalog.json`.

## Decisions

1. Chiral owns a two-entry allowlist. Broad Hugging Face search/import, tools, embeddings, vision and a separate coding family are deferred. A catalog entry is supported by the adapter but remains unqualified until measured.
2. Use the shared native Inkling CMake entry point plus Chiral's small HTTP/line-protocol bridge. Do not launch the upstream full management server with its download/tuning/fallback behavior. Only pinned upstream tokenizer/template/dialect helpers are imported.
3. Rust verifies every byte of the complete runtime/model inventories. A self-contained interpreter and fixed Python dependencies are part of the runtime hash. The bridge confirms imported dependencies are inside its package. Operator-supplied binary hashes identify packages; this is not execution or reproducible-build attestation.
4. `runtime.strata_catalog` explicitly selects Strata. An absent field retains legacy behavior for old profiles. Artifact/runtime schemas must agree. No existing configuration, agreement or balance is silently rewritten.
5. Preserve wire-profile shape. `engine:"strata"`, exact catalog ModelSpec and runtime/artifact hashes bind the new identity. Shared legacy execution fields take fixed values: zero GPU layers, no split, f16 caches, false flash-attention, batch/ubatch 512. Strata's real recipe is bound by the compiled/inventoried adapter: DSpark beam 1, FP16 KV, KV offload off, chunk 512, no anti-loop shortening, context 4096, one slot. Fresh evidence is required for any changed identity.
6. Native `T` events are accepted target-token observations. `DONE` must reconcile prompt and generated counts. No proposed/rejected draft count is billable. The same preparation function supplies private token IDs and native GEN input. Unsupported parameters fail before reserve; unknown terminal usage keeps existing no-charge semantics.
7. Cancellation sends STOP and drains before slot reuse. Failed drain stops the native process; native exit closes the bridge. Rust owns the process group/Windows job and kills descendants if the bridge exits. No automatic restart, context reduction, backend fallback or paid retry.
8. Hardware declarations remain separate from support. Initial paid backends are single-device CUDA/Metal; CPU is development only. Require 4 GiB available host RAM, including CUDA expert offload, plus the existing device headroom checks. Sampling checks host RAM during qualification. File size and upstream estimates do not certify low-end fit.

## Supersession map

| Active change | Amendment |
|---|---|
| `specify-managed-engine-launch` | llama-server pin/flags/GGUF clauses are legacy-only; common ownership, inventory, admission and failure invariants apply to both; this change specifies Strata's process tree and recipe. |
| `specify-public-inference-api` | Generic request/usage/security contract remains; GGUF/llama tokenization and logprobs are legacy-only; Strata supports only this change's text-chat subset. |
| `specify-inference-commercial-terms` | Admit `engine:strata` only for exact curated contracts/fixed settings. Existing agreement and evidence-renewal rules are unchanged. |
| `specify-generation-performance` | Artifact format is engine-specific; exact deployment evidence is required for either engine. |
| `specify-desktop-local-models` | Launch discovery is the local curated catalog, artifacts include pinned weights/tokenizer/template, and runtime is Strata. Broad repository search/GGUF selection/llama-only installation scenarios are deferred legacy scope. Remaining library, explicit loading, local chat and no-background-start requirements persist as target design. |
| `specify-desktop-share-online` | Launch shares refer only to exact catalog entries; legacy GGUF statements do not grant extra capabilities. Sharing/transport implementation remains separate. |

## Evidence and limits

See `docs/strata-runtime.md` for reproducible checks and host constraints. CPU/Metal native self-checks and pinned tokenizer integration passed locally. Protocol fixtures test failure boundaries without pretending to load real weights. Full-model inference, complete packaged runtime launch, actual gateway/SDK paid flow, hardware speed/fit and supported-platform installers remain qualification gates; no catalog entry is marked qualified. Existing llama.cpp evidence cannot close them.
