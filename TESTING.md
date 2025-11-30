# E2E P2P Testing Guide

## Recommended Testing Approach

### **Option 1: Rust Integration Tests (BEST)**

Run the Rust integration tests that simulate real P2P scenarios:

```bash
cd src-tauri
cargo test --test e2e_cross_network_transfer_test -- --nocapture
```

**Tests included:**
- ✅ `test_e2e_file_transfer_same_network` - Full publish → search → download flow
- ✅ `test_dht_provider_records` - Verify DHT provider announcement works
- ✅ `test_multi_peer_dht_propagation` - 3-node DHT propagation test

**Why this is better than Docker:**
- No network isolation issues
- Faster iteration
- Easy to debug with Rust debugger
- Can mock NAT scenarios directly in code

### **Option 2: Docker with Network Isolation (LIMITED)**

If you want to test in Docker:

```bash
# Build the image
docker build -t chiral-network:test .

# Run isolated test environment
docker-compose -f docker-compose.isolated.yml up

# View logs
docker logs chiral-bootstrap
docker logs chiral-seeder
docker logs chiral-downloader
```

**Limitations:**
- Still not true NAT simulation
- Requires manual firewall setup
- Harder to debug

### **Option 3: Manual Multi-Machine Testing (MOST REALISTIC)**

For testing with Shuai's node or real NAT scenarios:

1. **On your machine:**
   ```bash
   npm run tauri:build
   ./src-tauri/target/release/chiral-network --headless --enable-autorelay
   ```

2. **On Shuai's machine:**
   ```bash
   # Same command
   ```

3. **Publish file from Shuai's node**
4. **Search and download from your node**
5. **Check metrics** for DCUtR success

## Running Individual Tests

### Test DHT Connectivity
```bash
cargo test --test nat_traversal_e2e_test test_dht_peer_discovery -- --nocapture
```

### Test AutoNAT Detection
```bash
cargo test --test nat_traversal_e2e_test test_autonat_detection -- --nocapture
```

### Test File Publish and Search
```bash
cargo test --test nat_traversal_e2e_test test_file_publish_and_search -- --nocapture
```

## Debugging Failed Transfers

If E2E transfer fails, check in this order:

### 1. **DHT Connection**
```bash
# In Rust test output, look for:
✅ Service 1 peer count: 1
✅ Service 2 peer count: 1
```

If peer count is 0, DHT discovery failed.

### 2. **Provider Records**
```rust
// Add this to your test:
let seeders = node.get_seeders_for_file(file_hash).await;
println!("Seeders: {:?}", seeders);
```

If empty, provider record not announced.

### 3. **Bitswap Block Exchange**
Check logs for:
```
Bitswap: Block requested: <CID>
Bitswap: Block sent: <CID>
Bitswap: Block received: <CID>
```

### 4. **NAT Traversal**
Check metrics:
```rust
let metrics = node.metrics_snapshot().await;
println!("Reachability: {:?}", metrics.reachability);
println!("DCUtR attempts: {}", metrics.dcutr_hole_punch_attempts);
println!("DCUtR successes: {}", metrics.dcutr_hole_punch_successes);
```

## Test File Creation

Create test files for manual testing:

```bash
# Create test file
mkdir -p test-files
echo "Test content for P2P transfer" > test-files/test.txt

# Create larger file
dd if=/dev/urandom of=test-files/large.bin bs=1M count=10
```

## Expected Test Output

Successful E2E test should show:

```
🧪 E2E Test: File transfer between peers on same network
✅ Seeder started: 12D3KooW...
✅ Test file created: /tmp/chiral-test-seeder-file.txt (hash: abc123...)
✅ File published to DHT
✅ Seeder multiaddr: /ip4/127.0.0.1/tcp/14001/p2p/12D3KooW...
✅ Downloader started: 12D3KooW...
✅ Seeder peer count: 1
✅ Downloader peer count: 1
✅ File search initiated successfully
✅ E2E test validation points:
  ✓ Seeder published file to DHT
  ✓ Downloader discovered seeder via DHT
  ✓ File metadata propagated
  ? Bitswap download (requires implementation)
✅ E2E test completed successfully!
```

## Next Steps After Tests Pass

1. ✅ Verify DHT discovery works (peer count > 0)
2. ✅ Verify provider records exist
3. 🔧 Implement Bitswap download in test
4. 🔧 Test with encryption enabled
5. 🚀 Test cross-network with real peers

## Troubleshooting

### "Peers failed to discover each other via DHT"

**Cause:** Bootstrap nodes unreachable or DHT in client mode

**Fix:**
1. Check bootstrap node IPs are reachable
2. Ensure DHT is in server mode (not just client)
3. Add more bootstrap nodes

### "No file_content event received"

**Cause:** Bitswap not sending blocks

**Fix:**
1. Verify seeder has the blocks in blockstore
2. Check Bitswap wants/provides messages in logs
3. Ensure CIDs match between metadata and blockstore

### "Search failed: timeout"

**Cause:** DHT not fully connected

**Fix:**
1. Increase search timeout
2. Wait longer after node startup
3. Check routing table has peers

## Performance Benchmarks

Target metrics for E2E transfer:

- **DHT Discovery:** < 5 seconds
- **Provider Record Propagation:** < 10 seconds
- **Bitswap First Block:** < 2 seconds
- **1MB File Transfer:** < 5 seconds (same network)
- **1MB File Transfer:** < 15 seconds (cross-network with relay)
- **DCUtR Success Rate:** > 70% (when both peers NATed)

Monitor these in your metrics dashboard!
