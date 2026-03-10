# Chiral Network

Chiral Network is a Tauri desktop application for peer-to-peer file sharing, local Drive seeding, hosting marketplace coordination, and CHI-based payments.

## Stack

- Frontend: Svelte 5 + TypeScript + Tailwind
- Desktop runtime: Tauri 2
- Backend: Rust (`src-tauri`)
- Networking: libp2p (DHT, relay, chunked transfer)
- Chain integration: Core-Geth lifecycle + RPC helpers

## Core Features

- Wallet creation/import and account management
- DHT peer connectivity and discovery
- Drive with folders, sharing, and instant local seeding publish
- Download by hash, magnet, and torrent
- ChiralDrop direct transfers
- Hosting and Hosts marketplace flow
- CPU and GPU mining controls
- Local relay-aware gateway server (Drive/Hosting)

## Development

Install dependencies:

```bash
npm install
```

Run desktop app in dev mode:

```bash
npm run tauri:dev
```

Build frontend:

```bash
npm run build
```

Build desktop app:

```bash
npm run tauri:build
```

## Testing

Frontend tests:

```bash
npm test
```

Rust tests:

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

## Headless CLI Mode

The repo includes:

- `chiral_daemon` (runtime daemon)
- `chiral` (CLI client)

Build:

```bash
cargo build --manifest-path src-tauri/Cargo.toml --bin chiral --bin chiral_daemon
```

Start daemon (recommended form):

```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon start --port 9419
```

Status:

```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon status --port 9419
```

Stop:

```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon stop --port 9419
```

If your shell cannot resolve a binary path directly (for example on Windows), use the `cargo run --bin chiral -- ...` form above.

See [docs/headless-mode.md](docs/headless-mode.md) for full command coverage.
