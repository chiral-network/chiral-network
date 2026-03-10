# Chiral Network Development Guide

## Overview

Chiral Network is a Tauri desktop app with a Svelte frontend and Rust backend.

Primary domains:

- DHT peer networking and file transfer
- Drive file management + seeding
- Download orchestration and payments
- Hosting marketplace and relay publication
- CPU/GPU mining controls
- Headless daemon + CLI operations

## Current Page Surface

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

## Core Source Layout

- Frontend app shell: `src/App.svelte`
- Frontend stores/services: `src/lib/`
- Backend command layer: `src-tauri/src/lib.rs`
- DHT/libp2p: `src-tauri/src/dht.rs`
- Drive persistence/API: `src-tauri/src/drive_storage.rs`, `src-tauri/src/drive_api.rs`
- Hosting server: `src-tauri/src/hosting_server.rs`
- Geth/mining integration: `src-tauri/src/geth.rs`
- Ratings/reputation APIs: `src-tauri/src/rating_api.rs`, `src-tauri/src/rating_storage.rs`
- Headless binaries: `src-tauri/src/bin/chiral.rs`, `src-tauri/src/bin/chiral_daemon.rs`

## Command Surface (Tauri)

Command registration is in `tauri::generate_handler![...]` in `src-tauri/src/lib.rs`.

Main categories:

- DHT/network commands
- file transfer/download commands
- wallet/transaction commands
- geth/mining (CPU and GPU) commands
- drive CRUD/seeding/share commands
- hosting/marketplace commands
- encryption commands
- diagnostics and lifecycle commands

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
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon start --port 9419
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon status --port 9419
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon stop --port 9419
```

## Implementation Notes

- Prefer Tauri `invoke()` paths for app runtime behavior.
- Drive seeding state is restored from backend at DHT startup.
- Avoid duplicating reseed/publish logic in frontend when backend already owns recovery.
- Keep docs aligned to `src/App.svelte` routes and `generate_handler` command inventory.
