> **2026-11-25 scope amendment:** The llama-server pin, flags, single-native-entry-point and GGUF-specific clauses below govern only the explicit legacy llama.cpp path. Strata launch uses `strata-curated-runtime`; common inventory, lifecycle, occupied-port, credential and no-fallback requirements still apply. Its verified interpreter/native process tree and inventoried Linux library path are intentional Strata-specific differences. The author explicitly authorized revising the book; earlier read-only/book-line references record the prior baseline. See `adopt-strata-curated-launch` for supersession and current verification gates.

## Why

The read-only whitepaper (`docs/chiral-book.md`) defines how the provider daemon runs its engine. Line 73 says that Rust "starts and supervises a pinned private `llama-server` subprocess, supplies explicit settings and verifies artifacts before use". Line 75 says that the subprocess is not a sandbox and that "No automatic downloads, floating model updates or fallback engines are allowed at request time". Lines 90 and 92 set the headroom (4,096 MiB for unified memory, 256 MiB per dedicated device) and say that the managed launch "allocates total context as per-slot context × maximum concurrency, disables automatic fitting/context shifting and does not silently reduce the contract". Lines 615–619 require the inventory to be verified before each launch, physical devices to be observed independently, fit and headroom to be checked before startup and after loading, and the child to receive "a minimal environment and a protected API-key file path, not the secret in argv". Lines 627, 632 and 642 refuse an occupied engine port, forbid engine restart, and define how children are contained.

The code implements this in `engine_runtime.rs` and `app.rs`, but no current-runtime specification states it (`docs/apas-worker/comparison-2026-11-18.md` item 8). Only the target-design desktop specs `desktop-local-runtime` and `desktop-share-online` state similar rules, for the desktop. No test asserts the launch arguments or the cleared environment, so a later change could weaken them without a failing requirement.

## What Changes

- Add the `managed-engine-runtime` capability with 11 requirements and 48 scenarios. Each requirement cites its whitepaper lines. Numeric values that the book does not state are recorded in `design.md` as current implementation values; the child environment list and the contract-argument table are normative for the pinned engine.
- Specify that a managed deployment runs only the pinned native `llama-server`, started and supervised from Rust, with no download, floating revision or fallback, and that the child is not a sandbox.
- Specify inventory verification before every launch: the runtime and artifact inventories, the ModelSpec, the profile's equality with the runtime configuration, and runtime identity as the canonical manifest digest.
- Specify the runtime configuration's explicit bounds, including the 4,096 MiB unified and 256 MiB per-device reserves, refused rather than adjusted.
- Specify the literal-loopback engine endpoint and the occupied-port refusal, including a wildcard listener on the same port, while connections that only wait to expire do not occupy it.
- Specify the contract arguments: total context = per-slot context × concurrency, per-slot context, slots, output allowance, `--fit off`, `--no-context-shift`, devices, offload, split, KV cache, flash attention, batching, logging off, task and template.
- Specify the exact child environment, discarded output, and the protected engine key passed only by path and distinct from the signing key, administration token and RPC bearer.
- Specify independent device observation, the per-backend memory kind, headroom before startup and after loading, and observed devices equal to a serving profile.
- Specify that the contract is never reduced to make a model fit, and that readiness confirms the engine's per-slot context, slots, alias and build.
- Specify bounded startup, that a failed launch serves nothing, that an engine exit withdraws service with no restart, and how shutdown, a termination request during startup and parent termination stop the engines.
- Record a coverage table, three code differences, observations, two open questions and the risks in `design.md`. Code differences and missing tests are unchecked tasks; this step changes no code and no test.

## Capabilities

### New Capabilities

- `managed-engine-runtime`: The current provider daemon's managed llama.cpp engine: the pinned private child, inventory verification before every launch, bounded explicit runtime settings, the literal-loopback endpoint and occupied-port refusal, the contract arguments, the minimal environment and the protected key file, independent device observation and headroom before and after loading, no contract reduction, bounded startup, no restart after an exit, and containment at shutdown.

### Modified Capabilities

None. The spec references the pending `inference-commercial-terms`, `inference-generation-performance` and `inference-offer-discovery` capabilities by requirement name and adds no obligation to them. The design relates it to the pending `profile-path-resolution` and to the target-design counterparts `desktop-local-runtime` (M1, `specify-desktop-local-models`) and `desktop-share-online` (M4), which describe the desktop's engine and are not changed.

## Impact

- Authority: `docs/chiral-book.md` stays read-only and unchanged (SHA-256 `eda22ce01020e0f2c6d8cc51c8081b7d9d9113b5c611441cb769f225af7d3933`).
- Code that corresponds to these requirements:
  - `src-tauri/src/engine_runtime.rs`: `EngineRuntimeConfig::validate`, `validate_inventory`, `inventory`, `validate_native_executable`, `command`, `launch_command`, `ManagedEngine::start`, `await_health`, `is_alive`, `terminate`, `stop`, `Drop`, `capture`, `observe_devices` and `WindowsJob`;
  - `src-tauri/src/app.rs`: managed engine start, the engine supervisor, `refresh`, `stop_engines`, `ready` and `admin_runtime`;
  - `src-tauri/src/provider_config.rs`: `ModelConfig`, path resolution, `validate` and `read_secret`;
  - `src-tauri/src/bin/chiral_daemon.rs`: `--check` and shutdown;
  - `src-tauri/src/inference_backend.rs`: the engine bearer and the `/props` readiness checks.
- Three code differences were found. Tasks 2.1–2.3 track them:
  - the engine key is not checked against the signing key, administration token or RPC bearer, so a configuration naming the signing key file hands the wallet key to the engine;
  - the occupied-port probe uses address reuse, so on macOS a wildcard listener on the engine port is not detected;
  - the daemon installs no termination handler until it has started, so on macOS a `SIGTERM` or `SIGINT` while an engine loads orphans the engine.
- Missing acceptance evidence is tracked as unchecked tasks 3.1–3.11. The main gaps are tests of the launch arguments and the child environment, of the runtime-configuration bounds, of the occupied-port refusal, of device-observation parsing and post-load headroom, and of a provider-level engine exit with no restart.
- Real launch evidence on Linux CUDA, macOS Metal and Windows CUDA is task 4.1. It needs authorized private hosts and is never satisfied by fixtures.
- Two questions for the whitepaper author are recorded in `design.md` and `docs/apas-worker/report.md`: what "before startup" requires of fit, and what headroom applies to a CPU runtime outside development.
- No code, test, wire format, ledger schema, deployment or whitepaper change is part of this specification step. It grants no hardware, speed, public-testnet or release qualification.
