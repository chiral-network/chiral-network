# Chiral Network

Chiral Network is a decentralized file sharing application built on peer-to-peer networking with a native blockchain for payments and reputation tracking. It runs as a desktop application (Tauri 2 + Svelte 5 + Rust) and as a headless daemon for server deployments and automated testing.

---

## Table of Contents

- [Overview](#overview)
- [Features](#features)
- [Architecture](#architecture)
- [Getting Started](#getting-started)
- [Application Pages](#application-pages)
- [Backend Modules](#backend-modules)
- [Blockchain and Mining](#blockchain-and-mining)
- [Reputation System](#reputation-system)
- [File Transfer Protocol](#file-transfer-protocol)
- [Headless Mode and CLI](#headless-mode-and-cli)
- [Docker and Scaled Testing](#docker-and-scaled-testing)
- [Testing](#testing)
- [Project Structure](#project-structure)
- [Configuration](#configuration)

---

## Overview

Chiral Network enables users to share files directly between peers without relying on centralized servers. Files are discovered through a Kademlia DHT (Distributed Hash Table), transferred using a chunked protocol with per-chunk integrity verification, and paid for using CHI tokens on a private Ethereum-compatible blockchain.

The application consists of three layers:

1. **Frontend** -- Svelte 5 with TypeScript, rendered in a Tauri webview or browser.
2. **Backend** -- Rust, handling P2P networking (libp2p), blockchain interaction (Geth), file transfer, and local storage.
3. **Blockchain** -- A private Ethash proof-of-work chain (chain ID 98765) where users mine CHI tokens and pay for file downloads.

### Tech Stack

| Layer | Technology | Version |
|-------|-----------|---------|
| Desktop shell | Tauri | 2.x |
| Frontend framework | Svelte | 5.38 |
| Frontend language | TypeScript | 5.7 |
| Build tool | Vite | 7.1 |
| Styling | TailwindCSS | 3.4 |
| Backend language | Rust | 2021 edition |
| P2P networking | libp2p | 0.53 |
| HTTP server | Axum | 0.7 |
| Blockchain client | Core-Geth | 1.12.20 |
| Crypto | ethers.js (frontend), secp256k1 + ed25519-dalek (backend) |

---

## Features

### File Sharing
- Publish files to the DHT so other peers can discover and download them.
- Chunked file transfer protocol (256 KB chunks) with SHA-256 verification per chunk and full-file hash verification on completion.
- Set a CHI price per file. Payments are processed on-chain before the download begins.
- Unlimited download speed with a flat download fee of 0.001 CHI per MB.

### ChiralDrop
- Direct peer-to-peer file transfer between two users, similar to AirDrop.
- Discover nearby peers on the network.
- Accept or decline incoming transfer requests.
- Optional pricing for paid file drops.

### Drive
- Local file management system with folders, uploads, renaming, starring, and deletion.
- Files are stored locally at `~/.local/share/chiral-network/chiral-drive/`.
- Seed files to the P2P network directly from Drive.
- HTTP preview pages for downloaded files (images, video, audio, PDF, text).

### Mining
- CPU mining with configurable thread count and utilization percentage.
- GPU mining support via ethminer (limited to older NVIDIA GPUs, Compute Capability 7.5 and below).
- Mining rewards are 5 CHI per block.
- Real-time hash rate display via eth_hashrate RPC.

### Wallet
- Generate a new wallet from a 12-word BIP39 mnemonic.
- Import an existing wallet using a private key or recovery phrase.
- Send and receive CHI tokens.
- Optional one-time email backup of wallet credentials.
- Transaction history with type classification (send, receive, download payment, file sale).

### Reputation (Elo)
- Each wallet has an Elo reputation score (0-100) derived from file transfer outcomes.
- Completed transfers increase the score; failed transfers decrease it.
- Time-weighted: recent events within a 180-day lookback period carry more weight.
- Amount-weighted: larger transfers have a proportionally larger effect (logarithmic scaling).
- Batch lookup available for displaying seller reputations on the download page.

### Hosting Marketplace
- Publish a host advertisement to offer storage to the network.
- Browse available hosts, propose hosting agreements, and track active agreements.
- Hosted files are automatically seeded to the DHT.

---

## Architecture

```
+-----------------------------------------------------------+
|                     Desktop Application                    |
|  +-------------------+    +----------------------------+  |
|  |   Svelte 5 UI     |    |     Tauri IPC Bridge       |  |
|  |  (Pages, Stores,  |--->|  invoke() / listen()       |  |
|  |   Services)        |    |                            |  |
|  +-------------------+    +----------------------------+  |
+-----------------------------------------------------------+
            |                           |
            v                           v
+-----------------------------------------------------------+
|                     Rust Backend                           |
|  +----------+  +---------+  +--------+  +-------------+  |
|  | DhtService|  | Geth    |  | Drive  |  | File        |  |
|  | (libp2p)  |  | Process |  | API    |  | Transfer    |  |
|  +----------+  +---------+  +--------+  +-------------+  |
|  +----------+  +---------+  +--------+  +-------------+  |
|  | Rating   |  | Hosting |  | Wallet |  | Encryption  |  |
|  | Storage  |  | Server  |  | Backup |  | Keypair     |  |
|  +----------+  +---------+  +--------+  +-------------+  |
+-----------------------------------------------------------+
            |                           |
            v                           v
+-------------------------+    +------------------------+
|   P2P Network (libp2p)  |    |   Blockchain (Geth)    |
|   Kademlia DHT          |    |   Ethash PoW chain     |
|   TCP + Noise + Yamux   |    |   Chain ID: 98765      |
|   File chunk protocol   |    |   RPC: localhost:8545   |
+-------------------------+    +------------------------+
            |
            v
+-------------------------+
|   Relay Server           |
|   130.245.173.73:8080    |
|   - Reputation API       |
|   - Drive share proxy    |
|   - Email backup relay   |
+-------------------------+
```

### Data Flow: File Download

1. Publisher registers a file on the DHT with its hash, name, size, price, and peer ID.
2. Consumer searches the DHT by file hash or magnet link.
3. Consumer sees the file info, seeder list, and Elo scores.
4. Consumer confirms the download. If the file has a price, CHI is sent to a burn address.
5. Consumer's node sends chunk requests to the seeder over the libp2p file transfer protocol.
6. Each 256 KB chunk is SHA-256 verified on receipt.
7. After all chunks arrive, the full file hash is verified.
8. The file is saved to the download directory and optionally added to Drive.

---

## Getting Started

### Prerequisites

- Node.js 20+
- Rust toolchain (rustup)
- npm

### Development

```bash
# Install frontend dependencies
npm install

# Start the desktop app in development mode
npm run tauri:dev

# Build the frontend only
npm run build

# Run frontend tests
npm test

# Run Rust tests
cargo test --manifest-path src-tauri/Cargo.toml

# Type check the Rust backend
cargo check --manifest-path src-tauri/Cargo.toml
```

### Headless Mode

Run the application without a GUI for server deployments or automated testing:

```bash
# Start the daemon
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral_daemon -- --port 9419

# Start with auto-mining
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral_daemon -- \
  --port 9419 \
  --auto-mine \
  --miner-address 0xYOUR_WALLET \
  --mining-threads 4

# Use the CLI
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon status --port 9419
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- wallet create
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- dht start --port 9419
```

---

## Application Pages

| Route | Page | Description |
|-------|------|-------------|
| `/wallet` | Wallet | Create, import, or restore a wallet. Optional email backup of recovery phrase. |
| `/account` | Account | View wallet address, CHI balance, transaction history, and reputation score. Send CHI to other addresses. |
| `/network` | Network | Manage P2P connections. Start/stop the local Geth node. View peer list, bootstrap health, and DHT status. |
| `/download` | Download | Search for files by hash or magnet link. View seeder list with Elo scores. Pay and download files. |
| `/drive` | Drive | Local file manager with folders. Upload, rename, star, delete files. Seed files to the P2P network. |
| `/chiraldrop` | ChiralDrop | Direct peer-to-peer file transfers. Discover nearby peers and send/receive files. |
| `/hosts` | Hosts | Hosting marketplace. Publish storage offers, browse hosts, manage agreements. |
| `/mining` | Mining | CPU and GPU mining controls. View hash rate, block height, total mined CHI, and mined block history. |
| `/settings` | Settings | Appearance (dark mode, color theme, nav style), notification preferences, download directory. |
| `/diagnostics` | Diagnostics | System event log, DHT health, bootstrap status, Geth status, mining diagnostics, Geth log viewer. |

---

## Backend Modules

The Rust backend is organized into the following modules under `src-tauri/src/`:

| Module | File | Responsibility |
|--------|------|---------------|
| DHT Service | `dht.rs` | libp2p Kademlia DHT, peer management, file publishing/searching, chunk transfer protocol |
| File Transfer | `file_transfer.rs` | Chunked file sending/receiving, SHA-256 verification, retry logic |
| Geth Process | `geth.rs` | Manages the Core-Geth blockchain client, mining, RPC communication |
| Drive API | `drive_api.rs` | HTTP routes for file CRUD, share links, preview pages |
| Drive Storage | `drive_storage.rs` | On-disk manifest and file storage management |
| Hosting Server | `hosting_server.rs` | Axum gateway server combining Drive, Rating, and Hosting routes |
| Rating API | `rating_api.rs` | Elo reputation calculation and HTTP endpoints |
| Rating Storage | `rating_storage.rs` | Persistent storage for reputation events |
| Relay Share Proxy | `relay_share_proxy.rs` | Reverse proxy for Drive share links through the relay |
| Wallet Backup | `wallet_backup_api.rs` | SMTP email sending for wallet credential backup |
| Encryption | `encryption.rs` | X25519 key exchange and AES-GCM file encryption |
| Chain RPC | `chain_rpc_api.rs` | Blockchain RPC helpers |
| Speed Tiers | `speed_tiers.rs` | Download cost calculation (0.001 CHI/MB) |
| Event Sink | `event_sink.rs` | Frontend event emission abstraction |
| Geth Bootstrap | `geth_bootstrap.rs` | Bootstrap node health checking and selection |

### Binary Targets

| Binary | Source | Purpose |
|--------|--------|---------|
| `chiral-network` | `src-tauri/src/main.rs` | Desktop application (Tauri) |
| `chiral` | `src-tauri/src/bin/chiral.rs` | Command-line interface |
| `chiral_daemon` | `src-tauri/src/bin/chiral_daemon.rs` | Headless daemon server |
| `relay_server` | `src-tauri/src/bin/relay_server.rs` | Relay and reputation server |

---

## Blockchain and Mining

Chiral Network runs a private Ethereum-compatible blockchain using the Ethash proof-of-work consensus algorithm.

### Chain Parameters

| Parameter | Value |
|-----------|-------|
| Chain ID | 98765 |
| Network ID | 98765 |
| Consensus | Ethash |
| Block reward | 5 CHI |
| Genesis difficulty | 0x400000 (4,194,304) |
| Gas limit | 0x47b760 (4,700,000) |
| Gas price | 0 (free transactions) |
| Client | Core-Geth v1.12.20 |

### How Mining Works

1. The application auto-starts Geth with the wallet address as the coinbase (miner.etherbase).
2. On the Mining page, users can start CPU mining with a configurable number of threads.
3. Geth communicates via JSON-RPC on `localhost:8545`.
4. Mining status is polled every 5 seconds. Hash rate comes from `eth_hashrate`.
5. Mined blocks are queried by scanning recent blocks where the miner field matches the user's address.
6. The "Total Mined" display and wallet balance both query the local Geth node using `eth_getBalance`.

### Bootstrap Node

A bootstrap node runs at `130.245.173.73` and serves as the initial peer for new nodes joining the network. It runs both Geth (port 8545 for RPC, port 30303 for P2P) and the relay server (port 8080 for HTTP, port 4001 for libp2p).

---

## Reputation System

The Elo-based reputation system provides a trust score for each wallet address based on file transfer outcomes.

### Score Calculation

- Base Elo: 50
- Range: 0 to 100 (clamped)
- Input: transfer outcome (completed = 1.0, failed = 0.0)
- Time weight: events within the last 180 days are weighted more heavily using exponential decay
- Amount weight: logarithmic scaling based on the CHI value transferred

### API Endpoints (Relay Server)

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/ratings/:wallet` | GET | Get Elo score and event history for a wallet |
| `/api/ratings/batch` | POST | Batch lookup of Elo scores for multiple wallets |
| `/api/ratings/transfer` | POST | Record a transfer outcome (completed/failed) |

### Integration

- The Download page displays seeder Elo scores next to each search result.
- After a file transfer completes or fails, the outcome is automatically reported to the relay.
- Wallet addresses are normalized to lowercase for consistent lookup.

---

## File Transfer Protocol

Files are transferred using a custom request-response protocol built on libp2p.

### Protocol Details

| Property | Value |
|----------|-------|
| Protocol ID | `/chiral/file-request/2.0.0` |
| Chunk size | 256 KB |
| Encoding | CBOR (custom codec) |
| Request limit | 1 MB |
| Response limit | 32 MB |
| Verification | SHA-256 per chunk + full file hash |
| Retry | Up to 3 attempts per chunk |

### Message Types

**Request:** `ChunkRequest` enum with `FileInfo` (metadata request) and `Chunk` (data request with offset) variants.

**Response:** `ChunkResponse` enum with `FileInfo` (file metadata: name, size, hash, chunk count) and `Chunk` (data bytes + SHA-256 hash) variants.

---

## Headless Mode and CLI

### Daemon

The headless daemon runs the full backend without a GUI. It exposes an HTTP API on the configured port (default 9419).

```bash
chiral_daemon --port 9419 --auto-start-dht --auto-mine --miner-address 0xABC
```

| Flag | Env Var | Default | Description |
|------|---------|---------|-------------|
| `--port` | `CHIRAL_DAEMON_PORT` | 9419 | HTTP API port |
| `--auto-start-dht` | `CHIRAL_AUTO_START_DHT` | false | Start DHT on boot |
| `--auto-start-geth` | `CHIRAL_AUTO_START_GETH` | false | Start Geth on boot |
| `--auto-mine` | `CHIRAL_AUTO_MINE` | false | Start mining (implies DHT + Geth) |
| `--miner-address` | `CHIRAL_MINER_ADDRESS` | none | Wallet for mining rewards |
| `--mining-threads` | `CHIRAL_MINING_THREADS` | 1 | CPU mining threads |

### Daemon API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/health` | GET | Liveness probe (always 200) |
| `/api/ready` | GET | Readiness probe (checks DHT + Geth) |
| `/api/headless/runtime` | GET | Runtime status (DHT, peer ID, Geth) |
| `/api/headless/dht/start` | POST | Start DHT service |
| `/api/headless/dht/stop` | POST | Stop DHT service |
| `/api/headless/dht/health` | GET | DHT health details |
| `/api/headless/dht/peers` | GET | Connected peer list |
| `/api/headless/dht/put` | POST | Store a key-value pair in DHT |
| `/api/headless/dht/get` | POST | Retrieve a value from DHT |
| `/api/headless/geth/start` | POST | Start Geth node |
| `/api/headless/geth/stop` | POST | Stop Geth node |
| `/api/headless/geth/status` | GET | Geth status |
| `/api/headless/mining/start` | POST | Start CPU mining |
| `/api/headless/mining/stop` | POST | Stop mining |
| `/api/headless/mining/status` | GET | Mining status |

### CLI

The CLI tool communicates with a running daemon over HTTP.

```bash
chiral daemon status --port 9419
chiral wallet create
chiral wallet show
chiral account balance
chiral account send --to 0xADDRESS --amount 1.5
chiral dht start --port 9419
chiral dht peers --port 9419
chiral download search --hash FILEHASH --port 9419
chiral drive ls
chiral mining start --threads 4 --port 9419
chiral mining status --port 9419
```

---

## Docker and Scaled Testing

### Docker Images

The project includes a multi-stage Dockerfile that produces four image targets:

| Target | Binary | Ports | Purpose |
|--------|--------|-------|---------|
| `daemon` | `chiral_daemon` | 9419, 30303 | Headless P2P node |
| `relay` | `relay_server` | 4001, 8080 | Bootstrap relay server |
| `cli` | `chiral` | -- | Command-line tool |
| `test-node` | `chiral_daemon` + `chiral` | 9419, 30303 | Testing with healthcheck |

### Docker Compose

```bash
# Start a 3-node test network
docker compose up -d

# Scale to 20 nodes
docker compose up -d --scale node=20

# Tear down
docker compose down -v
```

### Scaled Integration Testing

A 17-phase automated test harness exercises every feature with configurable node counts (10-100).

```bash
./scripts/scaled-test.sh           # 10 nodes
./scripts/scaled-test.sh 50        # 50 nodes
./scripts/scaled-test.sh 100       # 100 nodes
./scripts/scaled-test.sh 20 --keep # Keep containers for debugging
```

#### Test Phases

| Phase | Name | What It Tests |
|-------|------|--------------|
| 01 | Health | All nodes up, DHT running, peer discovery |
| 02 | Wallets | Wallet address generation for each node |
| 03 | Mining | Geth startup, mining, block production |
| 04 | Drive Upload | File uploads (1KB, 100KB, 1MB) on seeder nodes |
| 05 | File Publish | Register shared files on the DHT |
| 06 | Search and Download | Search DHT, request and complete file downloads |
| 07 | ChiralDrop | P2P file transfers between random node pairs |
| 08 | Payments | CHI balance verification on mining nodes |
| 09 | Reputation | Batch Elo lookup and score validation |
| 10 | Drive CRUD | Folder create, upload, rename, delete cycle |
| 11 | Concurrent Downloads | All consumers download the same file simultaneously |
| 12 | Rapid Publish-Search | DHT propagation delay measurement |
| 13 | Network Partition | Stop 20% of nodes, verify recovery |
| 14 | Large File Transfer | 10MB file transfer to multiple consumers |
| 15 | Rapid Wallet Ops | Simultaneous balance queries across all nodes |
| 16 | DHT Flood | Each node stores and retrieves 10 key-value pairs |
| 17 | Long-Running Stability | 2-minute health monitoring with anomaly detection |

Node roles are assigned automatically: 20% miners, 30% seeders, 50% consumers.

---

## Testing

### Unit and Integration Tests (vitest)

```bash
npm test                    # Run all frontend tests
npm test -- tests/load/     # Run load tests only
```

The test suite contains 586 tests across 35 files:

| Category | Files | Tests | Coverage |
|----------|-------|-------|----------|
| Store/service unit tests | 24 | 497 | Stores, services, utilities, wallet, DHT, Drive |
| Load/stress tests | 9 | 89 | Concurrent operations, throughput, caching |
| Network tests (skipped in CI) | 2 | 43 | Relay server, gateway endpoints |

### Rust Tests

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

276 Rust tests covering: genesis validation, syncing logic, mining status, serialization, GPU error detection, DHT configuration, encryption, hosting server, and drive API.

### Scaled Integration Tests

17 bash-based test phases running against real Docker containers. See the [Docker and Scaled Testing](#docker-and-scaled-testing) section.

---

## Project Structure

```
chiral-network/
  src/                          # Frontend (Svelte 5 + TypeScript)
    App.svelte                  # Main app shell, routing, auto-start logic
    pages/                      # Page components (Account, Download, Mining, etc.)
    lib/
      stores.ts                 # Svelte stores (wallet, settings, peers)
      stores/                   # Complex stores (driveStore)
      services/                 # Service modules (walletService, gethService, etc.)
      components/               # Reusable UI components
      chiralDropStore.ts        # ChiralDrop transfer history
      encryptedHistoryService.ts # Encrypted DHT history sync
      dhtService.ts             # Frontend DHT service wrapper
      speedTiers.ts             # Download cost calculation
      toastStore.ts             # Toast notification system
      logger.ts                 # Structured logging

  src-tauri/                    # Backend (Rust)
    src/
      lib.rs                    # Tauri command registration, AppState
      dht.rs                    # DHT service (libp2p Kademlia)
      file_transfer.rs          # Chunked file transfer protocol
      geth.rs                   # Geth process management, mining
      drive_api.rs              # Drive HTTP routes and file serving
      drive_storage.rs          # Drive manifest and file storage
      hosting_server.rs         # Gateway server (Axum)
      rating_api.rs             # Reputation Elo calculation
      rating_storage.rs         # Reputation persistence
      relay_share_proxy.rs      # Relay share registry
      wallet_backup_api.rs      # Email backup endpoint
      encryption.rs             # AES-GCM + X25519 encryption
      geth_bootstrap.rs         # Bootstrap node management
      speed_tiers.rs            # Download cost logic
      bin/
        chiral.rs               # CLI binary
        chiral_daemon.rs        # Headless daemon binary
        relay_server.rs         # Relay server binary

  tests/                        # Frontend tests (vitest)
    load/                       # Load and stress tests
    scaled/                     # Scaled integration test phases (bash)

  scripts/
    scaled-test.sh              # Scaled test orchestrator
    docker-test.sh              # Basic Docker test script

  Dockerfile                    # Multi-stage build (daemon, relay, cli, test-node)
  docker-compose.yml            # Docker Compose for test networks
  docker-compose.scaled-test.yml # Compose override for scaled testing
```

---

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `CHIRAL_RPC_ENDPOINT` | `http://130.245.173.73:8545` | Blockchain RPC endpoint |
| `CHIRAL_BOOTSTRAP_NODES` | Built-in bootstrap list | Comma-separated enode URLs |
| `CHIRAL_DAEMON_PORT` | `9419` | Daemon HTTP port |
| `CHIRAL_AUTO_START_DHT` | `false` | Auto-start DHT on daemon boot |
| `CHIRAL_AUTO_START_GETH` | `false` | Auto-start Geth on daemon boot |
| `CHIRAL_AUTO_MINE` | `false` | Auto-start mining (implies DHT + Geth) |
| `CHIRAL_MINER_ADDRESS` | none | Wallet address for mining rewards |
| `CHIRAL_MINING_THREADS` | `1` | CPU mining thread count |
| `CHIRAL_GPU_MINER_PATH` | auto-detected | Path to ethminer binary |
| `CHIRAL_WALLET_EMAIL_SMTP_HOST` | none | SMTP server for email backup |
| `CHIRAL_WALLET_EMAIL_FROM` | none | Sender address for email backup |

### Local Storage Keys

User data is stored in localStorage with wallet-specific keys to prevent data leakage between accounts:

- `chiraldrop_history_<address>` -- ChiralDrop transfer history
- `chiraldrop_history_encrypted_<address>` -- Encrypted history cache
- `chiral_download_history_<address>` -- Download history
- `chiral_active_downloads_<address>` -- Active downloads
- `chiral_saved_recipients_<address>` -- Saved recipient addresses

### Data Directories

| Platform | Path |
|----------|------|
| Linux | `~/.local/share/chiral-network/` |
| macOS | `~/Library/Application Support/chiral-network/` |
| Windows | `%APPDATA%/chiral-network/` |

Subdirectories:
- `chiral-drive/` -- Drive file storage
- `geth/` -- Blockchain data and logs
- `headless/` -- Daemon PID file
