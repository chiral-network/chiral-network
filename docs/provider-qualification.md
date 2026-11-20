# Native provider provisioning and qualification

This is the operator companion to the [design book](chiral-book.md). It documents the current **Rust-managed native llama.cpp subprocess** path, on Conflux eSpace **testnet chain 71**. It is a procedure, not a record of an executed qualification, deployment, payment, or Windows/macOS hardware test. Public rollout remains deferred. Do not open public ingress merely because configuration generation succeeds.

There are three different activities:

1. **Free smoke:** use the [Apple Silicon macOS walkthrough](macos-testing.md). It uses a private debug provider, zero prices, no deposit and no generation speed claim. CPU development can likewise smoke-test execution, but cannot qualify a paid speed class.
2. **Private paid calibration:** the strict provisioner below requires real prices, signed policy and protected keys even with `--calibration`. It remains loopback-only and unpublished, with `serving: null`. Paid gateway measurements consume genuine, non-refundable prepaid funds.
3. **Qualified deployment:** only a passing measurement produces the qualified ModelSpec. Generation outside development additionally requires its evidence file, matching runtime and hardware, valid policy, readiness and operational release approval. A signature binds a provider declaration; it is not independent execution attestation.

## 1. Host, identity and safety prerequisites

Run the examples from the repository root. Shell blocks use a POSIX shell and `python` from a provisioning virtual environment. Native Windows supports the same Python and executable interfaces, but use PowerShell environment variables and save each Python here-document body to a temporary local file to execute it; POSIX shell syntax is not a Windows command. Do not run Windows providers through WSL and label the observation `windows`.

Obtain the operator-approved inputs before starting:

- Native `chiral`, `chiral_daemon`, `relay_server`, `chiral_rpc_gateway` and `chiral-policy-sign` binaries. Release builds require the real authority's **nonzero Ed25519 public key at compile time** as `CHIRAL_POLICY_PUBLIC_KEY`. The current runtime also supports a valid nonzero environment override, resolved/cached on first use; it changes the effective authority but does not remove the build requirement. Inspect the service environment and use only the intended authority consistently. The supplied policy must pass that authority's signature, validity-time and rollback checks. Keep its private key offline; it is not a provider input. Use `chiral-policy-sign verify --pub "$POLICY_PUBLIC_KEY" --in "$VERSION_POLICY_FILE"` to inspect an authority-supplied policy. Do not invent a policy authority to bypass a release gate.
- A fresh provider secp256k1 signing key in a protected regular, non-symlink file, as 64 hex characters (optional `0x` prefix), not a shell argument. Native `chiral ... wallet export --file ...` exports to a new protected file. Do not copy a live provider wallet to an independent nonce journal. If using a wallet profile for this provider, share its protected journal base with the provider's `data/provider`; importing/exporting a key is not permission to split nonce ownership.
- A separate random inbound RPC bearer file of at least 32 characters; optionally an upstream RPC bearer file. Provider administration and engine bearers are independently generated. Never put private key material or bearer values in JSON, argv, URLs, screenshots or logs.
- On POSIX, secret files must have no group/other permissions (for example `0600`) and private parent directories. On Windows the owner must be the current identity, inheritance disabled, with exactly one allow ACE for that identity; the provisioning module's `protect` helper implements this ACL. Use it rather than assuming `chmod` provides Windows isolation.
- The real platform beneficiary wallet (nonzero `0x` + 40 hex digits), approved upstream RPC, deployment ID, three distinct DNS names and ACME email. **These DNS/policy/price fields are mandatory even for loopback calibration**; calibration does not contact ACME or publish them. No example DNS name, wallet, hash or key below is a claim of a real deployment.
- Internet access for pinned downloads and upstream chain checks. No local Geth/miner or legacy CHI chain is part of this flow. CFX testnet balances and gas are distinct from historical CHI balances.

Set `WORK`, `MODEL_DIR`, `RUNTIME_DIR`, `DEPLOY_DIR` and `CHIRAL_BIN` to absolute operator-chosen paths; `CHIRAL_BIN` is the directory containing the native binaries. `WORK` should be a protected existing workspace; `MODEL_DIR`, `RUNTIME_DIR`, and the new `DEPLOY_DIR` must not already exist when their installers run. Reuse completed installations, never overwrite them to resume a run. The snippets use exclusive file creation and stop on existing output.

Set `umask 077` in **every POSIX operator terminal**, including those running qualification, so generated evidence and shell-captured JSON stay owner-private. Each JSON-writing Python snippet also sets this mask. On Windows first protect the workspace with the current-identity-only ACL described above; keep generated files within that workspace, and apply the same protected ACL to any files exported elsewhere. Never enable shell tracing around credential operations.

## 2. Install immutable model and runtime inventories

Only the provisioning environment needs Python/Hub tooling; the running provider does not:

```sh
umask 077
python3 -m venv "$WORK/venv"
. "$WORK/venv/bin/activate"
python -m pip install huggingface_hub==0.36.0
python scripts/provision-model.py --output "$MODEL_DIR" \
  --repo Qwen/Qwen2.5-0.5B-Instruct-GGUF \
  --revision 9217f5db79a29953eb74d5343926648285ec7e67 \
  --gguf-file qwen2.5-0.5b-instruct-q4_k_m.gguf \
  --task generation --context-length 4096 --max-output-tokens 256 \
  --accept-license apache-2.0
```

This is a pinned small-model installation example, **not** a speed claim. Accept the actual license/access conditions yourself. Repeat `--gguf-file` for every shard; use `--chat-template-name` if selecting a named embedded template. Embeddings use `--task embeddings --max-output-tokens 0` and zero output price, and do not qualify for this generation policy. Never enlarge model limits by editing `chiral-model.json` after installation: they are part of the immutable manifest.

The installer verifies upstream Git/LFS provenance, shard/tensor structure and bytes without executing model code. Quantization and tokenizer identity come from GGUF, not caller labels or Python tokenizer replacements. The manifest binds the original template bytes; `chat_template_sha256` binds the pinned Jinja lexer's effective source, including line-ending normalization and single trailing-newline removal. Installed architecture support is not a promise that arbitrary models execute correctly.

