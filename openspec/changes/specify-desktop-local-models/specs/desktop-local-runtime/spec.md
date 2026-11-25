> **2026-11-25 scope amendment:** Launch uses the checked-in Strata catalog and runtime contract in `strata-curated-runtime`. Broad Hugging Face search, branch/tag resolution, arbitrary GGUF set selection/import, llama-only archives/flags and embedded-tokenizer rules below are deferred legacy scope, not launch requirements. Launch artifacts include all pinned weights/tokenizer/template files. Shared explicit-download, native trust boundary, library, chat, cancellation and no-background-start requirements remain target design; this amendment does not mark the desktop milestone implemented. The author explicitly authorized revising the book; earlier read-only/book-line references record the prior baseline. See `adopt-strata-curated-launch` for supersession and current verification gates.

## ADDED Requirements

### Requirement: Runtimes come only from the pinned, verified llama.cpp release

The desktop SHALL run only the pinned llama.cpp runtime: engine v0.9.0, source revision `cd959bae99ff4bed22bef2604f45241c09bbb8c8`, official binary build b12110. Runtime packages SHALL be limited to a compiled allowlist of official release archives, each bound to its archive name, SHA-256, platform, architecture and backend, equal to the `OFFICIAL_ASSETS` table of `scripts/provision-runtime.py`. A runtime SHALL be obtained either from the same pinned archive bundled with the installer or by an explicit user request in Settings that downloads the official GitHub release asset over HTTPS. That download SHALL apply the client rules `desktop-model-acquisition` specifies for Hugging Face requests: for the initial host and every redirect target, one resolution under a five-second deadline whose answer set is nonempty, holds at most 32 answers and passes `offer_registry::public_ip` in full, with connections only to those pinned addresses; redirects followed hop by hop by the native client, at most 5 times, and only to `https` URLs on port 443 whose host is `github.com` or on a compiled allowlist of GitHub release download hosts; and no environment proxy settings. The desktop SHALL NOT download a runtime without an explicit user request in Settings, including when a load needs one; a missing runtime SHALL be reported with a pointer to the runtime settings. A runtime archive bundled with the installer SHALL be installed from that archive, through the same SHA-256 check and extraction and without any network request, at the first launch at which that runtime identity is neither installed nor previously removed by the user; after a removal it SHALL be reinstalled only on an explicit request in Settings, again from the bundled archive. If a bundled archive fails its check, the runtime SHALL NOT be installed, the failure SHALL be shown in the runtime settings, and no replacement SHALL be downloaded automatically. The archive's SHA-256 SHALL equal its compiled pin before extraction. Extraction SHALL apply the same rules as `scripts/provision-runtime.py`: no traversal or case-colliding names, no special, privileged or encrypted members, links only as in-package native-library aliases materialized as regular files, bounded member count and size, upstream tools omitted, exactly one `llama-server`, and native format and architecture checks for the server and libraries. It SHALL write a canonical `chiral-runtime.json` (schema `chiral-llama-runtime-v1`) byte-identical to the provisioner's for the same archive and target, and SHALL install the runtime at `<app data>/runtimes/<runtime identity>/`, where the identity is the SHA-256 of the canonical `chiral-runtime.json` bytes. There SHALL be no floating runtime updates: any other build, package or backend SHALL be a different identity installed alongside, never a replacement in place.

#### Scenario: Archive digest mismatch

- **GIVEN** a downloaded runtime archive whose SHA-256 differs from its compiled pin
- **WHEN** installation runs
- **THEN** nothing is extracted and the runtime is not installed

#### Scenario: Runtime inventory parity

- **GIVEN** the same official archive, platform, architecture and backend
- **WHEN** the desktop and `scripts/provision-runtime.py` each install it
- **THEN** their `chiral-runtime.json` files are byte-identical

#### Scenario: Unlisted package

- **WHEN** a runtime archive not on the compiled allowlist is offered for installation
- **THEN** the desktop refuses it

