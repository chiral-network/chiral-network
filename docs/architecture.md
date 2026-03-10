# Architecture

## Overview

Chiral Network is a desktop-first P2P application. The Svelte frontend talks to a Rust backend through Tauri `invoke()` commands. The backend owns networking, local file storage, wallet/transaction utilities, hosting flows, and mining process control.

## Runtime Topology

```text
Svelte UI (src/)
  -> Tauri invoke/event bridge
    -> Rust app state (src-tauri/src/lib.rs)
      -> DhtService (libp2p swarm)
      -> DriveState + local Drive storage
      -> Hosting state + local gateway server
      -> Geth process manager
      -> Rating/Elo APIs and persistence
```

## Backend State (`AppState`)

Main shared state is created in `src-tauri/src/lib.rs` and includes:

- `dht`: active libp2p service
- `file_transfer` and `file_storage`
- `geth`: managed Core-Geth process
- `download_tiers`, `tx_metadata`, `download_directory`
- `drive_state`
- `hosting_server_state` + relay tunnel handles

## Data Persistence

Primary persisted areas:

- Drive manifest/files: OS data dir under `chiral-network/chiral-drive`
- Hosting metadata and hosted sites
- Geth data dir and logs
- Reputation/rating storage
- Relay share registry (when used)

## Networking Model

`DhtService` uses libp2p with:

- Kademlia DHT
- mDNS local discovery
- Circuit relay client + DCUtR
- Identify + ping
- Custom protocols for file request/transfer and echo

File metadata is stored in DHT keys like `chiral_file_<hash>` and includes seeder entries and multiaddrs.

## Drive Seeding Model

Drive upload stores files locally first. Publishing to network:

1. Registers file in local shared-file map
2. Publishes/upserts metadata in DHT
3. Marks manifest item as `seeding=true`

On DHT startup, backend auto-reseeds persisted Drive items marked as seeding (including nested folders), so seeding survives app restarts.

## Local Gateway

An Axum gateway server starts during app setup (default port `9419`) and serves:

- Drive CRUD/share/download routes
- Hosted site routes
- relay-tunnel origin for NAT traversal use cases

## Frontend Routing

Current pages are defined in `src/App.svelte`:

- `/wallet`
- `/network`
- `/download`
- `/drive`
- `/chiraldrop`
- `/hosting`
- `/hosts`
- `/mining`
- `/account`
- `/settings`
- `/diagnostics`

## Headless Mode

Headless mode runs through `src-tauri/src/bin/chiral.rs` and `chiral_daemon.rs`, reusing the same backend service surface over an HTTP API. See `docs/headless-mode.md`.
