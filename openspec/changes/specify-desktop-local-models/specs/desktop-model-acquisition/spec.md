> **2026-11-25 scope amendment:** Launch uses the checked-in Strata catalog and runtime contract in `strata-curated-runtime`. Broad Hugging Face search, branch/tag resolution, arbitrary GGUF set selection/import, llama-only archives/flags and embedded-tokenizer rules below are deferred legacy scope, not launch requirements. Launch artifacts include all pinned weights/tokenizer/template files. Shared explicit-download, native trust boundary, library, chat, cancellation and no-background-start requirements remain target design; this amendment does not mark the desktop milestone implemented. The author explicitly authorized revising the book; earlier read-only/book-line references record the prior baseline. See `adopt-strata-curated-launch` for supersession and current verification gates.

## ADDED Requirements

### Requirement: Hugging Face discovery runs natively and renders as untrusted text

The desktop SHALL obtain Hugging Face search results, repository metadata, file listings and model-card text only through native commands that call the public Hugging Face API at the compiled origin `https://huggingface.co`. The webview SHALL NOT fetch any Hugging Face URL itself. Search SHALL return only repositories that contain GGUF files. Native Hugging Face requests SHALL NOT carry a wallet address, network profile, prompt or chat content. Repository names, card text, tags, license names and file names SHALL be rendered as escaped plain text, never as HTML or Markdown converted to HTML, and SHALL NOT cause the webview to load any remote resource. A failed search or metadata request (no connectivity, rate limiting, HTTP error) SHALL be reported as an error without fabricated or substituted results and SHALL NOT affect installed models, loaded models or chat. The Discover page SHALL disclose that search terms and downloads contact Hugging Face and that no prompt or chat content is sent there.

#### Scenario: Model-card markup stays text

- **GIVEN** a repository whose card contains HTML, a `<script>` element and a Markdown image
- **WHEN** the user opens its details
- **THEN** the card summary is shown literally as text
- **AND** no request is issued for the image URL and no script runs

#### Scenario: Search without connectivity

- **GIVEN** the machine has no network connectivity and one model is installed and loaded
- **WHEN** the user searches Discover
- **THEN** an error explains that Hugging Face could not be reached and no results are shown
- **AND** the loaded model keeps serving local chat

#### Scenario: Requests originate natively

- **WHEN** the user searches and opens repository details
- **THEN** every Hugging Face request is made by the native process
- **AND** the webview capability grants no HTTP permission

### Requirement: Selectable GGUF sets are complete and fully described

Repository details SHALL group the repository's GGUF files into selectable sets. A set SHALL be either exactly one unsharded `.gguf` file or a complete shard series named `<prefix>-NNNNN-of-MMMMM.gguf` that contains every index from 1 to MMMMM. Incomplete series, mixed series and combinations of several unsharded files SHALL NOT be selectable as one model, and files other than the selected GGUF files SHALL NOT be downloaded. For each set the desktop SHALL show the total size, the quantization declared by the repository (labeled as declared until verified from the GGUF header), the native context length when the repository metadata supplies it, and a fit estimate. Repository details SHALL show the declared license and whether the repository is gated. A repository whose identifier is not of the `organization/name` form accepted by `scripts/provision-model.py` SHALL be shown as not installable.

#### Scenario: Incomplete shard series

- **GIVEN** a repository lists `m-00001-of-00003.gguf` and `m-00003-of-00003.gguf` but not `m-00002-of-00003.gguf`
- **WHEN** the user opens its details
- **THEN** that series is not offered as a selectable set

#### Scenario: Complete set is described

- **GIVEN** a repository with a complete three-shard Q4_K_M series and one unsharded Q8_0 file
- **WHEN** the user opens its details
- **THEN** two sets are offered, each with its total size, declared quantization, declared context length and fit estimate

### Requirement: Fit estimates are labeled estimates against detected memory

