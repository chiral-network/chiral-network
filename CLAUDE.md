# Chiral Network Development Guide

## Direction and release status

Chiral Network is an **LLM-inference exchange**, not a file-sharing application or a general storage/container marketplace. Windows/macOS/Linux providers advertise signed model offers; consumers use provider-specific, prepaid, **non-refundable** balances through an OpenAI-compatible API. Public providers require HTTPS and public reachability regardless of OS.

- `docs/chiral-book.md` defines the current operational contract.
- `docs/llm-serving-migration-plan.md` retains the implementation sequence, evidence, and full **public-testnet** release gates. A private smoke run is not release completion.
- The code has cut over to inference with **native Conflux eSpace CFX**, testnet chain **71**, genesis **`0x3fb9d350f95424c3c27fcf248d9a6f633a83c2a75c11ef887ea41540c85e7804`**. Chiral-coin is deferred; no mainnet launch is authorized.
- Current scope is **private validation**, with public rollout deferred. The user approved **8 GB capacity and dedicating zoo-6 to private LLM testing**. CFX/CPU/GPU/SDK/native Linux/failure evidence is in the plan; small smoke runs are not speed certification or public-release approval. Retain public deployment, security, failure/load and installer gates.
- The user selected **llama.cpp only**, replacing vLLM. Use a pinned private server subprocess managed by Rust, not embedded FFI or a second engine adapter. Native CUDA/Metal/Vulkan profiles require separate hardware/OS qualification; CPU development is not a paid speed class. Do not claim measured superiority without comparable results.
- Whole models run within one provider's trusted network. No WAN tensor parallelism, home-provider NAT traversal, S3, container submission, CDN, Drive, ChiralDrop, or app-managed mining is part of this release.

## Architecture and source map

The desktop is Tauri 2 with Svelte 5 and Rust. Headless binaries share the Rust core without requiring desktop libraries.

| Area | Source |
|---|---|
| Desktop shell and pages | `src/App.svelte`, `src/pages/` |
| Native frontend boundary | `src/lib/native.ts` |
| Native command dispatch / streaming | `src-tauri/src/desktop.rs` |
| Consumer workflows and protected local state | `src-tauri/src/consumer.rs` |
| Signed wire types and canonical encoding | `src-tauri/src/protocol.rs` |
| Explicit network identity/configuration | `src-tauri/src/network.rs` |
| Wallet keys, signing, exact unit conversion | `src-tauri/src/wallet.rs` |
| Chain evidence and durable transaction journal | `src-tauri/src/chain_client.rs`, `rpc_client.rs` |
| Quotes, funding, credentials, reconciliation | `src-tauri/src/contracts.rs`, `billing_store.rs` |
| Public inference / private llama.cpp adapter | `src-tauri/src/inference_gateway.rs`, `inference_backend.rs` |
| Managed native process, inventories and device observation | `src-tauri/src/engine_runtime.rs` |
| Qualified profiles, token timing and inference waivers | `src-tauri/src/performance.rs` |
| Provider lifecycle and configuration | `src-tauri/src/app.rs`, `provider_config.rs` |
| Signed offer discovery | `src-tauri/src/dht.rs`, `offer_registry.rs` |
| Payment-gated reputation | `src-tauri/src/rating_api.rs`, `rating_storage.rs`, `reputation.rs` |
| Version authority and durable rollback protection | `src-tauri/src/version.rs` |
| Narrow chain RPC exposure | `src-tauri/src/chain_rpc_api.rs` |

Binaries: `chiral-desktop`, `chiral`, `chiral_daemon`, `relay_server`, `chiral_rpc_gateway`, and `chiral-policy-sign`. Registration of desktop commands is in `desktop.rs`; `lib.rs` exports the shared inference core, not legacy command wrappers.

Pages: marketplace, playground, provider administration, usage/API credentials, wallet, account/reputation, network, settings, and diagnostics. Browser previews must explicitly report that native operations are unavailable; never add simulated wallets, offers, balances, or inference output.

