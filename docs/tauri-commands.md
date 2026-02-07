# Tauri Command Reference

Local functionality inside the desktop app is exposed through Tauri commands and accessed from the Svelte frontend with `invoke(...)`. These commands run on the user’s machine; they are not part of the public HTTP/WebSocket API.

> **Usage Pattern**
>
> ```ts
> import { invoke } from "@tauri-apps/api/core";
> await invoke("<command-name>", {
>   /* parameters */
> });
> ```

## Account Session

### `create_chiral_account`

- **Parameters**: _(none)_
- **Returns**: `{ address: string; private_key: string }`
- **Description**: Generates a new Ethereum-compatible account, saves its private key in memory for the session, and marks it as the active account.

### `import_chiral_account`

- **Parameters**
  - `private_key: string`
- **Returns**: `{ address: string; private_key: string }`
- **Description**: Imports an existing account from a raw hex private key, storing it as the active session account.

### `has_active_account`

- **Parameters**: _(none)_
- **Returns**: `boolean`
- **Description**: Indicates whether an account is currently loaded in the desktop session.

### `logout`

- **Parameters**: _(none)_
- **Returns**: `void`
- **Description**: Clears the in-memory active account and wipes the cached private key (including the copy held by the WebRTC service).

## Keystore Management

### `save_account_to_keystore`

- **Parameters**
  - `address: string`
  - `private_key: string`
  - `password: string`
- **Returns**: `void`
- **Description**: Encrypts and writes the account to the disk keystore file so it can be restored later.

### `load_account_from_keystore`

- **Parameters**
  - `address: string`
  - `password: string`
- **Returns**: `{ address: string; private_key: string }`
- **Description**: Decrypts the stored key material, activates the account in session, and updates dependent services (e.g., WebRTC).

### `list_keystore_accounts`

- **Parameters**: _(none)_
- **Returns**: `string[]`
- **Description**: Lists the addresses currently available in the keystore.

## Blockchain Node Lifecycle

### `start_geth_node`

- **Parameters**
  - `data_dir: string`
  - `rpc_url?: string`
- **Returns**: `void`
- **Description**: Launches the bundled geth process, reusing any cached miner address and overriding the RPC URL if provided.

### `stop_geth_node`

- **Parameters**: _(none)_
- **Returns**: `void`
- **Description**: Stops the tracked geth process if it is running.

### `is_geth_running`

- **Parameters**: _(none)_
- **Returns**: `boolean`
- **Description**: Reports whether geth is currently active (either through the tracked child process or by probing the RPC port).

### `check_geth_binary`

- **Parameters**: _(none)_
- **Returns**: `boolean`
- **Description**: Checks whether the geth binary is already installed.

### `download_geth_binary`

- **Parameters**: _(none)_
- **Returns**: `void`
- **Description**: Downloads or updates the geth binary. Emits `geth-download-progress` events with percentage updates while running.

### `get_geth_status`

- **Parameters**
  - `data_dir?: string`
  - `log_lines?: number`
- **Returns**: `{ installed: boolean; running: boolean; binary_path?: string; data_dir: string; data_dir_exists: boolean; log_path?: string; log_available: boolean; log_lines: number; version?: string; last_logs: string[]; last_updated: number }`
- **Description**: Aggregates the local node status, including whether the binary exists, current runtime state, latest log lines, and version information.

### `set_miner_address`

- **Parameters**
  - `address: string`
- **Returns**: `void`
- **Description**: Persists the desired miner/etherbase address so future geth launches reuse it.

### `get_network_peer_count`

- **Parameters**: _(none)_
- **Returns**: `number`
- **Description**: Returns the current peer count from the local geth node.

## Mining Control

### `start_miner`

- **Parameters**
  - `address: string`
  - `threads: number`
  - `data_dir: string`
- **Returns**: `void` (promise resolves on success)
- **Description**: Starts CPU mining against the bundled geth instance. The command caches the etherbase address, tries `miner_setEtherbase`, and if unsupported restarts geth with the new configuration before issuing `miner_start`.
- **Example**
  ```ts
  await invoke("start_miner", {
    address: account.address,
    threads: 4,
    data_dir: "/Users/me/.chiral/geth-data",
  });
  ```

### `stop_miner`

- **Parameters**: _(none)_
- **Returns**: `void`
- **Description**: Stops any active mining threads via `miner_stop`.

