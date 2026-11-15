# Verification

The current runtime is LLM-only. File-transfer, Drive, CDN, relay-circuit and bundled-miner test commands from the previous application no longer apply.

## Local checks

```bash
npm ci
npm run check
npm test
npm run build
cargo test --locked --manifest-path src-tauri/Cargo.toml --no-default-features --all-targets
```

Headless Rust checks require no GTK/WebKit installation. They include real loopback TCP/Noise/Yamux discovery and connection-limit exercises, protected-storage/process-lock checks, signed-policy restart tests, and deterministic chain/engine fixtures. Fixtures are regression evidence, not a substitute for a real blockchain or GPU engine.

With the [Tauri platform prerequisites](https://v2.tauri.app/start/prerequisites/) installed:

```bash
npm run tauri:build -- --debug --no-bundle
cargo test --locked --manifest-path src-tauri/Cargo.toml --all-targets
```

The native build embeds the frontend; `--no-bundle` does not produce signed installers. CI defines native builds/tests on Linux, Windows and macOS, but a workflow definition is not evidence that those jobs or platform UI scenarios have passed. Release builds additionally require the actual nonzero policy-verification key at build time.

Workflow syntax/semantics can be checked with Go installed:

```bash
go run github.com/rhysd/actionlint/cmd/actionlint@v1.7.7 .github/workflows/test.yml
```

`.github/actionlint.yaml` declares the isolated GPU runner's custom label. Dispatch concurrency is separate from push/PR concurrency; running manual qualification is not automatically cancelled by a newer push/PR run.

## Real paid-inference qualification

Use only an explicitly provisioned, isolated chain/provider deployment and disposable funded test wallets. The harness sends non-refundable payments, consumes inference credit, and intentionally restarts the selected provider/engine. Never point it at a shared production deployment.

```bash
python3 -m venv .qualification-venv
.qualification-venv/bin/pip install -r tests/scaled/requirements.txt
.qualification-venv/bin/python tests/scaled/paid-inference.py \
  --cli /absolute/path/to/chiral \
  --config /absolute/path/to/consumer.json \
  --offer /absolute/path/to/verified-offer.json \
  --funding-wei 10000000000000000 \
  --restart-env /absolute/path/to/compose.env \
  --compose /absolute/path/to/docker-compose.yml \
  --allow-isolated-restart \
  --report /absolute/path/to/qualification-report.json
```

The funding amount must satisfy the selected offer and the wallet must also cover gas. A pending receipt is not spendable credit. Retain the original account/transaction journal and resume it; do not send a replacement payment merely because a test timed out. Keep secrets and signed raw transactions out of reports.

The opt-in `real_gpu_qualification` workflow requires a protected `llm-qualification` environment and an explicitly configured `chiral-isolated-gpu` runner. The complete release matrix includes stock Python/JavaScript SDKs, actual native surfaces, exact reconciliation, funding/reorg/cancellation, credential boundaries, malformed/failed engines, concurrency/load, backup/restore, public ingress and independently measured llama.cpp OS/backend workloads. Run `python3 -m unittest discover -s scripts -p 'test_*.py'` for offline GGUF/runtime inventory, native provisioning and qualification boundary regressions; these do not replace actual engine execution.

See the [migration plan's evidence and open gates](../docs/llm-serving-migration-plan.md#14-execution-evidence-and-open-release-checklist). Only observed runs belong in release evidence; mocked usage, browser-only preview, or a defined CI job cannot close public-testnet gates.

## Layout

- Frontend behavioral tests: `tests/*.test.ts`; environment setup: `tests/setup.ts`.
- Rust regressions: source modules under `src-tauri/src/`; discovery integration: `src-tauri/tests/inference_discovery_test.rs`.
- Real paid deployment exercise: `tests/scaled/paid-inference.py`.
- Stock SDK examples: `examples/openai_client.py` and `examples/openai-client.mjs`.
- Optional proof companion: `verified/README.md`; Cargo does not run Verus.
