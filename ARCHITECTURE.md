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
- Unlimited download speed with a flat download fee of 0.01 CHI per MB.
- 0.5% platform fee on all transactions (split between seller and platform wallet).

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
- CDN Servers tab: always-on infrastructure servers separated from peer hosts.

### CDN Service
- Always-on file hosting servers that keep files available when the uploader goes offline.
- Market-based dynamic pricing: `max(floor_price, median_peer_price × 1.2)`.
- Payment required before upload — verified on-chain with 5% tolerance for CHI rounding.
- Uploader sets a download price that other users pay to download from the CDN.
- Files auto-expire and are cleaned up when the paid hosting duration elapses.
- CDN re-seeds all active files to DHT on startup (15s after bootstrap).
- Download page queries CDN servers directly as fallback when DHT search is slow.
- Deployed at `130.245.173.73:9420` with 227 GB capacity.
- Desktop app: Hosts → CDN Servers tab → Upload from Drive with payment confirmation.

### Security
- ECDSA-signed file metadata (prevents seeder list poisoning and DHT tampering).
- ECDSA-signed seeder entries (prevents payment address redirection).
- On-chain payment verification before serving file chunks.
- On-chain payment verification for CDN uploads (with 5% rounding tolerance).
- 0.5% platform fee on all transactions (99.5% to seller, 0.5% to platform).
- Relay filters private IPs from Kademlia routing table.
- Stop seeding removes peer from DHT seeder list (prevents ghost seeders).

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
|  | Wallet   |  | RPC     |  | Hosting|  | Encryption  |  |
|  | (wallet. |  | Client  |  | Server |  | Keypair     |  |
|  |  rs)     |  | (pooled)|  |        |  |             |  |
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
|   130.245.173.73         |
|   :4001 libp2p relay     |
|   :8080 HTTP API         |
|   - Circuit relay v2     |
|   - Kademlia routing     |
|   - Reputation API       |
|   - Drive share proxy    |
|   - WebSocket tunnels    |
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
| `/mining` | Mining | CPU and GPU mining controls. View hash rate, block height, and total mined CHI. |
| `/settings` | Settings | Appearance (dark mode, color theme, nav style), notification preferences, download directory. |
| `/diagnostics` | Diagnostics | System event log, DHT health, bootstrap status, Geth status, mining diagnostics, Geth log viewer. |

---

## Backend Modules

The Rust backend is organized into the following modules under `src-tauri/src/`:

| Module | File | Responsibility |
|--------|------|---------------|
| Command Layer | `lib.rs` | Thin Tauri command wrappers, AppState management (103 commands) |
| Wallet | `wallet.rs` | Balance queries, transaction signing (EIP-155), history, metadata persistence, CHI/Wei conversion |
| RPC Client | `rpc_client.rs` | Connection-pooled HTTP client, batch JSON-RPC, response cache with TTL |
| DHT Service | `dht.rs` | libp2p Kademlia DHT, peer management, file publishing/searching, chunk transfer protocol |
| File Transfer | `file_transfer.rs` | Chunked file sending/receiving, SHA-256 verification, retry logic |
| Geth Process | `geth.rs` | Manages Core-Geth lifecycle, mining, batch RPC status queries |
| Drive API | `drive_api.rs` | HTTP routes for file CRUD, share links, preview pages |
| Drive Storage | `drive_storage.rs` | On-disk manifest and file storage management |
| Hosting Server | `hosting_server.rs` | Axum gateway server combining Drive, Rating, and Hosting routes |
| Hosting Types | `hosting.rs` | Site metadata, MIME detection, persistence |
| Rating API | `rating_api.rs` | Elo reputation calculation and HTTP endpoints |
| Rating Storage | `rating_storage.rs` | Persistent storage for reputation events |
| Relay Share Proxy | `relay_share_proxy.rs` | Reverse proxy + WebSocket tunnel for NAT traversal |
| Wallet Backup | `wallet_backup_api.rs` | SMTP email sending for wallet credential backup |
| Encryption | `encryption.rs` | X25519 key exchange and AES-GCM file encryption |
| Chain RPC | `chain_rpc_api.rs` | Blockchain RPC proxy |
| Speed Tiers | `speed_tiers.rs` | Download cost calculation (0.001 CHI/MB) |
| Event Sink | `event_sink.rs` | Frontend event emission abstraction |
| Geth Bootstrap | `geth_bootstrap.rs` | Bootstrap node health checking and selection |

Total: 23 Rust source files, 3 binary targets.

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

### Geth Configuration

| Setting | Value | Notes |
|---------|-------|-------|
| Sync mode | `full` | Replays all blocks from genesis; preserves full history on restart |
| GC mode | `archive` | Keeps all state; prevents block height regression on restart |
| Cache | 1024 MB | RAM cache for blockchain state |
| Max peers | 50 | Maximum Geth P2P connections |

### How Mining Works