The reviewed runtime pin is **v0.9.0**, source `cd959bae99ff4bed22bef2604f45241c09bbb8c8`, official binary build **b12110** (CLI version reports `0.9.0-dev`). Linux x86-64 CUDA example:

```sh
python scripts/provision-runtime.py \
  --url https://github.com/ggml-org/llama.cpp/releases/download/b12110/llama-b12110-bin-ubuntu-cuda-12.8-x64.tar.gz \
  --sha256 4af1e1da23661db4a5b84b29aae16ebc053df689c5b8b1427c4670f2c6406d8c \
  --source-revision cd959bae99ff4bed22bef2604f45241c09bbb8c8 \
  --output "$RUNTIME_DIR" --platform linux --architecture x86_64 --backend cuda
```

For a **Linux x86-64 CPU smoke only**, substitute asset `llama-b12110-bin-ubuntu-x64.tar.gz`, digest `368227edcc229be5f9bed16a2223d79b5e41d0ac2e19bcf5825e82291c060e55`, and `--backend cpu`. Other approved packages and their actual hashes are in [`OFFICIAL_ASSETS`](../scripts/provision-runtime.py): native Windows CUDA/Vulkan/CPU and macOS ARM64 Metal, among others. An allowlisted macOS x64 package does **not** make Intel/discrete Metal a supported managed provider; the observer requires native Apple Silicon. Never rename an asset or substitute a floating release.

`--archive "$LOCAL_ARCHIVE" --sha256 "$INDEPENDENT_PACKAGE_SHA256"` replaces `--url` for an explicitly hashed local package at the same source revision. The operator supplies that real digest and provenance. The installer validates architecture, code inventory and safe library aliases, materializing permitted aliases as regular bytes; traversal, escaping/cyclic links and modified inventories fail closed.

**Linux CUDA packaging pitfall:** the official server archive does not bundle all CUDA runtime/cuBLAS dependencies. Supply compatible libraries through the native platform loader, or package verified redistributable bytes with provenance/licenses into a separately hashed local archive. The managed child intentionally does not inherit `LD_LIBRARY_PATH`. The historical private zoo-6 package used NVIDIA `nvidia-cuda-runtime-cu12` 12.8.90 and `nvidia-cublas-cu12` 12.8.4.1 native libraries; that observation is not a portable package/digest supplied by this guide. No wheel code runs in the provider. Resolve loader failures before claiming device discovery or readiness.

To inspect actual engine selectors without exposing a key:

```sh
python - <<'PY'
import json, os, subprocess
from pathlib import Path
root = Path(os.environ['RUNTIME_DIR'])
manifest = json.loads((root / 'chiral-runtime.json').read_text())
server = root / manifest['executable']
subprocess.run([str(server), '--version'], check=True)
subprocess.run([str(server), '--list-devices'], check=True)
PY
```

## 3. Construct the strict provisioning input

The following **executable JSON generator includes every required native field**. Set the named environment variables to real operator inputs first; they are paths/public settings, never secret contents. It uses installed model limits rather than retyping them. `BOOTSTRAP_JSON` is a JSON list of operator-approved full peer multiaddresses, or `[]` for private calibration. `DEVICE_IDS_JSON` contains the selectors actually reported above, e.g. `["CUDA0"]`, `["MTL0"]`, `["Vulkan0"]`; CPU uses `[]`.

Required environment inputs: `WORK`, `MODEL_DIR`, `RUNTIME_DIR`, `RELEASE_ID`, `POLICY_PUBLIC_KEY`, `VERSION_POLICY_FILE`, `PROVIDER_DOMAIN`, `RELAY_DOMAIN`, `RPC_DOMAIN`, `ACME_EMAIL`, `DEPLOYMENT_ID`, `UPSTREAM_RPC_URL`, `PLATFORM_WALLET`, `PROVIDER_KEY_FILE`, `RPC_TOKEN_FILE`, `BACKEND`, `DEVICE_IDS_JSON`, `INPUT_WEI_PER_1K`, `OUTPUT_WEI_PER_1K`, `MIN_FUNDING_WEI`. Optional environment overrides are visible in the generator. The illustrative numeric defaults select one slot and do not certify memory fit or throughput.

```sh
python - <<'PY'
import json, os
from pathlib import Path
os.umask(0o077)
E = os.environ
absolute = lambda key: str(Path(E[key]).resolve(strict=True))
model = json.loads((Path(E['MODEL_DIR']) / 'chiral-model.json').read_text())
backend = E['BACKEND']
c = {
    'release_id': E['RELEASE_ID'],
    'policy_public_key': E['POLICY_PUBLIC_KEY'],
    'version_policy_file': absolute('VERSION_POLICY_FILE'),
    'provider_domain': E['PROVIDER_DOMAIN'], 'relay_domain': E['RELAY_DOMAIN'],
    'rpc_domain': E['RPC_DOMAIN'], 'acme_email': E['ACME_EMAIL'],
    'network': {
        'identity': {
            'chain_id': 71,
            'genesis_hash': '0x3fb9d350f95424c3c27fcf248d9a6f633a83c2a75c11ef887ea41540c85e7804',
            'deployment_id': E['DEPLOYMENT_ID'],
        },
        'rpc_url': E['UPSTREAM_RPC_URL'], 'rpc_bearer_file': None,
        'faucet_url': E.get('FAUCET_URL') or None,
        'explorer_url': E.get('EXPLORER_URL') or None,
        'policy_url': None,
    },
    'platform_wallet': E['PLATFORM_WALLET'],
    'provider_key_file': absolute('PROVIDER_KEY_FILE'),
    'rpc_token_file': absolute('RPC_TOKEN_FILE'),
    'model_dir': absolute('MODEL_DIR'), 'runtime_dir': absolute('RUNTIME_DIR'),
    'runtime': {
        'backend': backend, 'device_ids': json.loads(E['DEVICE_IDS_JSON']),
        'memory_reserve_mib': int(E.get('MEMORY_RESERVE_MIB', '4096' if backend == 'metal' else '256')),
        'gpu_layers': int(E.get('GPU_LAYERS', '0' if backend == 'cpu' else '999')),
        'split_mode': E.get('SPLIT_MODE', 'none'),
        'tensor_split': json.loads(E.get('TENSOR_SPLIT_JSON', '[]')),
        'cache_type_k': E.get('CACHE_TYPE_K', 'f16'),
        'cache_type_v': E.get('CACHE_TYPE_V', 'f16'),
        'flash_attention': json.loads(E.get('FLASH_ATTENTION_JSON', 'true')),
        'batch_size': int(E.get('BATCH_SIZE', '512')),
        'ubatch_size': int(E.get('UBATCH_SIZE', '128')),
        'startup_timeout_secs': int(E.get('STARTUP_TIMEOUT_SECS', '180')),
    },
    'input_wei_per_1k': E['INPUT_WEI_PER_1K'],
    'output_wei_per_1k': E['OUTPUT_WEI_PER_1K'],
    'min_funding_wei': E['MIN_FUNDING_WEI'],
    'max_concurrency': int(E.get('MAX_CONCURRENCY', '1')),
    'max_output_tokens': model['max_output_tokens'],
    'context_length': model['context_length'],
    'request_timeout_secs': int(E.get('REQUEST_TIMEOUT_SECS', '120')),
    'provider_p2p_port': int(E.get('PROVIDER_P2P_PORT', '14011')),
    'relay_p2p_port': int(E.get('RELAY_P2P_PORT', '14012')),
    'bootstrap': json.loads(E.get('BOOTSTRAP_JSON', '[]')),
}
if E.get('UPSTREAM_RPC_TOKEN_FILE'):
    c['upstream_rpc_token_file'] = absolute('UPSTREAM_RPC_TOKEN_FILE')
# Set CONTAINER=1 only for the separate Linux container path below.
if E.get('CONTAINER') == '1':
    c.update(rust_image=E['RUST_IMAGE'], runtime_image=E['RUNTIME_IMAGE'],
             caddy_image=E['CADDY_IMAGE'],
             container_device_ids=json.loads(E['CONTAINER_DEVICE_IDS_JSON']))
with (Path(E['WORK']) / 'calibration-inputs.json').open('x') as out:
    json.dump(c, out, indent=2)
    out.write('\n')
PY
```