## Financial and recovery invariants

- Network identity is the complete **chain ID + genesis hash + deployment ID** tuple. Never infer it from a URL or silently fall back to legacy Geth.
- Monetary wire values are canonical decimal wei strings. Use checked integer arithmetic, not floating point. Charge each request with one ceil-rounded token-cost calculation.
- The platform fee is 50 basis points of funding, provider-remitted. Remittance gas is the provider's cost. Track obligations and actual remittances; collection is not trustless.
- Prepare one exact signed opening transaction, obtain a durably signed provider funding-intent acknowledgement, then broadcast those same bytes. A provider receives the signed transaction and can technically broadcast it; disclose this trust boundary.
- Acknowledged terms and transaction evidence survive quote/admission-lease expiry. Admission capacity expiry must never erase the agreement or permit conflicting sender/nonce commitments.
- No mempool credit. Verify signed raw transaction identity, sender, recipient, value, calldata, successful canonical execution, and the healthy eSpace **`safe` frontier**, rechecked before credit. First execution, safe acceptance and PoS `finalized` are distinct; missing receipt alone is unavailable evidence, not a reorg.
- Preserve original-operation sender/nonce ownership across restart and chain regression. Explicit cancellation is a fixed same-nonce self-transfer, costs gas, can lose to the original payment, and is **not a refund**. No automatic fee bump or replacement.
- Re-inclusion of the same transaction refreshes its evidence without duplicate credit. Orphaned funding remains recoverable; restoration must compensate reversals exactly. RPC unavailability is not evidence of an orphan.
- Reserve worst-case cost atomically before dispatch. Settle only validated authoritative usage. Unknown usage releases the consumer hold at provider expense; never invent a token count. Request status/receipts must support ambiguous-outcome recovery instead of automatic paid retries.
- Resuming an already-open account must not rotate an exported API credential. Explicit revocation/issuance/rotation and historical funding recovery must remain reachable without abandoning accepted balances.
- Qualified performance misses waive only inference cost, preserving known authoritative usage. Funding/platform fees and chain gas are not refunded. A capacity tier or CPU smoke does not establish a 20/50/100-token/sec class.
- Approved evidence-only renewal admits strictly newer qualification for otherwise identical serving terms. Preserve original agreements/prices/fees; bind the actual admitted report durably before dispatch and retain it through settlement/restart. Changed hardware, artifacts, engine, admission or speed terms require their own agreement. Old withdrawal latches remain durable.

## Storage and secrets

- Consumer, provider, reputation, and wallet-journal state is network-scoped. Provider ledgers, locks, and offer revisions are additionally scoped by signing wallet and persistent DHT peer identity.
- Processes using the **same signing wallet must share the same base `data_dir`/nonce journal**, even across provider deployments. Independent hosts without shared journal storage need distinct signing wallets; separate local journals are not a nonce coordinator.
- Wallet/private engine/operator secrets use protected files, not command-line arguments, frontend stores, images, logs, or repository fixtures. Keep explicit backup/export paths owner-private and refuse unsafe overwrites/symlinks.
- Private wallet keys never enter Svelte. Use native commands for signing and privileged operations. Model output is untrusted data, never HTML or executable tool instructions.
- Inputs reach the chosen provider in plaintext at its HTTPS endpoint. Signed model metadata is not model-execution attestation or confidential inference.
- Preserve legacy wallet, chain, download, Drive, and hosting data in place. Do not automatically import old balances, convert CHI/CHR to CFX, migrate old keys, delete directories, or auto-start legacy services. Schema-3 provider ledgers refuse older schemas without converting money state.

## Provider, relay, and model boundaries

