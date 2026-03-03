# Chiral Network V2 - Test Suite

## Overview

The test suite covers both the frontend (TypeScript/Svelte) and backend (Rust/Tauri) components of Chiral Network V2.

**Total Tests: 168** (97 frontend + 71 backend)

---

## Running Tests

### Frontend Tests (TypeScript)

```bash
cd v2-chiral-network

# Run all frontend tests once
npm test

# Run tests in watch mode (re-runs on file changes)
npm run test:watch

# Run tests with coverage report
npm run test:coverage
```

### Backend Tests (Rust)

```bash
cd v2-chiral-network/src-tauri

# Run all Rust tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run tests for a specific module
cargo test encryption::tests
cargo test dht::tests
cargo test geth_bootstrap::tests
cargo test geth::tests
cargo test file_transfer::tests

# Run a specific test
cargo test test_encrypt_decrypt_roundtrip
```

### Run All Tests

```bash
cd v2-chiral-network

# Frontend + Backend
npm test && cd src-tauri && cargo test
```

---

## Frontend Test Files

| File | Module | Tests | Description |
|------|--------|-------|-------------|
| `tests/aliasService.test.ts` | `$lib/aliasService` | 14 | Alias generation, deterministic peer ID mapping, color codes |
| `tests/utils.test.ts` | `$lib/utils` | 11 | Tailwind class merging utility (`cn`) |
| `tests/toastStore.test.ts` | `$lib/toastStore` | 13 | Toast notifications, auto-dismiss, stacking, type conversion |
| `tests/stores.test.ts` | `$lib/stores` | 18 | Settings persistence, dark mode, wallet store, network stats |
| `tests/encryptionService.test.ts` | `$lib/services/encryptionService` | 23 | Encryption service (Tauri mock), file name helpers, web/Tauri mode |
| `tests/dhtBootstrap.test.ts` | Bootstrap config | 18 | Bootstrap node format validation, multiaddr/enode parsing |

### Test Details

**aliasService.test.ts** - Validates the Color + Animal alias system:
- Random alias generation returns valid `UserAlias` objects
- `aliasFromPeerId` produces deterministic results for the same peer ID
- Different peer IDs produce different aliases
- All 24 ALIAS_COLORS entries have valid hex codes

**utils.test.ts** - Tests the Tailwind CSS class merging utility:
- Basic class concatenation
- Tailwind conflict resolution (e.g., `p-2` + `p-4` = `p-4`)
- Conditional, array, and object class inputs
- Dark mode and responsive prefix handling

**toastStore.test.ts** - Tests the toast notification system:
- Toast creation with different types (success, error, info)
- Auto-dismiss with configurable duration
- Manual removal by ID
- Warning type conversion to info
- Toast stacking and unique ID assignment

**stores.test.ts** - Tests core Svelte stores:
- Settings store with localStorage persistence
- Settings reset and merge with defaults
- Dark mode derived store resolving theme values
- Wallet, network, and peer stores initialization

**encryptionService.test.ts** - Tests the E2E encryption frontend service:
- Non-Tauri environment gracefully returns null/empty/throws
- Tauri environment correctly calls `invoke` with proper arguments
- 0x prefix stripping on wallet private keys
- File name encryption detection and original name extraction

**dhtBootstrap.test.ts** - Validates bootstrap node configuration:
- libp2p multiaddr format (IPv4, TCP port, peer ID)
- Geth enode format (node ID, IP, port)
- Unique peer IDs across all nodes
- Port range validation
- Multiaddr and enode parsing logic

---

## Backend Test Files (Rust)

| Module | Tests | Description |
|--------|-------|-------------|
| `encryption::tests` | 19 | ECIES encryption, symmetric encryption, key derivation, serialization |
| `dht::tests` | 19 | Bootstrap nodes, multiaddr parsing, protocol message serialization |
| `geth_bootstrap::tests` | 14 | Enode parsing, node configuration, health report serialization |
| `geth::tests` | 12 | Chain configuration, genesis JSON, type serialization |
| `file_transfer::tests` | 7 | Transfer status, serialization, service initialization |

### Test Details

**encryption::tests** - Tests X25519 + AES-256-GCM encryption:
- Keypair generation and deterministic wallet-derived keys
- Encrypt/decrypt roundtrip (small and large data)
- Wrong key decryption failure
- Hex API roundtrip and invalid input handling
- Bundle field validation (key length, nonce length)
- Symmetric encryption with tamper detection
- Each encryption produces unique ciphertext (ephemeral keys)
- JSON serialization roundtrip

**dht::tests** - Tests DHT/libp2p configuration:
- Bootstrap node list completeness and uniqueness
- Multiaddr parsing and peer ID extraction
- Peer ID removal from multiaddrs
- Protocol message serialization (PingRequest, PingResponse, FileTransfer, FileRequest)
- PeerInfo and NetworkStats camelCase serialization

**geth_bootstrap::tests** - Tests Geth bootstrap node management:
- Enode URL parsing (valid, with query, invalid formats)
- Default node configuration validation
- Node priorities and required fields
- Health report and node health serialization

**geth::tests** - Tests Geth/blockchain configuration:
- Chain ID and Network ID constants
- Genesis JSON structure and content validation
- Faucet address allocation
- Extra data encoding ("Chiral Network Genesis")
- Type serialization (DownloadProgress, GethStatus, MiningStatus)
- Geth binary path structure

**file_transfer::tests** - Tests file transfer service:
- TransferStatus enum equality and serialization
- All transfer statuses serialize correctly
- PendingTransfer and FileTransferRequest serialization
- Service initialization with empty queues
- Decline nonexistent transfer error handling

---

## Configuration

### Frontend (vitest.config.ts)
- Environment: jsdom
- Path alias: `$lib` -> `./src/lib`
- Setup file: `tests/setup.ts` (mocks Tauri APIs and `window.matchMedia`)

### Backend (Cargo.toml)
- Tests compile and run with `cargo test`
- No additional test dependencies needed
- Tests are in `#[cfg(test)] mod tests` blocks within each source file

---

## Adding New Tests

### Frontend
1. Create a new file in `tests/` with the `.test.ts` extension
2. Import from `vitest` and use the `$lib` alias for project imports
3. Tauri `invoke` is automatically mocked via `tests/setup.ts`
4. Run `npm test` to verify

### Backend
1. Add a `#[cfg(test)] mod tests { ... }` block at the bottom of the source file
2. Use `#[test]` for sync tests or `#[tokio::test]` for async tests
3. Run `cargo test` to verify