### Input contract and units

| Fields | Exact constraints / meaning |
|---|---|
| `release_id` | 1–64 ASCII letters/digits/dot/underscore/hyphen, first character alphanumeric. Container image tag, not network identity. |
| `policy_public_key` / policy file | Nonzero 64 hex public key, no `0x`; signed policy has 128 hex signature characters. Provisioner checks shape; the binaries verify authority/time/monotonicity. Required in calibration too. |
| DNS/email | Three distinct lowercase DNS names containing a dot, not IP literals; valid ACME email. Native public ingress is an operator-run Caddy service; calibration generates only a loopback 503 Caddy placeholder. |
| `network.identity` | Exactly `chain_id`, pinned `genesis_hash`, `deployment_id`; deployment ID is 1–128 ASCII alphanumeric/underscore/hyphen. Do not change identity to escape old state. |
| `network.rpc_url` | Credential-free HTTPS, or literal `127.0.0.1` / `::1` HTTP; no userinfo/query/fragment. Must not point to the three generated ingress domains. Use `upstream_rpc_token_file`, not `network.rpc_bearer_file`, for upstream credentials. |
| Prices/funding | Canonical decimal **strings**, `0` or nonzero without leading zeros, less than `2^128`; base units of testnet CFX, with `10^18` wei per CFX. Prices are wei per 1,000 input/output tokens, not per token. Minimum funding must be positive and at least one price nonzero, even with `--calibration`. Fee is fixed at 50 basis points. |
| Model limits | Must exactly equal installed artifact. Context 128–32,768; output 0–32,768 and strictly below context. Generation output positive; embeddings output and output price zero. **Qualification needs context ≥4,096 and max output ≥256.** A 2,048-context/128-output smoke model cannot qualify. |
| Concurrency/timeouts | `max_concurrency` 1–256. Request timeout 1–3,600 seconds; startup timeout 1–3,600 seconds. Runner HTTP timeout is independently 120 seconds. |
| P2P/bootstrap | Distinct integer ports 1,024–65,535; bootstrap entries start `/` and contain `/p2p/`. Calibration forcibly uses no bootstrap and loopback DHT, regardless of the input list. |
| Runtime | Exactly the 12 shown keys; no arbitrary engine flags or path keys. Backend `cpu`, `cuda`, `metal`, `vulkan`; installed runtime must match backend and host platform. GPU selectors must be unique explicit names; CPU has none and zero GPU layers. GPU layers 1–999 for GPU execution. |
| Memory/splitting | Reserve is an integer 256–`2^32` MiB in the provisioner (Metal minimum 4,096 MiB), and must leave usable physical capacity. One/no GPU: `split_mode: none`, empty weights. Multiple GPUs: `layer` or `row`, one positive weight per device; qualified profile weights ≤1,000,000. |
| KV/batching | Cache types: `f32`, `f16`, `bf16`, `q8_0`, `q4_0`, `q4_1`, `q5_0`, `q5_1`, `iq4_nl`. Quantized V cache requires flash attention. Batch and microbatch 1–8,192, microbatch ≤ batch. Flash attention is a JSON boolean. |

Native is the default. The generator accepts **no unknown fields**. Apart from the optional upstream bearer, only `qualified_model_file` and `qualification_evidence_file` can be added in native mode; omit both in calibration and embeddings.

### Paths and generated files

Input-relative file paths are interpreted against the **provisioner process working directory**, not the input JSON directory. The example deliberately resolves them to absolute paths. `--output` must be a fresh directory. Native generated paths are absolute; if writing raw provider/relay configs by hand, their supported relative paths resolve against their own config file's parent directory. The RPC gateway resolves its upstream bearer relative to its config, while command-line `--journal-dir`/`--bearer-file` follow the process directory; use absolute paths.

Provisioning writes separate configurations, not a monolithic daemon config:

