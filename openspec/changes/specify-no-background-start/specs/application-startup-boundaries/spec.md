## ADDED Requirements

### Requirement: The application runs no chain node or miner

The native desktop (`chiral-desktop`), the CLI (`chiral`) and the headless binaries (`chiral_daemon`, `relay_server`, `chiral_rpc_gateway` and `chiral-policy-sign`) SHALL NOT bundle, install, download, start, stop or manage a blockchain node or a miner, such as Geth, Core-Geth, a Conflux node, or an Ethash or other mining process (book lines 17, 31, 777 and 793; `CLAUDE.md`: no "app-managed mining is part of this release"). They SHALL reach the chain only through the separately operated Conflux eSpace RPC endpoint that the explicit profile or configuration names (book line 31: "separately operated Conflux eSpace RPC verifies native CFX funding"). No desktop page or native operation, `chiral` command, provider operator action or stored setting SHALL start or stop mining or set a mining or coinbase address (`CLAUDE.md`: no "app-managed mining").

The chain identity, which admits only eSpace testnet chain 71 and never falls back to a legacy chain, is `inference-wire-foundation` (*Complete and explicit network identity*). The RPC gateway's refusal of mining and node-administration methods is `chain-rpc-gateway` (*Only an exact eth_* allowlist is forwarded, with fixed parameter shapes*). The provider's managed engine child, which is neither a node nor a miner, is `managed-engine-runtime` (*A managed deployment runs only the pinned llama-server as a private Rust-supervised child*).

#### Scenario: Starting the desktop and loading a profile starts no node or miner

- **GIVEN** a machine on which an earlier Chiral version installed Core-Geth and left its binary and chain data in place
- **WHEN** the desktop starts, a consumer profile is loaded and the wallet balance is read
- **THEN** no node or miner process is started, and the legacy binary is not executed
- **AND** every chain request goes to the profile's `rpc_url`

#### Scenario: No surface controls mining or a node

- **WHEN** the desktop's native operations and pages, the `chiral` subcommands and the provider operator actions are enumerated
- **THEN** none starts, stops or configures a node or mining, or sets a mining address

#### Scenario: A provider starts no node or miner

- **GIVEN** a provider configuration with a managed runtime
- **WHEN** `chiral_daemon --config <provider.json>` starts, serves and shuts down
- **THEN** it starts no node or miner process
- **AND** its chain requests go only to the configured eSpace RPC endpoint

### Requirement: Login and session start launch no node, miner or legacy service

Neither an operating-system login, nor the application's own launch, nor the start of an application session (loading a consumer profile with the desktop's `configure` or the CLI's `--config`, or loading, creating or importing its wallet) SHALL start, restart or schedule a chain node, a miner or a legacy service (book line 31: "The application never starts a node or miner on login"; `CLAUDE.md`: do not "auto-start legacy services"). A legacy service is a service of the retired product: file sharing, Drive, ChiralDrop, CDN or site publication and hosting, download payments, or the bundled Core-Geth node and Ethash mining (book line 793; line 17: "Historical storage/Drive/CDN/Core-Geth/in-app-mining guides are not current runtime instructions"). The application SHALL NOT start, restart, enable or schedule a legacy service that it finds installed on the host (`CLAUDE.md`: do not "auto-start legacy services").

The application SHALL NOT install or register a start-at-login entry, service or scheduled task that starts a chain node, a miner or a legacy service; examples are a macOS launch agent or login item, a Windows `Run` registry value, Startup-folder shortcut or scheduled task, an XDG autostart entry and a systemd unit. A stored preference of an earlier version that asked for such a start, such as an automatic mining start, SHALL NOT be restored or honoured: the persisted desktop preferences hold only the choices that `desktop-gui-pages` (*Settings loads a profile by path and states whether one is loaded*) lists (book line 963: they "contain no secrets or legacy runtime fields").

