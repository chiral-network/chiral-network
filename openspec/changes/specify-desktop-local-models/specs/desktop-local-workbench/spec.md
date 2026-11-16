## ADDED Requirements

### Requirement: Local mode works without a profile, wallet, chain or connectivity

With no consumer profile loaded, no wallet and no network connectivity, a user with an installed runtime and model SHALL be able to load it, chat with it and eject it. Local operations (library, load, eject, local chat and local history) SHALL NOT require a consumer profile or wallet, SHALL NOT start the DHT or a consumer session, and SHALL NOT open any connection other than loopback connections to the managed engine; in particular they SHALL NOT contact the chain RPC, a wallet service, a catalog, a relay, a provider or Hugging Face. Explicit-network selection and session scoping (A.1.3 and A.1.8) SHALL govern Chiral network actions and network sessions only: the no-profile banner and the disabling of network actions SHALL apply to network pages and network-thread actions and SHALL NOT block the Chat, Discover or My models pages, and logging out or loading another profile SHALL NOT cancel local-model chats. Hugging Face SHALL be a fixed compiled origin used only by Discover and model download, and SHALL NOT count as selecting a Chiral network.

#### Scenario: Offline local chat

- **GIVEN** no consumer profile is loaded and all non-loopback network access is blocked
- **WHEN** the user loads an installed model, chats with it and ejects it
- **THEN** every step succeeds
- **AND** the desktop opens no non-loopback connection

#### Scenario: Local pages without a profile

- **GIVEN** no consumer profile is loaded
- **WHEN** the user opens Chat, Discover or My models
- **THEN** the page is usable and is not blocked by the no-profile banner

### Requirement: Navigation exposes the local workflow

The native shell SHALL add the routes `#/chat`, `#/discover` and `#/models` with navigation items Chat, Discover and My models, each with a label, short label and icon, placed first in the navigation order (Chat, Discover, My models, then Local server and Share when they exist, then Network and Wallet). Today's Marketplace, Playground, Usage & API keys, Account (local account index), Network and Provider pages SHALL be grouped under a Network section, the Provider page remaining there for administering an external `chiral_daemon` over its loopback administration listener; Wallet SHALL remain its own item; Settings and Diagnostics SHALL remain, at the end of the navigation. The Network group SHALL hold its pages in the order Marketplace, Playground, Usage & API keys, Account, Network, Provider. In the Sidebar layout it SHALL appear as a labeled Network group holding those items; in the Top bar layout it SHALL appear as one Network menu whose items are those pages, and the Top bar's existing rules for folding later top-level items into More and for icon-only and narrow widths SHALL apply to the resulting top-level items without reordering them. Every existing route SHALL stay reachable. Navigation items for Local server and Share SHALL NOT appear until `specify-desktop-local-api-server` and `specify-desktop-share-online` respectively are implemented. This change SHALL NOT change the fallback route for unknown paths. The new navigation SHALL keep the accessibility rules: visible labels, `aria-current` on the active item, the skip-to-content link and text, not color alone, for status.

#### Scenario: New routes

- **WHEN** the user opens `#/chat`, `#/discover` or `#/models`
- **THEN** the Chat, Discover or My models page is shown and its navigation item is marked with `aria-current`

#### Scenario: Existing pages stay reachable

- **WHEN** the user opens any route that existed before this change
- **THEN** that page is still shown

#### Scenario: Navigation order and Network grouping in both layouts

- **GIVEN** neither `specify-desktop-local-api-server` nor `specify-desktop-share-online` is implemented
- **WHEN** the shell is shown in the Sidebar layout and in the Top bar layout
- **THEN** in each the top-level items appear in the order Chat, Discover, My models, Network, Wallet, Settings, Diagnostics
- **AND** the Network group (Sidebar) or Network menu (Top bar) contains Marketplace, Playground, Usage & API keys, Account, Network and Provider in that order
- **AND** no Local server or Share item appears

### Requirement: The status bar shows local engine and download state

