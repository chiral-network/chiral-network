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

## Headless CLI

The repository includes a headless daemon + CLI runtime (`chiral_daemon` + `chiral`).

Build binaries:

```bash
cargo build --manifest-path src-tauri/Cargo.toml --bin chiral --bin chiral_daemon
```

Start the daemon:

```bash
src-tauri/target/debug/chiral daemon start --port 9419
src-tauri/target/debug/chiral daemon status --port 9419
```

Or via `cargo run`:

```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon start --port 9419
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon status --port 9419
```

Status should show `headless_api=true`.

Stop daemon:

```bash
src-tauri/target/debug/chiral daemon stop --port 9419
```

CLI help:

```bash
src-tauri/target/debug/chiral --help
src-tauri/target/debug/chiral <group> --help
```

Example workflows:

```bash
# DHT lifecycle
src-tauri/target/debug/chiral dht start --port 9419
src-tauri/target/debug/chiral dht status --port 9419

# Wallet/account
src-tauri/target/debug/chiral wallet create
src-tauri/target/debug/chiral account balance

# Drive
src-tauri/target/debug/chiral drive ls --owner <wallet> --port 9419
```

Full headless documentation: `docs/headless-mode.md`.