That the GUI starts no network action before a profile is loaded is `desktop-gui-global-rules` (*Network actions wait for an explicitly loaded profile*). That the main window grants no shell permission is `desktop-window-lifecycle` (*Window permissions stay minimal*). In the target design, that a share never restarts by itself when the desktop starts is `desktop-share-online` (*Sharing is an explicit, ordered wizard*).

#### Scenario: A stored automatic mining preference is ignored

- **GIVEN** the desktop's stored preferences (`chiral-inference-settings`) are `{"autoStartMining": true, "downloadDirectory": "/tmp", "theme": "dark"}`, and the earlier application's `chiral-settings` entry holds `{"autoStartMining": true}`
- **WHEN** the desktop starts and the user loads a consumer profile
- **THEN** the preferences in use keep the theme and contain neither `autoStartMining` nor `downloadDirectory`
- **AND** the `chiral-settings` entry is not applied
- **AND** no node, miner or legacy service is started

#### Scenario: Operating-system login starts nothing the book retired

- **GIVEN** the application installed and used: a profile loaded, a wallet created and an account funded
- **WHEN** the user logs out of the operating system and logs in again
- **THEN** no chain node, miner or legacy service starts because of the login
- **AND** the application has registered no start-at-login entry, service or scheduled task that starts any of them

#### Scenario: A legacy service left on the host is not started

- **GIVEN** a host with a stopped, disabled legacy chain-node or file-sharing service unit from an earlier deployment
- **WHEN** the desktop, the CLI or a provider daemon starts, or a profile is loaded
- **THEN** that service stays stopped and is not enabled or scheduled

### Requirement: Legacy data stays in place and is never imported automatically

The desktop and the CLI SHALL NOT delete, move, rewrite, migrate or automatically import legacy data: wallet files and exports, chain databases, Drive and download files, hosting data, old CHI history or file-transfer reputation, whether it lies under the configured `data_dir` base or elsewhere (book line 679: the cutover "does **not** delete or migrate existing Drive files, chain databases, wallet exports, old CHI history, or file-transfer reputation", and "There is no automatic legacy data importer"; `CLAUDE.md`: "Preserve legacy wallet, chain, download, Drive, and hosting data in place"). They SHALL keep their wallet, account, credential, request-key and nonce-journal state in the separate network-scoped inference namespace under the `data_dir` base, not in a legacy location (book line 470: `inference/<hash-of-full-network-identity>/`; line 472: consumer accounts, credentials, request keys and wallet state "are also network-scoped"; line 679: "The cutover creates separate inference namespaces"). A legacy balance, contract, reputation or datum SHALL NOT become new-network credit, and CHI or CHR SHALL NOT be converted to CFX (book lines 675 and 679; `CLAUDE.md`). A supported legacy private key SHALL enter native protected storage only through an explicit import of a key file that the user names, which leaves that file unchanged (book line 679: "Existing supported private keys can be explicitly imported into native protected storage"). No wallet SHALL be loaded or created from a legacy file without that explicit import.

The Wallet page's statement that importing converts no CHI or CHR balance is `desktop-gui-pages` (*The Wallet page manages keys only through native file paths*). The provider daemon's part of this rule is `provider-daemon-lifecycle` (*The provider ledger opens only under its own schema, network and provider*). Where a relative `data_dir` resolves, and the refusal of state left at an old location without moving it, are `profile-path-resolution`.

#### Scenario: Legacy directories under the base are untouched

- **GIVEN** a consumer `data_dir` base that also holds a legacy `geth/` chain directory, a legacy wallet file and Drive and download directories
- **WHEN** the profile is loaded, a wallet is created and its balance is read, through the desktop or the CLI, and the user then logs out of the desktop
- **THEN** every legacy file is byte-identical and still in place
- **AND** the wallet key and nonce journal that were created are under `<data_dir>/inference/<network hash>/`

#### Scenario: A legacy wallet is not loaded automatically

