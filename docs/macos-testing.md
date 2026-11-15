# Compile and test on macOS (Apple Silicon)

This guide builds Chiral from source and runs a **private, free, local llama.cpp/Metal provider** plus the native desktop consumer. Start here on a **16 GB M-series Mac**. No Docker, CUDA, vLLM, local blockchain node, miner or testnet payment is needed. Internet access is still required for downloads and the configured eSpace RPC health checks.

The provider path requires **Apple Silicon running natively**, not an Intel Mac or an x86_64/Rosetta shell. Desktop consumer builds are a separate concern; the managed Metal observer currently rejects Intel/discrete-Metal providers. A 16 GB Mac has shared CPU/GPU memory, not 16 GB of dedicated VRAM. Keep at least 4 GiB headroom and begin with the small model below and one concurrent request.

Use a current supported macOS release. The pinned ARM64 package's Mach-O files declare **macOS 13.3** as their minimum; that loader metadata is a lower bound, not proof of runtime compatibility on every OS/device combination.

**Verification limit:** these are Mac test instructions, not a claim that Metal execution or a macOS installer has already been qualified. Record the results from your machine. The guide does not create a speed-qualified offer or a public provider.

## 1. Install native prerequisites

Use a checkout of this repository in a normal native Terminal. Install [Homebrew](https://brew.sh/) if it is not already available. [Tauri's macOS prerequisites](https://v2.tauri.app/start/prerequisites/#macos) permit Xcode Command Line Tools for desktop-only development:

```sh
xcode-select --install
```

Wait for the installer to finish; skip it if the tools are already installed. Then:

```sh
brew install node@22 python@3.11 cmake pkg-config protobuf
export PATH="$(brew --prefix node@22)/bin:$(brew --prefix python@3.11)/libexec/bin:$HOME/.cargo/bin:$PATH"
xcode-select -p
xcrun --find clang
uname -m
node --version
python3 --version
```

Expect `arm64` from `uname -m`, Node **22.12 or newer in the 22.x line**, and Python 3.11. If `uname` reports `x86_64` on an M-series Mac, disable **Open using Rosetta** for the terminal and reopen it before installing/building dependencies.

Install stable Rust using the official [rustup instructions](https://rustup.rs/) if missing, then reopen the terminal or source its environment:

```sh
. "$HOME/.cargo/env"
rustup show
rustc -vV
cargo --version
```

The Rust host must be `aarch64-apple-darwin`. Do not mix an Intel Homebrew/Rust installation with ARM64 runtime files. Use the native host target for this walkthrough; remove intentional cross-target overrides in your test shell before proceeding.

## 2. Compile Chiral

Run from the repository root. Keep this terminal's environment for the later commands:

```sh
export CHIRAL_REPO="$PWD"
npm ci
cargo build --locked --manifest-path src-tauri/Cargo.toml --no-default-features --bins
npm run tauri:build -- --debug --no-bundle
export CHIRAL_TARGET="$(cargo metadata --manifest-path src-tauri/Cargo.toml --format-version 1 --no-deps | python3 -c 'import json,sys; print(json.load(sys.stdin)["target_directory"])')"
export CHIRAL_BIN="$CHIRAL_TARGET/debug"
"$CHIRAL_BIN/chiral_daemon" --help
```

This builds the CLI/provider and a native desktop executable with the frontend embedded. Normally the executables are in `src-tauri/target/debug`; the metadata command also handles a custom target directory. Debug builds are deliberate: release builds require the real operator's compile-time `CHIRAL_POLICY_PUBLIC_KEY`. Do not invent a release authority to make a private test build pass.

Optional checks to include in your Mac test report:

```sh
npm run check
npm test
cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --all-targets
```

For frontend iteration, `npm run tauri:dev` opens the native app and starts Vite automatically. **`npm run dev` alone is only a browser preview** and cannot exercise native wallets, credentials or inference. The desktop does not automatically start a provider; run the daemon below separately.

## 3. Create a protected test workspace and install artifacts

Use fresh test wallets, not a copy of a wallet/journal from zoo-6 or another host. This creates a new directory outside the repository and never resets an existing ledger:

```sh
umask 077
export CHIRAL_MAC_ROOT="$(mktemp -d "$HOME/chiral-mac-test.XXXXXX")"
printf 'Test workspace: %s\n' "$CHIRAL_MAC_ROOT"
python3 -m venv "$CHIRAL_MAC_ROOT/venv"
. "$CHIRAL_MAC_ROOT/venv/bin/activate"
python -m pip install 'huggingface_hub==0.36.0' 'openai==3.41.0'
```

Keep that workspace path. In another terminal, set `CHIRAL_REPO`, `CHIRAL_BIN` and `CHIRAL_MAC_ROOT` to the same paths and activate the same venv; **do not run `mktemp` or wallet creation again** to resume this test.

Install the pinned upstream Apple Silicon binary package. This compiles **Chiral**, not llama.cpp itself; a floating Homebrew llama.cpp build is not interchangeable with the reviewed runtime inventory.

```sh
python scripts/provision-runtime.py \
  --url https://github.com/ggml-org/llama.cpp/releases/download/b12110/llama-b12110-bin-macos-arm64.tar.gz \
  --sha256 4f5b8de12e0b9516658ca6a5764c6ffb3d7c659c3dd538556b1413c7b4050662 \
  --source-revision cd959bae99ff4bed22bef2604f45241c09bbb8c8 \
  --output "$CHIRAL_MAC_ROOT/runtime" \
  --platform macos --architecture aarch64 --backend metal

python scripts/provision-model.py \
  --output "$CHIRAL_MAC_ROOT/model" \
  --repo Qwen/Qwen2.5-0.5B-Instruct-GGUF \
  --revision 9217f5db79a29953eb74d5343926648285ec7e67 \
  --gguf-file qwen2.5-0.5b-instruct-q4_k_m.gguf \
  --task generation --context-length 4096 --max-output-tokens 256 \
  --accept-license apache-2.0
```

`--accept-license` is an explicit acknowledgement of the upstream model license. Both installers refuse an existing output directory; reuse a successfully installed inventory rather than overwriting it. Do not manually change model/template hashes or substitute a different GGUF under the same contract.

Check the installed executable and actual Metal device before starting a server:

```sh
export LLAMA_SERVER="$(python -c 'import json,os,pathlib; p=pathlib.Path(os.environ["CHIRAL_MAC_ROOT"])/"runtime"; print(p/json.loads((p/"chiral-runtime.json").read_text())["executable"])')"
file "$LLAMA_SERVER"
"$LLAMA_SERVER" --version
"$LLAMA_SERVER" --list-devices
```

Expect an ARM64 Mach-O binary and an `MTL0` device. The reviewed source is **v0.9.0 / b12110**; its CLI may report `0.9.0-dev`. Stop if no Metal device appears. Do not launch a separate `llama-server`: Chiral will own its process and private port.

## 4. Generate local profiles and distinct wallets

This is an explicit choice of the [documented eSpace testnet RPC](https://doc.confluxnetwork.org/docs/espace/network-endpoints), not a new default or a public Chiral deployment. The software verifies chain **71** and the pinned genesis. Use an operator-approved alternative if this endpoint is unavailable; never weaken identity/freshness checks.

```sh
export CHIRAL_MAC_RPC='https://evmtestnet.confluxrpc.com'
python - <<'PY'
import json, os
from pathlib import Path
root = Path(os.environ['CHIRAL_MAC_ROOT'])
(root / 'secrets').mkdir(mode=0o700)
network = {
    'identity': {
        'chain_id': 71,
        'genesis_hash': '0x3fb9d350f95424c3c27fcf248d9a6f633a83c2a75c11ef887ea41540c85e7804',
        'deployment_id': 'macos-llama-local-v1',
    },
    'rpc_url': os.environ['CHIRAL_MAC_RPC'],
    'rpc_bearer_file': None, 'faucet_url': None,
    'explorer_url': None, 'policy_url': None,
}
for name, data in [('consumer.json', 'consumer-data'),
                   ('provider-wallet.json', 'provider-data')]:
    profile = {'network': network, 'data_dir': str(root / data),
               'catalog_url': None, 'development': True, 'dht': None}
    with (root / name).open('x') as out:
        json.dump(profile, out, indent=2)
PY

"$CHIRAL_BIN/chiral" --config "$CHIRAL_MAC_ROOT/provider-wallet.json" wallet create > "$CHIRAL_MAC_ROOT/provider-address.json"
"$CHIRAL_BIN/chiral" --config "$CHIRAL_MAC_ROOT/provider-wallet.json" wallet export --file "$CHIRAL_MAC_ROOT/secrets/provider.key"
"$CHIRAL_BIN/chiral" --config "$CHIRAL_MAC_ROOT/consumer.json" wallet create
"$CHIRAL_BIN/chiral" --config "$CHIRAL_MAC_ROOT/consumer.json" network status
```

Provider and consumer have different keys. The provider's wallet profile and daemon use the **same `provider-data` journal base**. Keys stay in protected files; none are pasted into JSON or shell arguments.

Generate the free provider configuration using the actual installed ModelSpec:

```sh
python - <<'PY'
import json, os, secrets
from pathlib import Path
root = Path(os.environ['CHIRAL_MAC_ROOT'])
network = json.loads((root / 'consumer.json').read_text())['network']
model = json.loads((root / 'model/chiral-model.json').read_text())
provider_address = json.loads((root / 'provider-address.json').read_text())['address']
for name in ['admin.token', 'engine.token']:
    with (root / 'secrets' / name).open('x') as out:
        out.write(secrets.token_hex(32) + '\n')
    (root / 'secrets' / name).chmod(0o600)
provider = {
    'network': network, 'data_dir': str(root / 'provider-data'),
    'signing_key_file': str(root / 'secrets/provider.key'),
    'admin_token_file': str(root / 'secrets/admin.token'),
    'public_endpoint': 'http://127.0.0.1:9419',
    'public_bind': '127.0.0.1:9419', 'admin_bind': '127.0.0.1:9421',
    'platform_wallet': provider_address, 'fee_bps': 50, 'min_funding_wei': '0',
    'development': True,
    'dht': {'listen': '/ip4/127.0.0.1/tcp/14011', 'bootstrap': [],
            'identity_file': str(root / 'secrets/peer.key')},
    'models': [{
        'model': model,
        'price': {'input_wei_per_1k': '0', 'output_wei_per_1k': '0'},
        'engine_url': 'http://127.0.0.1:8000',
        'engine_api_key_file': str(root / 'secrets/engine.token'),
        'max_concurrency': 1, 'request_timeout_secs': 120,
        'qualification_evidence_file': None,
        'runtime': {
            'runtime_dir': str(root / 'runtime'), 'artifact_dir': str(root / 'model'),
            'backend': 'metal', 'device_ids': ['MTL0'],
            'memory_reserve_mib': 4096, 'gpu_layers': 999,
            'split_mode': 'none', 'tensor_split': [],
            'cache_type_k': 'f16', 'cache_type_v': 'f16',
            'flash_attention': True, 'batch_size': 512, 'ubatch_size': 128,
            'startup_timeout_secs': 180,
        },
    }],
}
with (root / 'provider.json').open('x') as out:
    json.dump(provider, out, indent=2)
with (root / 'request.json').open('x') as out:
    json.dump({'model': model['id'], 'max_tokens': 64,
               'messages': [{'role': 'user', 'content': 'Explain what Metal does in two sentences.'}]}, out)
PY
```

The fee beneficiary is the newly created provider address **only for this zero-price local test**; no funding or fee is transferred. Do not convert this configuration into a paid/public service by merely changing prices. That requires the real operator's platform wallet, deployment policy and the separate paid-provider procedure. `serving` remains `null`: neither the model nor this configuration claims a measured speed class.

## 5. Run the managed provider

In the provider terminal:

```sh
"$CHIRAL_BIN/chiral_daemon" --config "$CHIRAL_MAC_ROOT/provider.json" --check
"$CHIRAL_BIN/chiral_daemon" --config "$CHIRAL_MAC_ROOT/provider.json"
```

`--check` validates configuration, protected credentials and inventories but does not run Metal. The second command starts the real engine and probes it. Wait for `Chiral provider public=... administration=...` before querying the service. Keep this terminal open. A debug placeholder-policy warning is expected in this private development setup, not acceptable for a public release.

In a second terminal with the same paths and venv:

```sh
cd "$CHIRAL_REPO"
curl --fail --max-time 15 http://127.0.0.1:9419/healthz
curl --fail --max-time 15 http://127.0.0.1:9419/readyz
python scripts/provider-admin.py --token-file "$CHIRAL_MAC_ROOT/secrets/admin.token" status
```

Ready means chain/engine checks actually passed. Temporary 503 readiness or 429 admission can occur during maintenance/probes; inspect status and wait for readiness rather than bypassing guards. `--check` alone is not evidence of a working Mac provider.

Save actual device/runtime observations without putting the admin bearer in argv:

```sh
python - <<'PY'
import json, os, urllib.request
from pathlib import Path
root = Path(os.environ['CHIRAL_MAC_ROOT'])
request = urllib.request.Request('http://127.0.0.1:9421/admin/runtime', headers={
    'Authorization': 'Bearer ' + (root / 'secrets/admin.token').read_text().strip()})
with urllib.request.urlopen(request, timeout=15) as response:
    result = json.load(response)
(root / 'runtime-observation.json').write_text(json.dumps(result, indent=2))
print(json.dumps(result, indent=2))
PY
```

Check `managed:true`, `ready:true`, `platform:"macos"`, `backend:"metal"`, `memory_kind:"unified"`, the observed Apple device, and `memory_reserve_mib:4096`. Do not fill missing hardware observations by hand. No relay, Caddy, public DHT bootstrap or port forwarding is required for this local test.

## 6. Open a free account and exercise real inference

Fetch the local signed offer through administration. The CLI still verifies it; this is not a fake offer or a bypass of payer authentication.

```sh
python scripts/provider-admin.py --token-file "$CHIRAL_MAC_ROOT/secrets/admin.token" offers > "$CHIRAL_MAC_ROOT/offers.json"
python - <<'PY'
import json, os
from pathlib import Path
root = Path(os.environ['CHIRAL_MAC_ROOT'])
offers = json.loads((root / 'offers.json').read_text())['offers']
assert len(offers) == 1, 'Expected one ready model; inspect provider status'
assert offers[0]['payload']['price'] == {'input_wei_per_1k': '0', 'output_wei_per_1k': '0'}
(root / 'offer.json').write_text(json.dumps(offers[0]))
PY
"$CHIRAL_BIN/chiral" --config "$CHIRAL_MAC_ROOT/consumer.json" quote --offer "$CHIRAL_MAC_ROOT/offer.json" --amount-wei 0 > "$CHIRAL_MAC_ROOT/quote.json"
export CHIRAL_CONTRACT="$(python -c 'import json,os,pathlib; print(json.loads((pathlib.Path(os.environ["CHIRAL_MAC_ROOT"])/"quote.json").read_text())["payload"]["id"])')"
"$CHIRAL_BIN/chiral" --config "$CHIRAL_MAC_ROOT/consumer.json" fund --offer "$CHIRAL_MAC_ROOT/offer.json" --quote "$CHIRAL_MAC_ROOT/quote.json"
"$CHIRAL_BIN/chiral" --config "$CHIRAL_MAC_ROOT/consumer.json" infer "$CHIRAL_CONTRACT" --request "$CHIRAL_MAC_ROOT/request.json" --kind chat --stream
"$CHIRAL_BIN/chiral" --config "$CHIRAL_MAC_ROOT/consumer.json" usage "$CHIRAL_CONTRACT"
"$CHIRAL_BIN/chiral" --config "$CHIRAL_MAC_ROOT/consumer.json" account "$CHIRAL_CONTRACT"
```

`fund` is the shared account-opening command: with these verified zero-price terms and `--amount-wei 0`, it opens a free session **without broadcasting a zero-value transaction**. Expect account status `free`, streamed real text, authoritative token usage and zero cost/reserved balance. Free credit is not payment-backed reputation eligibility. Run quote/open promptly because offers and quotes expire. On an interrupted opening, inspect `pending` and use `resume "$CHIRAL_CONTRACT"`; do not replace an ambiguous paid intent if you later test payments.

Also exercise the stock Python OpenAI SDK without printing an API secret:

```sh
"$CHIRAL_BIN/chiral" --config "$CHIRAL_MAC_ROOT/consumer.json" credentials "$CHIRAL_CONTRACT" issue > "$CHIRAL_MAC_ROOT/credential-metadata.json"
export CHIRAL_CREDENTIAL="$(python -c 'import json,os,pathlib; print(json.loads((pathlib.Path(os.environ["CHIRAL_MAC_ROOT"])/"credential-metadata.json").read_text())["id"])')"
python examples/openai_client.py \
  --chiral "$CHIRAL_BIN/chiral" --config "$CHIRAL_MAC_ROOT/consumer.json" \
  --contract "$CHIRAL_CONTRACT" --credential "$CHIRAL_CREDENTIAL" \
  --prompt 'What is 2 + 2? Answer briefly.'
```

The example exports the selected credential into its own process and disables automatic retries. Do not run `credentials export` directly into terminal logs. This verifies SDK compatibility, **not paid settlement or a performance guarantee**.

## 7. Run the native Mac app against that account

With the provider still running:

```sh
"$CHIRAL_BIN/chiral-desktop"
```

1. In **Settings**, enter the full path to `$CHIRAL_MAC_ROOT/consumer.json` (expand the variable yourself; the UI expects a real path). Click **Load native configuration**.
2. Open **Playground**, click **Refresh accounts**, and choose the Qwen free account created by the CLI. An empty Marketplace is expected: this test has no catalog or public bootstrap peers.
3. Choose **Chat**, set maximum output to 64, enter a prompt and click **Generate**. Confirm native streaming and **Reconcile request status and billing** show real usage and zero cost.
4. Try **Text completion**. For a cancellation check, use a long-list prompt and maximum output 256, then click **Cancel stream** while output is active. A small model may finish before cancellation; record what actually happened.
5. Check **Usage** for settled requests and no remaining reservation. Do not interpret the unqualified performance display as a hardware failure or a speed certification.

An optional local `.app` build uses the same embedded frontend:

```sh
npm run tauri:build -- --debug --bundles app
open "$CHIRAL_TARGET/debug/bundle/macos/Chiral Network.app"
```

This is a local debug bundle, not a notarized/distributable release. The `.app` does not bundle the downloaded model, provider configuration or llama.cpp runtime; keep the separately launched daemon and test workspace. Do not globally disable Gatekeeper or signature checks to work around an error.

## 8. Stop, restart and report

Stop the provider with **Ctrl-C** in its terminal and let graceful shutdown finish. Restart it with the same `provider.json`, keys, peer identity and data directory; confirm the existing consumer account still works. Do not use `kill -9` for routine shutdown: an abrupt macOS parent kill can orphan the engine. If a port is occupied, identify the owner before terminating anything:

```sh
lsof -nP -iTCP:8000 -sTCP:LISTEN
lsof -nP -iTCP:9419 -sTCP:LISTEN
```

For a useful report, retain: `sw_vers`, `uname -m`, `sysctl -n hw.memsize`, `rustc -vV`, Node version, the tested repository revision, build/test errors, runtime/artifact hashes, `runtime-observation.json`, request statuses/usage, and a native Playground screenshot. Note whether the run used AC power, memory-heavy applications or thermal throttling. Redact private machine identifiers before sharing if needed. **Never upload the whole workspace:** it contains wallet keys, bearer tokens and journals.

Troubleshooting:

- **Wrong architecture / missing Metal:** check native shell, Rust host and `file "$LLAMA_SERVER"`; do not substitute the x64 archive or silently fall back to CPU for a Metal test.
- **Insufficient memory/headroom:** close memory-heavy applications and inspect Activity Monitor / `vm_stat`. Keep the 4 GiB reserve, one slot and the small model. The reserve is a checked headroom requirement, not an OS-enforced memory partition.
- **HTTPS certificate errors:** repair the Python/OS CA trust configuration (or configure the legitimate organizational CA). Do not disable certificate verification or package hash checks.
- **Runtime, template or inventory mismatch:** keep the exact pinned files; do not edit hashes to suppress validation. Preserve the failure and provision into a new directory if replacement is necessary.
- **RPC/readiness failure:** inspect `network status` and provider status. Free inference still requires healthy configured chain identity; do not delete state or disable those checks.
- **Engine startup failure:** save the exact error plus macOS/device/runtime versions. A correct download or successful `--check` is not proof that the pinned binary loads on that OS.
- **Unsigned bundle / dyld loader failure:** report the exact message and installed OS/tools. Local debug testing is separate from signing/notarization and public installer support.

After a successful free run, use the [native provider runbook](chiral-book.md#native-provider-provisioning-and-operations) for separately authorized paid calibration and performance qualification. Do not publish this loopback-only configuration or label a short Mac smoke as a measured speed class.

## Validation performed while preparing this guide

- Parsed all 18 shell blocks and all four embedded Python setup/inspection blocks.
- Downloaded the exact macOS archive above, verified its SHA-256 and installed/validated its ARM64 native inventory on Linux without executing the Mac binaries. This exposed and fixed rejection of upstream's offline `ggml-metal-tuning` utility: it is now skipped like other unused upstream tools, not installed or executed. Unknown executables still fail closed; **46 Python tests passed**, including that regression.
- Executed the documented profile/wallet generation, signed free-account opening, CLI streaming and stock Python SDK flow against the real Linux CPU runtime, with an explicit CPU configuration substitution. Both inference requests completed with authoritative usage, zero cost and no wallet payment operation. This checks the instructions and shared flow, **not Metal performance or macOS execution**.
- Mac compilation, native UI behavior, Metal loading and restart results must still be collected on the tester's machine. Keep those observations distinct from this host-side validation.
