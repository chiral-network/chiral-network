> **2026-11-25 scope amendment:** The llama-server pin, flags, single-native-entry-point and GGUF-specific clauses below govern only the explicit legacy llama.cpp path. Strata launch uses `strata-curated-runtime`; common inventory, lifecycle, occupied-port, credential and no-fallback requirements still apply. Its verified interpreter/native process tree and inventoried Linux library path are intentional Strata-specific differences. The author explicitly authorized revising the book; earlier read-only/book-line references record the prior baseline. See `adopt-strata-curated-launch` for supersession and current verification gates.

## ADDED Requirements

### Requirement: A managed deployment runs only the pinned llama-server as a private Rust-supervised child

A provider model deployment that carries `runtime` SHALL be served by exactly one `llama-server` child process that the provider daemon starts and supervises from Rust (book line 73). The daemon SHALL NOT run inference code through FFI, start the engine through a shell, run a second engine kind, or use as that deployment's engine a process it did not start (book lines 71, 73 and 627). The entry point SHALL be the inventoried native `llama-server` executable (`llama-server.exe` on Windows) of the verified runtime, built for the host's operating system and architecture, of the pinned engine release `v0.9.0` with source revision `cd959bae99ff4bed22bef2604f45241c09bbb8c8` (book line 615).

The daemon SHALL NOT download a model, runtime, library or template, SHALL NOT follow a floating model or runtime revision, and SHALL NOT fall back to another engine, runtime or model, either at launch or at request time (book line 75). The child SHALL be given only the verified local GGUF file and, for a chat model, the verified local template file.

The child is a process-lifecycle and failure boundary, not an operating-system sandbox: it runs under the daemon's operating-system identity and can read whatever that identity can read (book line 75). Operator documentation SHALL NOT describe it as a sandbox. The daemon SHALL keep wallet credentials out of the child as *The child receives a minimal environment and its API key only as a protected file path* requires.

Which deployments must be managed is governed by `inference-commercial-terms` (*Native deployment constraints cannot be bypassed by declaration validity*). This capability governs how a managed engine is verified, launched, observed, supervised and stopped by the current provider daemon.

#### Scenario: A floating upstream identity is refused

- **GIVEN** a runtime whose `chiral-runtime.json` names source revision `master`
- **WHEN** the daemon starts, or `chiral_daemon --check` runs
- **THEN** the managed model is refused with a pinned-identity error
- **AND** no engine child or device observer is started from that runtime

#### Scenario: An entry point other than native llama-server is refused

- **WHEN** the runtime manifest's inventoried executable is named `run.sh`, or is a file named `llama-server` whose header is not a native executable for the host's platform and architecture
- **THEN** the launch is refused and the file is never executed

#### Scenario: Nothing is fetched at launch

- **WHEN** a managed engine is launched
- **THEN** its arguments name the verified local GGUF file and no repository, URL or download cache
- **AND** its environment carries no download, cache or access-token setting

#### Scenario: No fallback after an engine failure

- **GIVEN** a managed engine that has exited
- **WHEN** a request for its model arrives
- **THEN** no other engine, runtime or process is started or contacted to serve it

### Requirement: Runtime and artifact inventories are verified before every launch

Before every launch of a managed engine, the daemon SHALL verify (book lines 613, 615 and 627):

- **the runtime**: `chiral-runtime.json` is a bounded regular file with schema `chiral-llama-runtime-v1` whose platform, architecture and backend equal the host's and the configured backend, and whose engine version and source revision equal the pin. The runtime directory SHALL contain exactly the inventoried files: no file missing, no extra file, no symbolic link, and every inventory path a safe relative path. Each file's size and SHA-256 SHALL equal its inventory entry;
- **the artifact**: `chiral-artifact-sha256.json` (schema `chiral-gguf-artifact-v1`) and `chiral-model.json` bind exactly the files of the artifact directory under the same rules. The artifact's ModelSpec SHALL equal the configured ModelSpec, apart from the serving profile. The GGUF entry point SHALL be inventoried, and the model's API identifier SHALL be `<repository>/<GGUF file name>`. The task and template rules of `inference-commercial-terms` (*Native deployment constraints cannot be bypassed by declaration validity*) SHALL hold, with the selected template's digest computed over the pinned lexer's effective source (book line 613);
- **the serving profile**, when the model carries one: its engine, engine version, runtime and artifact-manifest digests, repositories, platform, backend, memory reserve, every execution setting, concurrency and engine device selectors SHALL equal the verified runtime and the runtime configuration.

