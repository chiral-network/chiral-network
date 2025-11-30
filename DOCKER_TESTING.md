# Docker Testing Setup

## ⚠️ Important Limitations

Docker networking has significant limitations for NAT traversal testing:

1. **Same Bridge Network:** Even with multiple Docker networks, containers can still communicate directly
2. **iptables Complexity:** Attempting to block direct peer communication with iptables is fragile and unreliable
3. **Not True NAT:** Docker's bridge networking doesn't simulate real-world NAT scenarios accurately

### When to Use Docker Testing

✅ **Good for:**
- Testing DHT connectivity
- Verifying bootstrap node discovery
- Checking relay server functionality
- Testing file publishing/searching
- Quick smoke tests before deployment

❌ **NOT good for:**
- True NAT traversal validation
- DCUtR hole-punching tests
- Real-world cross-network scenarios
- Circuit Relay v2 under actual NAT

### Better Alternatives

1. **Rust Integration Tests** (RECOMMENDED) - See `TESTING.md`
   ```bash
   cd src-tauri
   cargo test --test e2e_cross_network_transfer_test -- --nocapture
   ```

2. **Manual Multi-Machine Testing** - Deploy on actual separate networks
3. **Cloud VM Testing** - Rent VMs on different cloud providers

## Quick Start

### Prerequisites

- Docker installed and running
- Docker Compose v2+
- At least 4GB RAM available
- Ports 4001 available on host

### Option 1: Interactive Mode

```bash
chmod +x scripts/docker-test.sh
./scripts/docker-test.sh
```

Then select from menu:
1. Build image
2. Start network
9. Get peer IDs
10. Check connectivity

### Option 2: Command Line Mode

```bash
# Build the image
./scripts/docker-test.sh build

# Start network
./scripts/docker-test.sh start

# Check connectivity
./scripts/docker-test.sh check

# View logs
./scripts/docker-test.sh logs bootstrap
./scripts/docker-test.sh logs seeder
./scripts/docker-test.sh logs downloader

# Stop network
./scripts/docker-test.sh stop

# Clean everything
./scripts/docker-test.sh clean
```

### Option 3: Direct Docker Compose

```bash
# Build image
docker build -t chiral-network:test .

# Start all services
docker-compose -f docker-compose.test.yml up -d

# View logs
docker-compose -f docker-compose.test.yml logs -f

# Stop
docker-compose -f docker-compose.test.yml down
```

## Network Architecture

```
┌─────────────────────────────────────┐
│         Bootstrap Node              │
│  (Public Relay Server)              │
│  Network: public_net                │
│  IP: 172.20.0.10                    │
│  Ports: 4001 (DHT), 8545 (Geth)    │
└──────────┬──────────────────────────┘
           │
           ├──────────────────────────┐
           │                          │
  ┌────────▼────────┐       ┌────────▼────────┐
  │  Seeder Node    │       │ Downloader Node │
  │  (NAT Network A)│       │  (NAT Network B)│
  │  172.21.0.11    │       │   172.22.0.12   │
  └─────────────────┘       └─────────────────┘
```

**Limitation:** Seeder and Downloader are on different subnets (172.21.x vs 172.22.x) but **can still communicate directly** through Docker's bridge networking.

## What Gets Tested

### ✅ Working Tests

1. **DHT Bootstrapping**
   - Seeder and Downloader connect to Bootstrap
   - Bootstrap acts as relay server
   - Peer discovery via DHT

2. **File Publishing**
   - Seeder can publish files to DHT
   - Metadata propagates to DHT network
   - Provider records announced

3. **File Search**
   - Downloader can search DHT for files
   - Metadata retrieval works
   - Seeder list populated

4. **Relay Server Mode**
   - Bootstrap node runs as Circuit Relay v2 server
   - Connection statistics visible in logs

### ❌ Not Truly Tested

1. **NAT Traversal** - Nodes can connect directly, relay not forced
2. **DCUtR Hole Punching** - No real NAT to punch through
3. **AutoRelay Behavior** - Relay used opportunistically, not required
4. **Cross-Network Bitswap** - Works, but via direct connection not relay

## Interpreting Test Results

### Check if Nodes Connected

```bash
./scripts/docker-test.sh check
```

**Expected output:**
```
Bootstrap peer count: 2
Seeder peer count: 1-2
Downloader peer count: 1-2
```

If peer counts are 0, bootstrap failed.

### Check Peer IDs

```bash
./scripts/docker-test.sh peers
```

**Expected output:**
```
Bootstrap Node:
PeerId("12D3KooW...")

Seeder Node:
PeerId("12D3KooW...")

Downloader Node:
PeerId("12D3KooW...")
```

