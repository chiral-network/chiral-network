# Architecture

## Overview

Chiral Network is a decentralized peer-to-peer file sharing application built on Tauri 2. The frontend is Svelte 5 with TypeScript. The backend is Rust. Networking uses libp2p 0.53 with Kademlia DHT. Blockchain operations use a Geth node connected to a private Ethereum-compatible chain.

## Tech Stack

| Layer | Technology |
|-------|-----------|
| Frontend | Svelte 5, TypeScript, Tailwind CSS |
| Desktop Runtime | Tauri 2 (Rust) |
| Networking | libp2p 0.53 (Kademlia, mDNS, relay, DCUtR, Noise, Yamux) |
| Blockchain | Core-Geth, secp256k1 transaction signing |
| Encryption | AES-256-GCM, X25519 key exchange, Ed25519 signing |
| HTTP Server | Axum 0.7 (local gateway for Drive/Hosting) |
| Serialization | CBOR for P2P messages, JSON for DHT records |

## Runtime Model

The application runs as a Tauri desktop app. The Svelte frontend communicates with the Rust backend exclusively through Tauri's `invoke()` IPC mechanism. The backend manages all network connections, file I/O, blockchain operations, and cryptographic functions.

```
Svelte Frontend
    |
    | invoke("command_name", { params })
    v
Tauri IPC Bridge
    |
    v
Rust Backend (lib.rs)
    |
    +-- DhtService (dht.rs)        -- P2P networking, file transfer
    +-- Geth module (geth.rs)      -- Blockchain node lifecycle
    +-- Encryption (encryption.rs) -- File encryption, key management
    +-- Drive storage (drive_storage.rs) -- Local file management
    +-- Hosting (hosting.rs)       -- Static site hosting
    +-- Speed tiers (speed_tiers.rs) -- Download rate limiting
```

Events flow back from the backend to the frontend via Tauri's `app.emit()`. The frontend listens with `listen()` from `@tauri-apps/api/event`. Key events include `peer-discovered`, `file-download-progress`, `file-download-complete`, `file-download-failed`, `file-transfer-request`, and `connection-established`.

## State Management

Frontend state lives in Svelte stores defined in `src/lib/stores.ts`. Key stores:

| Store | Type | Purpose |
|-------|------|---------|
| `isAuthenticated` | writable(bool) | Auth gate for routing |
| `walletAccount` | writable | Wallet address and private key |
| `peers` | writable(PeerInfo[]) | Connected peers list |
| `networkConnected` | writable(bool) | DHT connection status |
| `settings` | writable(AppSettings) | App configuration |
| `blacklist` | writable(BlacklistEntry[]) | Blocked peers |
| `isDarkMode` | derived | Theme state |

Settings persist to localStorage. The wallet account persists to sessionStorage. Peer lists and network stats are ephemeral and populated from backend events.

## P2P Networking

The `DhtService` in `dht.rs` manages a libp2p swarm with 10 sub-behaviours composed into a single `DhtBehaviour` struct:

| Behaviour | Protocol | Purpose |
|-----------|----------|---------|
| `kad` | Kademlia | DHT record storage, peer routing |
| `mdns` | mDNS | Local network peer discovery |
| `relay_client` | Circuit Relay v2 | NAT traversal via relay nodes |
| `dcutr` | DCUtR | Direct connection upgrade through relay |
| `ping` | libp2p Ping | Liveness checks |
| `identify` | Identify | Protocol negotiation, address exchange |
| `ping_protocol` | `/chiral/ping/1.0.0` | Application-level ping with payload |
| `file_transfer` | `/chiral/file-transfer/1.0.0` | Direct single-message file transfer |
| `file_request` | `/chiral/file-request/3.0.0` | Chunked file download protocol |
| `echo_protocol` | `/chiral/echo/1.0.0` | Echo request/response for testing |

The swarm runs in a Tokio task. Commands from Tauri handlers are sent to the swarm via an MPSC channel (`SwarmCommand` enum). Responses return through oneshot channels or Tauri events.

## File Transfer Protocol

Files transfer using a chunked protocol. The requesting peer sends a `ChunkRequest::FileInfo` to get file metadata (size, chunk count, per-chunk SHA-256 hashes). It then requests each 256KB chunk individually with `ChunkRequest::Chunk`. Each chunk is verified against its hash before writing. The full file hash is verified on completion.

Speed tiers control download rate: Standard limits to 1 MB/s (~0.25s inter-chunk delay), Premium to 5 MB/s (~0.05s delay), Ultra has no limit. Tier payments go to a burn address. Per-file seeder pricing is separate and paid directly to the seeder's wallet.

Seeder multiaddresses are stored in DHT metadata alongside peer IDs, allowing downloaders to dial seeders directly without prior connection.

## Blockchain Integration

The application integrates with a private Ethereum-compatible chain via Core-Geth. The Geth binary is downloaded and managed by the app. RPC calls go to a shared endpoint at `130.245.173.73:8545` for balance queries and transaction history. Transaction signing happens locally using secp256k1 with Keccak hashing and RLP encoding.

Mining uses Geth's built-in ethash miner with configurable thread count. Block rewards are tracked and displayed in the Mining page.

## Encryption

File encryption uses AES-256-GCM with keys derived via X25519 Diffie-Hellman exchange. Each node generates an X25519 keypair derived from its wallet private key. Public keys are published to DHT for lookup by peer ID. Encrypted file bundles include the ephemeral public key, nonce, and ciphertext.

Transport security uses the Noise protocol (XX handshake) for all libp2p connections.

## Local Services

An Axum HTTP server starts automatically on port 9419 during app setup. It serves Drive file content and handles hosting operations. The server provides routes for Drive CRUD operations and share token resolution. For hosted sites, it can proxy through a relay server so content remains accessible when the host is behind NAT.

## Source Tree

```
src/
  App.svelte              -- Router, auth gate, global event listeners
  main.ts                 -- Svelte mount point
  pages/                  -- 11 application pages
  lib/
    stores.ts             -- Svelte stores (auth, peers, settings)
    dhtService.ts         -- Frontend DHT wrapper
    chiralDropStore.ts    -- ChiralDrop state and history
    components/           -- Shared UI components
    services/             -- Frontend service modules

src-tauri/src/
  lib.rs                  -- Tauri commands, AppState, orchestration
  main.rs                 -- Binary entry point
  dht.rs                  -- DhtService, DhtBehaviour, swarm event loop
  geth.rs                 -- Geth lifecycle and RPC
  geth_bootstrap.rs       -- Bootstrap node management
  encryption.rs           -- AES-256-GCM, X25519, key management
  file_transfer.rs        -- FileTransferService, retry logic
  speed_tiers.rs          -- Rate limiting, cost calculation
  drive_storage.rs        -- Drive manifest, local file storage
  drive_api.rs            -- Drive HTTP API routes
  hosting.rs              -- Static site hosting logic
  hosting_server.rs       -- Axum gateway server
  rating_storage.rs       -- Rating/reputation persistence
  rating_api.rs           -- Rating HTTP API routes
  relay_share_proxy.rs    -- Relay-based share proxying
```
