# Chiral Network V2

A fresh rebuild of the Chiral Network from scratch, focusing on clean architecture and simplicity.

## Tech Stack

- **Frontend**: Svelte 5 + TypeScript + Vite
- **Styling**: Tailwind CSS
- **Desktop**: Tauri 2 (Rust)

## Development

Install dependencies:
```bash
npm install
```

Run in development mode:
```bash
npm run tauri:dev
```

Build for production:
```bash
npm run tauri:build
```

## Headless CLI (WIP)

The repository now includes an initial headless runtime and CLI:

```bash
cargo build --manifest-path src-tauri/Cargo.toml --bin chiral --bin chiral_daemon
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- --help
```

Start/stop local headless daemon:

```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon start
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon status
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon stop
```
