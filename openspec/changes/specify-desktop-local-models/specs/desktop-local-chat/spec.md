> **2026-11-25 scope amendment:** Launch uses the checked-in Strata catalog and runtime contract in `strata-curated-runtime`. Broad Hugging Face search, branch/tag resolution, arbitrary GGUF set selection/import, llama-only archives/flags and embedded-tokenizer rules below are deferred legacy scope, not launch requirements. Launch artifacts include all pinned weights/tokenizer/template files. Shared explicit-download, native trust boundary, library, chat, cancellation and no-background-start requirements remain target design; this amendment does not mark the desktop milestone implemented. The author explicitly authorized revising the book; earlier read-only/book-line references record the prior baseline. See `adopt-strata-curated-launch` for supersession and current verification gates.

## ADDED Requirements

### Requirement: Chat threads carry their own source and parameters

The Chat page SHALL let the user create, select, rename and delete threads. Each thread SHALL have one source at a time: a local installation whose capabilities include `chat`, or a funded network account whose model advertises `chat`. The model picker SHALL label every entry with its source and cost: local entries as local and free, network entries with the provider, model and input and output price per 1,000 tokens in exact CFX. Network entries SHALL be listed only while a consumer profile is loaded. Installations without `chat`, and installations that failed verification, SHALL NOT be offered as chat sources. Selecting an installation that is not loaded SHALL offer loading it with explicit settings as specified by `desktop-local-runtime`, and sending SHALL stay disabled until it is loaded; a loaded local source SHALL offer Eject as specified by `desktop-local-runtime`. While the installation backs an active share or an in-progress qualification run as defined by `specify-desktop-share-online`, the desktop SHALL NOT offer to load it and SHALL refuse a load of it in a desktop engine, and local generation on it is dispatched or refused as that change specifies instead of being sent to a desktop engine. Each thread SHALL keep its own system prompt, temperature (0–2), top-p (greater than 0 and at most 1) and maximum output tokens (1 to the smaller of the installation's `max_output_tokens` or the network model's advertised maximum output). Values outside these bounds SHALL be refused, not clamped silently.

#### Scenario: Completions-only model is not a chat source

- **GIVEN** an installed model whose capabilities are only `completions`
- **WHEN** the user opens the Chat model picker
- **THEN** that model is not offered for chat

#### Scenario: Per-thread parameters

- **GIVEN** two threads with different system prompts and temperatures
- **WHEN** the user switches between them
- **THEN** each thread shows and uses its own parameters

### Requirement: Local generation streams through the native engine boundary

Sending a message in a local thread SHALL invoke a native command that builds an OpenAI-compatible chat request from the thread's system prompt, messages and parameters, with the thread's maximum output tokens as the request's output limit, and sends it to that model's loaded engine over loopback with the per-load key; the webview SHALL NOT contact the engine. The native engine client SHALL connect only to literal `127.0.0.1` on that load's port, SHALL ignore environment proxy settings, SHALL NOT follow redirects, and SHALL treat any 3xx response as a failed generation. While the installation backs an active share or an in-progress qualification run as defined by `specify-desktop-share-online`, local generation on it SHALL be dispatched or refused as that change specifies instead of being sent to a desktop engine. Generated text SHALL stream to the webview through a native event channel. The thread SHALL show a status (idle, generating, completed, cancelled or failed). **Stop** SHALL be idempotent and SHALL end the generation at the engine. Token counts SHALL be the prompt and completion counts the engine reports, and SHALL be shown as unavailable when the engine reports none; they SHALL NOT be estimated from text. Before dispatch the native command SHALL render the thread through the engine's `/apply-template`, count the rendered prompt with its `/tokenize`, and, without starting generation, refuse the message as a context overflow when the prompt tokens plus the thread's maximum output tokens exceed the installation's `context_length`, as the provider gateway does in `inference_backend.rs`. A generation that ends with finish reason `length` SHALL be shown as stopped at the length limit, not as a normal completion. The desktop SHALL NOT drop, truncate or summarize earlier messages. The desktop SHALL run at most the loaded concurrency of simultaneous generations per loaded model and SHALL report the model as busy beyond that. Local generation SHALL create no account, ledger entry, charge or request key and SHALL write nothing to consumer network state.

#### Scenario: Stop ends a local generation

- **GIVEN** a local generation is streaming
- **WHEN** the user chooses Stop
- **THEN** the engine stops generating and the thread shows cancelled with the text received so far

#### Scenario: Context overflow