The runtime identity SHALL be the SHA-256 of the canonical runtime manifest, so that a differently packaged build has a different runtime identity even at the same upstream commit (book line 615). The artifact identity SHALL be the SHA-256 of the canonical artifact manifest.

Verification SHALL run on each daemon start for every managed model, not only at provisioning. `chiral_daemon --check` SHALL run the same verification and start nothing. Any failure SHALL prevent the launch: no engine child and no device observer is started from that runtime, and the model is neither served nor advertised (book line 627).

#### Scenario: An unlisted library is refused

- **WHEN** a file that the runtime inventory does not list, such as `unlisted.dll`, is placed in the runtime directory
- **THEN** verification fails with an unexpected-file error and no engine child starts

#### Scenario: Altered, missing or resized bytes are refused

- **WHEN** one byte of the inventoried GGUF changes, an inventoried file is removed, or a file's size differs from its inventory entry
- **THEN** verification fails and no engine child starts

#### Scenario: A symbolic link is refused even when it points inside the inventory

- **WHEN** the artifact directory contains `alias.gguf` as a symbolic link to the inventoried GGUF
- **THEN** verification fails and no engine child starts

#### Scenario: The configuration must equal the installed artifact

- **WHEN** the configured ModelSpec's context length is one more than the artifact's `chiral-model.json`
- **THEN** verification fails with an artifact-identity error

#### Scenario: A profile must equal the launch settings

- **GIVEN** a profiled model whose serving profile states `batch_size` 512
- **WHEN** its runtime configuration states `batch_size` 256
- **THEN** verification fails because the managed launch settings differ from the immutable serving terms

#### Scenario: A repackaged runtime has another identity

- **GIVEN** two runtime packages of the same upstream commit whose inventories differ in one library digest
- **THEN** their runtime identities differ
- **AND** a serving profile that signs one of them is refused when the other is installed

### Requirement: Managed runtime settings are explicit, bounded and refused rather than adjusted

A managed runtime configuration SHALL contain exactly `runtime_dir`, `artifact_dir`, `backend`, `device_ids`, `memory_reserve_mib`, `gpu_layers`, `split_mode`, `tensor_split`, `cache_type_k`, `cache_type_v`, `flash_attention`, `batch_size`, `ubatch_size` and `startup_timeout_secs`. An unknown field SHALL be rejected, and no field SHALL carry an arbitrary engine option (book line 617). Configuration loading SHALL refuse a runtime configuration unless (book lines 90, 92 and 617):

- the backend is `cuda`, `vulkan` or `cpu`, or `metal` on macOS;
- engine device selectors are explicit, unique and of the backend's form `CUDA<n>`, `Vulkan<n>` or `MTL<n>`;
- a `cpu` backend selects no device, offloads zero layers, has split mode `none` and no split weights;
- an accelerated backend selects at least one device and offloads between 1 and 999 layers;
- one device uses split mode `none` with no weights, and several devices use `layer` or `row` with exactly one positive weight per device;
- each KV cache type is one of `f32`, `f16`, `bf16`, `q8_0`, `q4_0`, `q4_1`, `iq4_nl`, `q5_0` and `q5_1`, and a quantized value-cache type has flash attention enabled;
- the batch size is between 1 and 8,192 and the microbatch size between 1 and the batch size;
- the headroom reserve is at least 4,096 MiB for Metal unified memory, which SHALL be exactly one selected device, and at least 256 MiB per selected dedicated CUDA or Vulkan device (book line 90; `CLAUDE.md`: "reserve at least 4 GiB on unified-memory providers");
- the total context, the model's context length multiplied by its maximum concurrency, fits the engine's context bound;
- the startup timeout is within its bound.

