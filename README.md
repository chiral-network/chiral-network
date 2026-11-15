# Chiral Network

Chiral Network is an **LLM-serving marketplace**: discover signed model offers, fund a small provider-specific prepaid balance with **native CFX on Conflux eSpace testnet**, and use a capability-qualified OpenAI-compatible API. The implementation includes a Rust provider gateway/CLI, libp2p discovery and reputation relay, and a Tauri 2 / Svelte 5 consumer app. Whole models run behind each provider's private engine; inference goes directly to that provider, not through the DHT.

**Status: llama.cpp-only cutover implemented and privately exercised on Linux CPU/CUDA; not a public-testnet release.** Real paid chat, completions, embeddings, stock Python/Node SDKs, exact context limits, cancellation, engine failure and restart accounting were verified. Zoo-6's dedicated 8 GB RTX 4060 now runs a managed native llama.cpp provider. Windows/macOS provider paths are implemented but not hardware-qualified; the shared-memory UI was checked with an explicitly synthetic layout fixture. No smoke run certifies a speed class. Public HTTPS, supported-platform installers and the full security/failure/load matrix remain release gates. See the [execution evidence and open gates](docs/llm-serving-migration-plan.md#14-execution-evidence-and-open-release-checklist).

**Selected launch:** Conflux eSpace testnet, chain **71**, genesis **`0x3fb9d350f95424c3c27fcf248d9a6f633a83c2a75c11ef887ea41540c85e7804`**; chiral-coin is deferred. Funding requires successful canonical execution at the healthy **`safe` frontier**, rechecked before credit; first execution and PoS `finalized` are distinct. No mainnet launch or new mining pool is authorized. **llama.cpp is the only selected engine**, with native Windows/macOS/Linux provider support and a private managed subprocess. Prior vLLM measurements are historical, not qualification of the new runtime.

**Capacity tiers:** `8gb`, `16gb`, `32gb`, and `higher`. The 8 GB tier requires at least 7,500 MiB per selected device. Dedicated VRAM and Apple unified memory are labeled separately; a 16 GB Mac retains at least 4 GiB headroom and is not equivalent to a 16 GB dedicated GPU. Model fit, context, concurrency and speed require independent measurements.

The previous file/Drive/CDN/site-hosting/mining product and proposed storage/container marketplace are not runtime features. Legacy user data is preserved, **not automatically imported**. Neither old CHI nor historical CHR balances convert into CFX credit. Schema-3 provider ledgers refuse older schemas without conversion; preserve old ledgers and agreements.

## Read before funding

- Prepayments are **non-refundable**, provider-specific, and at risk if the provider disappears or meters dishonestly. Start small. Switching providers does not transfer credit.
- The provider sees your prompts and outputs. HTTPS protects transport; signed offers/receipts prove attribution, **not execution of the claimed model**, fair metering, or confidential inference.
- Funding incurs chain gas; 0.5% of accepted funding is charged as a provider-remitted platform fee. Remittance is journaled, not trustless.
- Never retry an ambiguous paid generation automatically. The examples disable SDK retries and use a request key for status lookup. Funding recovery is different: resume the original durable intent, do not create another payment.
- Qualified performance misses waive only the affected inference charge, retaining actual known token usage; funding, platform fees and chain gas are not refunded. Evidence-only renewal may keep existing balances usable on an otherwise identical qualified deployment; original agreements remain unchanged and each request binds its actual report. Neither the CPU run nor zoo-6's private GPU smoke certifies a speed class.

The [design reference and white paper](docs/chiral-book.md) defines the inference-only product, architecture, signed discovery and wire identities, payment-gated reputation, funding/accounting, exact performance guarantees, API limits and lifecycle/security boundaries. The [provider qualification guide](docs/provider-qualification.md) supplies complete provisioning inputs and measured-profile construction; the [migration plan](docs/llm-serving-migration-plan.md) retains dated evidence and public-release gates. [Known limitations](docs/bugs.md) separates defects from unexercised qualification.

## Build and develop