- `rpc-gateway.json`: approved upstream network and optional upstream bearer; independent inbound `secrets/rpc.token` is a gateway CLI argument.
- `provider.json`: provider HTTP `127.0.0.1:9419`, admin `127.0.0.1:9421`, managed engine `127.0.0.1:8000`, provider journal and key, model/runtime config; network RPC points at `http://127.0.0.1:8547/api/chain/rpc` with the inbound RPC bearer.
- `relay.json`: relay HTTP `127.0.0.1:8081`, separate DHT identity/data, same local RPC gateway, signed policy file.
- `version-policy.json`, `Caddyfile`, private `secrets/{provider.key,rpc.token,admin.token,engine.token,upstream-rpc.token}`, `qualification/`, and `data/{provider,relay,rpc,tls,caddy}`. Empty upstream bearer file is intentional when absent; it is not used.
- Container mode additionally writes `compose.env` and `provider-devices.json`; paths map to `/etc/chiral`, `/run/secrets`, `/model`, `/runtime`, and per-service `/var/lib/chiral` mounts.

Do not publish the generated directories. Configs are read-only (`0440`), secrets `0400`, private state directories `0700`; native ownership is current user, container ownership UID/GID 10001. Nothing is started by provisioning.

## 4. Generate, start and inspect private calibration