- `chiral_daemon --config <provider.json>` has distinct public and loopback-only operator listeners. Operator routes require their protected bearer and reject browser `Origin` requests. No signing-key, engine-management, or tokenization route belongs on the public listener.
- Public inference authenticates before body buffering, bounds simultaneous body readers and bytes, and separates body-read deadlines from generation/SSE deadlines. Preserve non-queuing admission and reservation limits.
- llama.cpp runs privately, with a protected API-key file and no wallet key. Do not expose tokenization, template, metrics, slots, model management or arbitrary engine endpoints through the public gateway. Native runtime and GGUF inventories must be verified before launch; inherited environment must not override signed execution settings.
- Qualify each advertised capability with exact tokenizer/template identity and real inference/usage probes. Only pinned immutable model/tokenizer revisions are acceptable. Do not assume OpenAI-compatible engines have interchangeable tokenization or cancellation behavior.
- Provision pinned native runtime packages and GGUF artifacts with `scripts/provision-runtime.py` and `scripts/provision-model.py`; `scripts/provision-provider.py` and `scripts/provider-preflight.py` configure/check deployment. Generation and embeddings have separate task/capability/limit contracts. Embedded GGUF tokenizer/template metadata replaces the separate Python tokenizer path. Dedicated VRAM and unified system memory are distinct; reserve at least 4 GiB on unified-memory providers.
- Public providers require HTTPS and public reachability. Literal-loopback development exceptions are not public deployment qualification. Resolve and pin untrusted offer endpoints; reject private/metadata addresses, redirects, and proxy bypasses.
- DHT discovery is bounded and signed, with network-scoped namespaces, stable peer identities, monotonic revisions, expiry, and connection admission limits. Relay catalogs are caches, not exhaustive or authoritative provider directories.
- `relay_server` serves discovery, reputation, policy, and bounded chain RPC access—not prompts or inference. Feedback requires authentic canonical funding evidence; revisions replace one agreement's vote instead of multiplying its weight.
- Version-policy acceptance is signed, network/authority-scoped, durable, and monotonic across processes. Expiry does not erase an authenticated rollback watermark. Corrupt/insecure policy persistence fails closed. Release builds require a non-placeholder policy authority; signing keys stay with the operator/offline signer.
- CLI stdout is machine-readable JSON or explicit streaming events. Diagnostics belong on stderr.

## Development and verification

```bash
npm install
npm run check
npm run build
npm test

# Headless core/binaries: no GTK/WebKit requirement.
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --all-targets
cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --bins

# Native desktop, with platform dependencies installed.
npm run tauri:dev
cargo build --manifest-path src-tauri/Cargo.toml --bin chiral-desktop --features tauri/custom-protocol
```

Use explicit consumer/provider/relay/network profiles; see `README.md` for current commands and operator setup. Do not reintroduce old embedded RPC/genesis/platform-wallet defaults. Shared tests must remain deterministic and isolated; actual chain/engine/SDK/native smoke evidence is separately required. `tests/scaled/paid-inference.py` can kill its explicitly authorized isolated Compose provider; never point it at a shared/live deployment.

Toast conventions: use `toasts.show`/`detail` for immediate user-action feedback and `notify`/`notifyDetail` for event-driven notifications respecting settings.

## Existing infrastructure is not the new release

Historical deployments at `130.245.173.73` included `relay-server.service` and `cdn-freshnet.service` (the latter also managed legacy Geth). Their existence is not evidence that new inference or native CFX artifacts are deployed. Retiring/replacing live services requires an explicit cutover, backups, and an operational rollback plan; do not apply old file-sharing restart instructions to the new network blindly.

The separate `chiral-coin` working tree may contain active unpublished work. Do not modify, publish, reset, or attribute that work to a public commit without verified provenance and authorization. Chiral-coin publication is not a launch prerequisite after the approved CFX selection; native network/deployment identity and lifecycle qualification remain required.

The user explicitly authorized **dedicating zoo-6's RTX 4060 to private inference**: its pilot miner is stopped, and `chiral-llm-8gb.service` remains on the existing qualification VM, loopback-only. Do not restart that miner automatically. Preserve unrelated chain services, keys, data and resource monitoring. Other mining interruptions, GPU rental and public deployment still require authorization. Capacity tiers include `8gb` (at least 7,500 MiB per device); a small-model GPU smoke is not a qualified speed class.