### `get_miner_status`

- **Parameters**: _(none)_
- **Returns**: `boolean` – `true` if mining is currently active.
- **Description**: Convenience check used to toggle the mining UI.

### `get_miner_hashrate`

- **Parameters**: _(none)_
- **Returns**: `string` – current hashrate reported by geth (e.g., `"125 MH/s"`).
- **Description**: Pulls the miner hashrate from the local RPC.

## Mining Insights

### `get_current_block`

- **Parameters**: _(none)_
- **Returns**: `number` – current block height.
- **Description**: Reads `eth_blockNumber` from the local geth RPC.

### `get_network_stats`

- **Parameters**: _(none)_
- **Returns**: `[string, string]` – tuple of `[difficulty, hashrate]`.
- **Description**: Fetches the current network difficulty and aggregate hashrate for dashboard displays.

### `get_miner_logs`

- **Parameters**
  - `data_dir: string`
  - `lines: number`
- **Returns**: `string[]` – newest log lines.
- **Description**: Tails the miner log located in the provided geth data directory.
- **Example**
  ```ts
  const logs = await invoke<string[]>("get_miner_logs", {
    data_dir: "/Users/me/.chiral/geth-data",
    lines: 200,
  });
  ```

### `get_miner_performance`

- **Parameters**
  - `data_dir: string`
- **Returns**: `[number, number]` – tuple of `[blocksFound, averageHashrate]`.
- **Description**: Parses miner logs to surface blocks found and average hashrate.

### `get_blocks_mined`

- **Parameters**
  - `address: string`
- **Returns**: `number` – total blocks credited to the address.
- **Description**: Queries the local node and caches the result briefly (500 ms) to avoid repeated heavy calls.

### `get_recent_mined_blocks_pub`

- **Parameters**
  - `address: string`
  - `lookback: number`
  - `limit: number`
- **Returns**: `Array<{ hash: string; timestamp: number; number: number; reward?: number; difficulty?: string; nonce?: string }>`
- **Description**: Retrieves recent mined block metadata (including optional difficulty/nonce info) for history UIs.

## Mining Pools _(Local + Persisted)_

These commands manage pool metadata locally and persist user-created pools to disk. Connectivity checks use TCP dial attempts against the pool’s `stratum+tcp://` endpoint. Pool stats are still simulated (shares, payout estimates) based on time connected.

### `discover_mining_pools`

- **Parameters**: _(none)_
- **Returns**: `MiningPool[]`
- **Description**: Combines predefined pools and user-created pools loaded from `user_pools.json` in the app data directory.

### `create_mining_pool`

