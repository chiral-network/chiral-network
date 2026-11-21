## ADDED Requirements

### Requirement: Relative configuration paths resolve against the configuration file's directory

Every relative path-valued field of a consumer profile, a provider configuration, a relay configuration and an RPC-gateway configuration SHALL be resolved against the directory that contains that configuration file when the file is loaded, independent of the process working directory (book line 449). An absolute path SHALL be used unchanged. Resolution SHALL be lexical: it SHALL NOT require the path to exist, and it SHALL NOT create, move or rewrite anything.

The path-valued fields are:

- consumer profile: `data_dir`, `network.rpc_bearer_file` and `dht.identity_file` (book lines 459, 461 and 464);
- provider configuration: `data_dir`, `signing_key_file`, `admin_token_file`, `dht.identity_file`, `network.rpc_bearer_file`, and for each model `engine_api_key_file`, `qualification_evidence_file`, `runtime.runtime_dir` and `runtime.artifact_dir`;
- relay configuration: `data_dir`, `dht.identity_file`, `network.rpc_bearer_file` and `version_policy_file`;
- RPC-gateway configuration: `rpc_bearer_file`.

A relative `--config` path is itself read from the process working directory; the paths inside the file still resolve against that file's directory. Path arguments given on the command line rather than inside a configuration file, such as a wallet import or export file, an offer or quote document, `--version-policy`, or the gateway's `--journal-dir` and `--bearer-file`, are not configuration paths and SHALL follow the process working directory.

#### Scenario: A consumer profile's state is created next to the profile

- **GIVEN** `/p/consumer.json` with `"data_dir": "data"` and a process working directory `/w`
- **WHEN** `chiral --config /p/consumer.json wallet create` runs
- **THEN** the wallet key, the nonce journal and the policy watermark directory are created under `/p/data/inference/<network-hash>/`
- **AND** nothing is created under `/w`

#### Scenario: An absolute data_dir is used unchanged

- **GIVEN** a profile in `/p` with `"data_dir": "/var/lib/chiral"`
- **WHEN** it is loaded from any working directory
- **THEN** its state is under `/var/lib/chiral`

#### Scenario: Every relative consumer path field resolves

- **GIVEN** a profile in `/p` with `"data_dir": "data"`, `"rpc_bearer_file": "secrets/rpc.token"` and a DHT `"identity_file": "peer.key"`
- **WHEN** it is loaded
- **THEN** the paths in use are `/p/data`, `/p/secrets/rpc.token` and `/p/peer.key`

#### Scenario: A provider configuration resolves its data_dir next to itself

- **GIVEN** `/p/provider.json` with `"data_dir": "data"`
- **WHEN** it is loaded from a different working directory
- **THEN** its `data_dir` is `/p/data`

### Requirement: One profile reaches one state from every working directory

Loading the same consumer profile from any working directory, with an absolute or a relative `--config` path, SHALL use one `data_dir`, and therefore one wallet, one agreement and credential store, one policy watermark and one nonce journal (book lines 470, 472 and 650). Two spellings of the same location, such as `data` and `./data`, SHALL be treated as that one location.

#### Scenario: Two working directories see one wallet

- **GIVEN** a profile in `/p` with `"data_dir": "data"`
- **WHEN** `wallet create` runs from `/a` and `wallet address` runs from `/b`
- **THEN** both report the same address, and neither `/a` nor `/b` holds any state

#### Scenario: A relative --config reaches the same wallet

- **GIVEN** the wallet created above
- **WHEN** `wallet address` runs from `/` with `--config p/consumer.json`, from `/p` with `--config consumer.json` and with `--config ./consumer.json`, and from `/b` with `--config ../p/consumer.json`
- **THEN** each reports the same address

### Requirement: Configurations in one folder naming the same relative data_dir share one journal

A consumer profile and a provider configuration in the same directory that name the same relative `data_dir` SHALL resolve it to the same base directory, so that, on the same network, they use one wallet journal at `<data_dir>/inference/<hash-of-full-network-identity>/wallet-journal/transactions.sqlite3` (book line 470). Neither process SHALL append that suffix to the configured value or keep a separate copy of the journal.

#### Scenario: A consumer and a provider in one folder share the journal

- **GIVEN** `/p/consumer.json` and `/p/provider.json` on one network, both with `"data_dir": "data"`
- **WHEN** the consumer creates its wallet from working directory `/w` and the provider configuration is loaded
- **THEN** the provider's `data_dir` is `/p/data`
- **AND** the consumer's journal is the file at the provider's journal path under `/p/data`
- **AND** nothing is created under `/w`

### Requirement: State found only at the old working-directory location is refused, never ignored or moved

Before paths in consumer profiles were resolved against the profile, a relative path named a place in the process working directory. A client SHALL NOT silently start with empty state at the resolved location while this network's state exists only at that older location, and it SHALL NOT move, copy, merge or delete state on its own: changing paths without deliberate state preservation is not an upgrade (book lines 472, 652 and 673).

