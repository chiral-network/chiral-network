# Headless Mode

## Goal
Run Chiral Network as a command-line-first runtime with a long-running daemon and full CLI coverage across wallet, account, DHT, drive, drop, hosting, market, mining, and geth flows.

## Current State
Headless mode now includes:

- `chiral_daemon`: persistent runtime process with:
  - existing gateway routes (Drive API, Ratings API, hosting routes)
  - headless runtime routes under `/api/headless/*`
  - in-daemon DHT lifecycle and peer operations
  - in-daemon geth/mining lifecycle and status operations
  - drop inbox/accept/decline handlers
- `chiral`: CLI with implemented handlers for all top-level command groups:
  - `daemon`, `settings`, `network`, `reputation`, `diagnostics`
  - `wallet`, `account`
  - `dht`, `download`, `drive`, `drop`
  - `hosting`, `market`, `mining`, `geth`

## Runtime Notes
- The daemon is the shared state holder for DHT and geth/mining.
- The CLI is stateless and calls daemon APIs for runtime operations.
- Local headless persistence is stored under `~/.local/share/chiral-network/headless` (platform-equivalent `dirs::data_dir()` path).

## Main CLI Workflows

### Start daemon
```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon start --port 9419
```

### Start DHT + inspect health
```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- dht start --port 9419
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- dht status --port 9419
```

### Wallet/account
```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- wallet create
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- account balance
```

### Drive
```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- drive mkdir --owner <wallet> --name docs --port 9419
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- drive ls --owner <wallet> --port 9419
```

### Mining/geth
```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- geth status --port 9419
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- mining status --port 9419
```