#### Scenario: Load without a runtime

- **GIVEN** a verified installation and no installed runtime for the selected backend
- **WHEN** the user chooses Load
- **THEN** the load is refused with a pointer to the runtime settings
- **AND** no runtime download starts

#### Scenario: Bundled runtime is installed without a download

- **GIVEN** an installer that bundles the pinned CPU archive for the host
- **WHEN** the desktop first launches
- **THEN** that runtime is installed after its archive digest matches its compiled pin
- **AND** no network request is made

#### Scenario: Runtime download ignores environment proxies and private redirects

- **GIVEN** `HTTPS_PROXY` names a reachable proxy and the release download redirects to an allowlisted host whose DNS answer is a private address
- **WHEN** the user requests the runtime download in Settings
- **THEN** no connection is made to the proxy or to that address
- **AND** the download fails and nothing is installed

### Requirement: Runtime and artifact inventories are verified before every launch

Before every engine launch the desktop SHALL verify the runtime's `chiral-runtime.json` inventory (exact file set, sizes, SHA-256 digests, pinned engine version and source revision, platform, architecture, backend, and a native executable for this host) and the model installation's complete artifact inventory and ModelSpec. A failed verification SHALL prevent the launch and SHALL be reported with the reason. The desktop SHALL NOT download, inject or point the loader at unverified native libraries, and SHALL NOT pass loader-path or library-injection environment variables to the engine; a missing platform dependency (for example CUDA runtime libraries the official Linux CUDA archive does not include) SHALL be reported as a load failure, not worked around.

#### Scenario: Modified runtime file

- **GIVEN** a file inside an installed runtime directory was changed after installation
- **WHEN** the user loads any model with that runtime
- **THEN** the launch is refused with an inventory mismatch and no engine process starts

#### Scenario: Missing platform library

- **GIVEN** a CUDA runtime whose required system CUDA libraries are absent
- **WHEN** the user loads a model with it
- **THEN** the load fails with an error naming the missing dependency
- **AND** the desktop does not download or inject any library

### Requirement: Backends are offered according to platform support and never as qualified

Settings SHALL list installed runtimes and the backends installable for the host platform and architecture (CPU, Metal, CUDA, Vulkan as the allowlist provides), each with the platform status note from the book's runtime-path table. No backend SHALL be labeled as qualified for paid serving or as having a speed class. Metal SHALL be offered for managed execution only on Apple Silicon. A backend whose device observation the managed runtime does not support on this host SHALL be shown as unavailable with the reason instead of being offered as working.

#### Scenario: Intel Mac

- **GIVEN** an Intel-based Mac
- **WHEN** the user opens the runtime settings
- **THEN** no managed backend is offered as available and the reason is shown

#### Scenario: Status note instead of qualification

- **GIVEN** a Linux host with a Vulkan runtime installed
- **WHEN** the user views it in Settings
- **THEN** it shows the platform status note that Vulkan lacks a qualifying memory measurement
- **AND** no qualification or speed badge is shown

### Requirement: Removing a runtime is explicit and refused while in use

Removing an installed runtime SHALL be an explicit action in the runtime settings that requires a confirmation naming the runtime identity, its backend and the space it frees. Removal SHALL delete only `<app data>/runtimes/<runtime identity>/` and SHALL NOT remove model installations, chat history or other runtimes. It SHALL be refused while any loaded engine uses that runtime, with an instruction to eject the models using it first. Refusal while a share or qualification run uses the runtime is specified by `specify-desktop-share-online`.

#### Scenario: Runtime in use cannot be removed

- **GIVEN** a model loaded with a runtime
- **WHEN** the user chooses to remove that runtime
- **THEN** the removal is refused with an instruction to eject the model first
- **AND** the runtime directory is unchanged

#### Scenario: Removal is scoped to one runtime

