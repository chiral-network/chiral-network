# Chiral Network Development Guide

## Overview

Chiral Network is a Tauri 2 desktop app with a Svelte 5 frontend and Rust backend.

Primary domains:

- DHT peer networking and file transfer
- Drive file management + seeding
- Download orchestration and payments
- Hosting marketplace and relay publication
- CPU/GPU mining controls
- Wallet management and reputation (Elo)
- Headless daemon + CLI operations

## Current Page Surface

- `/wallet` — wallet creation, import, backup
- `/network` — P2P connection, peer list, Geth status
- `/download` — file search, download with CHI payments
- `/drive` — local file management, seeding, sharing
- `/chiraldrop` — direct peer-to-peer file transfers
- `/hosts` — hosting marketplace (publish/browse/agreements)
- `/mining` — CPU/GPU mining controls
- `/account` — wallet info, reputation panel
- `/settings` — appearance, notifications, download directory
- `/diagnostics` — event log, system info

## Core Source Layout

- Frontend app shell: `src/App.svelte`
- Frontend stores/services: `src/lib/`
- Toast notifications: `src/lib/toastStore.ts`
- Backend command layer: `src-tauri/src/lib.rs` (thin wrappers)
- Wallet module: `src-tauri/src/wallet.rs` (balance, transactions, history, signing)
- Shared RPC client: `src-tauri/src/rpc_client.rs` (connection-pooled, batch requests)
- DHT/libp2p: `src-tauri/src/dht.rs`
- Drive persistence/API: `src-tauri/src/drive_storage.rs`, `src-tauri/src/drive_api.rs`
- Hosting server: `src-tauri/src/hosting_server.rs`
- Hosting types: `src-tauri/src/hosting.rs`
- Geth/mining integration: `src-tauri/src/geth.rs`
- Geth bootstrap health: `src-tauri/src/geth_bootstrap.rs`
- Speed/cost tiers: `src-tauri/src/speed_tiers.rs`
- Reputation (Elo): `src-tauri/src/rating_api.rs`, `src-tauri/src/rating_storage.rs`
- Wallet backup email: `src-tauri/src/wallet_backup_api.rs`
- Relay share proxy: `src-tauri/src/relay_share_proxy.rs`
- Encryption: `src-tauri/src/encryption.rs`
- File transfer: `src-tauri/src/file_transfer.rs`
- Chain RPC proxy: `src-tauri/src/chain_rpc_api.rs`
- Event sink: `src-tauri/src/event_sink.rs`
- Headless binaries: `src-tauri/src/bin/chiral.rs`, `src-tauri/src/bin/chiral_daemon.rs`
- Relay server: `src-tauri/src/bin/relay_server.rs`

## Architecture

```
rpc_client.rs  (shared HTTP client, batch JSON-RPC, response cache)
     ^               ^
     |               |
 wallet.rs       geth.rs  (process lifecycle, mining, RPC)
     ^               ^
     |               |
     +--- lib.rs ----+   (thin Tauri command wrappers, AppState)
              ^
              |
          dht.rs  (calls wallet::send_payment for file downloads)
```

No circular dependencies. `wallet.rs` imports from `rpc_client` and `geth` (for endpoint routing). `lib.rs` delegates to both.

## Command Surface (Tauri)

Command registration is in `tauri::generate_handler![...]` in `src-tauri/src/lib.rs`.

Main categories:

- DHT/network commands
- File transfer/download commands
- Wallet/transaction commands (delegated to `wallet.rs`)
- Geth/mining (CPU and GPU) commands
- Drive CRUD/seeding/share commands
- Hosting/marketplace commands
- Encryption commands
- Diagnostics and lifecycle commands

## Development Commands

```bash
# frontend deps
npm install

# desktop dev
npm run tauri:dev

# frontend build
npm run build

# frontend tests
npm test

# rust tests
cargo test --manifest-path src-tauri/Cargo.toml

# rust compile check
cargo check --manifest-path src-tauri/Cargo.toml
```

## Headless Mode

```bash
# Direct daemon start
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral_daemon -- --port 9419 --auto-start-dht

# Via CLI
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon start --port 9419
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon status --port 9419
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon stop --port 9419
```

Headless daemon API endpoints (port 9419 by default):