1. The application auto-starts Geth with the wallet address as the coinbase (miner.etherbase).
2. On the Mining page, users can start CPU mining with a configurable number of threads.
3. Geth communicates via JSON-RPC on `localhost:8545`.
4. Mining status is polled every 10 seconds using batch RPC (eth_mining + eth_hashrate + eth_coinbase + eth_blockNumber in one request).
5. Balance and total mined both query the local Geth node via `eth_getBalance` through the shared `rpc_client.rs` connection pool.
6. All wallet queries route through `effective_rpc_endpoint()`: local Geth if running, otherwise remote fallback at `130.245.173.73:8545`.

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

### Daemon API Endpoints (49 routes)

All headless paths prefixed with `/api/headless/` except health, ready, and drive routes.

| Category | Endpoints |
|----------|-----------|
| Health | `GET /api/health`, `GET /api/ready`, `GET runtime` |
| Wallet | `GET wallet`, `POST wallet/create`, `wallet/import`, `wallet/balance`, `wallet/send`, `wallet/receipt`, `wallet/history`, `wallet/faucet`; `GET wallet/chain-id` |
| DHT | `POST dht/start`, `dht/stop`, `dht/put`, `dht/get`, `dht/ping`, `dht/echo`; `GET dht/health`, `dht/peers`, `dht/peer-id`, `dht/listening-addresses` |
| Files | `POST file/search`, `dht/register-shared-file`, `dht/unregister-shared-file`, `dht/request-file`, `dht/send-file` |
| ChiralDrop | `GET drop/inbox`, `drop/outgoing`; `POST drop/accept`, `drop/decline` |
| Geth | `POST geth/install`, `geth/start`, `geth/stop`; `GET geth/status`, `geth/logs` |
| Mining | `POST mining/start`, `mining/stop`, `mining/miner-address`; `GET mining/status`, `mining/blocks` |
| Hosting | `POST hosting/publish-ad`; `GET hosting/registry` |
| CDN | `POST cdn/upload`; `GET cdn/files`, `cdn/pricing`, `cdn/status`; `DELETE cdn/files/:hash`; `PUT cdn/files/:hash` |
| Drive | Full CRUD via `/api/drive/*` (requires `X-Owner` header) |
| Diagnostics | `GET bootstrap-health` |

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

### Docker Compose Files

| File | Purpose |
|------|---------|
| `docker-compose.yml` | General test network (relay + scalable nodes) |
| `docker-compose.local-test.yml` | Local isolated testing with relay |
| `docker-compose.production-net.yml` | 30 nodes on host networking, connected to production relay |
| `docker-compose.scaled-test.yml` | Scaled integration test overlay |

```bash
# 30 production-connected nodes (host networking)
docker compose -f docker-compose.production-net.yml up -d

# Local isolated testing
docker compose -f docker-compose.local-test.yml up -d --scale node=10

# Tear down
docker compose -f docker-compose.production-net.yml down
```

### Kubernetes Deployment (k3s/Rancher)

Test nodes can also be deployed to the k3s cluster at `130.245.173.231`:

```bash
export KUBECONFIG=~/.kube/config-k3s
kubectl apply -f k8s/chiral-30-pods.yaml
kubectl get pods -n chiral-test
```

### Stress Testing

A 12-phase, 35-test stress suite exercises every feature across 30 nodes:

```bash
bash scripts/stress-test-30-nodes.sh
```

#### Stress Test Phases (stress-test-30-nodes.sh)

| Phase | Name | What It Tests |
|-------|------|--------------|
| 1 | Health & Connectivity | All 30 health/readiness endpoints |
| 2 | DHT Network | Unique peer IDs, peer counts, relay circuits, cross-node ping |
| 3 | DHT Storage | Cross-node put/get, 10 concurrent writes |
| 4 | Wallet | Create (10 nodes), import, balance query, chain ID |
| 5 | File Registration | Publish file, search from publisher + 5 remote nodes |
| 6 | Echo Protocol | Direct echo + fan-out to 10 nodes |
| 7 | Hosting Ads | Publish advertisement, query registry from remote node |
| 8 | Concurrent Stress | 30 simultaneous DHT puts, peer queries, health checks |
| 9 | Ping Mesh | 10 random node pairs |
| 10 | Drive Operations | List items, create folder |
| 11 | Bootstrap Health | Diagnostics report |
| 12 | DHT Reconnect | Stop DHT, restart, verify peer recovery |

---

## Testing

### Unit and Integration Tests (vitest)

```bash
npm test                    # Run all frontend tests
npm test -- tests/load/     # Run load tests only
```

The test suite contains 585+ tests across 35 files:

| Category | Files | Tests | Coverage |
|----------|-------|-------|----------|
| Store/service unit tests | 24 | 497 | Stores, services, utilities, wallet, DHT, Drive |
| Load/stress tests | 9 | 89 | Concurrent operations, throughput, caching |
| Network tests (skipped in CI) | 2 | 43 | Relay server, gateway endpoints |