- **GIVEN** two installed runtimes and no loaded model
- **WHEN** the user confirms removal of one of them
- **THEN** only that runtime's directory is deleted, and the other runtime and every model installation remain

### Requirement: Loading uses explicit settings within the installed limits

Loading SHALL require an installed runtime and a verified installation, and SHALL use explicit settings shown to the user before launch: runtime and backend; explicit engine device selectors taken from the native enumeration; GPU offload layers (zero with the CPU backend, 1–999 with an accelerated backend); concurrency (slots) of 1–256; memory headroom of at least 256 MiB per dedicated device or at least 4,096 MiB for unified memory; and the managed runtime's execution settings (KV cache types, flash attention, batch and microbatch sizes, split mode and weights) with stated defaults. The per-slot context length SHALL be the installation's `context_length`, shown but not editable at load; a different context requires a separate installation as specified by `desktop-model-acquisition`. Settings SHALL be validated with the same bounds as the managed runtime configuration, including that a quantized value cache requires flash attention and that multiple devices require a split mode with one positive weight per device. The engine SHALL allocate the per-slot context multiplied by concurrency, with automatic fitting and context shifting disabled, and its per-request output limit SHALL be the installation's `max_output_tokens`. The desktop SHALL NOT silently reduce any setting to make a model fit, and loading SHALL NOT modify the installation. While an installation backs an active share or an in-progress qualification run as defined by `specify-desktop-share-online`, the desktop SHALL NOT offer to load it and SHALL refuse a load of it in a desktop engine, so the model is never loaded twice; that change specifies how the model is served in either case.

Downloaded GGUF files, including their embedded chat templates, SHALL be treated as untrusted inputs. Before loading an installation from an unvetted repository, the load dialog SHALL disclose that the engine process is not an operating-system sandbox and runs as the user, so a file exploiting an engine or template-parser bug could read local secrets such as the wallet key, provider signing key, local API bearer and Hugging Face token; the load SHALL proceed only on the user's explicit Load action in that dialog. Until a repository-vetting mechanism is specified, every repository SHALL be treated as unvetted.

#### Scenario: Context is fixed by the installation

- **GIVEN** an installation with `context_length` 8,192
- **WHEN** the user opens the load dialog
- **THEN** the per-slot context is shown as 8,192 and cannot be changed
- **AND** the dialog states that another context needs a separate installation

#### Scenario: Untrusted-model disclosure before loading

- **WHEN** the user loads any installation
- **THEN** the load dialog shows the untrusted-input disclosure naming the local secrets at risk
- **AND** no engine starts until the user chooses Load in that dialog

#### Scenario: Quantized value cache without flash attention

- **WHEN** the user selects a `q4_0` value cache with flash attention off
- **THEN** the load is refused with the reason

#### Scenario: A shared or qualifying installation is not loaded twice

- **GIVEN** an installation that backs an active share or an in-progress qualification run
- **WHEN** the user opens it in My models or the Chat model picker
- **THEN** no Load action is offered for it
- **AND** a load request for it is refused and no desktop engine child starts

### Requirement: Fit is checked before and after loading

Before launch, the desktop SHALL estimate the memory the settings need, using the components defined for fit estimates in `desktop-model-acquisition`, against the memory currently observed free on the selected devices (or system memory for CPU) minus the configured headroom, and SHALL refuse settings whose estimate exceeds that budget, showing the estimate and the budget. For accelerated backends, after the engine reports health the desktop SHALL observe the selected devices again and SHALL stop the engine and report a failed load if any device has less free memory than its headroom, if the observed device identities changed, or if the process exited. While a model is loaded the desktop SHALL show the memory use it observes natively, labeled by memory kind, and SHALL show a value as unavailable rather than estimate it when it cannot be observed. These checks SHALL be described as not reserving memory exclusively; other processes can still change availability.

#### Scenario: Estimate exceeds the budget

- **GIVEN** settings whose estimated memory exceeds the observed free memory minus headroom
- **WHEN** the user chooses Load
- **THEN** the load is refused with the estimate and budget shown and no engine starts