Invalid settings SHALL be refused, never replaced by other values (book line 92). The same bounds apply to the serving profile under `inference-commercial-terms` (*Declared capacity and execution settings are internally consistent*); this requirement applies them to the runtime configuration whether or not the model carries a profile. The device-count, timeout and context bounds that the book does not state are current implementation values recorded in `design.md`.

#### Scenario: Unified memory needs one device and 4,096 MiB of headroom

- **WHEN** on macOS a Metal runtime configuration reserves 4,095 MiB, or selects `MTL0` and `MTL1`
- **THEN** configuration loading refuses it
- **AND** one device with a 4,096 MiB reserve is accepted

#### Scenario: Dedicated devices need 256 MiB of headroom each

- **WHEN** a CUDA runtime configuration reserves 255 MiB
- **THEN** configuration loading refuses it
- **AND** a 256 MiB reserve is accepted

#### Scenario: A CPU runtime cannot select GPU execution

- **WHEN** a `cpu` runtime configuration selects `CUDA0`, offloads one layer, or uses split mode `layer`
- **THEN** configuration loading refuses it

#### Scenario: An incompatible cache is refused, not replaced

- **WHEN** a runtime configuration selects value cache `q4_0` with flash attention off
- **THEN** configuration loading refuses it, and does not substitute `f16` or enable flash attention

#### Scenario: Split weights must describe the selected devices

- **WHEN** a CUDA runtime configuration selects `CUDA0` and `CUDA1` with split mode `layer` and no weights, or with three weights
- **THEN** configuration loading refuses it

#### Scenario: No field adds an engine option

- **WHEN** a runtime configuration contains a field `extra_args`
- **THEN** the provider configuration fails to load

### Requirement: The engine listens only on its literal-loopback endpoint, and an occupied port is refused

The `engine_url` of a managed model SHALL be `http://` with a literal loopback IP address and an explicit nonzero port, and without credentials, path, query or fragment. `localhost`, a private non-loopback address, `https://` and an omitted port SHALL be refused (book line 656; `CLAUDE.md`: "llama.cpp runs privately"). The launch SHALL pass exactly that host and port to the engine.

Before spawning the engine, the launcher SHALL check that the endpoint is free. A listener of another process on that port, whether bound to the same address or to the wildcard address of the same address family, SHALL make the launch fail with an occupied-port error. Connections on that port that only wait to expire (TCP `TIME_WAIT`), such as those of an engine that an earlier daemon run stopped, are not a listener and SHALL NOT make the launch fail. The check SHALL NOT start listening on a non-loopback address. The launcher SHALL NOT adopt that process, use it as its engine, signal or terminate it, or retry on another port (book lines 627 and 642).

An answer on the engine endpoint SHALL count as the engine's only while the launcher's own child is alive. A health answer received after that child exited SHALL NOT make the engine ready.

#### Scenario: A listener on the same address is not adopted

- **GIVEN** another process listening on `127.0.0.1:18080`
- **WHEN** a managed engine with `engine_url` `http://127.0.0.1:18080` is launched
- **THEN** the launch fails with an occupied-port error and no engine child is spawned
- **AND** the other process is not signalled and still accepts connections

#### Scenario: A wildcard listener on the same port is refused

- **GIVEN** another process listening on `0.0.0.0:18080`
- **WHEN** a managed engine with `engine_url` `http://127.0.0.1:18080` is launched
- **THEN** the launch fails with an occupied-port error and no engine child is spawned

#### Scenario: Expiring connections do not occupy the port

- **GIVEN** no process listening on port 18080, and a connection on `127.0.0.1:18080` that the listening side closed first, now in TCP `TIME_WAIT`
- **WHEN** a managed engine with `engine_url` `http://127.0.0.1:18080` is launched
- **THEN** the port check passes and the launch continues

