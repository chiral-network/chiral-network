# Chiral Network

Decentralized peer-to-peer file sharing, local Drive seeding, hosting marketplace, and CHI-based payments — built as a Tauri 2 desktop app with a headless daemon mode for servers and Docker.

## Stack

- **Frontend**: Svelte 5 + TypeScript + Tailwind CSS
- **Desktop runtime**: Tauri 2
- **Backend**: Rust (`src-tauri`)
- **Networking**: libp2p 0.53 (Kademlia DHT, circuit relay, chunked file transfer)
- **Blockchain**: Core-Geth v1.12.20 (CHI chain, Ethash PoW, chain ID 98765)
- **Relay**: Production relay at `130.245.173.73` (libp2p relay + HTTP API)

## Features

- **Wallet** — create, import (private key or mnemonic), optional email backup
- **Network** — DHT peer connectivity, Kademlia discovery, relay circuit NAT traversal
- **Download** — search files by hash, download with CHI payments to burn address
- **Drive** — local file management, seeding to P2P network, paid share links via relay
- **ChiralDrop** — direct peer-to-peer file transfers with optional pricing
- **Hosting** — marketplace: CDN servers (always-on), peer hosts, hosting agreements, auto-seed
- **CDN** — always-on file hosting service with market-based dynamic pricing
- **Mining** — CPU mining (GPU mining via ethminer on Linux/Windows, OpenCL on macOS)
- **Security** — ECDSA-signed DHT records, on-chain payment verification, 0.5% platform fee
- **Account** — wallet balance, transaction history, Elo reputation panel
- **Settings** — appearance (dark/light/system), notification preferences, download directory
- **Diagnostics** — structured event log, system info

## Development

```bash
# Install frontend dependencies
npm install

# Run desktop app in dev mode
npm run tauri:dev

# Build frontend only
npm run build

# Build desktop app
npm run tauri:build
```

## Testing

```bash
# Frontend tests (Vitest — 585+ tests)
npm test

# Rust tests (271+ tests)
cargo test --manifest-path src-tauri/Cargo.toml

# Rust compile check
cargo check --manifest-path src-tauri/Cargo.toml

# 30-node stress test (requires Docker containers running)
bash scripts/stress-test-30-nodes.sh
```

## Headless Daemon

The headless daemon (`chiral_daemon`) runs without a GUI for servers and Docker.

```bash
# Build
cargo build --manifest-path src-tauri/Cargo.toml --release --bin chiral_daemon

# Run with auto-start
./chiral_daemon --port 9419 --auto-start-dht

# With mining
./chiral_daemon --port 9419 --auto-start-dht --auto-start-geth --auto-mine \
  --miner-address 0xYOUR_ADDRESS --mining-threads 4
```

### API Endpoints

All endpoints prefixed with `/api/headless/` unless noted.

| Category | Endpoints |
|----------|-----------|
| Health | `GET /api/health`, `GET /api/ready` |
| Wallet | `POST wallet/create`, `wallet/import`, `wallet/balance`, `wallet/send`, `wallet/receipt`, `wallet/history`, `wallet/faucet`; `GET wallet`, `wallet/chain-id` |
| DHT | `POST dht/start`, `dht/stop`, `dht/put`, `dht/get`, `dht/ping`, `dht/echo`; `GET dht/health`, `dht/peers`, `dht/peer-id` |
| Files | `POST file/search`, `dht/register-shared-file`, `dht/request-file`, `dht/send-file` |
| Mining | `POST mining/start`, `mining/stop`, `mining/miner-address`; `GET mining/status` |
| Geth | `POST geth/install`, `geth/start`, `geth/stop`; `GET geth/status`, `geth/logs` |
| Hosting | `POST hosting/publish-ad`; `GET hosting/registry` |
| CDN | `POST cdn/upload`; `GET cdn/files`, `cdn/pricing`, `cdn/status`; `DELETE cdn/files/:hash` |
| Drive | Full CRUD via `/api/drive/*` (requires `X-Owner` header) |

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `CHIRAL_AUTO_START_DHT` | `false` | Auto-start DHT on daemon launch |
| `CHIRAL_AUTO_START_GETH` | `false` | Auto-start Geth node |
| `CHIRAL_AUTO_MINE` | `false` | Auto-start mining |
| `CHIRAL_MINER_ADDRESS` | — | Wallet address for mining rewards |
| `CHIRAL_MINING_THREADS` | `1` | CPU mining thread count |
| `CHIRAL_GETH_SYNCMODE` | `full` | Geth sync mode (`full` or `snap`) |
| `CHIRAL_RPC_ENDPOINT` | `http://130.245.173.73:8545` | Remote RPC fallback |

## Docker

```bash
# Build image from pre-compiled binary
docker build -f Dockerfile.local -t chiral-network-node .

# Run 30 nodes connected to production relay
docker compose -f docker-compose.production-net.yml up -d

# Run local isolated test cluster with relay
docker compose -f docker-compose.local-test.yml up -d --scale node=10
```

