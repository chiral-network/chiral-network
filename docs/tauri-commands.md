# Tauri Command Reference

Local functionality inside the desktop app is exposed through Tauri commands and accessed from the Svelte frontend with `invoke(...)`. These commands run on the user’s machine; they are not part of the public HTTP/WebSocket API.

> **Usage Pattern**
> ```ts
> import { invoke } from '@tauri-apps/api/core';
> await invoke('<command-name>', { /* parameters */ });
> ```

## Account Session

### `create_chiral_account`
- **Parameters**: *(none)*
- **Returns**: `{ address: string; private_key: string }`
- **Description**: Generates a new Ethereum-compatible account, saves its private key in memory for the session, and marks it as the active account.

### `import_chiral_account`
- **Parameters**
  - `private_key: string`
- **Returns**: `{ address: string; private_key: string }`
- **Description**: Imports an existing account from a raw hex private key, storing it as the active session account.

### `has_active_account`
- **Parameters**: *(none)*
- **Returns**: `boolean`
- **Description**: Indicates whether an account is currently loaded in the desktop session.

### `logout`
- **Parameters**: *(none)*
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
- **Parameters**: *(none)*
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
- **Parameters**: *(none)*
- **Returns**: `void`
- **Description**: Stops the tracked geth process if it is running.

### `is_geth_running`
- **Parameters**: *(none)*
- **Returns**: `boolean`
- **Description**: Reports whether geth is currently active (either through the tracked child process or by probing the RPC port).

### `check_geth_binary`
- **Parameters**: *(none)*
- **Returns**: `boolean`
- **Description**: Checks whether the geth binary is already installed.

### `download_geth_binary`
- **Parameters**: *(none)*
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
- **Parameters**: *(none)*
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
  await invoke('start_miner', {
    address: account.address,
    threads: 4,
    data_dir: '/Users/me/.chiral/geth-data'
  });
  ```

### `stop_miner`
- **Parameters**: *(none)*
- **Returns**: `void`
- **Description**: Stops any active mining threads via `miner_stop`.

### `get_miner_status`
- **Parameters**: *(none)*
- **Returns**: `boolean` – `true` if mining is currently active.
- **Description**: Convenience check used to toggle the mining UI.

### `get_miner_hashrate`
- **Parameters**: *(none)*
- **Returns**: `string` – current hashrate reported by geth (e.g., `"125 MH/s"`).
- **Description**: Pulls the miner hashrate from the local RPC.

## Mining Insights

### `get_current_block`
- **Parameters**: *(none)*
- **Returns**: `number` – current block height.
- **Description**: Reads `eth_blockNumber` from the local geth RPC.

### `get_network_stats`
- **Parameters**: *(none)*
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
  const logs = await invoke<string[]>('get_miner_logs', {
    data_dir: '/Users/me/.chiral/geth-data',
    lines: 200
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

## Mining Pools *(Mock Data)*

These commands currently operate on in-memory mock data to support “progressive decentralization.” They do **not** contact live pool infrastructure.

### `discover_mining_pools`
- **Parameters**: *(none)*
- **Returns**: `MiningPool[]`
- **Description**: Combines predefined and user-created mock pools after a simulated discovery delay.

### `create_mining_pool`
- **Parameters**
  - `address: string`
  - `name: string`
  - `description: string`
  - `fee_percentage: number`
  - `min_payout: number`
  - `payment_method: string`
  - `region: string`
- **Returns**: `MiningPool`
- **Description**: Registers a mock pool owned by the caller and stores it in the in-memory directory.

### `join_mining_pool`
- **Parameters**
  - `pool_id: string`
  - `address: string`
- **Returns**: `JoinedPoolInfo`
- **Description**: Simulates joining a pool and produces placeholder stats. Errors if a pool is already “connected.”

### `leave_mining_pool`
- **Parameters**: *(none)*
- **Returns**: `void`
- **Description**: Clears the current mock pool membership with a simulated disconnect delay.

### `get_current_pool_info`
- **Parameters**: *(none)*
- **Returns**: `JoinedPoolInfo | null`
- **Description**: Returns the stored mock pool session if one exists.

### `get_pool_stats`
- **Parameters**: *(none)*
- **Returns**: `PoolStats | null`
- **Description**: Generates synthetic stats (shares, hashrate, payout) based on elapsed “mining time.”

### `update_pool_discovery`
- **Parameters**: *(none)*
- **Returns**: `void`
- **Description**: Mutates the mock pool list (miner counts, block times) to emulate new network data.