#### Scenario: A non-literal or non-loopback endpoint is refused

- **WHEN** a managed model's `engine_url` is `http://localhost:18080`, `http://192.168.1.2:18080`, `https://127.0.0.1:18080`, `http://127.0.0.1` or `http://127.0.0.1:0`
- **THEN** configuration loading refuses it
- **AND** `http://[::1]:18080` is accepted

#### Scenario: Another process's answer is not readiness

- **GIVEN** a launched engine child that exits during startup while another process answers `/health` on the engine endpoint
- **THEN** the launch fails and the engine is never ready

### Requirement: The launch passes the serving contract to the engine as explicit arguments

The launch SHALL pass each contract term to the engine as an explicit argument, taking every value from the verified configuration and inventory and none from the environment or an engine default (book lines 73, 92, 601, 613, 617 and 619). For the pinned engine `v0.9.0` (book line 615) the arguments SHALL be:

| Contract term | Value | Engine option |
|---|---|---|
| Weights | Canonical path of the verified GGUF entry point | `--model` |
| Served model name | The model's API identifier | `--alias` |
| Endpoint | Host of `engine_url` without brackets, and its port | `--host`, `--port` |
| API key | Canonical path of the protected engine key file | `--api-key-file` |
| Total context | Context length × maximum concurrency | `--ctx-size` |
| Per-slot context | Context length | `--kv-unified-per-slot` |
| Slots | Maximum concurrency | `--parallel` |
| Output allowance | The model's `max_output_tokens` | `--n-predict` |
| Automatic fitting | Disabled | `--fit off` |
| Context shifting | Disabled | `--no-context-shift` |
| Devices | Configured selectors joined by commas in configured order, or `none` for CPU | `--device` |
| Offload | `gpu_layers` | `--gpu-layers` |
| Split | `split_mode`; the weights joined by commas only when several devices are selected | `--split-mode`, `--tensor-split` |
| KV cache | `cache_type_k`, `cache_type_v` | `--cache-type-k`, `--cache-type-v` |
| Flash attention | `on` or `off` | `--flash-attn` |
| Batching | `batch_size`, `ubatch_size` | `--batch-size`, `--ubatch-size` |
| Engine logging | Disabled | `--log-disable` |
| Embedding task | Present only for an embeddings artifact | `--embedding` |
| Chat template | The verified template file, only when the artifact selects one | `--jinja --chat-template-file` |

The launch MAY add options that only disable engine features outside the contract; the current ones are listed in `design.md`. It SHALL NOT pass an option that names a remote model source, enables automatic fitting or context shifting, opens another listener, or carries a secret value.

#### Scenario: Total context is per-slot context times concurrency

- **GIVEN** a managed model with context length 512 and maximum concurrency 2
- **WHEN** its launch command is built
- **THEN** it contains `--ctx-size 1024`, `--kv-unified-per-slot 512` and `--parallel 2`, together with `--fit off` and `--no-context-shift`

#### Scenario: Several CUDA devices are passed explicitly

- **GIVEN** a CUDA runtime configuration selecting `CUDA0` and `CUDA1` with split mode `layer`, weights `[3, 1]`, 999 layers and flash attention on
- **WHEN** its launch command is built
- **THEN** it contains `--device CUDA0,CUDA1`, `--split-mode layer`, `--tensor-split 3,1`, `--gpu-layers 999` and `--flash-attn on`

#### Scenario: A CPU launch selects no device

- **WHEN** the launch command of a `cpu` runtime is built
- **THEN** it contains `--device none`, `--gpu-layers 0` and `--split-mode none`, and no `--tensor-split`

#### Scenario: The key is passed by path only

- **WHEN** a launch command is built
- **THEN** `--api-key-file` is followed by the canonical path of the engine key file
- **AND** no argument contains the key's bytes

#### Scenario: The task and template follow the artifact