### View Real-Time Logs

```bash
# All nodes
docker-compose -f docker-compose.test.yml logs -f

# Specific node
docker-compose -f docker-compose.test.yml logs -f seeder
```

**Look for:**
- ✅ "Swarm event: NewListenAddr" - Node listening
- ✅ "Swarm event: ConnectionEstablished" - Peer connected
- ✅ "Published file to DHT" - File upload worked
- ✅ "File discovered" - Search worked
- ⚠️ "Bootstrap failed" - Connection issues
- ❌ "Failed to connect" - Network problems

## Manual Testing Workflow

### 1. Start Network

```bash
./scripts/docker-test.sh start
sleep 10  # Wait for initialization
```

### 2. Publish File from Seeder

```bash
# Shell into seeder
docker-compose -f docker-compose.test.yml exec seeder /bin/sh

# Note: You'll need to add API endpoints or use logs to trigger file publish
# This is a limitation - headless mode needs manual file management
```

### 3. Search from Downloader

```bash
# Shell into downloader
docker-compose -f docker-compose.test.yml exec downloader /bin/sh

# Search for file (implementation needed)
```

### 4. Check Metrics

View logs to see if:
- DHT queries succeeded
- Provider records found
- Bitswap blocks transferred

## Debugging Failed Tests

### Problem: Nodes Don't Connect

**Symptoms:**
- Peer count stays at 0
- No "ConnectionEstablished" logs

**Possible Causes:**
1. Bootstrap node didn't start first
2. Peer IDs hardcoded incorrectly
3. Network firewall blocking ports

**Fix:**
```bash
./scripts/docker-test.sh clean
./scripts/docker-test.sh start
```

### Problem: File Not Found in Search

**Symptoms:**
- Seeder published file
- Downloader search returns nothing

**Possible Causes:**
1. DHT not fully propagated (wait longer)
2. Record expired before search
3. Key mismatch between publish and search

**Debug:**
```bash
# Check seeder logs for publish confirmation
docker-compose -f docker-compose.test.yml logs seeder | grep "Published file"

# Check downloader logs for search activity
docker-compose -f docker-compose.test.yml logs downloader | grep "Searching"
```

### Problem: Download Hangs

**Symptoms:**
- File found in search
- Download never completes

**Possible Causes:**
1. Bitswap not exchanging blocks
2. CID mismatch
3. Blockstore missing chunks

**Debug:**
```bash
# Check for Bitswap activity
docker-compose -f docker-compose.test.yml logs | grep -i bitswap
```

## Environment Variables

You can customize the Docker setup:

```bash
# In docker-compose.test.yml
environment:
  - RUST_LOG=debug                    # More verbose logging
  - ENABLE_GETH=true                  # Enable Geth blockchain
  - IS_BOOTSTRAP=true                 # Mark as bootstrap node
  - DHT_PORT=4001                     # DHT listen port
```

## Performance Expectations

On a modern development machine:

- **Startup Time:** 10-15 seconds
- **DHT Connection:** 2-5 seconds
- **File Publish:** < 1 second
- **Metadata Search:** 2-10 seconds (depends on DHT propagation)
- **File Download:** Varies by size

## Known Issues

1. **Direct Communication:** Seeder and Downloader can talk directly, bypassing relay
2. **No True NAT:** Can't test hole-punching or AutoRelay failover
3. **Hardcoded Peer IDs:** Bootstrap peer ID in config may not match actual peer ID
4. **API Limitations:** Headless mode lacks REST API for easy file operations
5. **Volume Persistence:** Blockstore data persists between runs (can cause confusion)

## Cleanup

### Remove All Test Data

```bash
./scripts/docker-test.sh clean
```

This removes:
- All containers
- Docker networks
- Volumes (blockstore data)
- Dangling images

### Keep Blockstore Data

```bash
docker-compose -f docker-compose.test.yml down
```

This stops containers but preserves volumes.

## Next Steps

After basic Docker testing:

1. ✅ Run Rust integration tests for real validation
2. ✅ Add metrics dashboard to see detailed stats
3. ✅ Deploy to actual separate networks for real NAT testing
4. ✅ Test with Shuai's node

## Conclusion

Docker testing is **useful for development** but **insufficient for validating NAT traversal**. Use it for:
- Quick smoke tests
- DHT connectivity checks
- Local development iteration

For real E2E validation, use:
- **Rust integration tests** (best option)
- **Multi-machine manual tests** (most realistic)
- **Metrics dashboard** (see what's actually happening)

See `TESTING.md` for the recommended testing approach.
