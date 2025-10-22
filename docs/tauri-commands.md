# Tauri Command Reference

Local functionality inside the desktop app is exposed through Tauri commands and accessed from the Svelte frontend with `invoke(...)`. These commands run on the user’s machine; they are not part of the public HTTP/WebSocket API.

> **Usage Pattern**
> ```ts
> import { invoke } from '@tauri-apps/api/core';
> await invoke('<command-name>', { /* parameters */ });
> ```

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
- **Returns**: `[{ difficulty: string }, { hashrate: string }]` as a tuple.
- **Description**: Fetches network difficulty and aggregate hashrate for dashboard displays.

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
- **Returns**: `{ blocksFound: number; averageHashrate: number }`
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
- **Returns**: `Array<{ hash: string; timestamp: number; reward?: number }>`
- **Description**: Retrieves recent mined block metadata for history UIs.

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