The desktop SHALL compute fit estimates natively from memory it detects on the host; the webview SHALL NOT supply memory values. Each estimate SHALL be for the per-slot context length and intended concurrency of the prospective installation. The memory budget SHALL be, for dedicated GPU memory, each selected device's physical memory minus at least 256 MiB; for Apple unified memory, the physical memory counted once minus at least 4,096 MiB; and for CPU-only execution, physical system memory minus 4,096 MiB. The estimated requirement SHALL include the selected files' weight bytes, the KV cache for the per-slot context length chosen for installation multiplied by the intended concurrency at the stated cache types, and a stated runtime allowance; when a component cannot be computed before download, the estimate SHALL name the component it omits. Each estimate SHALL display its assumptions and memory kind and SHALL be labeled as an estimate that is not a capacity tier, a qualification, a speed claim or an exclusive memory reservation. When memory cannot be detected, the estimate SHALL read "unknown" rather than assume a value. Actual fit SHALL be checked after loading as specified by `desktop-local-runtime`.

#### Scenario: Unified memory headroom

- **GIVEN** an Apple Silicon Mac reporting 16,384 MiB of unified memory
- **WHEN** a fit estimate is shown
- **THEN** its budget is at most 12,288 MiB, counted once, and it is labeled as an estimate

#### Scenario: Undetectable memory

- **GIVEN** the native process cannot observe device or system memory
- **WHEN** a fit estimate is requested
- **THEN** it reads "unknown" and no fit verdict is shown

### Requirement: Downloads pin an immutable commit

When a download starts, the desktop SHALL resolve the selected branch or tag to the repository's full 40-hexadecimal-character commit through the Hugging Face API, SHALL fetch the repository metadata for that commit, and SHALL request every selected file by that commit rather than by a branch or tag name. If resolution does not yield a full commit, or the metadata fetched for that commit reports a different commit, the download SHALL NOT start. The installed model SHALL be that immutable revision, recorded as the manifest's `model.revision` and as the ModelSpec's `revision` and `tokenizer_revision`. The desktop SHALL NOT replace, rewrite or update an installed model in place; downloading a newer revision SHALL create a separate installation with its own identity alongside the existing one.

#### Scenario: Branch moves during a download

- **GIVEN** a download was resolved to commit C on branch `main`
- **WHEN** `main` advances to commit D before the transfer finishes
- **THEN** every file is still fetched from commit C and the installation records revision C

#### Scenario: Update installs alongside

- **GIVEN** revision C of a repository's set is installed
- **WHEN** the user downloads the same set at a newer revision D
- **THEN** revision D is installed as a separate library entry
- **AND** revision C's files and identity are unchanged

### Requirement: License acknowledgement is explicit and recorded

Before a download starts, the desktop SHALL show the license identifier declared by the repository metadata at the pinned commit, with any declared license name or link as plain text, and SHALL require the user to actively acknowledge it for that download. The acknowledgement control SHALL NOT be pre-selected. The acknowledged identifier SHALL be recorded as `model.license` in the installed artifact manifest exactly as `scripts/provision-model.py --accept-license` records it. A repository whose metadata at the pinned commit declares no license string of 1–128 characters SHALL NOT be installed. If the license declared at the pinned commit differs from the one the user acknowledged, the download SHALL stop and require a new acknowledgement. The desktop SHALL describe the acknowledgement as the user's acceptance of the repository's declared terms, not as legal advice or a grant of access.

#### Scenario: No declared license

- **GIVEN** a repository whose metadata declares no license
- **WHEN** the user tries to download one of its sets
- **THEN** the download is refused with an explanation and nothing is staged

#### Scenario: Acknowledged license is recorded

- **GIVEN** a repository declaring `apache-2.0`
- **WHEN** the user acknowledges the license and the installation completes
- **THEN** the installed manifest's `model.license` is `apache-2.0`

### Requirement: Gated repositories use a protected token file