The native shell SHALL show a persistent status bar with each loaded model's engine state (loading, verifying, running or failed, or none loaded), the memory use observed natively against the budget, and the number of active downloads. When a consumer profile and wallet are loaded it SHALL also show the wallet balance as exact wei converted to CFX without floating point; without a profile it SHALL NOT query any chain RPC for the status bar. Sharing state and transport are added by `specify-desktop-share-online`. Status changes SHALL be announced with `role="status"` and failures with `role="alert"`, and every state SHALL carry text.

#### Scenario: Engine and downloads in the status bar

- **GIVEN** one model loaded and two downloads in progress, with no profile loaded
- **WHEN** the user looks at the status bar
- **THEN** it shows the engine as running with observed memory use and two active downloads
- **AND** no wallet balance is shown and no chain RPC request is made

### Requirement: Settings expose local storage, token, runtime and history controls

Settings SHALL add sections for **Storage** (read-only display of the app data, models, runtimes and chats locations), **Hugging Face token file** (a file path input showing only whether a usable token is configured), **Runtimes and backends** (as specified by `desktop-local-runtime`) and **Chat history** (the on/off preference and Clear all). Sleep prevention is specified by `specify-desktop-share-online`. These preferences SHALL be stored natively in a settings file under the app data directory so that the native process enforces them; the webview's existing preference storage SHALL continue to contain no secrets and no chat content.

#### Scenario: Token configuration reveals no secret

- **WHEN** the user configures a valid Hugging Face token file path
- **THEN** Settings shows the path and that a token is configured
- **AND** the token value never reaches the webview

### Requirement: Local features stay behind the native boundary

All local features SHALL run through native commands. The webview capability SHALL gain only allow-permissions for those custom commands and SHALL NOT gain shell, filesystem, HTTP, dialog or opener plugin permissions. The webview SHALL NOT receive the Hugging Face token, engine keys, engine URLs or ports, or the contents of any secret file; it may receive file paths and public metadata. `<app data>` SHALL be the per-user local, non-roaming application data directory (Tauri's `app_local_data_dir`; on Windows under `%LOCALAPPDATA%`, never the roaming `%APPDATA%`). The directories this change creates (`models/`, `runtimes/`, `downloads/`, `chats/` and `engine/`), the native settings file, and everything the native process writes under those directories SHALL be owner-only: on POSIX, directories `0700`, runtime executables and libraries `0700`, and all other files `0600`, with the `<app data>` root itself `0700`; on Windows, an owner-only ACL. Files that the webview runtime or other software keeps under `<app data>` are outside this rule. Outside the native runtime, the Chat, Discover and My models pages SHALL show the browser-preview notice, and every local action SHALL fail with an explanatory error without fabricating search results, installations, runtimes, memory values or model output. Native diagnostics MAY report runtime identities, engine states and download states, and SHALL NOT include secrets, prompts or generated text.

#### Scenario: Capability file

- **WHEN** the desktop capability file is inspected after this change
- **THEN** its only new permissions allow the new custom native commands
- **AND** it contains no shell, filesystem, HTTP, dialog or opener permission

#### Scenario: Browser preview

- **GIVEN** the GUI runs outside the native runtime
- **WHEN** the user searches Discover or opens My models
- **THEN** a preview notice is shown and the action fails with an explanation
- **AND** no search results or installations are fabricated

#### Scenario: Protected app data

- **WHEN** the permissions of the `<app data>` root, the `models/`, `runtimes/`, `downloads/`, `chats/` and `engine/` directories with their contents, and the native settings file are inspected on POSIX
- **THEN** every directory is `0700`, runtime executables and libraries are `0700`, every other file is `0600`, and all are owned by the current user

#### Scenario: Large local state is not roamed on Windows

- **GIVEN** a Windows user profile
- **WHEN** the desktop installs a model and a runtime
- **THEN** their directories are under the user's `%LOCALAPPDATA%`, not under `%APPDATA%`
