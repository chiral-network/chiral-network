# Curated Strata runtime

Strata is the launch selection. This implementation integrates a narrow private adapter with the existing Rust provider, accounting and qualification paths. The desktop model library/chat/local-server/share-online milestones remain separate work; selecting Strata does not mark those features implemented. Nothing here starts a public deployment or migrates an existing account.

## Catalog and limits

[`dev/strata/catalog.json`](../dev/strata/catalog.json) is the authoritative allowlist. `python3 scripts/provision-strata.py list` prints it without network requests or subprocesses.

| Catalog ID | Weight bytes | Initial capabilities | Qualification |
|---|---:|---|---|
| `inkling-2-mini-sq-10` | 10,471,526,400 | Text chat, streaming | Unqualified |
| `inkling-2-mini-sq-15` | 14,348,316,672 | Text chat, streaming | Unqualified |

Both use 4,096 tokens of context, at most 512 output tokens, and one concurrent request. Tokenizer/template files add about 26 MB. Installation also needs space for the runtime and temporary files. These are file sizes, not RAM requirements: CPU expert placement, KV allocation and loading overhead affect fit. The former 4 GB zoo-6 VM configuration cannot meet a 4 GiB **available** host-RAM reserve. An 8 GB GPU alone is insufficient evidence of fit.

The initial platform matrix is Linux/Windows CPU or CUDA, and Apple Silicon CPU or Metal. CPU is private development only; no platform/backend is paid-qualified by its inclusion in the catalog. Vulkan, arbitrary GGUF imports, embeddings, raw completions, tools, reasoning output and vision are outside this launch adapter.

Pins:

- Strata source: [`b28ca95e3e6847f2d5e317994f253b66716deeaa`](https://github.com/Niko1221/Strata/tree/b28ca95e3e6847f2d5e317994f253b66716deeaa), app release `0.2.22`, native Inkling component `0.1.69`.
- Model repository: [`strata-engine/Inkling-2-Mini-Strata`](https://huggingface.co/strata-engine/Inkling-2-Mini-Strata/tree/04996a1e2c45009b6a04f2af126cb6bdb2ba3587), revision `04996a1e2c45009b6a04f2af126cb6bdb2ba3587`, declared license `apache-2.0`.
- The catalog binds each weight/tokenizer/template SHA-256 and size; `upstream-files.json` binds the reviewed upstream support code. Adding a configuration requires a reviewed catalog change and new compatibility evidence.

## Install artifacts explicitly

Run with Python 3.12 or newer. This is an operator installer; desktop pause/resume/download UI is not implemented by it. It refuses to replace an existing destination and never starts inference.

```bash
python3 scripts/provision-strata.py model \
  --catalog-id inkling-2-mini-sq-10 \
  --accept-license apache-2.0 \
  --output /absolute/new/model-directory
```

`--source-dir /absolute/already-downloaded-files` installs the same pinned files offline after their checksums pass. It does not admit arbitrary imports. Failed installs remove their own staging directory and never produce an installed manifest. No old model or money state is deleted.

## Build and package the private runtime

Build the reviewed source's shared `engine/src/inkling2` CMake target, **not** its older standalone Metal runner. For example, after checking out the exact source commit:

```bash
cmake -S /absolute/Strata/engine/src/inkling2 \
  -B /absolute/new/strata-build -DCMAKE_BUILD_TYPE=Release
cmake --build /absolute/new/strata-build --target strata-inkling --parallel 4
```

macOS automatically includes the shared Metal backend. CUDA builds require the supported toolkit and `-DSTRATA_ENABLE_CUDA=ON`; qualify the compiler, GPU architecture and native runtime libraries on the deployment host. On Windows use the built `strata-inkling.exe`. No CUDA/Windows build was performed in this migration's local validation.

Supply a reviewed, self-contained Python 3.12+ distribution with `regex==2026.9.10`, `Jinja2==3.1.6` and `MarkupSafe==3.0.3` already installed. The Unix entry point must be `bin/python3`, Windows `python.exe`. A normal venv linked to a system interpreter is not a runtime package. Dependencies must import from inside this distribution; startup verifies their versions and locations under isolated Python (`-I -B`). Preserve their licenses. The interpreter and all support files are hashed as part of the runtime identity; the provider does not run pip or consult a system Python environment.

```bash
python3 scripts/provision-strata.py runtime \
  --source-dir /absolute/Strata \
  --engine-file /absolute/strata-build/strata-inkling \
  --engine-sha256 REVIEWED_BINARY_SHA256 \
  --python-dir /absolute/self-contained-python \
  --platform macos --architecture aarch64 --backend metal \
  --output /absolute/new/runtime-directory
```

Use `linux`/`windows`, `x86_64`/`aarch64` and `cpu`/`cuda`/`metal` as appropriate. Optional `--engine-libraries` copies a reviewed directory into `lib`; Linux loads only that inventoried directory through Chiral's explicit loader setting. System drivers remain platform prerequisites. Directory aliases and dependencies escaping their package are refused; internal file aliases are materialized. Native entry points must match the selected platform/architecture. A supplied binary checksum and source declaration are not a reproducible-build attestation.

```bash
python3 scripts/provision-strata.py check \
  --model-dir /absolute/model-directory \
  --runtime-dir /absolute/runtime-directory
```

The check verifies inventories, not execution or performance. Runtime files must remain immutable; any adapter change requires repackaging and new runtime qualification.

## Provider configuration and qualification

Use the existing [provider procedure](provider-qualification.md) for network identity, protected keys, policy, prices, private calibration, candidate profiles, funded measurements and deployment. Substitute the Strata model/runtime directories and these explicit runtime settings:

```json
{
  "strata_catalog": "inkling-2-mini-sq-10",
  "backend": "metal",
  "device_ids": ["MTL0"],
  "memory_reserve_mib": 4096,
  "gpu_layers": 0,
  "split_mode": "none",
  "tensor_split": [],
  "cache_type_k": "f16",
  "cache_type_v": "f16",
  "flash_attention": false,
  "batch_size": 512,
  "ubatch_size": 512,
  "startup_timeout_secs": 180
}
```

Set outer `context_length:4096`, `max_output_tokens:512`, `max_concurrency:1`. For CUDA choose `backend:"cuda"`, one `CUDA<n>` device and a VRAM reserve of at least 256 MiB; CPU chooses `backend:"cpu"`, no devices and reserve 4,096. Every Strata backend also requires 4,096 MiB available host RAM before and after load and during qualification. The shared legacy layer/attention fields above are fixed identity values, not flags forwarded to llama.cpp. The actual Strata recipe is DSpark beam 1, FP16 KV, KV offload off, chunk 512, one slot, fixed context and no anti-loop shortening.

The generated provider adds runtime/artifact paths. Use a fresh private endpoint and preserve existing providers, ledgers and obligations. Rust verifies the entire inventory before spawning the isolated interpreter; that adapter owns the native compute process. A normal stop, bridge failure or native failure withdraws service; there is no automatic restart, backend change, context reduction or fallback. The process boundary is not an OS sandbox.

For a measured candidate, set `serving.engine:"strata"`, `engine_version:"0.2.22"`; bind the actual runtime/artifact hashes, physical device observation and fixed settings above. `provision-provider.py` and `qualify-provider.py` select the verified Strata format explicitly. The qualification tool uses the same private prompt preparation and accepted-token stream as the provider. Passing legacy evidence cannot qualify a new Strata identity or renew an old agreement with changed engine terms.

## API and accounting

The public gateway remains the only client endpoint. Initially use `/v1/models` and `/v1/chat/completions`, plain string messages with system/user/assistant roles, optional `temperature`, `top_p`, nonnegative 32-bit `seed`, output limit and streaming. Log probabilities, stop strings, penalties, tools and other unsupported parameters are rejected before a funds reservation. `max_completion_tokens` is normalized by the public gateway. No reasoning or tool channel is advertised.

Private adapter routes (`/health`, `/v1/models`, `/chiral/props`, `/chiral/prepare`, `/v1/chat/completions`) all require the engine bearer, reject browser Origins and bind literal loopback only. No management, filesystem, download or generic engine proxy route is exposed. Body readers, bytes, native line length, deadlines and concurrency are bounded.

`/chiral/prepare` uses the identical pinned tokenizer/template path as generation, including protection of literal user-written control tokens. The native protocol's accepted `T` IDs determine generated-token observations. `DONE` must agree with those IDs and the exact prepared prompt count before authoritative usage is emitted. Proposed/rejected draft tokens never enter billing. Private `chiral_token_ids` become internal gateway timing/accounting metadata and are removed from client responses. Disconnection sends STOP and drains the same native process before its slot can be reused; failed drain kills it. A failed/truncated stream cannot settle an invented charge, and requests are never retried automatically.

## Verification and remaining gates

On 2026-11-25 this Mac compiled the exact pinned native source and passed its CPU and Metal self-checks (`Apple M4`, reported 17 GiB working set). These checks do not load the selected weights. The exact tokenizer/template files were downloaded and hashed; the adapter rendered multilingual text, protected literal control tokens and enforced context/parameter limits. Deterministic protocol tests cover usage mismatch, draft exclusion, cancellation, busy admission, native death, private routes and streaming/nonstreaming usage parity.

Repository regression checks passed: 510 Rust library tests (12 helper entry points ignored), 92 Python tests, 124 frontend tests, frontend type check/build, headless all-target compile check and all 35 strict OpenSpec validations. The final Strata and process-tree regression checks also passed after review fixes.

Real SQ-10/SQ-15 generation, a complete packaged-interpreter launch, full provider/gateway/SDK paid inference, sustained fit/performance and supported-platform installers still require actual validation. This workstation had approximately 4 GB free before builds, less than the 10.5 GB smallest weight file; no weight download was attempted. The recorded `chiral-zoo6-llm` SSH alias was unavailable here. Catalog entries therefore remain `unqualified`. No speed, 8 GB fit or public-release claim follows from these checks.

Reproduce offline checks with:

```bash
python3 -m unittest discover -s scripts -p 'test_*.py'
cargo test --manifest-path src-tauri/Cargo.toml --lib
openspec validate --all --strict
```

Optional tokenizer integration uses the three pinned Python dependencies in a test environment, `STRATA_SOURCE_DIR` pointing at the exact reviewed checkout and `STRATA_METADATA_DIR` containing the two verified tokenizer/template files. Tests clearly label protocol fixtures separately from real-model qualification.