The desktop SHALL support an optional Hugging Face access token configured only as a file path in Settings. The native process SHALL read it with the protected secret-file rules used for other native secrets: a regular file that is not a symbolic link or reparse point, owner-only access on POSIX and an owner-only ACL on Windows, nonempty and at most 4,096 bytes. The file SHALL contain exactly one Hugging Face user access token: `hf_` followed by at least 30 ASCII letters or digits, ignoring surrounding whitespace. The native process SHALL also refuse a token path whose canonical path lies under the desktop's engine key directory or the loaded consumer profile's `data_dir`, or equals that profile's `network.rpc_bearer_file`, both when the path is saved and before each use. It SHALL refuse a file that violates these rules with an error that does not contain the file's contents. A refused or unconfigured token SHALL leave every Hugging Face request without an authorization header. The token SHALL be sent only as an HTTP bearer authorization header on requests whose origin is exactly `https://huggingface.co`; it SHALL NOT be sent to redirect targets on other hosts, placed in URLs or command lines, returned to the webview, or written to logs, diagnostics, settings files, error messages or chat history. The webview SHALL receive only the configured path and whether a usable token is configured. The desktop SHALL NOT accept a repository's gating terms on the user's behalf; a gated repository the configured token cannot read SHALL fail with an explanation that access must be requested on Hugging Face.

#### Scenario: Insecure token file

- **GIVEN** a token file readable by other users
- **WHEN** the user configures its path
- **THEN** the desktop refuses it and reports why without showing the contents

#### Scenario: A non-token secret is refused

- **GIVEN** the user enters the path of an owner-only wallet key file containing 64 hexadecimal characters
- **WHEN** the path is saved
- **THEN** the desktop refuses it as not a Hugging Face token without showing the contents
- **AND** no request to `https://huggingface.co` carries an authorization header

#### Scenario: Redirect target does not receive the token

- **GIVEN** a configured token and a download whose resolve request redirects to an allowlisted download host
- **WHEN** the redirect is followed
- **THEN** the request to the download host carries no authorization header containing the token

#### Scenario: Gated repository without granted access

- **GIVEN** a gated repository for which the configured token has no access
- **WHEN** the user tries to download a set
- **THEN** the download fails with an explanation that access must be requested on Hugging Face

### Requirement: Transfers are native, constrained and resumable

Every native Hugging Face request (search, metadata, model card and file transfer) SHALL run in the native process over HTTPS only. For the initial host and for every redirect target, the client SHALL resolve the host once under a five-second deadline, SHALL reject it when the answer set is empty, holds more than 32 answers or contains any address failing `offer_registry::public_ip`, and SHALL connect only to the validated addresses pinned for that request, as `offer_registry::safe_http_client` does. Redirects SHALL be followed hop by hop by the native client, never by an HTTP library's automatic redirect policy, at most 5 times per request, and only to `https` URLs on port 443 whose host is `huggingface.co` or on a compiled allowlist of Hugging Face download hosts; any other redirect SHALL fail the request (for a download, that file) without connecting to its target. The client SHALL NOT use environment proxy settings. Search, JSON metadata and model-card responses SHALL be read with a bound of 8 MiB per response, the bound the consumer HTTP client uses, and a larger response SHALL fail that request. The Discover download queue SHALL show each item's state (queued, downloading, paused, verifying, installed, failed or cancelled) with bytes received and total, and SHALL support pause, resume and cancel; queue state changes SHALL be announced with `role="status"` and failures with `role="alert"`. Pause SHALL keep staged bytes; resume SHALL continue from the staged length with an HTTP range request, or restart that file from zero when the server does not honor the range; cancel SHALL delete the item's staged bytes. The desktop SHALL refuse to start a download when free space on the app-data volume is less than the bytes remaining, and SHALL pause an item with an error when space runs out during transfer. Closing the main window SHALL NOT pause or cancel downloads. When the process ends, in-progress downloads SHALL stop and remain resumable from staging at the next launch.

#### Scenario: Redirect off the allowlist

- **GIVEN** a resolve request that redirects to a host not on the allowlist
- **WHEN** the download runs
- **THEN** the file fails without contacting that host and nothing is installed

#### Scenario: Allowlisted redirect target resolving to a private address

- **GIVEN** a resolve request that redirects to an allowlisted download host whose DNS answer is a private address
- **WHEN** the download runs
- **THEN** no connection is made to that address and the file fails

#### Scenario: Redirect limit

- **GIVEN** a resolve request whose redirects between allowlisted hosts continue past five hops
- **WHEN** the download runs
- **THEN** the file fails without following the sixth redirect