- **WHEN** the launch command of an embeddings artifact is built
- **THEN** it contains `--embedding` and neither `--jinja` nor `--chat-template-file`
- **AND** the launch command of a chat artifact contains `--jinja --chat-template-file` followed by the verified template's canonical path, and no `--embedding`

### Requirement: The child receives a minimal environment and its API key only as a protected file path

The launcher SHALL start every engine child from an empty environment (book line 619; `CLAUDE.md`: "inherited environment must not override signed execution settings"). The environment that the launcher passes to the child SHALL contain only:

- `CUDA_DEVICE_ORDER=PCI_BUS_ID`, set by the launcher so that CUDA selectors follow the PCI bus order that device observation uses; and
- `SystemRoot`, `WINDIR`, `TEMP`, `TMP` and `TMPDIR`, each copied from the daemon's environment only when present there, because the operating-system loader and temporary-file handling need them.

No other variable SHALL be passed. In particular, `PATH`, `HOME`, proxy settings (`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`, in either letter case), dynamic-library loading and injection variables (`LD_PRELOAD`, `LD_LIBRARY_PATH`, `DYLD_INSERT_LIBRARIES`, `DYLD_LIBRARY_PATH`), engine options (`LLAMA_ARG_*` and other `LLAMA_*` or `GGML_*` variables), device-visibility masks (`CUDA_VISIBLE_DEVICES` and its equivalents), download and access-token settings, and wallet or provider configuration SHALL NOT reach the child (book line 619). The same environment rule SHALL apply to every process the launcher starts to observe devices.

The child's standard input SHALL be empty and its standard output and standard error SHALL be discarded (book line 619). Its working directory SHALL be the directory of the verified executable. Native driver and runtime dependencies SHALL come from the platform loader or the verified runtime package (book line 619). The launcher SHALL NOT add a loader search path or a library to make a missing dependency load; a missing dependency SHALL fail the launch.

A managed engine SHALL require `engine_api_key_file`. The key file SHALL be a protected regular file of 1 to 4,096 bytes, opened without following a symbolic link: on Unix owned by the daemon's effective user with no group or other permission bits, on native Windows in owner-protected storage (book line 474). It SHALL hold exactly one literal token, at least 32 characters long, not beginning with `#`, optionally followed by one newline and nothing else, because the engine reads the file literally. The key SHALL be passed to the engine only as that file's path, and SHALL NOT appear in any argument, environment variable, log line or diagnostic (book lines 474 and 619). The daemon SHALL present the same key as the bearer of its private requests to the engine.

Every engine key file that the provider configuration names, for a managed or an external engine, SHALL be a separate credential: its token SHALL differ from the provider signing key in any spelling (with or without `0x`, in any letter case, and with leading zeros added or removed), from the administration token and from the RPC bearer (book line 474; `CLAUDE.md`: llama.cpp runs "with a protected API-key file and no wallet key"). The daemon SHALL refuse such a configuration before starting any engine, without printing either value.

#### Scenario: The daemon's environment does not reach the engine

- **GIVEN** a daemon whose environment consists of `LLAMA_ARG_CTX_SIZE=1`, `LLAMA_ARG_N_GPU_LAYERS=0`, `CUDA_VISIBLE_DEVICES=1`, `HTTP_PROXY`, `HTTPS_PROXY`, `LD_PRELOAD`, `DYLD_INSERT_LIBRARIES`, `HF_TOKEN`, `PATH`, `HOME` and `TMPDIR`
- **WHEN** the launcher starts a child on Unix
- **THEN** the environment passed to the child is exactly `CUDA_DEVICE_ORDER=PCI_BUS_ID` and the daemon's `TMPDIR`

#### Scenario: A key the engine would misread is refused

- **WHEN** the engine key file holds a token beginning with `#`, a token preceded by a space, a 31-character token, or two lines
- **THEN** the launch is refused and no engine child starts

#### Scenario: An exposed or linked key file is refused

- **WHEN** on Unix the engine key file has mode `0644`, or is a symbolic link to an otherwise valid key file
- **THEN** the launch is refused and no engine child starts