- **GIVEN** a local thread whose rendered prompt tokens plus maximum output tokens exceed the installation's `context_length`
- **WHEN** the user sends a message
- **THEN** the desktop reports a context overflow before any token is generated
- **AND** no earlier message is removed or altered

#### Scenario: Output limit reached

- **GIVEN** a local generation that reaches the thread's maximum output tokens
- **WHEN** the engine ends it with finish reason `length`
- **THEN** the thread shows the reply as stopped at the length limit rather than as completed

#### Scenario: Environment proxy is ignored for the engine

- **GIVEN** `HTTP_PROXY`, `HTTPS_PROXY` and `ALL_PROXY` name a non-loopback proxy
- **WHEN** the user sends a local message
- **THEN** the generation reaches the engine on `127.0.0.1`
- **AND** no connection is made to the proxy

#### Scenario: Engine-reported usage

- **WHEN** a local generation completes
- **THEN** the thread shows the prompt and completion token counts reported by the engine

### Requirement: Chat output renders as untrusted plain text

Model output, including any reasoning text or tool-call content, SHALL render as escaped plain text, never as HTML or Markdown converted to HTML. Rendering SHALL NOT load remote resources, open URLs or execute or dispatch tool calls. Copying a message SHALL copy its plain text.

#### Scenario: Markup in output

- **GIVEN** a model response containing `<img src="https://example.invalid/x.png">`, a Markdown image and a `<script>` element
- **WHEN** the response is displayed
- **THEN** it appears literally as text
- **AND** no request is made to any of the referenced URLs and no script runs

### Requirement: Network threads keep every consumer rule

