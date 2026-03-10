# Chiral Network Implementation Book

Version: Current code snapshot

This document is a lightweight index of the implemented system. It replaces older long-form design text that had diverged from the codebase.

## 1. Product Surface

Implemented desktop pages:

- Wallet (`/wallet`)
- Network (`/network`)
- Download (`/download`)
- Drive (`/drive`)
- ChiralDrop (`/chiraldrop`)
- Hosting (`/hosting`)
- Hosts (`/hosts`)
- Mining (`/mining`)
- Account (`/account`)
- Settings (`/settings`)
- Diagnostics (`/diagnostics`)

## 2. Runtime Architecture

- Frontend: Svelte 5 + TypeScript (`src/`)
- Desktop runtime: Tauri 2
- Backend entry: `src-tauri/src/lib.rs`
- P2P subsystem: `src-tauri/src/dht.rs`
- Drive subsystem: `src-tauri/src/drive_storage.rs`, `src-tauri/src/drive_api.rs`
- Hosting subsystem: `src-tauri/src/hosting.rs`, `src-tauri/src/hosting_server.rs`
- Chain/mining subsystem: `src-tauri/src/geth.rs`
- Ratings/reputation subsystem: `src-tauri/src/rating_storage.rs`, `src-tauri/src/rating_api.rs`

## 3. Key Behavioral Guarantees

- Drive seeding publishes metadata and seeder availability to DHT.
- Drive seeding state is restored automatically on DHT startup, including files in nested folders.
- Drive delete removes manifest entries and physical file storage, with error reporting on failed file deletion.
- Download path includes local/same-node fast-path behavior to reduce relay retries and latency.

## 4. Protocol and Discovery

- DHT metadata key pattern for files: `chiral_file_<hash>`
- Seeder metadata includes `peerId`, pricing, and dialable `multiaddrs`
- Connectivity supports relay + direct upgrade flows

## 5. Marketplace and Hosting

- Host advertisement publish/unpublish
- Host discovery and agreement flow
- Relay publication/tunnel support for hosted resources

## 6. Mining

- CPU mining controls
- GPU mining controls, capability detection, status reporting

## 7. Headless Mode

- CLI + daemon binaries under `src-tauri/src/bin/`
- See `docs/headless-mode.md` for operator commands

## 8. Living References

Use these documents as current source:

- `docs/architecture.md`
- `docs/backend-api.md`
- `docs/networking.md`
- `docs/pages.md`
- `docs/headless-mode.md`