#### Scenario: The wallet key cannot be the engine key

- **GIVEN** a provider configuration whose `engine_api_key_file` names the signing key file, or another protected file holding the same key with a `0x` prefix in upper case
- **WHEN** the daemon starts or `chiral_daemon --check` runs
- **THEN** it refuses the configuration before any engine starts, without printing the key

#### Scenario: Operator credentials cannot be the engine key

- **WHEN** the engine key file holds the administration token or the RPC bearer
- **THEN** the daemon refuses the configuration before any engine starts
- **AND** a distinct protected token of at least 32 characters is accepted

### Requirement: Physical devices are observed independently, and headroom is enforced before startup and after loading

After inventory verification and before spawning the engine, the launcher SHALL observe each selected device independently of the configuration (book line 617). It SHALL list devices with the verified engine's own enumeration (`--list-devices`), which gives each selector's name, memory and free memory, and SHALL read physical identity and driver from platform tools at fixed system locations: `nvidia-smi` for CUDA, which also gives physical and free memory; `sysctl`, `ioreg`, `vm_stat` and `sw_vers` on macOS, which also give physical and available memory; and `vulkaninfo` for Vulkan. `vulkaninfo --summary` reports no memory, so a Vulkan device's memory rests on the engine's enumeration alone (book line 99: Vulkan lacks a trustworthy per-device memory measurement). It SHALL NOT take a device name, identifier, memory size or driver from the configuration. Observation output SHALL be bounded in size and time. A missing tool, a failed or timed-out observation, or unparsable or ambiguous output SHALL fail the launch.

Memory kind SHALL be observed per backend (book line 90):

- **CUDA**: `dedicated`. Each selector SHALL be matched to the physical device at the same position in PCI bus order, and the engine's and the platform's device names SHALL agree;
- **Metal**: only on Apple silicon, `unified`, as one device whose memory is the machine's physical memory, counted once;
- **Vulkan**: only a discrete GPU, matched by a unique device name, as `dedicated`. An integrated or otherwise shared-memory Vulkan device SHALL be refused;
- **CPU**: no device is observed, and the memory kind is `system`.

Before startup, each selected dedicated device SHALL have free memory of at least the reserve and physical memory above the reserve. Unified memory SHALL have available memory (free, inactive and speculative pages, capped at physical memory) of at least the reserve (book line 90). Headroom is subtracted from the fit budget, not from the capacity label. These checks are not an exclusive operating-system reservation (book lines 90 and 617).

When the model carries a serving profile, the observed memory kind and devices (physical identifier, name, physical memory, driver version and engine selector, in order) SHALL equal the profile's, so that the signed capacity tier rests on observed memory (tier rules: `inference-commercial-terms`, *Declared capacity and execution settings are internally consistent*). Any difference, including a driver update, SHALL fail the launch (book line 640).

After the engine reports health, the launcher SHALL observe the devices again. The launch SHALL fail, and the child SHALL be stopped, if the observed memory kind or devices changed, if a dedicated device's free memory or the unified available memory is below the reserve, or if the child has exited (book line 617: fit and headroom are "checked after actual loading as well as before startup").

The launcher does not monitor competing workloads after the engine is ready. Sustained memory behavior is measured by qualification under `inference-generation-performance` (*Qualification measures memory, records distributions and fails closed without trustworthy counters*; book lines 568 and 617).

#### Scenario: Insufficient headroom before startup

- **GIVEN** a CUDA runtime reserving 256 MiB whose selected device `CUDA0` reports 200 MiB free
- **WHEN** the managed engine is launched
- **THEN** the launch fails before the engine is spawned

#### Scenario: A changed driver is a different deployment

- **GIVEN** a serving profile that signs `CUDA0` with driver `550.54.14`
- **WHEN** `nvidia-smi` reports driver `555.42.02` for that device
- **THEN** the launch fails because the observed devices differ from the qualified inventory

#### Scenario: Headroom violated after loading