## Blockchain

| Parameter | Value |
|-----------|-------|
| Chain ID | 98765 |
| Consensus | Ethash (PoW) |
| Block reward | 5 CHI |
| Difficulty | 0x400000 (genesis) |
| Sync mode | full (archive GC) |
| Gas price | 0 (free transactions) |
| Download cost | 0.01 CHI per MB |
| Platform fee | 0.5% on all transactions |
| Gas limit | 4,700,000 per block |
| P2P port | 30303 |
| RPC port | 8545 (local only) |
| Bootstrap | `130.245.173.73:30303` |

## Relay Server

The relay server (`relay_server` binary) runs on `130.245.173.73`:
- **Port 4001**: libp2p circuit relay v2 + Kademlia DHT routing
- **Port 8080**: HTTP API (reputation, drive proxy, wallet backup, site hosting)

```bash
# Build and deploy
cargo build --release --bin relay_server
scp target/release/relay_server root@130.245.173.73:/usr/local/bin/
ssh root@130.245.173.73 'systemctl restart relay-server'
```

The relay filters private IPs from its Kademlia routing table — only stores public and relay circuit addresses so remote peers get routable entries from DHT lookups.

## CDN Service

Always-on file hosting — users upload files to the CDN so they stay available when the user goes offline.

- **Deployed at**: `130.245.173.73:9420` (systemd service: `cdn-server.service`)
- **Capacity**: 227 GB available
- **Pricing**: Market-based — `max(floor, median_peer_price × 1.2)`

```bash
# Upload a file
curl -X POST http://130.245.173.73:9420/api/cdn/upload \
  -H "Content-Type: application/json" \
  -d '{"fileName":"file.pdf","fileData":"<base64>","ownerWallet":"0xYOU","durationDays":30}'

# List your files
curl http://130.245.173.73:9420/api/cdn/files?owner=0xYOU

# Check pricing
curl http://130.245.173.73:9420/api/cdn/pricing?sizeMb=100&durationDays=30

# Delete
curl -X DELETE http://130.245.173.73:9420/api/cdn/files/HASH?owner=0xYOU
```

## Security

- **Signed DHT records**: File metadata and seeder entries are ECDSA-signed by the publisher/seeder wallet
- **Payment verification**: Seeders verify on-chain tx receipt before serving file chunks
- **Platform fee**: 0.5% of all transactions (split: 99.5% to seller, 0.5% to platform)
- **Relay IP filtering**: Only public and relay circuit addresses stored in Kademlia

## Reputation System

Elo scores (0–100, base 50) computed from transfer outcomes only:
- **Completed transfer**: positive adjustment
- **Failed transfer**: negative adjustment
- **Amount weighting**: logarithmic scaling based on CHI
- **Time decay**: 180-day lookback with recency weighting

Endpoints on relay (`130.245.173.73:8080`):
- `POST /api/ratings/transfer` — record outcome
- `GET /api/ratings/:wallet` — get Elo + history
- `POST /api/ratings/batch` — batch lookup

## Project Structure

```
src/                          # Svelte 5 frontend
├── pages/                    # 10 route pages
├── lib/
│   ├── stores.ts             # Svelte stores (wallet, peers, settings)
│   ├── dhtService.ts         # DHT service singleton
│   ├── services/             # 8 service modules
│   ├── components/           # Reusable components
│   └── types/                # TypeScript type definitions
src-tauri/
├── src/
│   ├── lib.rs                # Tauri command wrappers (thin delegation)
│   ├── wallet.rs             # All wallet/transaction logic
│   ├── rpc_client.rs         # Shared HTTP client, batch RPC, cache
│   ├── dht.rs                # libp2p DHT, Kademlia, file transfer
│   ├── geth.rs               # Geth process management, mining
│   ├── geth_bootstrap.rs     # Bootstrap node health checking
│   ├── drive_api.rs          # Drive HTTP API routes
│   ├── drive_storage.rs      # Drive persistence layer
│   ├── hosting.rs            # Hosting types and persistence
│   ├── hosting_server.rs     # Site hosting HTTP server
│   ├── relay_share_proxy.rs  # Relay proxy + WebSocket tunnel
│   ├── rating_api.rs         # Reputation API routes
│   ├── rating_storage.rs     # Elo computation
│   ├── encryption.rs         # File encryption
│   ├── file_transfer.rs      # Chunked transfer protocol
│   ├── speed_tiers.rs        # Download cost calculation
│   └── bin/
│       ├── chiral.rs          # CLI client
│       ├── chiral_daemon.rs   # Headless daemon (57+ API endpoints)
│       └── relay_server.rs    # Production relay server
scripts/
├── stress-test-30-nodes.sh   # 12-phase, 35-test stress suite
├── local-test-cluster.sh     # Local process-based test cluster
└── full-feature-test.sh      # Feature validation suite
```

## License

Proprietary — all rights reserved.