- **Parameters**
  - `address: string`
  - `name: string`
  - `url: string` _(stratum+tcp://host:port)_
  - `description: string`
  - `fee_percentage: number`
  - `min_payout: number`
  - `payment_method: string`
  - `region: string`
- **Returns**: `MiningPool`
- **Description**: Registers a user pool owned by the caller and persists it to `user_pools.json`. URL format is validated.

### `join_mining_pool`

- **Parameters**
  - `pool_id: string`
  - `address: string`
- **Returns**: `JoinedPoolInfo`
- **Description**: Validates the pool URL and performs TCP connectivity checks (with retry). Returns simulated stats and errors if already connected.

### `leave_mining_pool`

- **Parameters**: _(none)_
- **Returns**: `void`
- **Description**: Clears the current mock pool membership with a simulated disconnect delay.

### `get_current_pool_info`

- **Parameters**: _(none)_
- **Returns**: `JoinedPoolInfo | null`
- **Description**: Returns the stored mock pool session if one exists.

### `get_pool_stats`

- **Parameters**: _(none)_
- **Returns**: `PoolStats | null`
- **Description**: Generates synthetic stats (shares, hashrate, payout) based on elapsed “mining time.”

### `update_pool_discovery`

- **Parameters**: _(none)_
- **Returns**: `void`
- **Description**: Mutates the mock pool list (miner counts, block times) to emulate new network data.

## DHT Node & Peer Management

### `start_dht_node`

- **Parameters**
  - `port: number`
  - `bootstrap_nodes: string[]`
  - `enable_autonat?: boolean` (defaults to `true`)
  - `autonat_probe_interval_secs?: number`
  - `autonat_servers?: string[]`
  - `proxy_address?: string` _(SOCKS5 endpoint)_
  - `is_bootstrap?: boolean`
  - `chunk_size_kb?: number`
  - `cache_size_mb?: number`
- **Returns**: `string` – the local libp2p peer ID.
- **Description**: Boots the libp2p/Kademlia node, wires up file-transfer and multi-source services, and starts emitting events (`dht_peer_*`, `nat_status_update`, `found_file`, etc.) to the frontend.

### `stop_dht_node`

- **Parameters**: _(none)_
- **Returns**: `void`
- **Description**: Shuts down the running DHT service and clears cached proxy state (emits `proxy_reset`).

### `stop_publishing_file`

- **Parameters**
  - `file_hash: string`
- **Returns**: `void`
- **Description**: Removes a previously published file advertisement from the DHT.

### `connect_to_peer`

- **Parameters**
  - `peer_address: string` _(libp2p multiaddr or host:port)_
- **Returns**: `void`
- **Description**: Dials the specified peer through the DHT transport stack.

### `get_dht_peer_count`

- **Parameters**: _(none)_
- **Returns**: `number`
- **Description**: Current number of connected peers; returns `0` when the DHT isn’t running.

### `get_dht_peer_id`

- **Parameters**: _(none)_
- **Returns**: `string | null`
- **Description**: The local peer ID if the node is active.

### `get_dht_connected_peers`

- **Parameters**: _(none)_
- **Returns**: `string[]`
- **Description**: List of peer IDs currently connected to this node.

### `get_dht_health`

- **Parameters**: _(none)_
- **Returns**: `DhtMetricsSnapshot | null`
- **Description**: Captures node health including peer counts, reachability, observed addresses, and related metrics.

### `get_dht_events`

- **Parameters**: _(none)_
- **Returns**: `string[]`
- **Description**: Drains up to 100 queued DHT events. Each entry is a colon-delimited token such as `peer_discovered:<peer>:<addresses>` or JSON payloads for file/reputation events.

### `test_backend_connection`

- **Parameters**: _(none)_
- **Returns**: `string`
- **Description**: Smoke test; resolves with `"DHT service is running"` when the node is alive, otherwise errors.

### `get_bootstrap_nodes_command`

- **Parameters**: _(none)_
- **Returns**: `string[]`
- **Description**: Returns the default bootstrap multiaddresses bundled with the app.

## Stream Authentication & Key Exchange

### `create_auth_session`

- **Parameters**
  - `session_id: string`
  - `hmac_key: number[]`
- **Returns**: `void`
- **Description**: Registers a new authenticated stream session for the provided ID.

### `verify_stream_auth`

- **Parameters**
  - `session_id: string`
  - `auth_message: AuthMessage`
- **Returns**: `boolean`
- **Description**: Validates the message signature/sequence for an active session.

### `generate_hmac_key`

- **Parameters**: _(none)_
- **Returns**: `number[]`
- **Description**: Generates a random 32-byte HMAC key.

### `cleanup_auth_sessions`

- **Parameters**: _(none)_
- **Returns**: `void`
- **Description**: Prunes expired sessions and key exchanges.

### `initiate_hmac_key_exchange`

- **Parameters**
  - `initiator_peer_id: string`
  - `target_peer_id: string`
  - `session_id: string`
- **Returns**: `HmacKeyExchangeRequest`
- **Description**: Begins an X25519-based key exchange and returns the request payload to send to the peer.

### `respond_to_hmac_key_exchange`

- **Parameters**
  - `request: HmacKeyExchangeRequest`
  - `responder_peer_id: string`
- **Returns**: `HmacKeyExchangeResponse`
- **Description**: Responds to an incoming exchange request with the responder’s public material and confirmation data.

### `confirm_hmac_key_exchange`

- **Parameters**
  - `response: HmacKeyExchangeResponse`
  - `initiator_peer_id: string`
- **Returns**: `HmacKeyExchangeConfirmation`
- **Description**: Completes the initiator side of the exchange and produces a confirmation payload.

### `finalize_hmac_key_exchange`

- **Parameters**
  - `confirmation: HmacKeyExchangeConfirmation`
  - `responder_peer_id: string`
- **Returns**: `void`
- **Description**: Finalizes the exchange on the responder side and stores the derived session key.

### `get_hmac_exchange_status`

- **Parameters**
  - `exchange_id: string`
- **Returns**: `string | null` _(human-readable state such as `"Initiated"` or `"Completed"`)_
- **Description**: Looks up the lifecycle state for an exchange.

### `get_active_hmac_exchanges`

- **Parameters**: _(none)_
- **Returns**: `string[]`
- **Description**: Lists exchange IDs still tracked by the service.

## File Transfer Service

### `start_file_transfer_service`

- **Parameters**: _(none)_
- **Returns**: `void`
- **Description**: Spins up the local file-transfer daemon, WebRTC service, and multi-source scheduler (pumps events to `file_transfer_*` and `multi_source_*` channels).

### `upload_file_to_network`

- **Parameters**
  - `file_path: string`
- **Returns**: `void`
- **Description**: Uploads a file using the active account credentials, stores it locally for seeding, and publishes metadata to the DHT when available.

### `download_blocks_from_network`

- **Parameters**
  - `file_metadata: FileMetadata`
  - `download_path: string`
- **Returns**: `void`
- **Description**: Retrieves file blocks via the DHT blockstore and writes them to `download_path`.

### `download_file_from_network`

- **Parameters**
  - `file_hash: string`
  - `output_path: string`
- **Returns**: `string` – status message describing how the download was initiated.
- **Description**: Searches the DHT for metadata, negotiates WebRTC with a seeder, and triggers a P2P download (returns early with diagnostic text; progress arrives via events).

### `show_in_folder`

- **Parameters**
  - `path: string`
- **Returns**: `void`
- **Description**: Opens the OS file manager revealing the provided path.

### `save_temp_file_for_upload`

- **Parameters**
  - `file_name: string`
  - `file_data: number[]`
- **Returns**: `string` – absolute path to the temp file.
- **Description**: Persists dropped files to a temporary location before upload.

### `start_streaming_upload`

- **Parameters**
  - `file_name: string`
  - `file_size: number`
- **Returns**: `string` – streaming upload session ID.
- **Description**: Begins a chunked upload session (requires an active account and running DHT).

### `upload_file_chunk`

- **Parameters**
  - `upload_id: string`
  - `chunk_data: number[]`
  - `chunk_index: number`
  - `is_last_chunk: boolean`
- **Returns**: `string | null` – the root CID/Merkle hash when the final chunk finishes; otherwise `null`.
- **Description**: Streams chunk data, stores blocks in Bitswap, and assembles metadata when the upload completes.

### `cancel_streaming_upload`

- **Parameters**
  - `upload_id: string`
- **Returns**: `void`
- **Description**: Removes a pending streaming session.

### `write_file`

- **Parameters**
  - `path: string`
  - `contents: number[]`
- **Returns**: `void`
- **Description**: Writes raw bytes to disk (utility used by various features).

### `get_file_transfer_events`

- **Parameters**: _(none)_
- **Returns**: `string[]`
- **Description**: Drains recent file-transfer events (upload/download notifications, errors, download attempt JSON blobs).

### `get_download_metrics`

- **Parameters**: _(none)_
- **Returns**: `DownloadMetricsSnapshot`
- **Description**: Summarizes counts of successful/failed/retried downloads plus the last 20 attempt snapshots.

### `get_available_storage`

- **Parameters**: _(none)_
- **Returns**: `number`
- **Description**: Estimated free disk space (in GB) using platform-specific heuristics.

### `get_file_data`

- **Parameters**
  - `file_hash: string`
- **Returns**: `string` – base64-encoded file data stored locally for seeding.
- **Description**: Reads cached file bytes via the file-transfer service.

### `store_file_data`

- **Parameters**
  - `file_hash: string`
  - `file_name: string`
  - `file_data: number[]`
- **Returns**: `void`
- **Description**: Adds or updates a locally stored file for seeding.

## Multi-Source Downloads & Proxy Optimization

### `start_multi_source_download`

- **Parameters**
  - `file_hash: string`
  - `output_path: string`
  - `max_peers?: number`
  - `chunk_size?: number`
- **Returns**: `string` – confirmation message.
- **Description**: Starts or resumes a multi-peer download; events arrive on `multi_source_*` channels.

### `cancel_multi_source_download`

- **Parameters**
  - `file_hash: string`
- **Returns**: `void`
- **Description**: Cancels an active multi-source session.

### `get_multi_source_progress`

- **Parameters**
  - `file_hash: string`
- **Returns**: `MultiSourceProgress | null`
- **Description**: Snapshot of per-peer assignment, speed, ETA, and completion stats for a running download.

### `update_proxy_latency`

- **Parameters**
  - `proxy_id: string`
  - `latency_ms?: number`
- **Returns**: `void`
- **Description**: Updates proxy latency stats used for routing decisions.

### `get_proxy_optimization_status`

- **Parameters**: _(none)_
- **Returns**: `Record<string, unknown>` – JSON with `enabled`, optional `best_proxy`, and status text.
- **Description**: Reports whether proxy routing should be enabled based on measured latencies.

### `download_file_multi_source`

- **Parameters**
  - `file_hash: string`
  - `output_path: string`
  - `prefer_multi_source?: boolean`
  - `max_peers?: number`
- **Returns**: `string` – message describing how the download was initiated.
- **Description**: Attempts a multi-source download and falls back to single-source behavior if the service is unavailable.

## ed2k Commands

These commands expose functionality for interacting with the eDonkey (ed2k) network, managing file sources, and testing connections.

### `add_ed2k_source`

Adds an ed2k link as a potential source for a given file hash.

* **Parameters**:
    * `file_hash: string`
    * `ed2k_link: string`
* **Returns**: `void`
* **Description**: Parses the ed2k link, creates an `Ed2kSourceInfo` struct, and adds it to the `FileMetadata` for the specified `file_hash`.

### `list_ed2k_sources`

Retrieves all registered ed2k sources for a given file hash.

* **Parameters**:
    * `file_hash: string`
* **Returns**: `Ed2kSourceInfo[]`
* **Description**: Fetches the `FileMetadata` and returns the `ed2k_sources` array, or an empty array if none exist.

### `remove_ed2k_source`

Removes a specific ed2k source from a file's metadata based on its server URL.

* **Parameters**:
    * `file_hash: string`
    * `server_url: string`
* **Returns**: `void`
* **Description**: Finds and removes the `Ed2kSourceInfo` entry that matches the `server_url` for the given `file_hash`.

### `test_ed2k_connection`

Attempts to connect to a given ed2k server to check its status.

* **Parameters**:
    * `server_url: string`
* **Returns**: `Ed2kServerInfo`
* **Description**: Uses the `ed2k_client` to connect to the server and request its info. Throws an error if the connection fails.

### `search_ed2k_file`

Searches the ed2k network for a file based on a query string.

* **Parameters**:
    * `query: string`
    * `server_url?: string` (Optional: uses a default server if not provided)
* **Returns**: `Ed2kSearchResult[]`
* **Description**: Connects to an ed2k server and performs a file search.

### `get_ed2k_download_status`

Gets the current download status for a file from ed2k sources.

* **Parameters**:
    * `file_hash: string`
* **Returns**: `Ed2kDownloadStatus`
* **Description**: Retrieves the download status for an ed2k file (placeholder, as full download logic is in `multi_source_download`).

### `parse_ed2k_link`

A utility function to parse an ed2k link string into its structured `Ed2kSourceInfo` format.

* **Parameters**:
    * `ed2k_link: string`
* **Returns**: `Ed2kSourceInfo`
* **Description**: Exposes the backend's `Ed2kSourceInfo::from_ed2k_link` parser to the frontend.

## Encryption & Packaging Helpers

### `encrypt_file_with_password`

- **Parameters**
  - `input_path: string`
  - `output_path: string`
  - `password: string`
- **Returns**: `EncryptionInfo`
- **Description**: Writes an encrypted copy of the input file using AES-256-GCM (with PBKDF2-derived key).

### `decrypt_file_with_password`

- **Parameters**
  - `input_path: string`
  - `output_path: string`
  - `password: string`
  - `encryption_info: EncryptionInfo`
- **Returns**: `number` – decrypted file size in bytes.
- **Description**: Restores a password-protected file.

### `encrypt_file_for_upload`

- **Parameters**
  - `input_path: string`
  - `password?: string`
- **Returns**: `[string, EncryptionInfo]` – path to the encrypted file plus metadata.
- **Description**: Produces an `.enc` file ready for uploading, using either a provided password or a random key.

### `encrypt_file_for_self_upload`

- **Parameters**
  - `file_path: string`
- **Returns**: `FileManifestForJs` _(fields: `merkle_root`, `chunks: ChunkInfo[]`, `encrypted_key_bundle: string`)_
- **Description**: Chunks and encrypts a file for seeding using the active account’s keypair and stores chunk data under the app directory.

### `encrypt_file_for_recipient`

- **Parameters**
  - `file_path: string`
  - `recipient_public_key?: string`
- **Returns**: `FileManifestForJs`
- **Description**: Same as above but allows targeting a specific recipient’s X25519 public key. Defaults to self if omitted.

### `upload_and_publish_file`

- **Parameters**
  - `file_path: string`
  - `file_name?: string`
  - `recipient_public_key?: string`
- **Returns**: `{ merkle_root: string; file_name: string; file_size: number; is_encrypted: boolean; peer_id: string; version: number }`
- **Description**: All-in-one helper that encrypts, publishes to the DHT, and stores file metadata for the active account.

### `decrypt_and_reassemble_file`

- **Parameters**
  - `manifest_js: FileManifestForJs`
  - `output_path: string`
- **Returns**: `void`
- **Description**: Reassembles encrypted chunks into `output_path` using the active account’s private key (runs work in a blocking task).

## File Discovery & Metadata

### `search_file_metadata`

- **Parameters**
  - `file_hash: string`
  - `timeout_ms?: number`
- **Returns**: `void`
- **Description**: Triggers an asynchronous metadata search in the DHT (results arrive via `found_file` events).

### `get_file_seeders`

- **Parameters**
  - `file_hash: string`
- **Returns**: `string[]`
- **Description**: Lists peer IDs currently advertising the file.

## Analytics & Diagnostics

### `get_bandwidth_stats`

- **Parameters**: _(none)_
- **Returns**: `BandwidthStats`
- **Description**: Current upload/download totals and last-updated timestamp.

### `get_bandwidth_history`

- **Parameters**
  - `limit?: number`
- **Returns**: `BandwidthDataPoint[]`
- **Description**: Historical bandwidth samples (timestamped rates) up to the optional limit.

### `get_performance_metrics`

- **Parameters**: _(none)_
- **Returns**: `PerformanceMetrics`
- **Description**: Aggregate transfer speeds, totals, and latency estimates.

### `get_network_activity`

- **Parameters**: _(none)_
- **Returns**: `NetworkActivity`
- **Description**: Counts of active/queued/completed transfers and peer totals.

### `get_resource_contribution`

- **Parameters**: _(none)_
- **Returns**: `ResourceContribution`
- **Description**: Storage/bandwidth contributed, shared file count, seedtime hours, and reputation score.

### `get_contribution_history`

- **Parameters**
  - `limit?: number`
- **Returns**: `ContributionDataPoint[]`
- **Description**: Historical contribution snapshots (bandwidth, storage, files seeded).

### `reset_analytics`

- **Parameters**: _(none)_
- **Returns**: `void`
- **Description**: Clears analytics counters and history.

### `get_cpu_temperature`

- **Parameters**: _(none)_
- **Returns**: `number | null`
- **Description**: Uses platform-specific probes (sysinfo, WMI, sensors, thermal zones) to return a smoothed CPU temperature in °C when available.

## FTP Operations

### `list_ftp_directory`

- **Parameters**
  - `url: string` - FTP directory URL (e.g., `ftp://ftp.example.com/pub/`)
  - `username?: string` - FTP username (optional for anonymous FTP)
  - `password?: string` - FTP password (optional for anonymous FTP)
  - `use_ftps: boolean` - Enable FTPS (FTP over TLS)
  - `passive_mode: boolean` - Use passive mode (recommended for NAT)
- **Returns**: `FtpFileEntry[]`
- **Description**: Lists files and directories in the specified FTP directory. Returns array of entries with name, size, type (file/directory), permissions, and modification date.

### `delete_ftp_file`

- **Parameters**
  - `url: string` - FTP file/directory URL to delete
  - `username?: string` - FTP username
  - `password?: string` - FTP password
  - `use_ftps: boolean` - Enable FTPS
  - `passive_mode: boolean` - Use passive mode
- **Returns**: `void`
- **Description**: Deletes a file or directory on the FTP server. Automatically detects whether the target is a file or directory.

### `rename_ftp_file`

- **Parameters**
  - `url: string` - FTP file/directory URL to rename
  - `new_name: string` - New name for the file/directory
  - `username?: string` - FTP username
  - `password?: string` - FTP password
  - `use_ftps: boolean` - Enable FTPS
  - `passive_mode: boolean` - Use passive mode
- **Returns**: `void`
- **Description**: Renames a file or directory on the FTP server.

### `create_ftp_directory`

- **Parameters**
  - `url: string` - FTP directory URL to create
  - `username?: string` - FTP username
  - `password?: string` - FTP password
  - `use_ftps: boolean` - Enable FTPS
  - `passive_mode: boolean` - Use passive mode
- **Returns**: `void`
- **Description**: Creates a new directory on the FTP server.

### `test_ftp_connection`

- **Parameters**
  - `url: string` - FTP server URL to test
  - `username?: string` - FTP username
  - `password?: string` - FTP password
  - `use_ftps: boolean` - Enable FTPS
  - `passive_mode: boolean` - Use passive mode
- **Returns**: `void`
- **Description**: Tests the connection to an FTP server. Throws an error if connection fails with detailed error message.

### `upload_to_external_ftp`

- **Parameters**
  - `file_path: string` - Local file path to upload
  - `ftp_url: string` - FTP destination URL
  - `username?: string` - FTP username
  - `password?: string` - FTP password
  - `use_ftps: boolean` - Enable FTPS
  - `passive_mode: boolean` - Use passive mode
- **Returns**: `string` - URL of uploaded file
- **Description**: Uploads a file to an external FTP server. Returns the full FTP URL of the uploaded file.

### `start_ftp_download`

- **Parameters**
  - `url: string` - FTP file URL to download
  - `output_path: string` - Local output path
  - `username?: string` - FTP username
  - `password?: string` - FTP password
- **Returns**: `string` - Transfer ID
- **Description**: Starts an FTP download with progress tracking. Supports automatic resume of interrupted downloads.

## FTP Bookmarks

### `load_ftp_bookmarks`

- **Parameters**: _(none)_
- **Returns**: `FtpBookmark[]`
- **Description**: Loads all saved FTP server bookmarks from the config directory.

### `add_ftp_bookmark`

- **Parameters**
  - `bookmark: FtpBookmark` - Bookmark object with server configuration
- **Returns**: `FtpBookmark[]` - Updated bookmarks list
- **Description**: Adds a new FTP server bookmark. Returns the updated list of all bookmarks.

### `update_ftp_bookmark`

- **Parameters**
  - `bookmark: FtpBookmark` - Updated bookmark object
- **Returns**: `FtpBookmark[]` - Updated bookmarks list
- **Description**: Updates an existing FTP server bookmark by ID. Returns the updated list of all bookmarks.

### `delete_ftp_bookmark`

- **Parameters**
  - `id: string` - Bookmark ID to delete
- **Returns**: `FtpBookmark[]` - Updated bookmarks list
- **Description**: Deletes an FTP server bookmark by ID. Returns the updated list of all bookmarks.

### `search_ftp_bookmarks`

- **Parameters**
  - `query: string` - Search query
- **Returns**: `FtpBookmark[]` - Matching bookmarks
- **Description**: Searches FTP bookmarks by name, URL, tags, or notes. Returns all matching bookmarks.

### `record_ftp_bookmark_usage`

- **Parameters**
  - `id: string` - Bookmark ID
- **Returns**: `void`
- **Description**: Records that a bookmark was used. Increments use count and updates last used timestamp.

## Type Definitions

### `FtpFileEntry`

```typescript
interface FtpFileEntry {
  name: string;           // File or directory name
  size: number;           // Size in bytes
  is_directory: boolean;  // True if directory, false if file
  modified: string | null; // Modification date string
  permissions: string | null; // Unix-style permissions (e.g., "-rw-r--r--")
}
```

### `FtpBookmark`

```typescript
interface FtpBookmark {
  id: string;                      // Unique identifier
  name: string;                    // User-friendly name
  url: string;                     // FTP server URL
  username: string | null;         // FTP username
  encrypted_password: string | null; // Encrypted password
  use_ftps: boolean;               // Enable FTPS
  passive_mode: boolean;           // Use passive mode
  port: number | null;             // Custom port (null = default 21)
  notes: string | null;            // User notes
  tags: string[];                  // Tags for categorization
  last_used: number | null;        // Unix timestamp
  use_count: number;               // Number of times used
}
```
