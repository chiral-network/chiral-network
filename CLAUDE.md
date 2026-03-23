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
- `/download` — file search, download, speed tiers
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
- Backend command layer: `src-tauri/src/lib.rs`
- DHT/libp2p: `src-tauri/src/dht.rs`
- Drive persistence/API: `src-tauri/src/drive_storage.rs`, `src-tauri/src/drive_api.rs`
- Hosting server: `src-tauri/src/hosting_server.rs`
- Geth/mining integration: `src-tauri/src/geth.rs`
- Reputation (Elo): `src-tauri/src/rating_api.rs`, `src-tauri/src/rating_storage.rs`
- Wallet backup email: `src-tauri/src/wallet_backup_api.rs`
- Relay share proxy: `src-tauri/src/relay_share_proxy.rs`
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

## Reputation System

Elo scores are computed from transfer outcomes only (no user ratings). The formula uses:
- Transfer outcome: completed (1.0) or failed (0.0)
- Time weighting: recent events (within 180-day lookback) weighted more heavily
- Amount weighting: logarithmic scaling based on CHI transferred
- Bounded Elo updates clamped to 0–100

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

The relay server runs on `130.245.173.73:8080` as a systemd service (`relay-server.service`). It provides:
- Reputation API endpoints
- Drive share proxy (`RelayShareRegistry`)
- Wallet backup email endpoint (requires SMTP env vars)

SMTP env vars: `CHIRAL_WALLET_EMAIL_SMTP_HOST`, `CHIRAL_WALLET_EMAIL_FROM` (required); `CHIRAL_WALLET_EMAIL_SMTP_USERNAME`, `CHIRAL_WALLET_EMAIL_SMTP_PASSWORD` (optional for local postfix).

## Implementation Notes

- Prefer Tauri `invoke()` paths for app runtime behavior.
- Drive seeding state is restored from backend at DHT startup.
- Avoid duplicating reseed/publish logic in frontend when backend already owns recovery.
- App shows a close confirmation dialog before quitting (wired in `src/App.svelte`).
- Wallet backup email step is optional (skip button) during wallet creation.
- Keep docs aligned to `src/App.svelte` routes and `generate_handler` command inventory.