For the design/configuration contract, see [native provider provisioning and operations in the book](chiral-book.md#native-provider-provisioning-and-operations).

```sh
python scripts/provision-provider.py --inputs "$WORK/calibration-inputs.json" \
  --output "$DEPLOY_DIR" --calibration
python scripts/provider-preflight.py --deployment "$DEPLOY_DIR" \
  --model-dir "$MODEL_DIR" --runtime-dir "$RUNTIME_DIR" \
  --daemon "$CHIRAL_BIN/chiral_daemon" --calibration
"$CHIRAL_BIN/chiral_daemon" --config "$DEPLOY_DIR/provider.json" \
  --version-policy "$DEPLOY_DIR/version-policy.json" --check
```

`--check` validates config, credentials and managed inventories, **not live inference**. Start each following process in its own terminal/service under the same protected native identity (set the same paths in each terminal). Start the RPC gateway first:

```sh
"$CHIRAL_BIN/chiral_rpc_gateway" --config "$DEPLOY_DIR/rpc-gateway.json" \
  --bind 127.0.0.1:8547 --journal-dir "$DEPLOY_DIR/data/rpc" \
  --bearer-file "$DEPLOY_DIR/secrets/rpc.token"
```

```sh
"$CHIRAL_BIN/relay_server" --config "$DEPLOY_DIR/relay.json"
```

```sh
"$CHIRAL_BIN/chiral_daemon" --config "$DEPLOY_DIR/provider.json" \
  --version-policy "$DEPLOY_DIR/version-policy.json"
```

The provider starts its own engine. Do **not** start an independent `llama-server`, expose an engine route, or restart over an occupied port. Then in an operator terminal:

```sh
python scripts/provider-preflight.py --deployment "$DEPLOY_DIR" \
  --model-dir "$MODEL_DIR" --runtime-dir "$RUNTIME_DIR" \
  --daemon "$CHIRAL_BIN/chiral_daemon" --calibration --running
python scripts/provider-admin.py --token-file "$DEPLOY_DIR/secrets/admin.token" status
```

Running preflight requires provider `/healthz` and `/readyz`, relay `/api/health` and `/api/ready`, and RPC gateway `/healthz` and `/readyz` all return 200, and that the RPC gateway answers a `POST /api/chain/rpc` without its bearer with 401; it checks authenticated managed runtime identity and that private/retired routes remain inaccessible via provider ingress. Listener inspection requires `ss` on Linux, `lsof` on macOS, or PowerShell `Get-NetTCPConnection` on Windows. Private ports 8000, 9419, 9421, 8547 and 8081 must remain loopback. Readiness is not qualification; a temporary 503/429 is not permission to bypass guards.

### Memory fit before choosing concurrency

The launcher sets aggregate `--ctx-size = context_length × max_concurrency`, `--kv-unified-per-slot = context_length`, and `--parallel = max_concurrency`. Context is **per slot**, not a total shared among all requests. Weights, per-slot KV cache, batches, scratch buffers and OS/driver usage must all fit. Automatic fitting/context shifting/prompt-cache reuse are disabled. Increasing concurrency can fail startup or headroom checks even if a single request fits. Start small; changing context, quantization, cache type, offload, batching, device allocation or concurrency changes the signed deployment terms and requires matching new evidence.

Capacity tier is a hardware bucket, not a promise that every model fits: `8gb` ≥7,500 MiB, `16gb` ≥15,000 MiB, `32gb` ≥30,000 MiB, `higher` ≥45,000 MiB. For dedicated devices the first three require that capacity on **each** device; `higher` uses their sum. Metal has one physical unified-memory inventory counted once, not CPU RAM plus invented VRAM. Reserve is enforced per dedicated device or once for unified memory. A nominal 16 GB Mac with 4 GiB reserve does not have 16 GB free for model allocations.

## 5. Construct the candidate from authenticated observations

Run on the managed runtime host while it is ready. Set `REGION` to a truthful nonempty location description (≤256 UTF-8 bytes, no control characters), `CAPACITY_TIER` to one of the four buckets above, `OUTPUT_TOKENS_PER_SECOND` to `20`, `50` or `100`, and `ARRIVAL_MILLI_RPS` to an integer 1–1,000,000. The latter is **milli-requests/second**: `50` = 0.05 requests/s, `1000` = 1 request/s. This is an offered-load envelope, not a measured result. Choose realistic load before spending; a deliberately low arrival rate is valid but makes a long run. `max_concurrency` comes from the observation, not a second caller guess.

```sh
python - <<'PY'
import hashlib, importlib, json, os, sys
from pathlib import Path
os.umask(0o077)
sys.path.insert(0, str(Path('scripts').resolve()))
p = importlib.import_module('provision-provider')
q = importlib.import_module('qualify-provider')
root = Path(os.environ['MODEL_DIR']).resolve(strict=True)
model, task = p.validate_artifact(root)
installed = json.loads((root / 'chiral-model.json').read_text())
assert model == installed and task == 'generation' and model['serving'] is None
assert model['context_length'] >= 4096 and model['max_output_tokens'] >= 256
manifest_bytes = (root / 'chiral-artifact-sha256.json').read_bytes()
manifest = json.loads(manifest_bytes)
token = q.secret(Path(os.environ['DEPLOY_DIR']) / 'secrets/admin.token')
rows = q.get_json('http://127.0.0.1:9421', '/admin/runtime', token)['models']
rows = [row for row in rows if row['model_id'] == model['id']]
assert len(rows) == 1 and rows[0]['managed'] is True and rows[0]['alive'] is True
o = rows[0]['observation']
assert o and o['model_id'] == model['id'] and o['pid'] > 0
assert o['engine_url'].rstrip('/') == 'http://127.0.0.1:8000'
assert o['context_length'] == model['context_length']
assert o['artifact_manifest_sha256'] == hashlib.sha256(manifest_bytes).hexdigest()
assert o['memory_kind'] in ('dedicated', 'unified'), 'CPU cannot qualify'
profile = {field: o[field] for field in q.OBSERVED_FIELDS}
profile.update(
    hf_repo=manifest['model']['repo_id'],
    tokenizer_repo=manifest['model']['repo_id'],
    engine='llama.cpp', region=os.environ['REGION'],
    capacity_tier=os.environ['CAPACITY_TIER'],
    output_tokens_per_second=int(os.environ['OUTPUT_TOKENS_PER_SECOND']),
    arrival_milli_rps=int(os.environ['ARRIVAL_MILLI_RPS']),
    policy='warm-generation-v1',
)
assert profile['capacity_tier'] in ('8gb', '16gb', '32gb', 'higher')
assert profile['output_tokens_per_second'] in (20, 50, 100)
assert 1 <= profile['arrival_milli_rps'] <= 1000000
assert 1 <= len(profile['region'].encode()) <= 256
assert not any(ord(c) < 32 or 127 <= ord(c) <= 159 for c in profile['region'])
# Deliberately NO qualification digest/time: only the runner may create evidence.
model['serving'] = profile
with (Path(os.environ['WORK']) / 'candidate-model.json').open('x') as out:
    json.dump(model, out, indent=2)
    out.write('\n')
PY
```

`/admin/runtime` reports the managed engine process's liveness as `alive` (`null` for an operator-owned engine) and keeps it apart from readiness: its `ready` is the model's latest readiness result, false once the engine has exited. A live process is not a ready model; check readiness with `GET /readyz` or the model's `ready` in `/admin/status`, as running preflight does.

This copies platform, backend, memory kind, all observed physical device IDs/names/memory/driver versions/selectors, runtime/artifact SHA256, engine version, reserve, GPU layers, split/weights, K/V cache, flash attention, batch/microbatch and concurrency. Do not replace a driver string or physical ID with a label. `revision`, `tokenizer_revision`, template hash, quantization, capabilities and context/output limits stay exactly as installed. Both repository identities come from `manifest.model.repo_id` because the tokenizer is embedded in this GGUF artifact.

The **candidate is a runner input, not a deployable Rust ModelSpec**: Rust `ServingProfile` requires `qualification`, but the Python runner intentionally accepts its absence and supplies it after measurement. Never add fabricated hashes/timestamps to make a candidate load as production. Do not copy observation-only `pid`, `engine_url` or `context_length` into `serving`; the schema rejects unknown fields.

## 6. Prepare paid accounts and protected inference keys

**Stop before this section if only doing a free/CPU smoke.** Candidate creation and readiness do not need inference payments. Qualification does: it refuses a zero-funded or zero-price gateway agreement. Prepayments are provider-specific and **non-refundable**; failed/aborted experiments can spend inference balance and gas. Do not assume a failed qualification means nothing was charged.

Create a separate protected consumer profile from the generated network, not the provider wallet. This generator only writes configuration; it does not create keys, request faucet funds, quote or broadcast:

```sh
python - <<'PY'
import json, os
from pathlib import Path
os.umask(0o077)
root = Path(os.environ['WORK'])
provider = json.loads((Path(os.environ['DEPLOY_DIR']) / 'provider.json').read_text())
profile = {'network': provider['network'], 'data_dir': str(root / 'consumer-data'),
           'catalog_url': None, 'development': True, 'dht': None}
with (root / 'consumer.json').open('x') as out:
    json.dump(profile, out, indent=2)
PY
"$CHIRAL_BIN/chiral" --config "$WORK/consumer.json" wallet create
"$CHIRAL_BIN/chiral" --config "$WORK/consumer.json" wallet address
"$CHIRAL_BIN/chiral" --config "$WORK/consumer.json" network status
```

On a resumed workspace, use `wallet address`, not `wallet create`. Obtain testnet CFX through the operator-approved faucet/funding procedure **manually**, and check `wallet balance`. There is no automatic transaction or faucet action in the setup snippets. Consumer and provider wallets must differ; do not import an existing active wallet onto a second host with independent journals.

Retrieve the actual private signed offer and select its exact model:

```sh
python - <<'PY'
import importlib, json, os, sys
from pathlib import Path
os.umask(0o077)
sys.path.insert(0, str(Path('scripts').resolve()))
q = importlib.import_module('qualify-provider')
root, deployment = Path(os.environ['WORK']), Path(os.environ['DEPLOY_DIR'])
model = json.loads((Path(os.environ['MODEL_DIR']) / 'chiral-model.json').read_text())
rows = q.get_json('http://127.0.0.1:9421', '/admin/offers',
                  q.secret(deployment / 'secrets/admin.token'))['offers']
rows = [row for row in rows if row['payload']['model'] == model]
assert len(rows) == 1, 'Require one ready offer with the exact unqualified artifact'
with (root / 'offer.json').open('x') as out:
    json.dump(rows[0], out, indent=2)
PY
"$CHIRAL_BIN/chiral" --config "$WORK/consumer.json" quote \
  --offer "$WORK/offer.json" --amount-wei "$APPROVED_FUNDING_WEI"
```

`APPROVED_FUNDING_WEI` is the operator's chosen decimal base-unit budget, at least the offer minimum and large enough for the run plus gas in the consumer wallet. Save the quote JSON to a fresh protected `$WORK/quote.json` (for example, redirect that quote command with shell noclobber enabled). Inspect the signed network, provider, endpoint, model, prices, fee beneficiary, concurrency, expiry and amount. Offers and quotes expire; do not change signed fields. The CLI verifies them again.

The explicit **manual spending step**, not part of an unattended setup script, is:

```text
chiral --config CONSUMER_JSON fund --offer OFFER_JSON --quote QUOTE_JSON
```

Substitute the actual file paths only after approving the non-refundable amount. `fund` prepares a durable intent, obtains an acknowledgement and broadcasts/opens; the provider receives signed bytes and could broadcast even without acknowledging. On ambiguous failure inspect `pending` and `resume CONTRACT_ID`, retaining the original transaction/intent. Never create a new intent to retry an unknown payment. `cancel-funding` is paid nonce recovery, not a refund, and the original may win. Copy the real contract ID from the quote payload's `id` into `CONTRACT_ID`, then confirm `account CONTRACT_ID` and `funding CONTRACT_ID` show the intended funded agreement.

Issue a key with `infer` **and** `read` scopes and enough concurrency for the test. The CLI's default **issue options** allow concurrency 1 (the automatic account-opening credential uses the agreement's limit), so write explicit options. Set `CREDENTIAL_EXPIRES_AT` to a future Unix second covering the full run, at most 90 days ahead, and `CREDENTIAL_SPEND_CAP_WEI` to an approved decimal budget (or the literal JSON `null` to rely on account balance). Credential concurrency cannot exceed the account limit; at most 64 active credentials are permitted per account. No funding is performed by these commands:

```sh
python - <<'PY'
import json, os, time
from pathlib import Path
os.umask(0o077)
root = Path(os.environ['WORK'])
profile = json.loads((root / 'candidate-model.json').read_text())['serving']
expires = int(os.environ['CREDENTIAL_EXPIRES_AT'])
now = int(time.time())
assert now < expires <= now + 90 * 86400
cap = os.environ['CREDENTIAL_SPEND_CAP_WEI']
options = {'expires_at': expires, 'spend_cap_wei': None if cap == 'null' else cap,
           'max_concurrency': profile['max_concurrency'], 'scopes': ['infer', 'read']}
with (root / 'credential-options.json').open('x') as out:
    json.dump(options, out)
PY
python - <<'PY'
import importlib, json, os, subprocess, sys
from pathlib import Path
os.umask(0o077)
sys.path.insert(0, str(Path('scripts').resolve()))
p = importlib.import_module('provision-provider')
root = Path(os.environ['WORK'])
base = [str(Path(os.environ['CHIRAL_BIN']) / 'chiral'), '--config', str(root / 'consumer.json'),
        'credentials', os.environ['CONTRACT_ID']]
metadata = json.loads(subprocess.run(base + ['issue', '--options', str(root / 'credential-options.json')],
                                    check=True, capture_output=True, text=True).stdout)
# CLI export is JSON; the runner needs the raw api_key only, never the JSON object.
exported = json.loads(subprocess.run(base + ['export', '--id', metadata['id']],
                                    check=True, capture_output=True, text=True).stdout)
path = root / 'gateway-infer-read.key'
fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
p.protect(path, 0o600)
with os.fdopen(fd, 'w') as out:
    out.write(exported['api_key'] + '\n')
with (root / 'qualification-credential-metadata.json').open('x') as out:
    json.dump(metadata, out, indent=2)
print('Saved protected infer+read bearer and non-secret metadata; no bearer printed.')
PY
```

Keep the metadata/contract-to-key mapping. Issue/export operations do not spend CFX, but the resulting bearer authorizes spending account credit. Cap must cover **maximum reserved** request usage as well as final charges; admission can fail even when final expected output is affordable. Read scope is needed for `/v1/requests/:id` and `/v1/contracts/:id` reconciliation.

Rate limiting is **120 inference attempts per account per minute**, not per credential; rejected attempts can count. Repeating a key or issuing more keys for the same account does not increase that limit. For higher arrival loads, prepare **distinct funded agreements** with the same exact artifact, prices as intended, and advertised concurrency, then repeat `--gateway-key-file` for their protected files. The runner rotates files round-robin; warmups and the completion echo probe use the first key, so leave headroom. Low arrival rate cannot hide insufficient concurrency: every declared concurrency must actually overlap in its sweep. Calibration has no signed arrival limiter, but qualification schedules and verifies the proposed envelope.

Budget from actual modes and prices, not a nominal single request: for `E` advertised generation endpoints and `N` samples/mode, each concurrency has `E` engine modes and `2E` paid gateway modes; the runner rounds `max(modes × N, concurrency)` upward to a multiple of concurrency. Add one warmup per mode and a paid 1-token completion echo probe when completions are advertised. Paid samples request 256 output tokens and up to 2,048 prompt tokens. Failed, interrupted and repeated runs require reconciliation and potentially additional budget; this guide never automatically tops up.

## 7. Run measured qualification deliberately

