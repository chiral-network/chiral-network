# Headless Mode

Headless mode runs Chiral Network without the desktop UI, using:

- `chiral_daemon`: long-running runtime process
- `chiral`: command-line client

The daemon owns runtime state (DHT, geth/mining, transfers). The CLI calls daemon APIs.

## Quick Start

### 1. Build binaries
```bash
cargo build --manifest-path src-tauri/Cargo.toml --bin chiral --bin chiral_daemon
```

### 2. Start daemon
```bash
src-tauri/target/debug/chiral daemon start --port 9419
```

Alternative (without calling the binary path directly):
```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon start --port 9419
```

The CLI waits for both:
- `GET /health`
- `GET /api/headless/runtime`

If an older daemon process is found without headless routes, `daemon start` will restart it.

### 3. Check daemon status
```bash
src-tauri/target/debug/chiral daemon status --port 9419
```

Alternative:
```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon status --port 9419
```

Expected output includes:
- `alive=true`
- `health=true`
- `headless_api=true`

### 4. Stop daemon
```bash
src-tauri/target/debug/chiral daemon stop --port 9419
```

Alternative:
```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon stop --port 9419
```

## Core Workflows

### DHT
```bash
src-tauri/target/debug/chiral dht start --port 9419
src-tauri/target/debug/chiral dht status --port 9419
src-tauri/target/debug/chiral dht peer-id --port 9419
```

### Wallet + Account
```bash
src-tauri/target/debug/chiral wallet create
src-tauri/target/debug/chiral wallet show
src-tauri/target/debug/chiral account balance
src-tauri/target/debug/chiral account meta
```

### Drive
```bash
src-tauri/target/debug/chiral drive mkdir --owner <wallet> --name docs --port 9419
src-tauri/target/debug/chiral drive ls --owner <wallet> --port 9419
src-tauri/target/debug/chiral drive tree --owner <wallet> --port 9419
```

### Marketplace
```bash
src-tauri/target/debug/chiral market advertise --wallet <wallet> --port 9419
src-tauri/target/debug/chiral market browse --port 9419
```

### Geth + Mining
```bash
src-tauri/target/debug/chiral geth status --port 9419
src-tauri/target/debug/chiral mining status --port 9419
```

## Command Surface

`chiral` command groups currently available:

- `daemon`
- `settings`
- `network`
- `reputation`
- `diagnostics`
- `wallet`
- `account`
- `dht`
- `download`
- `drive`
- `drop`
- `hosting`
- `market`
- `mining`
- `geth`

Use help for detailed flags:

```bash
src-tauri/target/debug/chiral --help
src-tauri/target/debug/chiral <group> --help
src-tauri/target/debug/chiral <group> <subcommand> --help
```

## Data Location

Headless data persists under the OS data directory:

- Linux: `~/.local/share/chiral-network/headless`
- macOS: `~/Library/Application Support/chiral-network/headless`
- Windows: `%APPDATA%\\chiral-network\\headless`