#### Scenario: Environment proxy is ignored

- **GIVEN** `HTTP_PROXY`, `HTTPS_PROXY` and `ALL_PROXY` name a reachable proxy
- **WHEN** the user searches Discover and downloads a set
- **THEN** no connection is made to the proxy

#### Scenario: Pause and resume

- **GIVEN** a download paused after receiving part of a file
- **WHEN** the user resumes it
- **THEN** the transfer continues from the staged length with a range request
- **AND** the completed file is still fully verified before installation

#### Scenario: Cancel removes staged bytes

- **WHEN** the user cancels an in-progress download
- **THEN** its staged bytes are deleted and the queue shows it as cancelled

#### Scenario: Insufficient disk space

- **GIVEN** the app-data volume has less free space than the set's remaining bytes
- **WHEN** the user starts the download
- **THEN** it is refused with the required and available space shown

#### Scenario: Resume after relaunch

- **GIVEN** the process ended while a download was in progress
- **WHEN** the desktop is launched again
- **THEN** the item appears paused with its staged bytes and can be resumed or cancelled

### Requirement: Nothing is installed before complete verification

Staged files SHALL be kept in a staging area outside the model library and SHALL NOT be listed, loadable or usable as a model. Each selected GGUF file's size and SHA-256 SHALL equal the size and LFS SHA-256 that the repository metadata reports for that file at the pinned commit; a selected file for which the metadata reports no LFS SHA-256 SHALL NOT be installed. After the digests match, the desktop SHALL validate the GGUF set with the same acceptance rules as `scripts/provision-model.py`: complete and consistent shard metadata, no duplicate tensors across shards, bounded metadata, aligned, non-overlapping and in-bounds tensors, file and tensor types supported by the pinned runtime, an embedded tokenizer, a model rather than adapter type, and a model architecture. GGUF metadata SHALL be read with the artifact contract's bounded reader, capping declared lengths and counts before allocating memory for them. The desktop SHALL NOT execute any model-supplied code and SHALL NOT require Python, Docker or WSL to download, verify or install a model. Only an artifact that passes complete validation SHALL be moved atomically into the library. A digest mismatch SHALL discard that file's staged bytes; any failure SHALL install nothing and SHALL report the reason.

#### Scenario: Digest mismatch

- **GIVEN** a downloaded file whose SHA-256 differs from the repository's LFS SHA-256
- **WHEN** verification runs
- **THEN** the file's staged bytes are discarded, the item is marked failed with the reason, and no library entry appears

#### Scenario: Structurally invalid GGUF

- **GIVEN** a downloaded file whose digest matches but whose tensor data is truncated relative to its header
- **WHEN** GGUF validation runs
- **THEN** installation fails and nothing is added to the library

#### Scenario: Staged bytes are invisible

- **GIVEN** a download is in progress or paused
- **WHEN** the user opens My models or the Chat model picker
- **THEN** the staged set is not listed and cannot be loaded

### Requirement: Task, template and limits follow the artifact contract

Before a download starts, the user SHALL choose the install-time contract fields that `scripts/provision-model.py` fixes: the task (`generation` or `embeddings`), the per-slot context length (128–32,768), the maximum output tokens (1 to context minus 1 for generation, 0 for embeddings) and, for generation, the embedded chat template to use. The desktop SHALL show that these values are written into the immutable manifest, that the per-slot context is fixed for the installation, and that a different context requires a separate installation with a new artifact identity. It SHALL indicate when chosen generation limits are below the qualification minimums that a paid generation share requires (context 4,096, output 256). After verification, the desktop SHALL apply the provisioner's rules to the actual GGUF metadata: generation requires a causal model with no pooling; embeddings require mean, CLS or last-token vector pooling and SHALL have a null template name and no template file; the context SHALL NOT exceed the GGUF's native context. Choosing the default embedded template SHALL record `chat_template_name` as null and choosing a named embedded template SHALL record its name; when the GGUF has named templates but no default, a template name SHALL be chosen; when it has no template, the model SHALL be installed without `chat`. If the verified metadata contradicts a chosen value (wrong task, a context above the native context, an absent or missing required template), installation SHALL stop and ask the user to choose again; the desktop SHALL NOT silently change a chosen value. The template file SHALL be written byte-for-byte from the GGUF and never edited or substituted. Capabilities SHALL be `["chat", "completions"]` with a selected template, `["completions"]` without one, and `["embeddings"]` for embeddings. Strings read from GGUF metadata, such as template names and the architecture, SHALL be displayed as escaped plain text. Installed values SHALL NOT change after installation.

