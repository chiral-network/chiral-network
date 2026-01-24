# P2P Chunk Network - Download Recovery Tests

This directory contains comprehensive tests for the P2P chunk-based file recovery system.

## Test Components

### 1. Cargo Unit Tests (43 tests)
Located in: `src-tauri/src/p2p_chunk_network.rs`

Tests core functionality:
- Chunk metadata and sizing calculations
- Merkle tree computation and verification
- Download state management
- Multi-peer coordinator
- Peer stats and banning logic
- Corruption detection
- State persistence

Run with:
```bash
cargo test --lib p2p_chunk -- --test-threads=1
```

### 2. Standalone Integration Test
File: `p2p_chunk_network_integration.rs`

Tests file I/O operations without external dependencies:
- Hash generation and verification
- Chunk creation and sizing
- State persistence to disk
- Corruption detection with actual files
- Multi-peer coordinator simulation
- Full download flow simulation

Run with:
```bash
rustc p2p_chunk_network_integration.rs -o /tmp/test && /tmp/test
```

### 3. DHT Integration Test
File: `test_dht_integration.sh`

Tests actual DHT node operations:
- Node startup and initialization
- Bootstrap peer connections
- DHT/Kademlia operations
- Memory usage verification
- Provider functionality

Run with:
```bash
./test_dht_integration.sh
```

### 4. REPL DHT Operations Test
File: `test_repl_dht_operations.sh`

Tests REPL commands and DHT operations:
- File creation and hashing
- REPL command execution
- Chunk size calculations
- Merkle tree calculations

Run with:
```bash
./test_repl_dht_operations.sh
```

## Running All Tests

Execute the comprehensive test runner:
```bash
./run_all_tests.sh
```

This runs all test suites and provides a summary.

## Test Coverage

| Feature | Unit Tests | Integration Tests | E2E Tests |
|---------|------------|-------------------|-----------|
| Chunk sizing | ✓ | ✓ | |
| Hash verification | ✓ | ✓ | ✓ |
| Merkle tree | ✓ | ✓ | |
| State persistence | ✓ | ✓ | |
| Corruption detection | ✓ | ✓ | |
| Multi-peer coordinator | ✓ | ✓ | |
| Peer stats/banning | ✓ | ✓ | |
| DHT provider queries | | | ✓ |
| DHT announcements | | | ✓ |
| REPL commands | | | ✓ |

## Requirements

- Rust toolchain
- Bash
- Network access (for DHT tests)

## Network Testing

For full DHT network testing, you need:

1. **Bootstrap nodes**: The tests connect to Chiral Network bootstrap nodes
2. **Internet access**: Required for DHT peer discovery
3. **Port availability**: Tests use ports 4002, 4003 by default

If behind NAT/firewall, some DHT operations may not work fully.

## Adding New Tests

1. **Unit tests**: Add to `p2p_chunk_network.rs` in the `#[cfg(test)]` module
2. **Integration tests**: Extend `p2p_chunk_network_integration.rs`
3. **E2E tests**: Create new `.sh` scripts in this directory

## Troubleshooting

### Tests timeout
- Increase timeout values in test scripts
- Check network connectivity
- Verify bootstrap nodes are reachable

### DHT tests fail
- Ensure no other DHT node is running on the same ports
- Check firewall settings
- Try with `--disable-autonat` flag

### Compilation errors
- Run `cargo build` first to check for errors
- Ensure all dependencies are installed