- **Health**: GET `/api/health`, GET `/api/ready`
- **Wallet**: POST `wallet/create`, `wallet/import`, `wallet/balance`, `wallet/send`, `wallet/receipt`, `wallet/history`, `wallet/faucet`; GET `wallet`, `wallet/chain-id`
- **DHT**: POST `dht/start`, `dht/stop`, `dht/put`, `dht/get`, `dht/ping`, `dht/echo`; GET `dht/health`, `dht/peers`, `dht/peer-id`, `dht/listening-addresses`
- **Files**: POST `dht/register-shared-file`, `dht/unregister-shared-file`, `dht/request-file`, `dht/send-file`, `file/search`
- **ChiralDrop**: GET `drop/inbox`, `drop/outgoing`; POST `drop/accept`, `drop/decline`
- **Geth**: POST `geth/install`, `geth/start`, `geth/stop`; GET `geth/status`, `geth/logs`
- **Mining**: POST `mining/start`, `mining/stop`, `mining/miner-address`; GET `mining/status`, `mining/blocks`
- **Hosting**: POST `hosting/publish-ad`; GET `hosting/registry`
- **Drive**: Full CRUD via `/api/drive/*` routes (requires `X-Owner` header)
- **Diagnostics**: GET `bootstrap-health`

All headless paths are prefixed with `/api/headless/` except health, ready, and drive routes.

## Docker Testing

```bash
# 30 production-connected nodes (host networking)
docker compose -f docker-compose.production-net.yml up -d

# Run stress test against 30 nodes
bash scripts/stress-test-30-nodes.sh

# Local isolated testing with relay
docker compose -f docker-compose.local-test.yml up -d --scale node=10
```

## Geth / Blockchain

- Chain ID: `98765` (Chiral Network)
- Consensus: Ethash (PoW)
- Block reward: 5 CHI
- Sync mode: `full` (default, configurable via `CHIRAL_GETH_SYNCMODE`)
- GC mode: `archive` (keeps all state, prevents block regression on restart)
- Genesis difficulty: `0x400000`
- Bootstrap enode: `130.245.173.73:30303`
- RPC: local at `127.0.0.1:8545` when running, remote fallback `130.245.173.73:8545`

## Reputation System

Elo scores are computed from transfer outcomes only (no user ratings). The formula uses:
- Transfer outcome: completed (1.0) or failed (0.0)
- Time weighting: recent events (within 180-day lookback) weighted more heavily
- Amount weighting: logarithmic scaling based on CHI transferred
- Bounded Elo updates clamped to 0-100

REST endpoints on the relay server (`http://130.245.173.73:8080`):
- `POST /api/ratings/transfer` — record transfer outcome
- `GET /api/ratings/:wallet` — get Elo + event history
- `POST /api/ratings/batch` — batch Elo lookup

## Toast Notifications

The toast store (`src/lib/toastStore.ts`) provides:
- `toasts.show(message, type, duration?)` — always show
- `toasts.detail(message, description, type, duration?)` — always show with title + description
- `toasts.notify(key, message, type, duration?)` — respects notification settings
- `toasts.notifyDetail(key, message, description, type, duration?)` — respects notification settings

Use `notify`/`notifyDetail` for event-driven notifications (downloads, network, mining, payments) so users can toggle them in Settings. Use `show`/`detail` for user-initiated action feedback.

## Relay Server

The relay server runs on `130.245.173.73` as a systemd service (`relay-server.service`):
- **P2P relay**: port 4001 (libp2p circuit relay v2, Kademlia routing)
- **HTTP API**: port 8080 (reputation, drive proxy, wallet backup, site hosting)
- Filters private IPs from Kademlia routing table (only stores public + relay circuit addresses)
- Max 256 circuit reservations, 16 per peer

SMTP env vars: `CHIRAL_WALLET_EMAIL_SMTP_HOST`, `CHIRAL_WALLET_EMAIL_FROM` (required); `CHIRAL_WALLET_EMAIL_SMTP_USERNAME`, `CHIRAL_WALLET_EMAIL_SMTP_PASSWORD` (optional for local postfix).

## Implementation Notes

- Prefer Tauri `invoke()` paths for app runtime behavior.
- All wallet logic lives in `wallet.rs` — `lib.rs` contains thin wrappers only.
- All RPC calls use the shared `rpc_client.rs` (connection-pooled, 5s timeout).
- Balance queries use a 5-second `RpcCache` to avoid duplicate RPC calls.
- Transaction sending batches nonce + balance + gasPrice in a single HTTP request.
- Transaction metadata persisted to `~/.local/share/chiral-network/tx_metadata.json`.
- Drive seeding state is restored from backend at DHT startup.
- DHT auto-restarts on login (tied to `isAuthenticated` store).
- Kademlia re-bootstraps every 30 seconds to discover new peers.
- Event listeners registered before DHT start to avoid missed events.
- Stale peers removed after 5 minutes without Kademlia/connection activity.
- Hosting: accepted agreements auto-seed files via `seed_hosted_file` command.
- Hosting: cancellation removes seeder from DHT and cleans up Drive.
- App shows a close confirmation dialog before quitting (wired in `src/App.svelte`).
- Wallet backup email step is optional (skip button) during wallet creation.
- Logout has 5s timeout on DHT stop + loading state to prevent hanging.