Prerequisites: Rust/Cargo, Node.js/npm, and—for native desktop builds—the [Tauri 2 platform prerequisites](https://v2.tauri.app/start/prerequisites/). Providers need an explicit compatible chain/RPC, a pinned native llama.cpp runtime and a verified GGUF artifact. Native execution does not require Python, Docker or WSL; hardware drivers and platform packaging still require qualification. No chain, faucet, or provider URL is silently selected.

```bash
npm ci
# Native desktop; load an explicit consumer JSON profile in Settings.
npm run tauri:dev

# Headless CLI/provider/relay/RPC gateway, without GTK/WebKit dependencies:
cargo build --manifest-path src-tauri/Cargo.toml --no-default-features --bins
```

Headless debug binaries are under `src-tauri/target/debug/`. Put that directory on `PATH` for the commands below. `npm run dev` is a browser-only UI preview: it does **not** emulate wallets, payments, credentials, or inference.

**Testing on a Mac?** Follow the [Apple Silicon compile/run guide](docs/macos-testing.md): native prerequisites, desktop build, pinned Metal runtime, protected local profiles, and a free end-to-end CLI/SDK/desktop smoke test without transferring testnet funds.

Developer checks (commands, not a claim that this documentation edit ran them):

```bash
npm run check
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml --no-default-features --all-targets
```

Release builds require `CHIRAL_POLICY_PUBLIC_KEY` to be the operator's nonzero 32-byte hex Ed25519 **public** policy key at compile time. Keep its private half offline; do not replace it with an arbitrary throwaway key to bypass release policy checks. `npm run tauri:build` builds native packaging; packaging and cross-platform qualification remain separate release work.

## Developer API quick start

Obtain a deliberate `consumer.json` profile for eSpace testnet chain 71 and the pinned genesis above, plus legitimate test funding; see [configuration](docs/chiral-book.md#configuration-and-state). Public chain availability does not imply a released Chiral provider/catalog. For isolated development only, use `development: true` and explicit loopback provider endpoints. The CLI accepts `--config PATH` or `CHIRAL_CONSUMER_CONFIG`.

```bash
chiral --config consumer.json wallet create
chiral --config consumer.json network status
chiral --config consumer.json wallet address
chiral --config consumer.json wallet balance
chiral --config consumer.json discover --model Qwen/Qwen2.5-0.5B-Instruct-GGUF/qwen2.5-0.5b-instruct-q4_k_m.gguf
```

Fund the displayed wallet using the configured network's funding procedure, allowing for gas. Save **one verified signed offer object**, not the entire discovery result, to `offer.json`. Obtain and inspect its signed quote; save the quote object to `quote.json`:

```bash
# 0.01 CFX at 10^18 wei/CFX; ensure this meets the selected offer's minimum.
chiral --config consumer.json quote --offer offer.json --amount-wei 10000000000000000
chiral --config consumer.json fund --offer offer.json --quote quote.json
chiral --config consumer.json pending
```

Use the quote ID as `CONTRACT_ID`. Pending settlement is not spendable credit: run `chiral --config consumer.json resume CONTRACT_ID` until the original payment is resolved, then inspect `account CONTRACT_ID`. Do not create a fresh quote merely because the first request timed out. A signed transaction disclosed during registration could be broadcast by the provider even if its acknowledgement never reaches you.

Fresh or upgraded provider ledgers conservatively initialize their quote-signing budget: the first valid proposal can receive HTTP 429 until a strictly newer minute. Signing work is limited to 600 attempts per provider ledger per effective minute, separately from 600 committed quotes; failed signing or insertion does not refund work. These limits do not reserve opening capacity or gate recovery of accepted accounts/payments. Retry an unpaid quote deliberately, never replace an ambiguous paid operation.

Create a separate application credential; its returned metadata supplies `CREDENTIAL_ID`:

```bash
chiral --config consumer.json credentials CONTRACT_ID issue
python -m pip install openai==3.41.0
python examples/openai_client.py --config consumer.json --contract CONTRACT_ID --credential CREDENTIAL_ID
chiral --config consumer.json usage CONTRACT_ID
chiral --config consumer.json receipt CONTRACT_ID
```

The example invokes explicit CLI credential export into its own process; it does not print the API key. Keep exports, wallet backups, journals, and bearer files private. A [JavaScript example](examples/openai-client.mjs) is also provided. Stock Python 3.41.0 and Node 7.45.1 OpenAI SDKs exercised the native-CFX-funded llama.cpp backend; this does not qualify all models or GPU performance. Supported endpoints are `/v1/models`, `/v1/chat/completions`, `/v1/completions`, and `/v1/embeddings` **only for a separately configured and qualified embedding model**. GGUF generation artifacts derive chat/completion capabilities from their task/template; tools and structured output need separate qualification. Completion echo preserves prompt text, but prompt-token probabilities remain unknown; see the [API limitations](docs/chiral-book.md#api-compatibility-and-model-qualification). This is not compatibility with every architecture, OpenAI API or parameter.

## Windows, macOS and Linux provider operations

Use the [complete provider qualification guide](docs/provider-qualification.md) and the book's [runtime/lifecycle contract](docs/chiral-book.md#native-provider-provisioning-and-operations). The guide covers `scripts/provision-model.py`, `scripts/provision-runtime.py`, `scripts/provision-provider.py` and `scripts/qualify-provider.py`, including the strict input schema and construction of a candidate from actual observations. Rust owns the private llama.cpp child; no Python inference environment or Docker is required. Linux Compose remains optional and has no separate engine service. The [private zoo-6 runbook](docs/zoo6-private-pilot.md) is deployment-specific, not a public provider default.

Production generation additionally requires observed qualification evidence. `--calibration` creates an unpublished loopback-only configuration, not a speed-certified public provider, and starts no services. For native host-path configuration:

```bash
chiral_daemon --config provider.json --version-policy version-policy.json --check
chiral_daemon --config provider.json --version-policy version-policy.json
relay_server --config relay.json
```

Run separate services/processes for provider and relay. The daemon's `--check` validates configuration, protected credentials and managed artifact/runtime inventories, **not** live inference or release readiness. Do not pass container-path configuration to a host binary. Public ingress must route only public gateway endpoints; engine/tokenizer/metrics ports and bearer-authenticated loopback administration must never be exposed. Local operator status:

```bash
python scripts/provider-admin.py --token-file /srv/chiral/deployment/secrets/admin.token status
```

The gateway does not install/start a chain node or miner. Use separately operated native eSpace RPC; mining payouts normally arrive in Core space and need an explicit Core-to-eSpace transfer before payment. Back up ledger and shared signing-wallet journal consistently before upgrades; never roll a stale database back over live accepted payments or delete old state to bypass schema refusal.

## License

Proprietary — all rights reserved.
