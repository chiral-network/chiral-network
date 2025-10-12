# Chiral Network Test Suite Documentation

Complete guide to all tests in the project: what they test, where the code is based, and how the architecture works.

---

## Test Overview

| Test File | Tests | Status | Framework | Focus Area |
|-----------|-------|--------|-----------|------------|
| **wallet.test.ts** | 19 | ✅ All passing | Vitest | Account & wallet management |
| **transactions.test.ts** | 23 | ✅ All passing | Vitest | Transaction state & calculations |
| **mining.test.ts** | 34 | ✅ All passing | Vitest | Mining state management |
| **multi-source-download.test.mjs** | 25 | ✅ All passing | Node.js | P2P download algorithms |
| **signalingService.test.ts** | 14 | ❌ 9 failing | Vitest | WebRTC signaling |
| **uploadHelpers.test.mjs** | ~8 | ❌ Not running | Node.js | Upload validation |
| **peerSelection.test.mjs** | ~7 | ❌ Not running | Node.js | Peer selection logic |
| **dhtHelpers.test.mjs** | ~2 | ❌ Not running | Node.js | DHT utilities |
| **signaling.\*.test.mjs** | Various | ❌ Not running | Node.js | Signaling protocols |
| **TOTAL** | **115+** | **106 passing, 9 failing** | | |

---

## Architecture: Dual-Environment System

Chiral Network operates in **two modes** to enable development and testing:

### 1. Desktop Mode (Tauri - Production)
- Real Ethereum Classic blockchain via geth node
- Actual cryptocurrency mining with proof-of-work
- Real blockchain transactions with digital signatures
- File system access for P2P file sharing
- Full Rust backend integration

### 2. Demo Mode (Non-Tauri - Development/Testing)
- Mock accounts with random but valid ETC addresses
- Simulated mining state (UI updates without actual hashing)
- In-memory transaction history
- No blockchain or file system dependencies
- Perfect for unit testing and local development

### Key Detection

```typescript
// wallet.ts line 59
this.isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
```

This check determines which mode methods run in.

---

## Core Svelte Stores (State Management)

All application state lives in Svelte stores (`src/lib/stores.ts`):

| Store | Line | Purpose | Requires Tauri? |
|-------|------|---------|-----------------|
| `transactions` | 274 | Transaction history array | ❌ No |
| `miningState` | 378-389 | Mining config & stats | ❌ No |
| `wallet` | 272 | Account balance & metadata | ❌ No |
| `etcAccount` | 346 | Current ETC account | ❌ No |
| `totalSpent` | 398-402 | Derived total sent | ❌ No |
| `totalEarned` | 393-396 | Derived mining rewards | ❌ No |

**Important**: Stores are pure JavaScript data structures. They work in BOTH modes. Only **populating** stores from blockchain requires Tauri.

---

## 1. wallet.test.ts (19 tests - ✅ ALL PASSING)

### What It Tests
Complete Wallet Service functionality for Ethereum Classic account management.

### Source Code
- **Primary**: `src/lib/wallet.ts` - WalletService class (lines 50-501)
- **Stores**: `src/lib/stores.ts` - etcAccount (346), wallet (272), transactions (274)

### Test Coverage Details

#### Account Creation (4 tests)
**Based on**: `createAccount()` (lines 253-268) → `createDemoAccount()` (lines 483-488)

- **`should create new demo account with valid Ethereum address format`**
  - Generates 0x-prefixed 40-character hex addresses (20 bytes, ETC standard)
  - Generates 0x-prefixed 64-character hex private keys (32 bytes)
  - Validates format: `/^0x[0-9a-f]{40}$/` for address, `/^0x[0-9a-f]{64}$/` for key

- **`should persist created account to etcAccount store`**
  - Calls `setActiveAccount()` (lines 437-449)
  - Updates `etcAccount` Svelte store
  - Makes account immediately available to UI components via reactivity

- **`should update wallet store with new account address`**
  - Updates `wallet.address` field
  - Syncs account info across stores

- **`should clear existing transactions when creating new account`**
  - Calls `transactions.set([])` (lines 256, 266)
  - Prevents mixing transaction histories between accounts

#### Account Import (5 tests)
**Based on**: `importAccount()` (lines 270-291)