#### Scenario: Named templates only

- **GIVEN** a generation GGUF with named embedded templates and no default template
- **WHEN** verification completes without a chosen template name
- **THEN** the desktop asks the user to choose one template name before installing
- **AND** the chosen name is recorded as `chat_template_name`

#### Scenario: Chosen task contradicts the model

- **GIVEN** the user chose `generation` for a GGUF that declares mean pooling
- **WHEN** verification runs
- **THEN** installation stops and asks the user to choose again, and nothing is installed

#### Scenario: Reranker is not installed

- **GIVEN** a GGUF declaring rank pooling
- **WHEN** verification runs
- **THEN** installation fails because the model supports neither task

#### Scenario: Chosen context exceeds the native context

- **GIVEN** the user chose a per-slot context of 32,768 and the verified GGUF declares a native context of 8,192
- **WHEN** verification completes
- **THEN** installation stops and asks the user to choose a context of at most 8,192
- **AND** nothing is installed until the user chooses

### Requirement: Installation writes the provisioner's artifact contract

Installation SHALL write `chiral-artifact-sha256.json` with schema `chiral-gguf-artifact-v1` and exactly the fields `schema`, `model` (`repo_id`, `revision`, `license`), `task`, `quantization`, `context_length`, `max_output_tokens`, `capabilities`, `chat_template_name`, `model_file` and `files`, where each file entry has exactly `sha256`, `size_bytes`, `upstream_repo`, `upstream_path` and `upstream_digest`. It SHALL write `chiral-model.json` as the base ModelSpec: `id` equal to the repository ID followed by `/` and the base name of `model_file`, the first file of the sorted GGUF set; `revision` and `tokenizer_revision` equal to the commit; `chat_template_sha256` over the template's effective source; `quantization` from the GGUF file type; the installed limits; the derived capabilities; and `serving` null. Both files SHALL be canonical JSON, and the installation directory SHALL contain only the selected GGUF files, the optional `chiral-chat-template.jinja` and these two files. The implementation SHALL be native Rust. For the same repository, commit, files, license, task, template selection and limits, both files SHALL be byte-identical to the output of `scripts/provision-model.py`, and the installation SHALL be accepted unchanged by that script's artifact validation and by the managed runtime's inventory validation used by the CLI, the daemon and qualification. The installation's identity SHALL be the SHA-256 of the canonical manifest bytes, and it SHALL be installed at `<app data>/models/<organization>/<name>/<commit>/<artifact identity>/`. A selection whose identity is already installed SHALL be reported as already installed and SHALL NOT be downloaded again. On Windows, a selection whose installed file paths exceed 260 characters SHALL either install, verify, load and chat successfully or be refused before any byte is staged, with a reason naming the path length; it SHALL NOT fail only after its files are downloaded.

#### Scenario: Byte parity with the provisioner

- **GIVEN** the same pinned repository, commit, GGUF file, license, task and limits
- **WHEN** the desktop and `scripts/provision-model.py` each install it
- **THEN** their `chiral-artifact-sha256.json` and `chiral-model.json` files are byte-identical

#### Scenario: Desktop installation is usable by operator tooling

- **GIVEN** a model installed by the desktop
- **WHEN** the provisioner's artifact validation and the managed runtime's inventory validation are run against its directory
- **THEN** both accept it without modification

#### Scenario: Duplicate selection

- **GIVEN** a selection whose manifest identity is already installed
- **WHEN** the user starts the same download again
- **THEN** the desktop reports it as already installed and stages nothing

#### Scenario: Long installed path on Windows

