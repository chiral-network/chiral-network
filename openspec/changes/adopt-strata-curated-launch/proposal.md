## Why

The author selected Strata and a small curated model catalog for launch on 2026-11-25, then explicitly requested the book, implementation and OpenSpec revision. The previous llama.cpp-only and unrestricted Hugging Face acquisition requirements conflict with that decision. Strata has a different native protocol and tokenization path; changing a label would not preserve billing or qualify hardware.

## What Changes

- Select pinned Strata with two immutable Inkling-2-Mini configurations, SQ-10 and SQ-15, initially text chat/streaming only.
- Add catalog artifact installation, complete runtime packaging, Rust-managed process ownership, a private bounded HTTP/native-protocol adapter, exact preparation and accepted-token usage.
- Support Strata in existing provider provisioning, immutable serving profiles and qualification tooling. Reject unsupported requests before reservation.
- Preserve llama.cpp as explicit legacy development/recovery support, old evidence and all existing money state. Forbid automatic fallback and qualification relabeling.
- Reconcile active specifications and record real-model/platform qualification as open gates. This does not implement the separate desktop local-client/libp2p milestones or authorize deployment.

## Capabilities

### New Capabilities

- `strata-curated-runtime`: Launch catalog, pinned runtime/artifacts, native process protocol, private API, metering and qualification identity.

### Modified Capabilities

None of the archived baseline capabilities changes. Related unarchived changes are amended explicitly: `managed-engine-runtime`, `public-inference-api`, `inference-commercial-terms`, `inference-generation-performance`, and desktop acquisition/runtime/workbench/share specifications. Common accounting, credentials, agreement, lifecycle and transport requirements remain applicable.

## Impact

Book/README/operator guidance; `engine_runtime`, `inference_backend`, `performance`; provisioning and qualification scripts; deterministic protocol/inventory tests. Strata introduces an inventoried Python dependency alongside its native compute binary. No schema conversion, wallet migration, paid retry, live-service replacement or public publishing occurs.