- **GIVEN** a legacy wallet file and CHI transaction history in their legacy location, and no wallet in the new namespace
- **WHEN** a consumer profile is loaded
- **THEN** no wallet is loaded or created from the legacy file
- **AND** the desktop offers only generating a wallet or importing a key file by its path, and no CHI or CHR balance is shown or credited

#### Scenario: An explicit import converts no balance

- **GIVEN** a supported legacy private key in an owner-only file
- **WHEN** the user imports it by path
- **THEN** the key is stored in native protected storage, the source file is unchanged, and the address's balance is its eSpace CFX balance on the configured network only

### Requirement: Core-space funds become usable only through the user's explicit transfer to eSpace

Conflux Core space and eSpace balances are separate (book line 940). The balance that the application shows and the funds that it spends SHALL be the eSpace balance of the configured network, read through that network's eSpace RPC endpoint (book lines 17 and 31). The application SHALL NOT present a Core-space balance or mining payout as eSpace balance, funding or provider credit, and SHALL NOT transfer funds from Core space to eSpace without the user's explicit action. A mining payout SHALL become usable for funding only after the user's explicit Core-to-eSpace transfer has executed on eSpace (book line 31: "Core mining payouts require an explicit Core-to-eSpace transfer before use").

Provider credit comes only from eSpace funding evidence, as `inference-funding-settlement` (*Provider credit requires authenticated settlement under the policy*) specifies. The Wallet page's disclosure that Core and eSpace balances are separate and that funding an eSpace wallet from Core needs an explicit transfer is `desktop-gui-pages` (*The Wallet page manages keys only through native file paths*) (book lines 31 and 940).

#### Scenario: A Core payout is not an eSpace balance

- **GIVEN** mining payouts held in Core space for the user, and an eSpace balance of 0 at the wallet's address
- **WHEN** the user reads the balance on the desktop Wallet page or with `chiral wallet balance`
- **THEN** the balance is 0 CFX on the configured eSpace network
- **AND** no Core-space request, transaction or cross-space transfer is made

#### Scenario: A transfer the user made is used only after it executes on eSpace

- **GIVEN** the user has explicitly transferred CFX from Core space to the wallet's eSpace address with a Conflux wallet outside Chiral
- **WHEN** the transfer has executed on eSpace and the user funds an opening
- **THEN** the opening spends the eSpace balance, and the provider credits it only from the eSpace funding evidence

### Requirement: Native provisioning starts no service

Native provisioning with `scripts/provision-provider.py` (without `--container`), `scripts/provision-runtime.py` and `scripts/provision-model.py` SHALL only verify its inputs and write configuration, protected secrets and verified artifacts. It SHALL NOT start, enable, install or schedule a service, daemon, engine, node or miner, and SHALL NOT create a start-at-login or boot entry; a helper process it runs SHALL finish before the script exits (book line 644: "Native provisioning starts no service"; line 607: provisioning and "separate native service startup"). Starting a native provider SHALL remain the operator's explicit action with `chiral_daemon --config <provider.json>`. The optional Linux Compose deployment is deployment machinery with no standalone engine service (book line 644), and it SHALL contain no chain node or miner service (book line 31: settlement uses a "separately operated" eSpace RPC; `CLAUDE.md`: no "app-managed mining").

#### Scenario: Provider provisioning leaves nothing running

- **GIVEN** a host on which no Chiral process runs
- **WHEN** `scripts/provision-provider.py` completes without `--container`
- **THEN** no process it started is still running, and it created no service unit, launch agent, scheduled task or autostart entry
- **AND** no provider runs until the operator starts one

#### Scenario: Runtime and model provisioning start nothing

- **WHEN** `scripts/provision-runtime.py` and `scripts/provision-model.py` complete
- **THEN** no service, engine, node or miner is running or scheduled because of them

#### Scenario: The Compose deployment has no node or miner

- **WHEN** the reference Compose deployment's services are listed
- **THEN** none of them is a chain node, a miner or a standalone engine service