- **`should import account with 0x-prefixed private key`**
  - Accepts standard Ethereum private key format
  - In demo mode: stores key without cryptographic validation
  - In Tauri mode: calls `invoke('import_chiral_account')` for validation

- **`should auto-prefix private key with 0x if missing`**
  - Normalizes user input (lines 485-487 in `createDemoAccount`)
  - Handles keys pasted from various wallets

- **`should reject empty private key`**
  - Validation at line 271-273
  - Throws: "Private key is required"

- **`should reject whitespace-only private key`**
  - Uses `privateKey?.trim()` check
  - Prevents accidental whitespace submission

- **`should set imported account as active in stores`**
  - Same `setActiveAccount()` flow as creation
  - Immediately makes account usable

#### Wallet Export (3 tests)
**Based on**: `exportSnapshot()` (lines 371-384)

- **`should export wallet snapshot without private key by default`**
  - Creates JSON-serializable backup
  - Includes: address, balance, pendingTransactions, totalEarned, totalSpent
  - Excludes: privateKey (unless explicitly requested)
  - Use case: Sharing wallet status or balance history safely

- **`should include private key when explicitly requested`**
  - Pass `{ includePrivateKey: true }` option
  - Returns full backup including sensitive key
  - Use case: Migrating wallet to new device

- **`should export wallet snapshot with undefined address when no account set`**
  - Graceful handling of edge case
  - Can export wallet state even without active account

#### Transaction Sending (2 tests)
**Based on**: `sendTransaction()` (lines 293-328)

- **`should reject transaction when no active account`**
  - Guards against sending from null account (lines 294-297)
  - Throws: "No active account"

- **`should reject transaction in demo mode (non-Tauri environment)`**
  - Checks `!this.isTauri` (lines 298-300)
  - Throws: "Transactions are only available in the desktop app"
  - Rationale: Demo mode can't sign real transactions without Rust cryptography

#### Balance/Transaction Refresh (2 tests)
**Based on**: `refreshBalance()` (176-239), `refreshTransactions()` (147-174)

- **`should safely skip balance refresh in demo mode`**
  - Early return at lines 178-180: `if (!account || !this.isTauri) return;`
  - No-op in test environment, prevents errors

- **`should safely skip transaction refresh in demo mode`**
  - Early return at lines 149-151
  - Full implementation queries geth node via `invoke('get_recent_mined_blocks_pub')`

#### Initialization & Lifecycle (3 tests)

- **`should initialize wallet service in demo mode`**
  - Constructor sets `isTauri = false` in test environment
  - `isDesktopEnvironment()` returns false

- **`should handle multiple initialize calls safely`**
  - Uses `initialized` flag (line 51) to prevent double-init
  - Idempotent: safe to call multiple times

- **`should shutdown cleanly without errors`**
  - Cleans up polling intervals (lines 87-90)
  - Unsubscribes from stores (lines 91-94)
  - Clears seen hashes (line 96)

