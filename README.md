# Chiral Network

Chiral Network is a Tauri 2 desktop application for decentralized peer-to-peer file sharing, local Drive seeding, hosting marketplace coordination, and CHI-based payments.

## Stack

- Frontend: Svelte 5 + TypeScript + Tailwind CSS
- Desktop runtime: Tauri 2
- Backend: Rust (`src-tauri`)
- Networking: libp2p (Kademlia DHT, chunked file transfer)
- Blockchain: Core-Geth (CHI chain, CPU/GPU mining)

## Core Features

- **Wallet** — creation, import, and one-time email backup
- **Network** — DHT peer connectivity, discovery, and Geth node management
- **Download** — search by hash/magnet/torrent, speed tiers with CHI payments
- **Drive** — local file management, seeding, paid share links via relay proxy
- **ChiralDrop** — direct peer-to-peer file transfers with optional pricing
- **Hosts** — hosting marketplace (publish, browse, propose/accept agreements)
- **Mining** — CPU and GPU mining controls (OpenCL on macOS, CUDA on Linux/Windows)
- **Account** — wallet info and Elo reputation panel
- **Settings** — appearance, notification preferences, download directory
- **Diagnostics** — event log and system info

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
# Frontend tests (Vitest)
npm test

# Rust tests
cargo test --manifest-path src-tauri/Cargo.toml

# Rust compile check
cargo check --manifest-path src-tauri/Cargo.toml
```

## Headless CLI Mode

The repo includes a CLI client (`chiral`) and runtime daemon (`chiral_daemon`).

```bash
# Build
cargo build --manifest-path src-tauri/Cargo.toml --bin chiral --bin chiral_daemon

# Start daemon
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon start --port 9419

# Check status
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon status --port 9419

# Stop daemon
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon stop --port 9419
```

See [docs/headless-mode.md](docs/headless-mode.md) for full command coverage.

## Relay Server

The relay server (`relay_server` binary) runs on `130.245.173.73:8080` and provides:

- Elo reputation API (`/api/ratings/*`)
- Drive share proxy (reverse-proxies to sharer's local gateway)
- Wallet backup email endpoint (`/api/wallet/backup-email`)

## Reputation System

Elo scores (0–100, base 50) are computed from transfer outcomes only — no user ratings. Factors:

- **Transfer outcome**: completed (+) or failed (−)
- **Amount weighting**: logarithmic scaling based on CHI transferred
- **Time decay**: only the last 180 days count, with recent events weighted more heavily

See [REPUTATION_SYSTEM_PLAN.md](REPUTATION_SYSTEM_PLAN.md) for details.