- **GIVEN** a CUDA runtime reserving 256 MiB whose device has 4,000 MiB free before startup
- **WHEN** the engine reports health and the device then has 100 MiB free
- **THEN** the engine child is stopped, the launch fails, and the model is neither served nor advertised

#### Scenario: Shared-memory devices are not signed as dedicated

- **WHEN** a Vulkan runtime selects an integrated GPU, or a Metal runtime runs on a Mac without Apple silicon
- **THEN** the launch fails

### Requirement: The contract is never reduced to make a model fit

The launch SHALL NOT reduce the context, concurrency, output allowance, offload, batch sizes, cache types or devices to make a model fit, and SHALL NOT let the engine reduce them: automatic fitting and context shifting SHALL be disabled (book line 92). When the model does not load with exactly the configured settings, the launch SHALL fail under *Startup is bounded, and a failed launch serves and advertises nothing*. Other settings form other terms (book line 640; `inference-commercial-terms`, *Existing agreements permit only evidence-only renewal of identical terms*).

Before a model on a managed engine is ready, the daemon SHALL confirm from the engine itself that it runs the pinned source revision, serves the model under its API identifier, provides exactly the maximum-concurrency number of slots, and gives each slot exactly the model's context length. Any difference SHALL keep the model not ready (book lines 92 and 628). Readiness also requires the model, template, tokenization and capability probes (book line 628), which this capability does not specify. A loaded and ready model is not a qualified speed class (book line 92; `inference-generation-performance`, *A speed class rests only on qualification of the exact profile*).

The engine's slot count equals the model's admission concurrency, so the engine holds no queue beyond admission (`inference-generation-performance`, *Overload is refused before paid dispatch, not queued*).

#### Scenario: A smaller per-slot context is not ready

- **GIVEN** a managed model with context length 512
- **WHEN** the engine reports a per-slot context of 256
- **THEN** the model is not ready and no offer is published for it

#### Scenario: A different slot count is not ready

- **GIVEN** a managed model with maximum concurrency 2
- **WHEN** the engine reports one slot
- **THEN** the model is not ready and no offer is published for it

#### Scenario: A model that does not fit fails instead of shrinking

- **GIVEN** settings whose weights and KV cache for concurrency 4 exceed the device's memory
- **WHEN** the engine is launched
- **THEN** the launch fails
- **AND** neither the context nor the concurrency is lowered, and no offer is published

### Requirement: Startup is bounded, and a failed launch serves and advertises nothing

After spawning the engine, the launcher SHALL poll the engine's private health endpoint with the engine key as bearer, without a proxy and without following redirects, until it answers success or `startup_timeout_secs` elapse (book line 627). If the child exits first or the timeout elapses, the launcher SHALL stop the child and fail the launch. It SHALL NOT retry, relaunch or lower any setting. A health answer alone SHALL NOT make a model ready (book line 628); it only allows the post-load observation of *Physical devices are observed independently, and headroom is enforced before startup and after loading* to run.

A failed launch of any managed model SHALL fail the provider daemon's start: no public or administration request is served, no offer is published, and engine children already started for other models of the deployment are stopped (book line 627).

#### Scenario: A startup timeout stops the child

- **GIVEN** an engine endpoint that never answers health
- **WHEN** the startup timeout elapses
- **THEN** the child is stopped and reaped, the launch fails, and nothing is retried

#### Scenario: An engine that exits before readiness is not relaunched

- **WHEN** the engine child exits before its health endpoint answers success
- **THEN** the launch fails with an exited-before-readiness error, and no second child is started

#### Scenario: One failed model stops the whole start

- **GIVEN** a deployment with two managed models whose first engine has started
- **WHEN** the second model's launch fails
- **THEN** the daemon exits with an error, the first engine child is stopped, and no request is served and no offer published

### Requirement: An engine exit withdraws service, and nothing restarts the engine