A thread whose source is a network account SHALL use the existing native consumer operations for that account (`inference_stream`, `inference_cancel` and `request_status`) and SHALL NOT add any payment, funding or accounting path. It SHALL show the provider wallet, the provider endpoint or transport reported natively, the account status and available balance, and each request's cost from native usage data, all money as exact wei converted to CFX without floating point. It SHALL state that the provider sees prompts and responses in plaintext and that signed metadata does not prove execution. Sending SHALL be enabled only for an `active` or `free` account whose model advertises `chat`. A cancelled request SHALL be labeled "Cancelled — generated tokens may be billed", a request whose stream fails or ends ambiguously "Failed — inspect request status before retrying", and a request whose dispatch fails before a stream opens "Failed — inspect Usage before retrying". When streamed output exceeds the desktop display limit (the Playground's 1,000,000 characters), the desktop SHALL request cancellation and ask the user to inspect usage before retrying. Each cancelled or failed request that has a request ID SHALL show its request ID with a **Reconcile** action that reads request status. The desktop SHALL NOT resend a failed or cancelled paid request automatically; sending again SHALL be a new explicit user request. Funding and top-up SHALL NOT occur in the Chat page; it SHALL link to the Marketplace and Usage pages, where the existing funding disclosures apply. Per-request path labels for libp2p providers (Direct, Hole-punched, or Relayed with the relay's details) are specified by `specify-p2p-provider-transport`; its relay-metadata disclosure is given before funding, not in Chat.

#### Scenario: Stream error is not retried

- **GIVEN** a network thread whose stream ends with an error
- **WHEN** the error is shown
- **THEN** the request ID and Reconcile action are shown with "Failed — inspect request status before retrying"
- **AND** no request is sent again until the user sends one

#### Scenario: Output above the display limit

- **GIVEN** a network-thread stream whose output passes 1,000,000 characters
- **WHEN** the next delta arrives
- **THEN** the desktop requests cancellation through `inference_cancel`
- **AND** the thread asks the user to inspect usage before retrying and nothing is resent

#### Scenario: Dispatch failure label

- **GIVEN** a network thread whose request fails before a stream opens
- **WHEN** the error is shown
- **THEN** the request is labeled "Failed — inspect Usage before retrying" and nothing is resent

#### Scenario: Frozen account cannot send

- **GIVEN** a network thread whose account is frozen
- **WHEN** the user tries to send a message
- **THEN** sending is disabled and the account status is shown

### Requirement: Thread content leaves the machine only by explicit choice

A local thread SHALL NOT send its content anywhere except to the loopback engine. Changing a thread's source from a local installation to a network account, or from one network account to another, SHALL require an explicit confirmation that names the provider and states that the thread's system prompt and earlier messages will be sent to that provider in plaintext with the next request; declining SHALL leave the thread's source unchanged and send nothing. Switching a thread between local installations SHALL NOT require that confirmation. Stored history SHALL NOT be sent anywhere except as part of a request the user makes.

#### Scenario: Local thread switched to a network account

- **GIVEN** a thread with earlier messages exchanged with a local model
- **WHEN** the user selects a funded network account as its source
- **THEN** a confirmation names the provider and states that earlier messages will be sent to it
- **AND** if the user declines, the thread keeps its local source and nothing is sent

### Requirement: Chat history is local, protected and under user control

The chat-history preference SHALL be on by default: when no preference is stored, history SHALL be on. The first time the Chat page opens with no stored preference, and before any thread is stored, the desktop SHALL show a disclosure stating that chat history is on, that threads are stored only on this machine under `<app data>/chats/`, that they are not encrypted by Chiral, and that history can be turned off and cleared, with a Turn off action that sets the preference to off before anything is stored. The desktop SHALL record natively that this disclosure was shown and SHALL NOT show it automatically again; dismissing it SHALL keep history on. When chat history is on, the desktop SHALL store threads natively under `<app data>/chats/` with owner-only protection: title, source reference, parameters, messages, timestamps, token counts and, for network threads, the network identity, the consumer wallet address, the account ID and request IDs. History SHALL NOT contain wallet keys, credentials, the Hugging Face token or engine keys. Chat content SHALL NOT be stored in webview storage (localStorage, sessionStorage or IndexedDB), logs, diagnostics or telemetry, and SHALL NOT be published to the DHT, a catalog, a relay or a provider, or synced anywhere. The user SHALL be able to turn history off, delete one thread, and clear all history after a confirmation. Turning history off SHALL stop all further writes, keep threads only in memory for the rest of the app session, and offer to clear existing stored history without deleting it automatically. The Chat page SHALL disclose whether history is on, where it is stored, and that it is not encrypted by Chiral.

#### Scenario: History is on by default with a first-run disclosure

- **GIVEN** a desktop with no stored chat-history preference and no stored threads
- **WHEN** the user opens the Chat page for the first time
- **THEN** history is on and a disclosure states that threads are stored only on this machine under `<app data>/chats/`, are not encrypted by Chiral, and can be turned off and cleared
- **AND** nothing is written under `<app data>/chats/` before the disclosure has been shown

#### Scenario: History turned off from the first-run disclosure

- **GIVEN** the first-run history disclosure is shown
- **WHEN** the user chooses Turn off and then chats
- **THEN** the preference is stored as off and nothing from that session is written under `<app data>/chats/`
- **AND** the disclosure is not shown again on the next launch

#### Scenario: History off

- **GIVEN** history is turned off
- **WHEN** the user chats and then quits and relaunches the desktop
- **THEN** threads from that session are not restored
- **AND** nothing from that session was written under `<app data>/chats/`

#### Scenario: Clear all history

- **WHEN** the user clears all history and confirms
- **THEN** all stored threads are deleted from `<app data>/chats/` and the thread list is empty

#### Scenario: Content stays out of webview storage and diagnostics

- **GIVEN** history is on and threads exist
- **WHEN** the webview's storage and the native diagnostics output are inspected
- **THEN** neither contains any prompt or response text

### Requirement: Chat streams follow session scoping and the window lifecycle

Closing the main window SHALL NOT cancel local or network chat streams. Session scoping (A.1.3 and A.1.8) SHALL govern network threads only: logging out or loading a different consumer profile SHALL cancel every active network-thread stream and clear displayed network account data, and SHALL NOT cancel local-model chats, eject loaded models or delete stored history. Local-model chat SHALL need no profile. Network threads SHALL be listed only while the loaded profile's network identity and consumer wallet address both equal those stored with the thread and the thread's account exists in the loaded profile's account store; other network threads SHALL be hidden, not deleted. Local threads SHALL remain listed regardless of the loaded profile. Confirmed Quit SHALL end all chat streams as part of the existing quit sequence. In the browser preview the Chat page SHALL show the preview notice and SHALL NOT fabricate models, threads or output.

#### Scenario: Logout cancels network streams only

- **GIVEN** a loaded profile, a streaming network thread and a streaming local generation
- **WHEN** the user logs out
- **THEN** the network-thread stream ends cancelled
- **AND** the local generation keeps streaming, the model stays loaded, and stored threads remain

#### Scenario: Profile for another network

- **GIVEN** stored network threads for network A
- **WHEN** the user loads a profile for network B
- **THEN** network A's threads are not shown and remain stored
- **AND** local threads are still shown

#### Scenario: Profile for the same network with another wallet

- **GIVEN** stored network threads created under wallet A on network N
- **WHEN** the user loads a profile for network N whose wallet is B
- **THEN** wallet A's network threads are not shown and remain stored
- **AND** local threads are still shown