#### Scenario: Headroom violated after loading

- **GIVEN** an accelerated load whose device free memory falls below the headroom once weights and KV buffers are allocated
- **WHEN** the post-load observation runs
- **THEN** the engine is stopped and the model is reported as not loaded

### Requirement: Each loaded model runs in a private loopback engine child

Each loaded model SHALL run in its own managed `llama-server` child process started from the verified runtime; the desktop SHALL NOT embed the engine or use any other engine. The child SHALL bind only literal loopback `127.0.0.1` on a port the desktop selects for that load, and the desktop SHALL refuse to use a port that is already occupied rather than adopt another process. The engine SHALL require an API key that the desktop generates for that load and writes to a protected file passed to the engine by path; the key SHALL NOT appear in command-line arguments, the webview, logs or diagnostics. The child SHALL receive a minimal environment without wallet credentials, proxy settings, dynamic-library injection variables or inherited engine options, and SHALL run with engine output, prompt logging and the engine web UI disabled. The engine URL, port and key SHALL NOT be sent to the webview, advertised, or exposed through any non-loopback listener. Loading another model SHALL start another child that passes its own fit check, subject to the refusals and acknowledgements that `specify-desktop-share-online` adds while a share is active or a qualification run is in progress.

#### Scenario: Engine is not reachable off-host

- **GIVEN** a loaded model
- **WHEN** the engine's listening sockets are inspected
- **THEN** it listens only on `127.0.0.1` and rejects requests without its per-load key

#### Scenario: Occupied port

- **GIVEN** the port selected for a load is occupied by another process
- **WHEN** the engine would start
- **THEN** the load fails and the other process is not used

### Requirement: Eject, failure and quit stop engines without restart or replay

**Eject** SHALL stop that model's engine child, end its active local generations with a cancelled status, delete that load's key file and release the model. Eject of a model that backs an active share SHALL be refused until sharing stops, as specified by `specify-desktop-share-online`. If an engine child exits unexpectedly, the desktop SHALL show the model as failed and not loaded, SHALL end its active generations with an error, and SHALL NOT restart the engine or replay any generation automatically; the user may load it again. Closing the main window SHALL NOT eject models or stop engines. Logging out or loading a different consumer profile SHALL NOT eject models or stop local generations. On every application exit the native runtime observes (a confirmed Quit, after `logout` succeeds and before the main window is destroyed; an operating-system quit command that bypasses the window, such as Cmd+Q on macOS when `desktop-window-lifecycle` does not intercept it; or operating-system logout or shutdown) the desktop SHALL stop every engine child before the process exits: a process-group kill on macOS and Linux and closing the job object on Windows, followed by a bounded wait for each child to exit. Parent-death termination on Linux and the kill-on-close job object on Windows SHALL remain as backstops for an abrupt end of the desktop process. An orphan left by an abrupt termination on macOS SHALL NOT be adopted by a later load. Key files left by an earlier session SHALL be deleted at the next launch.

#### Scenario: Engine crash

- **GIVEN** a loaded model with an active local generation
- **WHEN** its engine process exits unexpectedly
- **THEN** the generation ends with an error, the model is shown as not loaded, and no engine is restarted

#### Scenario: Window close keeps the model loaded

- **GIVEN** a loaded model
- **WHEN** the user closes the main window
- **THEN** the engine keeps running and the model is still loaded when the window is restored

#### Scenario: Quit stops engines

- **GIVEN** two loaded models
- **WHEN** the user confirms Quit
- **THEN** both engine processes are stopped before the desktop process exits

#### Scenario: Operating-system quit stops engines

- **GIVEN** a loaded model on macOS, with no active share and no qualification run
- **WHEN** the user quits with Cmd+Q, bypassing the Quit confirmation
- **THEN** no `llama-server` process started by that session remains after the desktop process exits