When a consumer profile is loaded, for each relative path-valued field whose resolved location differs from its working-directory interpretation:

- for `data_dir`, if the network-scoped directory `<data_dir>/inference/<hash-of-full-network-identity>` exists at the working-directory interpretation and does not exist at the resolved location;
- for `data_dir`, also if the consumer wallet key `<data_dir>/inference/<hash-of-full-network-identity>/consumer/wallet.key` exists at the working-directory interpretation and does not exist at the resolved location, even when the resolved network-scoped directory exists. Loading a profile creates that directory, an empty journal and the policy directory, so a command run earlier from another working directory can leave a location that holds no wallet; that location SHALL NOT hide the old wallet;
- for `network.rpc_bearer_file` and `dht.identity_file`, if the file exists at the working-directory interpretation and does not exist at the resolved location,

the load SHALL be refused before any directory or file is created. The refusal SHALL name the profile, the field, both locations as absolute paths, and the two deliberate remedies: set the field to the absolute path of the existing location, or stop every process using that state and move it to the resolved location, keeping the wallet key, nonce journal, agreements and policy watermark together. It SHALL state that nothing was created, moved or deleted.

The check SHALL examine only those two candidate locations per field; it SHALL NOT scan other directories. When none of these conditions holds, in particular when the resolved location already holds this network's state and a consumer wallet key, the resolved location SHALL be used and the working-directory location SHALL be left untouched.

#### Scenario: Old state only at the working-directory location is refused

- **GIVEN** this network's state, with wallet address A, under `/w/data`, created by an earlier client run from `/w` with the profile `/p/consumer.json` naming `"data_dir": "data"`
- **AND** no state for this network under `/p/data`
- **WHEN** `chiral --config /p/consumer.json wallet create` or `wallet address` runs from `/w`
- **THEN** the command fails with an error naming `/w/data`, `/p/data` and both network-scoped directories, and saying that nothing was created, moved or deleted
- **AND** no new wallet is created, `/p/data` does not exist, and every file under `/w` is unchanged

#### Scenario: A location created from another working directory does not hide the old wallet

- **GIVEN** this network's state with wallet address A under `/w/data`, created by an earlier client run from `/w` with the profile `/p/consumer.json` naming `"data_dir": "data"`
- **AND** a command such as `network show` later run with that profile from another working directory `/o`, which created `/p/data/inference/<network-hash>/` without a wallet key
- **WHEN** `chiral --config /p/consumer.json wallet create` or `wallet address` runs from `/w`
- **THEN** the command fails with an error naming `/w/data`, `/p/data` and the missing and existing wallet key paths, and saying that nothing was created, moved or deleted
- **AND** no second wallet is created, and every file under `/w` and `/p` is unchanged

#### Scenario: An absolute data_dir keeps the old state

- **GIVEN** the refused profile above
- **WHEN** the user sets `"data_dir": "/w/data"`
- **THEN** the profile loads from any working directory and reports address A

#### Scenario: A deliberate move keeps the wallet

- **GIVEN** the refused profile above
- **WHEN** the user moves `/w/data` to `/p/data`
- **THEN** the unchanged relative profile loads and reports address A

#### Scenario: The profile's own state wins over another working-directory copy

- **GIVEN** this network's state with address A under `/p/data` and other state with address B under `/w/data`
- **WHEN** the profile naming `"data_dir": "data"` is loaded from `/w`
- **THEN** it reports address A, and `/w/data` is unchanged

#### Scenario: A stranded bearer or identity file is refused

- **GIVEN** a profile in `/p` naming `"rpc_bearer_file": "rpc.token"` or a DHT `"identity_file": "peer.key"`, the file present in working directory `/w` and absent from `/p`
- **WHEN** the profile is loaded from `/w`
- **THEN** the load fails naming the field, `/w/<file>` and `/p/<file>`
- **AND** no file is created under `/p` and the file under `/w` is unchanged

### Requirement: The CLI and the desktop show a refused profile

A refused profile SHALL be reported where the user loaded it. The `chiral` CLI SHALL write the refusal to standard error, write nothing to standard output and exit with a nonzero status (`CLAUDE.md`: CLI stdout is machine-readable JSON and diagnostics belong on stderr). The desktop's Settings *Network profile* action SHALL show the refusal as an error and SHALL NOT mark the session configured (book lines 449 and 960); a profile loaded earlier in the same session SHALL stay loaded.

#### Scenario: The CLI reports a refusal on stderr

- **WHEN** a refused profile is given to any `chiral` command
- **THEN** standard output is empty, standard error carries the refusal, and the exit status is nonzero

#### Scenario: Desktop Settings shows a refusal

- **GIVEN** a desktop session with profile P1 loaded
- **WHEN** the user loads a refused profile P2 in Settings
- **THEN** Settings shows the refusal as an error, and P1 stays the loaded profile
