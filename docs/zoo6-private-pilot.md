# Private zoo-6 inference pilot

Deployment-specific operator reference, separated from the [system design](chiral-book.md). This records the previously observed managed-native pilot, not a fresh health check, public deployment, or performance qualification. The [dated execution evidence](llm-serving-migration-plan.md#14-execution-evidence-and-open-release-checklist) distinguishes prior vLLM work from the current llama.cpp cutover.

## Authorization and preserved infrastructure

The user authorized retiring zoo-6's pilot miner and keeping its RTX 4060 dedicated to private LLM testing. The existing qualification VM is **125** on `zoo06`; its chain-related data and unrelated resource monitor were left intact. The VM has 4 GB system RAM, an independent constraint on model loading. No hypervisor/driver upgrade or public deployment was performed. Do not restart the retired miner automatically or alter unrelated chain services.

## Recorded deployment

- Guest service: **`chiral-llm-8gb.service`**, running as `debian`, enabled for guest boot. The host's VM-autostart setting was not changed.
- Protected deployment: `/home/debian/chiral-llama-20261104/`. The previous vLLM service is retired; its artifacts/history are retained.
- Engine: native `chiral_daemon` managing pinned llama.cpp **v0.9.0 / b12110**, source `cd959bae99ff4bed22bef2604f45241c09bbb8c8`.
- Model: `Qwen/Qwen2.5-0.5B-Instruct-GGUF`, revision `9217f5db79a29953eb74d5343926648285ec7e67`, file `qwen2.5-0.5b-instruct-q4_k_m.gguf`.
- Execution: 4,096-token context per slot, two slots, full GPU offload and f16 KV caches.
- Gateway/admin/engine: **loopback only**, respectively `127.0.0.1:19429`, `127.0.0.1:19431`, `127.0.0.1:8000`.
- NVIDIA driver: **550.163.01**; observed physical GPU memory: **8,188 MiB**.
- Runtime inventory SHA-256: `3d3c32fb571c7f8be4c9e88128dcb25d74ea5c0983d4662e652d2a22f5fa6636`.
- Locally assembled CUDA package SHA-256: `4e9f9d880fccc42c854dc4b5665711502583831f2d6d01459c54287af3ddcca2`.

The official Linux CUDA archive does not include all CUDA runtime/cuBLAS dependencies. This private package combines the official archive with verified NVIDIA `nvidia-cuda-runtime-cu12` **12.8.90** and `nvidia-cublas-cu12` **12.8.4.1** native libraries, preserving licenses and source/digest provenance. No Python wheel code runs in the provider. The managed child does not inherit `LD_LIBRARY_PATH`; a repeat installation must satisfy native loading independently. See the [qualification guide](provider-qualification.md).

## Operator access

On the configured development workstation, `chiral-zoo6-llm` is an SSH alias through `zoo06`, with the previously pinned guest host key. These aliases are local operator configuration, not public endpoints:

```bash
ssh chiral-zoo6-llm 'sudo systemctl status chiral-llm-8gb.service --no-pager'
# Optional operator-only tunnel; not public inference ingress:
ssh -N -L 127.0.0.1:18000:127.0.0.1:8000 chiral-zoo6-llm
```

The tunnel does not remove engine bearer authentication. Do not expose the engine or admin listener, print credentials, or start a competing server on its port. Drain and preserve accounting before planned maintenance; restart does not clear performance withdrawal or authorize replay of an ambiguous paid request.

## Money state and qualification limits

The guest uses a **new, distinct** provider wallet, `0x35df56bdd7f4b7763cc7f44df666d0b5bbfcf6d1`, with a protected key and shared provider/nonce-journal base directory. No existing signing wallet was copied into a separate journal. Agreements for the GGUF terms are separate from historical credit; changing engines does not migrate balances.

All pilot models remain `serving:null`. Real small-model paid smoke runs demonstrate a limited working path, not a sustained 20/50/100-token/sec class, a production `qualified_model_file`, compatibility with larger models, or public-release approval. Preserve ledgers, exact signed funding evidence, DHT identity, nonce journals, keys and request histories. Do not convert historical CHI/CHR or erase old obligations.
