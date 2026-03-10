# Headless Mode

Headless mode runs Chiral without the desktop UI.

- `chiral_daemon`: long-running backend runtime
- `chiral`: CLI client for daemon APIs

## Build

```bash
cargo build --manifest-path src-tauri/Cargo.toml --bin chiral --bin chiral_daemon
```

## Start Daemon

Recommended:

```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon start --port 9419
```

Direct binary form (after build):

```bash
src-tauri/target/debug/chiral daemon start --port 9419
```

If direct binary launch fails with "program not found", use the `cargo run --bin chiral -- ...` form.

## Status / Stop

```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon status --port 9419
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- daemon stop --port 9419
```

## Common Workflows

### DHT

```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- dht start --port 9419
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- dht status --port 9419
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- dht peer-id --port 9419
```

### Wallet / Account

```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- wallet create
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- wallet show
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- account balance
```

### Drive

```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- drive ls --owner <wallet> --port 9419
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- drive tree --owner <wallet> --port 9419
```

### Mining

```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- geth status --port 9419
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- mining status --port 9419
```

## Command Groups

Current `chiral` groups:

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

Use built-in help:

```bash
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- --help
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- <group> --help
cargo run --manifest-path src-tauri/Cargo.toml --bin chiral -- <group> <subcommand> --help
```

## Data Location

Headless runtime data is stored under OS data directories in `chiral-network/headless`.