The daemon SHALL observe the exit of a managed engine child without waiting for the next maintenance refresh (book line 632). From then on, that engine SHALL report not alive permanently. The daemon SHALL close admission and new-funding readiness and withdraw its published offers as `inference-offer-discovery` requires (*The provider publishes fresh offers only while it can accept new funding*, and *Provider liveness and readiness are point-in-time observations, separate from new-funding readiness*). It SHALL keep administration and recovery available while the daemon stays alive (book line 632).

The daemon SHALL NOT restart the engine, start a replacement, route the model to another engine or replay any generation (book lines 75 and 632). No maintenance refresh, `resume`, `offers/refresh` or other administration action SHALL restart it or make its model admissible again. Only an operator restart of the daemon, after reconciling requests, launches the engine again (book line 632), and that launch follows every requirement of this capability, including the occupied-port refusal (book line 642). An exit SHALL NOT erase accepted agreements, credit or ledger records.

#### Scenario: An engine exit withdraws offers and nothing restarts it

- **GIVEN** a ready provider with one managed engine and a published offer
- **WHEN** the engine process is killed
- **THEN** `GET /readyz` answers HTTP 503, the offer is withdrawn, and `/admin/runtime` reports the model as not ready
- **AND** no new engine process is started, including after the next maintenance refresh and after `POST /admin/resume`

#### Scenario: Administration stays available after an engine exit

- **GIVEN** a provider whose managed engine has exited
- **WHEN** the operator calls `GET /admin/status`, `GET /admin/accounts` or `GET /admin/fees` with the administration bearer
- **THEN** each answers, and accepted accounts and fee obligations are unchanged

### Requirement: Shutdown and parent termination stop the engines the daemon owns

On normal shutdown the daemon SHALL drain, let in-flight requests settle within its shutdown bound, then stop every managed engine child and wait, within a bound, for each to exit (book line 634). Stopping SHALL kill the child's whole process group on Unix and the child process on Windows. Dropping the daemon's handle to an engine SHALL also kill the child. A termination request (`SIGTERM` or `SIGINT` on Unix, Ctrl-C on Windows) that arrives while the daemon is still starting, including while an engine loads, SHALL also stop every engine child already started, and the daemon SHALL then exit without serving (book lines 634 and 642: only an abrupt `SIGKILL` may leave a macOS orphan).

For an abrupt end of the daemon (book line 642):

- on Linux, the child SHALL be started with a parent-death kill signal, and SHALL refuse to start if its parent has already changed;
- on Windows, the child SHALL be assigned to a kill-on-close job object, and a failure to assign it SHALL fail the launch;
- on macOS, normal shutdown and drop kill the child's process group; an abrupt `SIGKILL` of the daemon can leave an orphan that the operator must remove.

A later launch SHALL NOT adopt an orphaned engine: its port makes the launch fail under *The engine listens only on its literal-loopback endpoint, and an occupied port is refused* (book line 642).

#### Scenario: Normal shutdown stops the engine

- **GIVEN** a running daemon with one managed engine
- **WHEN** the daemon receives `SIGTERM`
- **THEN** the engine process has exited before the daemon process exits

#### Scenario: A termination request during startup leaves no engine behind

- **GIVEN** a daemon whose managed engine child has been spawned and has not yet answered health
- **WHEN** the daemon receives `SIGTERM` (Ctrl-C on Windows)
- **THEN** the daemon exits without serving, and the engine process no longer exists within five seconds, on macOS as on Linux and Windows

#### Scenario: Dropping the handle does not strand the engine

- **WHEN** the daemon drops its handle to a running engine
- **THEN** the engine process no longer exists within five seconds

#### Scenario: A killed parent takes its engine with it on Linux and Windows

- **WHEN** on Linux or Windows the daemon process is killed abruptly
- **THEN** its engine child is terminated by the operating system

#### Scenario: A macOS orphan blocks the next start instead of being adopted

- **GIVEN** an engine left listening on `127.0.0.1:8000` after the daemon was killed with `SIGKILL` on macOS
- **WHEN** the daemon is started again with the same configuration
- **THEN** the launch fails with an occupied-port error, and the orphan is neither used nor signalled