### Why Demo Mode Works
`WalletService` intelligently falls back to demo account generation when Tauri is unavailable. This enables:
- Unit testing without blockchain
- Local development without running geth
- Web preview deployments (though transactions won't work)

---

## 2. transactions.test.ts (23 tests - ✅ ALL PASSING)

### What It Tests
Transaction store operations, filtering, sorting, and amount calculations.

### Source Code
- **Type**: `src/lib/stores.ts` lines 128-137 (Transaction interface)
- **Store**: `src/lib/stores.ts` line 274 (`transactions` writable store)
- **Derived**: `src/lib/stores.ts` lines 398-402 (`totalSpent` derived store)

### Architecture
```typescript
export const transactions = writable<Transaction[]>(dummyTransactions);
```

This is a **Svelte writable store** - an in-memory array that triggers UI updates on changes. Works in ALL environments.

**How it's populated**:
- Desktop mode: `WalletService.refreshTransactions()` fetches from blockchain
- Demo mode: Manually added for testing/development

### Test Coverage Details

#### Store Operations (3 tests)
- **`should initialize with empty array`** - Basic store creation
- **`should add new transaction`** - Single transaction insertion
- **`should handle multiple transactions`** - Array operations with 3+ transactions

#### Transaction Types (2 tests)
- **`should handle received transactions`**
  - Type: "received"
  - Has `from` address (sender)
  - No `to` address

- **`should handle sent transactions`**
  - Type: "sent"
  - Has `to` address (recipient)
  - No `from` address

#### Transaction Status (3 tests)
- **`should handle pending transactions`** - Status: "pending"
- **`should handle completed transactions`** - Status: "completed"
- **`should update transaction status from pending to completed`**
  - Uses `.update()` and `.map()` to modify specific transaction
  - Simulates blockchain confirmation

#### Filtering (4 tests)
**Test Setup**: 4 mixed transactions (2 received, 2 sent, 3 completed, 1 pending)

- **`should filter received transactions`**
  - Filter: `tx.type === "received"`
  - Expects: 2 results

- **`should filter sent transactions`**
  - Filter: `tx.type === "sent"`
  - Expects: 2 results

- **`should filter pending transactions`**
  - Filter: `tx.status === "pending"`
  - Expects: 1 result (id: 4)

- **`should filter completed transactions`**
  - Filter: `tx.status === "completed"`
  - Expects: 3 results

#### Amount Calculations (2 tests)
- **`should calculate total received amount`**
  - Manually using `.filter()` and `.reduce()`
  - Test data: 100 + 50 = 150

- **`should calculate total sent amount using derived store`**
  - Uses `totalSpent` derived store (lines 398-402)
  - Auto-updates when transactions change
  - Test data: 10 + 25 = 35

#### Date Handling (2 tests)
- **`should maintain transaction date`**
  - Dates stored as `Date` objects
  - Test: `new Date("2024-01-15T10:30:00")`

- **`should sort transactions by date`**
  - Sorts by `date.getTime()` (milliseconds)
  - Descending order (newest first)

#### Wallet Integration (3 tests)
- **`should update wallet balance after transaction`**
  - Updates `wallet.balance` field
  - Test: 1000 + 50 = 1050

- **`should decrement balance for sent transactions`**
  - Test: 1000 - 100 = 900

- **`should track pending transactions count`**
  - Updates `wallet.pendingTransactions`

#### Edge Cases (4 tests)
- **`should handle zero amount transactions`** - Amount: 0
- **`should handle large amounts`** - Amount: 999999999.99
- **`should handle empty description`** - Description: ""
- **`should maintain transaction order when adding to beginning`**
  - Tests array prepending: `[newTx, ...existing]`

### Why No Tauri Required
`transactions` is just an array in memory. The **population** logic (WalletService) requires Tauri, but the store itself is environment-agnostic.

---

## 3. mining.test.ts (34 tests - ✅ ALL PASSING)

### What It Tests
Mining state management, configuration, progress tracking, and reward calculations.

### Source Code
- **Interface**: `src/lib/stores.ts` lines 365-376 (MiningState interface)
- **Store**: `src/lib/stores.ts` lines 378-389 (`miningState` writable store)
- **Progress**: `src/lib/stores.ts` line 391 (`miningProgress` store)
- **Derived**: `src/lib/stores.ts` lines 393-396 (`totalEarned` derived store)

### The Mining Architecture Explained

**Common Misconception**: "Mining requires Tauri/desktop environment"

**Reality**: Mining **state** and mining **execution** are separate:

#### Mining State (Tested Here)
```typescript
export const miningState = writable<MiningState>({
  isMining: false,        // Boolean flag
  hashRate: "0 H/s",      // Display string
  totalRewards: 0,        // Number
  blocksFound: 0,         // Number
  activeThreads: 1,       // Configuration
  minerIntensity: 50,     // Configuration
  recentBlocks: [],       // Block history
  miningHistory: [],      // Chart data
});
```

This is **pure JavaScript data** - works everywhere.

#### Mining Execution (NOT Tested Here)
- Proof-of-work hashing (SHA-256, finding nonces)
- CPU/GPU utilization
- Geth node integration
- Blockchain block submission

This runs in **Rust backend** and requires Tauri.

### How It Works in Each Mode

**Desktop App (Tauri)**:
```
User clicks "Start Mining" button
  → UI: miningState.update(s => ({ ...s, isMining: true }))
  → Rust backend: Starts mining thread with actual CPU hashing
  → Mining loop: Finds valid block after 10 minutes of work
  → Rust: Calls miningState.update() with new block data
  → UI: Automatically re-renders via Svelte reactivity
```

**Tests (Non-Tauri)**:
```typescript
// Directly manipulate state
miningState.update(s => ({ ...s, isMining: true }));

// Manually add mock block
miningState.update(s => ({
  ...s,
  recentBlocks: [{ hash: "0xabc", reward: 2, ...}],
  totalRewards: 2,
  blocksFound: 1
}));

// Verify calculations work
expect(get(totalEarned)).toBe(2);
```

### Test Coverage Details

#### Initialization (2 tests)
- **`should initialize with default values`**
  - All fields set to defaults from lines 378-389
  - `isMining: false`, `hashRate: "0 H/s"`, etc.

- **`should have undefined session start time initially`**
  - `sessionStartTime?: number` is optional
  - Undefined until mining starts

#### State Updates (4 tests)
- **`should toggle mining state`** - `isMining`: false → true → false
- **`should update hash rate`** - String format: "1500 H/s", "15.5 MH/s"
- **`should update total rewards`** - Number field increments
- **`should update blocks found`** - Counter increments

#### Configuration (3 tests)
- **`should update active threads`** - Range: 0-16 (depending on CPU cores)
- **`should update miner intensity`** - Range: 0-100 (percentage of CPU usage)
- **`should change selected pool`** - Options: "solo", "pool1", "pool2", etc.

#### Session Tracking (2 tests)
- **`should set session start time when mining starts`** - Stores `Date.now()` timestamp
- **`should clear session start time when mining stops`** - Set to `undefined` on stop

#### Recent Blocks (4 tests)
- **`should add blocks to recent blocks list`**
  - Block structure: `{ id, hash, reward, timestamp, difficulty, nonce }`

- **`should maintain multiple blocks`** - Array of block objects

- **`should limit recent blocks to 50`**
  - Implementation: `.slice(0, 50)` after adding
  - Prevents unbounded memory growth

- **`should calculate rewards from blocks`**
  - Formula: `blocks.reduce((sum, block) => sum + block.reward, 0)`

#### Mining History (3 tests)
- **`should track hash rate history`** - For charts: `{ timestamp, hashRate, power }`
- **`should maintain multiple history points`** - Time series data
- **`should track power consumption over time`** - Average calculation test

#### Progress (4 tests)
- **`should initialize progress at zero`** - `{ cumulative: 0, lastBlock: 0 }`
- **`should update cumulative progress`** - Tracks overall progress
- **`should reset last block progress on new block`** - `lastBlock` resets to 0 when block found
- **`should increment progress over time`** - Simulates 10 increments of 10% each

#### Derived Stores (2 tests)
- **`should calculate total earned from mining state`**
  - `totalEarned = miningState.totalRewards`
  - Lines 393-396

- **`should update when mining rewards change`** - Reactive updates test

#### Edge Cases (6 tests)
- **`should handle zero hash rate`** - "0 H/s"
- **`should handle high hash rates`** - "15.5 MH/s"
- **`should handle zero threads`** - 0 threads (stopped)
- **`should handle many threads`** - 16 threads (max)
- **`should handle empty block history`** - `[]`
- **`should handle empty mining history`** - `[]`

#### Lifecycle (1 test)
- **`should track complete mining session`**
  - Start mining → mine blocks → stop mining
  - Verifies rewards persist after stopping

#### Reward Integration (3 tests)
- **`should increment rewards when block is found`** - `totalRewards += blockReward`
- **`should track cumulative rewards over multiple blocks`** - 10 blocks × 2 reward = 20 total

### Why These Tests Don't Require Tauri
Tests verify **state management logic**:
✅ State updates correctly
✅ Derived values calculate properly
✅ Array limits enforced
✅ Progress tracking works

They do NOT test:
❌ Actual CPU hashing
❌ Finding valid nonces
❌ Geth integration
❌ Blockchain submission

---

## 4. multi-source-download.test.mjs (25 tests - ✅ ALL PASSING)

### What It Tests
Multi-source P2P download algorithms for BitTorrent-like file sharing.

### Source Code
**Note**: Tests contain copied/extracted functions rather than direct imports. Based on conceptual logic for P2P downloads in Chiral Network's file sharing system.

### Test Runner
Node.js native test runner (`import test from "node:test"`).

### Conceptual Coverage

#### Chunk Management
- Split large files into downloadable chunks
- Track which chunks are downloaded
- Verify chunk integrity (checksums)
- Reassemble chunks into complete file

#### Peer Selection
- Choose fastest peers for each chunk
- Handle peer disconnections gracefully
- Redistribute chunks when peer fails
- Load balancing across available peers

#### Download Strategy
- Parallel downloads from multiple peers simultaneously
- Sequential fallback when limited peers
- Bandwidth optimization
- Rarest-first chunk selection (like BitTorrent)

#### Error Handling
- Detect corrupt chunks
- Retry failed downloads
- Timeout handling
- Graceful degradation

---

## 5. signalingService.test.ts (14 tests - ❌ 9 FAILING)

### What It Tests
WebRTC signaling service for establishing P2P connections via DHT (Distributed Hash Table).

### Source Code
- **Main**: `src/lib/services/signalingService.ts` - SignalingService class
- **Related**: DHT backend, WebSocket connections

### Test Coverage

#### Connection Management (2 passing, 0 failing)
- ✅ **`should create a unique client ID`** - Each instance gets unique identifier
- ✅ **`should initialize stores with default values`** - `connected: false`, `peers: []`

#### DHT Integration (0 passing, 3 failing)
- ❌ **`should connect successfully when DHT is running`**
  - Calls `invoke('get_dht_peer_id')` for Tauri DHT
  - Calls `invoke('get_dht_connected_peers')` for peer list
  - Error: "Received network error or non-101 status code"

- ❌ **`should handle DHT not running gracefully`**
  - Should catch errors when DHT unavailable
  - Error: WebSocket connection failure

- ❌ **`should handle null peer ID gracefully`**
  - Edge case: DHT returns null
  - Error: WebSocket connection failure

#### Message Sending (0 passing, 3 failing)
- ❌ **`should send signaling message successfully`**
  - WebRTC offer/answer/ICE candidate relay
  - Error: WebSocket connection failure

- ❌ **`should throw error when DHT not connected`**
  - Guards against sending without connection
  - Error: WebSocket connection failure

- ❌ **`should handle send failure`**
  - Network error handling
  - Error: WebSocket connection failure

#### Disconnect (0 passing, 1 failing)
- ❌ **`should reset connection state`**
  - Should clear `connected` flag and `peers` array
  - Error: WebSocket connection failure

#### Peer Refresh (0 passing, 2 failing)
- ❌ **`should update peers list`**
  - Polls DHT for peer updates
  - Error: WebSocket connection failure

- ❌ **`should handle peer refresh failure`**
  - Should continue operating despite errors
  - Error: WebSocket connection failure

#### Get Client ID (2 passing, 0 failing)
- ✅ **`should return the client ID`** - Returns string ID
- ✅ **`should return consistent client ID`** - Same ID on multiple calls

#### Set Message Handler (1 passing, 0 failing)
- ✅ **`should accept message handler (currently no-op)`** - Callback registration

### Why Tests Fail
Tests attempt to connect to WebSocket server at `ws://localhost:9000` which is not running in test environment. The SignalingService class tries to establish real WebSocket connections during initialization.

---

## 6. Helper Test Files (.mjs - ❌ NOT RUNNING)

These tests exist but vitest doesn't execute them due to Node.js test runner syntax incompatibility with vitest configuration.

### dhtHelpers.test.mjs
**Focus**: DHT (Distributed Hash Table) utility functions

**Functions Tested**:
- **`resetConnectionAttempts(attempts, connectionSuccessful)`**
  - Returns 0 if connection successful
  - Keeps attempt count if failed
  - Simple retry logic

### uploadHelpers.test.mjs
**Focus**: File upload validation and storage management

**Functions Tested**:
- **`isDuplicateHash(files, hash)`**
  - Checks if file hash already exists in array
  - Prevents duplicate uploads
  - Validates input types

- **`getStorageStatus(freeGb, thresholdGb)`**
  - Returns "low" if `freeGb < thresholdGb`
  - Returns "ok" if sufficient space
  - Returns "unknown" for invalid inputs
  - Default threshold: 5 GB

### peerSelection.test.mjs
**Focus**: Peer selection algorithms for optimal downloads

**Functions Tested**:
- **`formatBytes(bytes)`**
  - Converts bytes to human-readable format
  - Examples: "0 Bytes", "1 KB", "1.5 MB", "1 GB"

- **`getPeerHealthScore(metrics)`**
  - Calculates weighted score: reliability (40%) + latency (30%) + success rate (30%)
  - Range: 0-100

- **`formatPeerId(peerId)`**
  - Truncates long peer IDs: "test_peer_12..."

- **`getStrategyForFileSize(fileSize, preferEncryption)`**
  - Small files (<100MB): "fastest" peer
  - Large files (>100MB): "bandwidth" optimized
  - Encryption preference: "encryption" strategy

- **`getPeerCountForParallelDownload(availablePeers, maxPeers)`**
  - Returns `Math.min(maxPeers, availablePeers.length)`

### Other .mjs Files
- **signaling.client.test.mjs** - WebRTC client signaling tests
- **signaling.client.node.test.mjs** - Node.js-specific signaling tests
- **signaling.server.test.mjs** - Server-side signaling tests
- **signaling.test.mjs** - General signaling tests

All show "No test suite found in file" error due to test runner mismatch.

---

## Running Tests

### All Tests
```bash
npm test
```

### Specific Files
```bash
npm test -- wallet.test.ts
npm test -- transactions.test.ts mining.test.ts
```

### Watch Mode
```bash
npm run test:watch
```

### Coverage Report
```bash
npm test -- --coverage
```

---

## Test Architecture Insights

### What These Tests Demonstrate

1. **Store Independence**
   - Svelte stores work without Tauri
   - State management is environment-agnostic
   - UI logic testable in isolation

2. **Dual-Mode Design**
   - WalletService adapts to environment
   - Demo mode enables development without blockchain
   - Production mode uses real cryptography

3. **Pure Function Testing**
   - Algorithm tests (multi-source, peer selection) are pure logic
   - No external dependencies
   - Deterministic and fast

4. **Mock Data Patterns**
   - `dummyTransactions`, `dummyWallet`, `dummyFiles` in stores.ts (lines 145-268)
   - Enables UI development before backend ready

### What's NOT Tested (And Why)

**Desktop-Only Features** (Require Tauri + Geth):
- Real blockchain transaction signing
- Actual proof-of-work mining (CPU hashing)
- Geth node communication
- File system P2P sharing

**UI Components** (Different Testing Approach):
- Svelte component rendering
- Form interactions
- Navigation flows
- Visual regressions

**Integration Tests** (Cross-System):
- Mining rewards → transaction list pipeline
- Wallet balance updates from mining
- File sharing → network statistics

---

## Key Architectural Insights

### 1. State vs. Execution
**Mining Example**:
- **State** (tested): `{ isMining: true, hashRate: "1.5 MH/s" }`
- **Execution** (not tested): Actual CPU hashing in Rust

Tests verify state management, not algorithm implementation.

### 2. Store-First Design
All application state lives in Svelte stores:
- Makes state observable by any component
- Enables testing state changes independently
- Provides single source of truth

### 3. Environment Detection
```typescript
this.isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
```

Enables graceful fallback to demo mode.

### 4. Mock Data Strategy
Stores initialize with dummy data:
- Enables UI development
- Provides realistic test scenarios
- Shows expected data shapes

---

## Summary

- **115+ tests** covering core functionality
- **106 tests passing** (92% success rate)
- **9 tests failing** (signaling service WebSocket issues)
- **~20 tests not running** (.mjs files with Node.js test runner)
- **Pure state management tests** (wallet, transactions, mining) all passing
- **Algorithm tests** (multi-source downloads) passing
- **Environment-agnostic design** enables testing without blockchain
- **Dual-mode architecture** supports both development and production

Tests verify that state management, calculations, and UI logic work correctly before integrating with Rust backend and blockchain infrastructure.

---

*Documentation covers all test files in project*
*Focus: What tests do, where code is based, how architecture enables testing*