For the exact signed performance contract and its scope, see [performance accounting and withdrawal](chiral-book.md#performance-accounting-and-withdrawal) and [API compatibility](chiral-book.md#api-compatibility-and-model-qualification). The operational thresholds below are what the current runner actually exercises.

Set `PROBE_LOCATION` to a truthful description of where this host/probe sits. Use fresh output paths for every attempt. The following command sends **paid requests** and must be run only after approving the test budget:

```sh
python scripts/qualify-provider.py --candidate-model "$WORK/candidate-model.json" \
  --artifact-root "$MODEL_DIR" \
  --admin-url http://127.0.0.1:9421 \
  --admin-key-file "$DEPLOY_DIR/secrets/admin.token" \
  --engine-url http://127.0.0.1:8000 --gateway-url http://127.0.0.1:9419 \
  --engine-key-file "$DEPLOY_DIR/secrets/engine.token" \
  --gateway-key-file "$WORK/gateway-infer-read.key" \
  --probe-location "$PROBE_LOCATION" --samples-per-mode 100 \
  --evidence-out "$WORK/evidence.json" --model-out "$WORK/qualified-model.json"
```

Add further `--gateway-key-file /absolute/private/account-N.key` arguments as needed; do not pass secrets themselves. Admin/engine endpoints must be literal loopback HTTP origins, without path/query/fragment/credentials; run on the actual host. The runner disables environment proxies and redirects. It checks runtime identity both before and after the run.

Actual qualification requirements:

- Only generation with `chat` and/or `completions`, no embeddings; context ≥4,096 and output limit ≥256. Samples per mode **100–1,000**, not a quick two-request benchmark.
- All concurrency levels **1 through advertised maximum**, on engine and paid gateway. Engine tests streaming with authoritative single-token IDs; gateway tests streaming and non-streaming for every advertised generation endpoint. Completion echo/logprobs is probed on the public paid contract too.
- Warm, unique/non-repeated prompts in approximately 128, 512, 1,024 and 2,048 token buckets (each at target−8 through target), measured with the pinned tokenizer and actual template. Each mode/bucket must contain at least 20 samples. Warmups are real and discarded from the evidence statistics.
- Each passing sample has known final usage, exactly **256 output tokens**, prompt tokens 1–2,048, TTFT ≤**2,000,000 microseconds**, positive decode time and `(256−1) / decode_seconds ≥ declared 20/50/100 tokens/s`. Elapsed time must cover TTFT plus decode. It is a joint test, not separate favorable percentiles.
- At least **95% joint passes** in each sweep, mode and prompt-size bucket; offered arrival within **±5%** of declared milli-RPS; measured peak concurrency equals the swept level. Scheduling overload is recorded as failed load, not silently queued at a lower rate.
- Physical memory sampled every 0.2 seconds: minimum available at least the configured reserve, peak used at most total minus reserved capacity. Dedicated reserves apply per device. CUDA uses `nvidia-smi` UUID/total/used/free; Metal uses physical `hw.memsize` and conservative `vm_stat` free pages (cache/compressed/inactive pages count as used). Unsupported counters fail closed **before paid dispatch**. Current runner supports CUDA dedicated memory on Linux/Windows and Apple Silicon Metal unified memory; Vulkan execution is available but lacks a qualifying counter path. CPU/system memory cannot certify a GPU tier.
- Evidence is bounded to 64 MiB, profile/runtime/artifact/device identity must remain unchanged, and Rust revalidates the evidence at deployment/readiness. A runner success is not permission to skip that gate.

Private-engine timing, gateway internal timing, and external/client timing are recorded separately; a loopback run does not establish an Internet RTT guarantee. Qualification evidence is `chiral-performance-evidence-v2`, not a Docker-image attestation.

### Interruption, failures and request journal

The runner exclusively creates `evidence.json.requests.jsonl` **before inspecting or dispatching**. It fsyncs each prepared paid key and later completion mapping; the journal contains no prompts, output or credentials. It sets both `X-Request-Id` and `Idempotency-Key`. An existing journal prevents reuse of the same output identity even if no evidence/model was emitted.

After interruption, retain journal, consumer/provider ledgers, keys and account mapping. A `prepared` entry without `completed` is **ambiguous**, not proof of no charge; a completion journal entry is not by itself proof that all subsequent accounting checks passed. Reconcile each key/request ID via the owning account's `/v1/requests/:id` (or `chiral ... request CONTRACT_ID REQUEST_ID`) and inspect account usage before deciding what to do. The qualifier's journal is separate from CLI-generated request history. If dispatch failed before a provider request existed, lookup may return not found; do not infer that an ambiguous funding operation was absent from chain.

Do not replay old paid requests, delete journals, synthesize evidence or rename failed outputs to “qualified.” A fully measured failed class retains evidence but emits no qualified model; earlier exceptions can leave only the journal. A new attempt gets new file names, new keys and a separately approved budget. If reducing arrival/concurrency/speed or changing runtime settings, construct a corresponding new candidate; do not rewrite evidence. Historical reports are retained and never relabeled.

## 8. Promotion and state-preserving operations

Only after an actual passing run, generate a **new** deployment directory using a new input document. This operation generates files only; it does not publish or transfer balances:

```sh
python - <<'PY'
import json, os
from pathlib import Path
os.umask(0o077)
root = Path(os.environ['WORK'])
c = json.loads((root / 'calibration-inputs.json').read_text())
c['qualified_model_file'] = str((root / 'qualified-model.json').resolve(strict=True))
c['qualification_evidence_file'] = str((root / 'evidence.json').resolve(strict=True))
with (root / 'qualified-inputs.json').open('x') as out:
    json.dump(c, out, indent=2)
PY
python scripts/provision-provider.py --inputs "$WORK/qualified-inputs.json" \
  --output "$QUALIFIED_DEPLOY_DIR"
python scripts/provider-preflight.py --deployment "$QUALIFIED_DEPLOY_DIR" \
  --model-dir "$MODEL_DIR" --runtime-dir "$RUNTIME_DIR" \
  --daemon "$CHIRAL_BIN/chiral_daemon"
```

`QUALIFIED_DEPLOY_DIR` is another operator-selected absolute fresh path. Prices, devices, runtime, context, concurrency and all profile controls must still match. Do not run the old/new providers concurrently on the fixed ports or let the same provider key acquire independent active nonce journals. The new generator directory has **empty state**, not migrated accounts. Before cutover, drain and reconcile old service, back up protected state, and make an explicit state-preserving deployment plan. Do not execute a blind copy/reset from this guide.

Admin controls (loopback token file, no bearer argv):

```sh
python scripts/provider-admin.py --token-file "$DEPLOY_DIR/secrets/admin.token" drain
python scripts/provider-admin.py --token-file "$DEPLOY_DIR/secrets/admin.token" requests
python scripts/provider-admin.py --token-file "$DEPLOY_DIR/secrets/admin.token" accounts
python scripts/provider-admin.py --token-file "$DEPLOY_DIR/secrets/admin.token" fees
```

`resume` and `offers/refresh` are explicit later actions. `fees/remit` can send funds: **do not include it in smoke/qualification automation**. `provider-admin.py` does not expose a `runtime` action; use the authenticated Python recipe above for `/admin/runtime`.

Preserve accepted ledgers, acknowledgements, provider and peer identities, shared nonce journals, fee obligations, evidence and authenticated version-policy rollback watermarks across cutover/restart. Watermarks are authority/network-scoped protected local storage; corruption or insecure state fails closed. New artifact/runtime/profile terms are **not** evidence-only renewal, do not entitle old agreements to execute a different model, and do not transfer old credit. `serving: null` calibration agreements do not become qualified agreements by replacing config. Retain historical money/data and a deliberate way to reconcile existing obligations; do not delete old balances as “test cleanup.” Only a newer qualification of identical immutable execution terms can renew evidence for an existing qualified agreement.

Start the generated RPC/relay/provider configs using the same native interfaces, supply signed policy, then run preflight without `--calibration`, with `--running`. Its public endpoint check requires approved DNS/TLS ingress. Until explicit rollout approval, keep public services/firewall closed; offline config success does not override that gate. For an approved native public deployment, Caddy uses the generated `Caddyfile` (`caddy run --config "$QUALIFIED_DEPLOY_DIR/Caddyfile" --adapter caddyfile`) under an operator-managed service with persistent writable TLS/config storage and appropriate low-port permissions. Never reverse proxy admin, raw engine, tokenization, template or legacy wallet routes.

Normal shutdown stops the managed child. Linux uses parent-death termination; Windows uses a kill-on-close Job Object. On macOS graceful shutdown/drop stops the child group, but abrupt parent `SIGKILL` can leave an orphan; identify and clean it up before restarting. Never kill an unrelated process merely because port 8000 is occupied.

## 9. Optional Linux container extension

Native remains the default on Linux, Windows and macOS. The reference Compose path is **Linux host networking only**, CUDA or development CPU, not a Docker requirement and not a Windows/macOS container recipe. There is no standalone engine container; Rust manages the child inside the provider container with verified read-only model/runtime mounts.

For container input generation set `CONTAINER=1` and supply these **additional required fields** through the generator:

| Field / environment input | Required value |
|---|---|
| `rust_image` / `RUST_IMAGE` | Actual operator-approved builder image reference `name@sha256:<real 64 lowercase hex digest>`. |
| `runtime_image` / `RUNTIME_IMAGE` | Actual immutable Debian/runtime base image digest; this is not `runtime_dir` or a llama.cpp image. |
| `caddy_image` / `CADDY_IMAGE` | Actual immutable Caddy image digest. |
| `container_device_ids` / `CONTAINER_DEVICE_IDS_JSON` | Physical CUDA UUIDs `GPU-…`, one per selected engine device, same order, no duplicates; CPU uses `[]`. These are not `CUDA0` labels. Qualified inputs must match observed `devices[].id`. |

Run provisioning as Linux root for isolated UID/GID 10001 ownership, with the intended paths/environment explicitly available (do not assume `sudo` preserves environment). Paths must be absolute and Compose-safe: no newline, carriage return, `$`, space, `#`, `:`, or backslash in deployment/model/runtime paths. Keep native CUDA dependencies usable inside the base image/verified runtime package. Add `--container` to the calibration or qualified provisioner command; keep `--calibration` for the private measurement stage.

```sh
python scripts/provider-preflight.py --deployment "$DEPLOY_DIR" \
  --model-dir "$MODEL_DIR" --runtime-dir "$RUNTIME_DIR" \
  --compose docker-compose.yml --calibration
docker compose --env-file "$DEPLOY_DIR/compose.env" \
  -f docker-compose.yml -f "$DEPLOY_DIR/provider-devices.json" up -d --build
```

Repeat preflight with `--running` after startup. `docker-compose.yml` itself extends the generated device file, so preflight resolves that extension even though its `--compose` argument takes one path. Include the override explicitly in lifecycle Compose commands. Calibration sets `COMPOSE_PROFILES` empty, so public ingress is not started; qualified generation sets it to `public`, so **do not run its Compose startup until public rollout is approved**.

Container secrets/state are owned by UID 10001: run authorized host-side inspection/qualification with an identity permitted to read them (typically root), never widen file modes. Qualification still runs on the runtime host, uses loopback endpoints and real physical counters, and does not certify an image ID. Container/native path mappings differ; do not feed container configs to native startup commands.

## Documentation validation and limits

This revision was exercised on **Linux x86-64 with the existing verified CPU runtime and GGUF artifact**, not a Mac/Windows GPU or a qualified speed profile. The strict input generator, provisioner, offline preflight and daemon `--check` passed using a fresh isolated test-only policy authority, fresh unfunded provider key, deliberately reserved `.test` DNS names and loopback calibration. Those values were validation inputs, not release configuration. Separate native RPC, relay and managed-provider processes then passed running preflight against the configured eSpace testnet's read-only RPC.

The candidate recipe reached the real authenticated runtime observation and rejected CPU memory with `CPU cannot qualify`, without writing a candidate or evidence. Consumer-profile creation, a fresh wallet and signed **quote only** succeeded; no funding transaction or paid qualification was performed.

Separately, the documented credential issue/export fragment was exercised against an existing **free** local account with explicit one-slot credential options. A real managed CPU chat returned `verified`, authoritative usage **36 input / 2 output tokens**, and **zero charge**. Reusing its idempotency key returned HTTP 409; after explicit revocation the exported credential returned HTTP 401. All temporary services stopped and their private listening ports were released. Protected local evidence is at `/tmp/chiral-book-reference-g497mkf2/documentation-validation.json`; it is not a published qualification report.

Shell/Python snippets and local document links were checked. This does not claim execution of the paid runner, a passing GPU candidate/profile, container or public HTTPS deployment, Windows/macOS commands, or the complete release matrix. The full provisioning/qualification path still requires the actual operator authority, funded budgets and measured target hardware described above.

## Source contracts

The operator-facing behavior above is derived from [`provision-model.py`](../scripts/provision-model.py), [`provision-runtime.py`](../scripts/provision-runtime.py), [`provision-provider.py`](../scripts/provision-provider.py), [`provider-preflight.py`](../scripts/provider-preflight.py), [`provider-admin.py`](../scripts/provider-admin.py), [`qualify-provider.py`](../scripts/qualify-provider.py), [`protocol.rs`](../src-tauri/src/protocol.rs), [`performance.rs`](../src-tauri/src/performance.rs), [`engine_runtime.rs`](../src-tauri/src/engine_runtime.rs), [`provider_config.rs`](../src-tauri/src/provider_config.rs), the [native CLI](../src-tauri/src/bin/chiral.rs), and [Compose configuration](../docker-compose.yml). If release pins or schemas change, regenerate and requalify rather than editing signed inventories to resemble an earlier result.
