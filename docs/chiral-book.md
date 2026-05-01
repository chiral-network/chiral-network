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
- [Version Enforcement](#version-enforcement)
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

### Folder Bundles
- Sell an entire folder under a single content-addressed hash (SHA-256 of the owner address plus the sorted `(rel_path, file_hash)` list of every file in the folder).
- The seller publishes a `chiral_folder_<hash>` manifest to the DHT and registers as a Kademlia provider for that hash. Each child file is seeded as normal.
- Buyers paste the folder hash into Search, see the file list, total CHI cost, and the set of seeders that hold every file in the bundle (the "common seeders" intersection).
- `Download Folder` confirms the total cost once and starts each child file's existing chunked transfer against the chosen seeder.

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
- Expiration cleanup runs every 60 seconds — files past their paid duration are removed from disk and from the DHT seeder list.
- CDN can also host static sites (HTML/JS/CSS bundles), separate from per-file uploads.
- Download page queries CDN servers directly as fallback when DHT search is slow.
- Deployed at `130.245.173.73:9420` with 227 GB capacity.
- Desktop app: Hosts → CDN Servers tab → Upload from Drive with payment confirmation.

### Security

Trust boundaries are enforced cryptographically end-to-end. Every long-lived record on the DHT and every stateful HTTP endpoint goes through ECDSA verification before its contents are trusted.

**Signed records (writers refuse to publish unsigned; readers drop unsigned/invalid):**

- File metadata (`chiral_file_<hash>`) — signed by publisher wallet over a length-prefixed canonical payload. `search_file` rejects unsigned/invalid metadata as not-found.
- Seeder entries (`chiral_seeder_<hash>_<peer>`) — signed by the seeder's wallet, binding peer ID + file hash + wallet address. `fetch_seeders` drops empty-signature non-stub entries.
- Folder manifests (`chiral_folder_<hash>`) — signed by `owner_wallet`. `search_folder` drops unsigned/invalid bundles.
- Chunked-transfer `FileInfo` envelopes — signed by the seeder's wallet. The downloader verifies before consuming the seeder's claimed `wallet_address` / `price_wei` and fails over to other seeders on bad signatures (closes the payment-redirection vector where a hostile seeder could substitute its own wallet).

**HTTP authentication (replaces the previously-trusted bare `X-Owner` header):**

- Authenticated routes require both `X-Owner: 0x<wallet>` and `X-Owner-Sig: <unix_ts>:<hex_signature>` headers.
- Signed payload is length-prefixed canonical bytes binding wallet ↔ HTTP method ↔ path-with-query ↔ timestamp; a captured proof can't be replayed against a different endpoint within its ±5-minute window.
- Server-side `auth::owner_proof_middleware` recovers the secp256k1 signer and rejects with 401 on mismatch / expiry.
- Applied to: `/api/drive/*`, `POST /api/ratings/transfer`, and the unregister DELETEs on relay register routes.
- Tauri command `compute_owner_proof` produces the header in-process; wallet private keys never leave the desktop app.

**Relay registration (FM-A04/A05):**

- `register_share` / `register_site` POST bodies carry an ECDSA signature by `owner_wallet` over `(operation, id, owner_wallet, origin_url)`. Captured proofs can't be reused with a substituted origin URL.
- First-claim-wins is enforced: an existing record can only be overwritten by the wallet that originally signed it.
- Origin-URL allowlist rejects RFC1918, CGNAT, link-local (incl. AWS / GCP cloud metadata at `169.254.169.254`), unique-local IPv6, multicast, broadcast, IPv4-mapped IPv6 variants of any of those, and anything outside `http(s)://`. Loopback stays accepted because `fix_origin_url` substitutes the registrant's public IP at request time.

**Payment verification:**

- On-chain tx receipt checked before serving file chunks. Chain ID is verified so cross-chain replays of signed txs are rejected.
- Spent-tx ledger keys on `(tx_hash, file_hash)` so one payment ↔ one file delivery (no replay across different priced files seeded by the same wallet).
- Drive shares additionally bind each redeemed `tx_hash` to the first share token it unlocks, so a publicly-shared `?access=<tx>` URL can't unlock any of the wallet's other shares.
- `wait_for_tx_mined` is checked separately from `verify_tx_details` so the seeder can return a retryable "not yet confirmed" answer when chain propagation is slow.
- CDN payment uses exact `u128` ceil-rounded math — no `f64` truncation, no percentage tolerance.

**Operational hardening:**

- Local-daemon CORS allowlists only Tauri webview origins (blocks CSRF from arbitrary websites visited by the user); relay-mode keeps `Any`.
- `dht_put` headless route refuses raw writes to reserved-namespace keys (returns 403); each namespace has its own dedicated signed-publication command.
- Drive multipart upload caps body at 500 MiB before allocation; `is_item_under_shared_root` short-circuits parent cycles.
- ECDSA signatures enforce low-`s` (EIP-2), so signature hex is unique per (key, message).
- Relay filters private IPs from Kademlia routing table.
- Stop seeding removes peer from DHT seeder list (prevents ghost seeders).
- 0.5% platform fee on all transactions (99.5% to seller, 0.5% to platform); `split_payment` is the single source of truth and `seller + fee == total` exactly.
- Wallet RPC reads use an ordered fallback list (`rpc_client::call_with_fallbacks`): direct canonical Geth → relay's `/api/chain/rpc` proxy. Either path can be down without taking the wallet UI offline.
- RPC failures surface as a yellow "canonical RPC unreachable" banner in the wallet UI rather than a misleading `0.00`. Mining page renders an inline divergence warning when local-Geth balance disagrees with canonical-RPC balance for the miner address (private-fork diagnostic).
- Geth's `--http.api` deliberately omits `admin` so `admin_stopRPC` cannot be called over the public RPC port (admin remains available over the IPC socket).

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
| Version Policy | `version.rs` | `VersionPolicy` types, Ed25519 sign/verify, `is_acceptable_remote_policy`, global effective-policy slot |

Total: 24 Rust source files, 5 binary targets.

### Binary Targets

| Binary | Source | Purpose |
|--------|--------|---------|
| `chiral-network` | `src-tauri/src/main.rs` | Desktop application (Tauri) |
| `chiral` | `src-tauri/src/bin/chiral.rs` | Command-line interface |
| `chiral_daemon` | `src-tauri/src/bin/chiral_daemon.rs` | Headless daemon server |
| `relay_server` | `src-tauri/src/bin/relay_server.rs` | Relay and reputation server |
| `chiral-policy-sign` | `src-tauri/src/bin/chiral_policy_sign.rs` | Operator CLI: keygen / sign / verify a `VersionPolicy` with the project's offline Ed25519 key |

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

The Elo-based reputation system provides a trust score for each wallet address based on file transfer outcomes. It replaces an earlier 1-to-5-star user-rating model; historical rating data was reset to start the new system fresh.

### Scale

- Range: `0` to `100` (clamped)
- Base score for new wallets: `50`

### Inputs

Only events within the last **180 days** (the lookback window) are considered. Within the window, older events are weighted less than recent events.

1. File transfer outcomes — completed or failed.
2. Amount of CHI transferred on completed payments (logarithmic weighting).

### Event Effects

- Successful transfer: positive Elo adjustment.
- Failed transfer: negative Elo adjustment.
- Higher CHI amount (recent): larger positive contribution via the amount weight.

### Elo Formula

For each event in the lookback window:

1. **Time weight** (`w_time`): linear decay from `1.0` (today) to `0.0` (180 days ago).
2. **Amount weight** (`w_amount`): `1.0 + clamp(ln(1 + chi) / ln(51), 0, 1)` — ranges from `1.0` (free transfer) to `2.0` (50+ CHI).
3. **Outcome**: `1.0` for completed, `0.0` for failed.
4. **Expected score**: `1 / (1 + 10^((50 - elo) / 12))`.
5. **K factor**: `4 * w_time * w_amount`.
6. **Update**: `elo = clamp(elo + K * (outcome - expected), 0, 100)`.

### API Endpoints (Relay Server)

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/ratings/:wallet` | GET | Get Elo score and event history for a wallet |
| `/api/ratings/batch` | POST | Batch lookup of Elo scores for multiple wallets |
| `/api/ratings/transfer` | POST | Record a transfer outcome (completed/failed) |

Wallet addresses are normalized to lowercase for consistent lookup.

### Integration

- The Download page displays seeder Elo scores next to each search result.
- After a file transfer completes or fails, the outcome is automatically reported to the relay.
- Paid-transfer events are verified against the on-chain tx (sender, recipient, amount) before being recorded — the backend does not trust frontend-submitted event data. Future hardening can add per-event cryptographic attestations.

---

## Version Enforcement

Chiral Network ships a defence-in-depth scheme for keeping vulnerable client builds off the network. It is layered so a single bypass does not disable enforcement.

### `VersionPolicy`

The on-the-wire policy (`src-tauri/src/version.rs`) carries:

| Field | Meaning |
|-------|---------|
| `minRequired` | Versions strictly below this are blocked. |
| `recommended` | Versions below this trigger a soft "update available" nudge. |
| `downloadUrl` | Where the UI sends users to upgrade. |
| `message` | Optional human-readable reason (e.g. "fixes payment bug"). |
| `issuedAt` | Unix-seconds the policy was issued (used for rollback protection). |
| `validUntil` | Unix-seconds after which clients should re-fetch. `0` = no expiry. |
| `signature` | Hex Ed25519 signature over a length-prefixed canonical payload. |

Comparing the running build's `CARGO_PKG_VERSION` against the effective policy returns one of three states:

- `ok` — version ≥ `recommended`, no UI.
- `recommended` — `recommended > version ≥ minRequired`, soft banner the user can dismiss for the session.
- `required` — `version < minRequired`, full-screen blocking modal (`UpdateGate.svelte`).

### Enforcement layers

1. **UI gate (`UpdateGate.svelte`)** — driven by `versionStore` over the Tauri `get_version_status` command; renders the soft banner / hard modal.
2. **Tauri command gate** — `ensure_version_supported` is called from `start_dht_internal` and `start_download` so a stale build cannot join the DHT or initiate a download.
3. **HTTP middleware** — every `/api/*` route on the gateway server (relay, daemon, desktop hosting) reads `X-Chiral-Client-Version` and returns `426 Upgrade Required` (with the policy JSON in the body) when the client is below `minRequired`. Health and `/api/version-policy` are exempted.
4. **libp2p Identify** — `agent_version` is set to `chiral/<version>` and the Identify handler disconnects peers whose advertised version is below `minRequired`, blacklisting them so they aren't re-dialled.

### Distribution

Every binary embeds a `bundled_policy()` snapshot at compile time. On startup, the desktop app probes `http://130.245.173.73:8080/api/version-policy` (the relay's gateway) and promotes the result via `update_effective_policy()` if it passes acceptance. The same `/api/version-policy` route is mounted by the relay server, the headless daemon, and the desktop's hosting server, so any peer can read the network's current view of the policy.

A global `EFFECTIVE_POLICY` slot (`OnceCell<RwLock<VersionPolicy>>`) holds the live policy. It's a sync `parking_lot::RwLock` because the libp2p event loop reads it from non-async contexts.

### Acceptance rules (`is_acceptable_remote_policy`)

A fetched policy replaces the current effective policy only if:

1. **Rollback protection** — `remote.issuedAt` is not older than `current.issuedAt`.
2. **Signature** — if `remote.signature` is non-empty, it must verify against `POLICY_PUBLIC_KEY`.
3. **Unsigned transitional path** — if `current.signature` is empty *and* `remote.signature` is empty *and* `remote.minRequired` does not raise the floor above the binary's bundled `minRequired`, the policy is accepted. Once a signed policy has been adopted, unsigned remotes are no longer accepted.

The transitional path lets relays advertise the recommended-version nudge before the project's offline signing key is wired in, while still preventing a hostile relay from raising `minRequired` to lock honest peers out.

### Operator CLI: `chiral-policy-sign`

The `chiral-policy-sign` binary signs and verifies policies with the project's offline Ed25519 key:

```bash
# Generate a new project keypair (paste the public hex into POLICY_PUBLIC_KEY).
chiral-policy-sign keygen

# Sign a policy JSON.
chiral-policy-sign sign --key <secret-hex> --in policy.json --out policy.signed.json

# Verify (defaults to the binary's compiled-in public key; --pub overrides).
chiral-policy-sign verify --in policy.signed.json
```

The compile-time `POLICY_PUBLIC_KEY` constant is a 32-byte zero placeholder. Operators activate signed policies at deploy time without recompiling by setting the `CHIRAL_POLICY_PUBLIC_KEY` environment variable to the 32-byte public key (hex, with or without `0x` prefix); `version::policy_public_key()` resolves the env var on first access and caches it. All three binaries (desktop, daemon, relay) print a `[VERSION]` line on startup confirming whether signed policies are enabled or warning that the placeholder is still in use. Until a real key is wired in, only the unsigned-transitional path can promote a remote policy.

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

All headless paths are prefixed with `/api/headless/` except health, ready, drive, and the publicly-mounted `/api/version-policy`.

| Category | Endpoints |
|----------|-----------|
| Health | `GET /api/health`, `GET /api/ready`, `GET runtime` |
| Version policy | `GET /api/version-policy` — returns the currently-effective `VersionPolicy` (mounted on the gateway router; available on relay, daemon, and desktop hosting server alike) |
| Wallet | `GET wallet`, `POST wallet/create`, `wallet/import`, `wallet/balance`, `wallet/send`, `wallet/receipt`, `wallet/history`, `wallet/faucet`; `GET wallet/chain-id` |
| DHT | `POST dht/start`, `dht/stop`, `dht/put`, `dht/get`, `dht/ping`, `dht/echo`; `GET dht/health`, `dht/peers`, `dht/peer-id`, `dht/listening-addresses` |
| Files | `POST file/search`, `dht/register-shared-file`, `dht/unregister-shared-file`, `dht/request-file`, `dht/send-file` |
| ChiralDrop | `GET drop/inbox`, `drop/outgoing`; `POST drop/accept`, `drop/decline` |
| Geth | `POST geth/install`, `geth/start`, `geth/stop`; `GET geth/status`, `geth/logs` |
| Mining | `POST mining/start`, `mining/stop`, `mining/miner-address`; `GET mining/status`, `mining/blocks` |
| Hosting | `POST hosting/publish-ad`; `GET hosting/registry` |
| Folder bundles | Tauri-only: `publish_drive_folder`, `unpublish_drive_folder`, `search_folder` (one content-addressed hash per folder) |
| CDN | `POST cdn/upload`; `GET cdn/files`, `cdn/pricing`, `cdn/status`; `DELETE cdn/files/:hash`; `PUT cdn/files/:hash` |
| Drive | Full CRUD via `/api/drive/*` (requires both `X-Owner` and `X-Owner-Sig: <unix_ts>:<hex_signature>` headers; see Authentication below) |
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
| `CHIRAL_POLICY_PUBLIC_KEY` | placeholder zeros | 32-byte hex (with or without `0x` prefix) of the project's Ed25519 policy-signing public key. Setting this activates signed `VersionPolicy` updates without recompiling. Generate the matching keypair with `chiral-policy-sign keygen`. |
| `CHIRAL_WALLET_KEY_FILE` | none | Path to a file containing a single hex secp256k1 private key (with or without `0x` prefix; mode 0600 expected). At startup the daemon loads the key, derives the address, and populates `state.wallet` so the CDN module can sign `chiral_seeder_*` / `chiral_file_*` records and `ChunkResponse::FileInfo` envelopes. Without it, the CDN runs with empty signatures and clients reject every record it publishes. Used in production at `/etc/chiral-cdn-wallet.key` on the canonical relay. |

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