- **GIVEN** a Windows host and a selection whose GGUF path inside the installation makes its full installed path at least 260 characters long
- **WHEN** the user downloads it, loads it and sends a chat message
- **THEN** every step succeeds, or the download is refused before anything is staged with a reason naming the path length

### Requirement: Downloads start only on explicit user action

Model downloads SHALL start only from an explicit user action on the Discover page. Loading a model, sending a chat message, starting the application, any request handled by a later local API server, and any sharing operation SHALL NOT download, update or repair model files. The desktop SHALL NOT poll for or download newer revisions in the background.

#### Scenario: Loading never downloads

- **GIVEN** an installation that fails verification because a file is missing
- **WHEN** the user tries to load it
- **THEN** the load is refused with the verification failure and no download starts

### Requirement: My models lists installations and their verification state

The My models page SHALL list every installation under `<app data>/models/` with its repository, full commit (shortened for display with the full value available), quantization, total size, capabilities, task, installed limits, license, model API ID, and verification state: verified with the time of the last full verification, verifying, or failed with the reason. Enumeration SHALL check at least each installation's file set and sizes; a full digest and structure verification SHALL run at installation, on **Re-verify**, and before every load. An installation that fails verification SHALL NOT be loadable, and the desktop SHALL NOT repair or rewrite its files in place; the user may delete and download it again. A verification failure caused by files outside the inventory SHALL name each unexpected path. Each entry SHALL offer **Load**, **Re-verify**, **Reveal location** and **Delete**. Reveal location SHALL take an installation identity, resolve its directory natively under `<app data>/models/`, and reveal that directory selected within its parent directory where the platform file manager supports selection (Finder `open -R`, Explorer `/select,`), otherwise open the parent directory. It SHALL NOT accept a path or URL from the webview, and SHALL NOT grant the webview filesystem or shell permissions. The page SHALL state that downloaded GGUF files, including their embedded chat templates, are untrusted inputs to the native engine, whose process is not an operating-system sandbox, and that installation or successful local loading is not qualification, a speed class or proof of API compatibility for paid serving. The disclosure required before loading is specified by `desktop-local-runtime`. The share status of an installation is specified by `specify-desktop-share-online`.

#### Scenario: Tampered installation

- **GIVEN** an installed GGUF file is modified after installation
- **WHEN** the user re-verifies or loads that model
- **THEN** its state becomes failed with the reason and loading is refused
- **AND** the desktop does not rewrite the file

#### Scenario: Reveal location

- **WHEN** the user chooses Reveal location for an installation
- **THEN** the operating-system file manager shows that installation's directory selected within its parent directory, or the parent directory where selection is unsupported
- **AND** the native command accepts only an installation identity and no path or URL argument from the webview
- **AND** the webview capability gains no filesystem or shell permission

#### Scenario: Unexpected file is named

- **GIVEN** a `.DS_Store` file was written inside an installation directory after installation
- **WHEN** the user re-verifies that installation
- **THEN** its state becomes failed with a reason naming `.DS_Store` as an unexpected file
- **AND** the desktop does not delete or rewrite any file in the installation

### Requirement: Deleting an installation is explicit and refused while in use

Deletion SHALL require a confirmation that names the installation and the space it frees, and SHALL remove only that installation's directory and any parent directories it leaves empty. It SHALL NOT remove other installations, runtimes or chat history; chat threads that used the model SHALL remain and show the model as unavailable. The desktop SHALL refuse deletion while the installation is loaded, while it is being verified, and while it backs an active share or an in-progress qualification run as specified by `specify-desktop-share-online`, and the refusal SHALL say which action must happen first (eject, wait for verification, stop sharing, or stop qualification).

#### Scenario: Loaded model cannot be deleted

- **GIVEN** an installation that is currently loaded
- **WHEN** the user chooses Delete
- **THEN** the deletion is refused with an instruction to eject the model first

#### Scenario: Deletion is scoped to one installation

- **GIVEN** two installations of the same repository at different quantizations and a chat thread that used one of them
- **WHEN** the user confirms deletion of that one
- **THEN** only its directory is removed, the other installation remains, and the thread remains with the model shown as unavailable