### Rust Tests

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

251+ Rust tests across 13 modules covering: wallet CHI/Wei conversion, genesis validation, syncing logic, mining status, serialization, GPU error detection, Kademlia peer filtering, encryption, hosting server, rating storage, relay share proxy, and drive API.

### Scaled Integration Tests

12-phase stress test running against 30 Docker/k8s containers. See the [Docker and Scaled Testing](#docker-and-scaled-testing) section.

---

## Project Structure

```
chiral-network/
  src/                          # Frontend (Svelte 5 + TypeScript)
    App.svelte                  # Main app shell, routing, DHT auto-start on login
    pages/                      # 11 page components
      Account.svelte            # Wallet balance, transactions, reputation
      Download.svelte           # File search, download with CHI payments
      Drive.svelte              # Local file manager, seeding, sharing
      Mining.svelte             # CPU/GPU mining controls
      Network.svelte            # Peer list, DHT health, bootstrap status
      Hosts.svelte              # Hosting marketplace, agreements
      ChiralDrop.svelte         # Direct P2P file transfers
      Wallet.svelte             # Wallet creation, import, backup
      Settings.svelte           # Appearance, notifications, download dir
      Diagnostics.svelte        # Event log, system info
    lib/
      stores.ts                 # Svelte stores (wallet, settings, peers)
      dhtService.ts             # Frontend DHT service (event listeners before start)
      services/                 # 8 service modules
        walletService.ts        # Balance caching (10s TTL), chain ID
        gethService.ts          # Geth/mining status polling (10s interval)
        hostingService.ts       # Host discovery, agreements, echo retry
        driveApiService.ts      # Drive CRUD operations
        ratingApiService.ts     # Reputation batch lookups
        encryptionService.ts    # File encryption helpers
        walletBackupService.ts  # Email backup
        colorThemeService.ts    # Theme management
      components/               # Reusable UI components
      chiralDropStore.ts        # Wallet-specific ChiralDrop history
      toastStore.ts             # Toast notification system
      logout.ts                 # Logout with 5s DHT timeout + loading state

  src-tauri/                    # Backend (Rust, 23 source files)
    src/
      lib.rs                    # Thin Tauri command wrappers (103 commands)
      wallet.rs                 # All wallet logic (balance, tx, history, signing)
      rpc_client.rs             # Connection-pooled HTTP, batch RPC, cache
      dht.rs                    # libp2p Kademlia DHT, peer discovery, file transfer
      geth.rs                   # Geth lifecycle, mining, batch status queries
      geth_bootstrap.rs         # Bootstrap node health checking
      file_transfer.rs          # Chunked protocol (256KB, SHA-256)
      drive_api.rs              # Drive HTTP routes, preview pages
      drive_storage.rs          # Drive manifest and file storage
      hosting.rs                # Hosting types, MIME detection, persistence
      hosting_server.rs         # Axum gateway server
      relay_share_proxy.rs      # Reverse proxy + WebSocket tunnel
      rating_api.rs             # Reputation HTTP endpoints
      rating_storage.rs         # Elo computation
      encryption.rs             # AES-GCM + X25519 encryption
      wallet_backup_api.rs      # Email backup endpoint
      chain_rpc_api.rs          # Blockchain RPC proxy
      speed_tiers.rs            # Download cost (0.001 CHI/MB)
      event_sink.rs             # Frontend event emission
      bin/
        chiral.rs               # CLI client
        chiral_daemon.rs        # Headless daemon (44 API routes)
        relay_server.rs         # Production relay server

  tests/                        # Frontend tests (vitest, 585+ tests)
  scripts/
    stress-test-30-nodes.sh     # 12-phase, 35-test stress suite
    local-test-cluster.sh       # Local process-based test cluster
    full-feature-test.sh        # Feature validation suite
    extended-feature-test.sh    # Extended feature tests
    scaled-test.sh              # Scaled test orchestrator
    docker-test.sh              # Basic Docker test

  Dockerfile                    # Multi-stage build (daemon, relay, cli, test-node)
  Dockerfile.local              # Pre-built binary image
  docker-compose.yml            # General test network
  docker-compose.local-test.yml # Local isolated testing
  docker-compose.production-net.yml # 30 nodes, host networking, production relay
```

---

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `CHIRAL_RPC_ENDPOINT` | `http://130.245.173.73:8545` | Remote blockchain RPC fallback |
| `CHIRAL_GETH_SYNCMODE` | `full` | Geth sync mode (`full` or `snap`) |
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
- `geth/` -- Blockchain data and logs (archive mode)
- `agreements/` -- Hosting agreement JSON files
- `sites/` -- Hosted site files
- `headless/` -- Daemon PID file
- `tx_metadata.json` -- Persisted transaction metadata
- `hosted_sites.json` -- Hosted site registry
