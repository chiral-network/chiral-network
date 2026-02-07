# Real E2E Testing Guide

## Overview
This guide covers running **real End-to-End tests** with actual Chiral Network nodes communicating over the network.

## Test Types

### 1. Mock-based E2E Tests (Fast, Isolated)
Located in: `tests/e2e/*.test.ts` (except `real-network.test.ts`)
- Uses mocks and simulators
- No real network I/O
- Fast execution (~seconds)
- Good for CI/CD

**Run:**
```bash
npm test tests/e2e/
```

### 2. Real Network E2E Tests (Slow, Full Integration)
Located in: `tests/e2e/real-network.test.ts`
- Launches actual node processes
- Real DHT network communication
- Real file transfers
- Real payment transactions
- Slow execution (~minutes)

## Running Real E2E Tests

## Option1: Attach Mode (Recommended, Cross-Machine, Real Network + Real Chain)

In this mode, you **run the Uploader/Downloader nodes separately**, then execute the test suite from **one machine only** (typically the Downloader side) to validate the full flow:
**upload -> DHT search -> HTTP Range download -> payment (tx receipt)**.

> Note (Local chain): By default, Chiral uses a **local Geth chain** (chainId/networkId: `98765`) with RPC at `http://127.0.0.1:8545`.
> For real cross-machine attach mode with payments, you typically run Geth on the **Uploader VM** and make the Downloader access it via **SSH local port forwarding** (no public RPC exposure).

### 1) Node environment variables (common)

- `CHIRAL_HEADLESS=true`
- `CHIRAL_E2E_API_PORT=<PORT>`: E2E control API port for the node
- `CHIRAL_PRIVATE_KEY=0x...`: node wallet private key (**do not print this in logs**)
- `CHIRAL_RPC_ENDPOINT=http://...`: both nodes should point to the same chain/RPC

### 2) Uploader (VM / public-IP machine)

- `CHIRAL_PUBLIC_IP=<VM_PUBLIC_IP>`: used to build dialable URLs/addresses for other peers

```bash
export CHIRAL_HEADLESS=true
export CHIRAL_E2E_API_PORT=8081
export CHIRAL_PUBLIC_IP=<VM_PUBLIC_IP>
export CHIRAL_PRIVATE_KEY=0x...
export CHIRAL_RPC_ENDPOINT=http://...

# Enable P2P services (WebRTC/Bitswap) in headless mode (required for non-HTTP protocols)
export CHIRAL_ENABLE_P2P=1

# VM-friendly headless mode (recommended)
cd src-tauri
cargo run --release -- --headless
```

#### GCP (Ubuntu) uploader setup quickstart (optional)
If you're using a Google Cloud Ubuntu VM as the **Public-IP uploader**, you may need:

1) **Firewall rules** (GCP):
- E2E API: `8081/tcp`
- HTTP file server: `8080-8090/tcp`
- DHT: `4001/tcp`

2) **Build prerequisites**:
```bash
sudo apt-get update -y
sudo apt-get install -y build-essential pkg-config libssl-dev curl unzip
```

3) **Install `geth` binary (Core-Geth)**:
`apt install geth` may not work on some images. Place `geth` next to the built binary under `bin/`.
```bash
cd ~/chiral-network
curl -L -o core-geth.zip https://github.com/etclabscore/core-geth/releases/download/v1.12.20/core-geth-linux-v1.12.20.zip
unzip -o core-geth.zip -d /tmp/core-geth
mkdir -p src-tauri/target/release/bin
cp /tmp/core-geth/geth src-tauri/target/release/bin/geth
chmod +x src-tauri/target/release/bin/geth
```

### 3) Downloader (laptop / local)

**Important:** The Downloader is the one sending the payment (`/api/pay`), so it must load a wallet too:
- `CHIRAL_PRIVATE_KEY` is required (otherwise `/api/pay` returns 400).

#### If the chain RPC is on the Uploader VM (recommended): SSH tunnel for RPC
On the Downloader machine, open a local port forward so `http://127.0.0.1:8545` forwards to the VM’s local geth RPC:

1) (One-time) generate SSH config via gcloud:
```bash
gcloud compute config-ssh
```

2) Open the tunnel (keep this terminal open):
```bash
ssh -N -L 8545:127.0.0.1:8545 <VM_SSH_HOSTNAME>
```

Now set the Downloader node to use the local forwarded RPC:

Recommended: run the Downloader as a **desktop app (GUI)** on your local machine, so you can inspect logs/state more easily.

##### Downloader (GUI / `tauri dev`)
```bash
export CHIRAL_E2E_API_PORT=8082
export CHIRAL_PRIVATE_KEY=0x...
export CHIRAL_RPC_ENDPOINT=http://127.0.0.1:8545

npm run tauri dev
```

##### Downloader (optional: headless)
```bash
export CHIRAL_HEADLESS=true
export CHIRAL_E2E_API_PORT=8082
export CHIRAL_PRIVATE_KEY=0x...
export CHIRAL_RPC_ENDPOINT=http://127.0.0.1:8545

cd src-tauri
cargo run --release -- --headless
```

### 4) Run the test (Attach)

On the Downloader machine:

```bash
export E2E_ATTACH=true
export E2E_UPLOADER_API_URL=http://<VM_PUBLIC_IP>:8081
export E2E_DOWNLOADER_API_URL=http://127.0.0.1:8082

# Optional: tune real-network timeouts (milliseconds)
# - Global P2P download wait (WebRTC/Bitswap/FTP): default 600000 (10min)
export E2E_P2P_DOWNLOAD_TIMEOUT_MS=600000
# - Per-protocol overrides (take precedence over E2E_P2P_DOWNLOAD_TIMEOUT_MS)
export E2E_WEBRTC_DOWNLOAD_TIMEOUT_MS=180000
export E2E_BITSWAP_DOWNLOAD_TIMEOUT_MS=240000
export E2E_FTP_DOWNLOAD_TIMEOUT_MS=180000
export E2E_BITTORRENT_DOWNLOAD_TIMEOUT_MS=240000
# - BitTorrent-specific internal caps (downloader node; defaults are now shorter too)
export E2E_BITTORRENT_START_TIMEOUT_MS=30000
export E2E_BITTORRENT_NO_PROGRESS_FAIL_MS=60000
# - Vitest test-level timeout (prevents a single protocol from blocking the whole run)
export E2E_TEST_TIMEOUT_MS=600000
export E2E_BITTORRENT_TEST_TIMEOUT_MS=300000
# - DHT search per-attempt budget (overall search timeout is set by the test)
export E2E_SEARCH_ATTEMPT_TIMEOUT_MS=15000

# Optional: seeder connection grace period before WebRTC fails as "no reachable seeders"
# (useful for NAT/relay scenarios; default 5000ms, max 120000ms)
export E2E_SEEDER_CONNECT_WAIT_MS=30000

npm run test:e2e:real
```

### 5) Quick sanity checks

- Uploader: `GET /api/health`
- Downloader: `GET /api/health`
- DHT connectivity: Downloader `GET /api/dht/peers` should return a non-empty array

### 6) Funding the Downloader wallet (required for payment on local chain)
If `/api/pay` fails with `Insufficient balance`, the local chain likely has **no prefunded accounts**.
You must mine a few blocks to fund the Downloader address.

**PowerShell tip:** Prefer `Invoke-RestMethod` to avoid quoting issues.

Example checks (Downloader machine, RPC forwarded at `http://127.0.0.1:8545`):

```powershell
# Block number
$body = @{ jsonrpc="2.0"; method="eth_blockNumber"; params=@(); id=1 } | ConvertTo-Json -Compress
Invoke-RestMethod "http://127.0.0.1:8545" -Method Post -ContentType "application/json" -Body $body

# Mining status
$body = @{ jsonrpc="2.0"; method="eth_mining"; params=@(); id=2 } | ConvertTo-Json -Compress
Invoke-RestMethod "http://127.0.0.1:8545" -Method Post -ContentType "application/json" -Body $body

# Coinbase (mining reward address)
$body = @{ jsonrpc="2.0"; method="eth_coinbase"; params=@(); id=4 } | ConvertTo-Json -Compress
Invoke-RestMethod "http://127.0.0.1:8545" -Method Post -ContentType "application/json" -Body $body
```

To mine to your Downloader address, set coinbase (etherbase) then start mining:

```powershell
$addr = "<DOWNLOADER_ADDRESS>"

$body = @{ jsonrpc="2.0"; method="miner_setEtherbase"; params=@($addr); id=10 } | ConvertTo-Json -Compress
Invoke-RestMethod "http://127.0.0.1:8545" -Method Post -ContentType "application/json" -Body $body

$body = @{ jsonrpc="2.0"; method="miner_start"; params=@(1); id=11 } | ConvertTo-Json -Compress
Invoke-RestMethod "http://127.0.0.1:8545" -Method Post -ContentType "application/json" -Body $body
```

Then confirm balance is non-zero:

```powershell
$body = @{ jsonrpc="2.0"; method="eth_getBalance"; params=@($addr,"latest"); id=12 } | ConvertTo-Json -Compress
Invoke-RestMethod "http://127.0.0.1:8545" -Method Post -ContentType "application/json" -Body $body
```

### Single Machine (Two Local Nodes)

Run both uploader and downloader on the same machine:

```bash
npm run test:e2e:real
```

This will:
1. Launch uploader node on port 4001 (DHT) and 8081 (API)
2. Launch downloader node on port 4002 (DHT) and 8082 (API)
3. Run tests with actual network communication
4. Clean up automatically

**Timeout:** Each test has extended timeouts (2-5 minutes) for real operations.

### Cross-Machine Testing (Two Physical Computers)

Test real network communication across different machines on the same network.

#### Machine 1 (Uploader):
```bash
# Set your machine's IP address
export CHIRAL_UPLOADER_IP=192.168.1.100

npm run test:e2e:real:uploader
```

This will:
- Launch the uploader node
- Wait for downloader to connect
- Process upload requests

#### Machine 2 (Downloader):
```bash
# Point to Machine 1's IP
export CHIRAL_BOOTSTRAP_NODES=/ip4/192.168.1.100/tcp/4001

npm run test:e2e:real:downloader
```

This will:
- Launch the downloader node
- Connect to uploader via DHT
- Run download tests

## Test Scenarios

### WebRTC Communication
- **Small file (5MB)**: Full upload -> search -> download -> payment flow
- **Large file (50MB)**: Tests streaming and memory efficiency
- **Verification**: File integrity check after download

### Bitswap Communication
- **Medium file (3MB)**: Block-based transfer
- **CID verification**: Validates IPFS content addressing
- **Payment validation**: Ensures payment is processed

### Payment Checkpoints
- **25MB file**: Triggers 10MB and 20MB checkpoints
- **Multiple payments**: Validates checkpoint payments + final payment
- **Pause/resume**: Tests checkpoint pause and resume logic

## Test Architecture

```
┌─────────────────┐         DHT Network         ┌─────────────────┐
│  Uploader Node  │[<]──────────────────────────[>]│ Downloader Node │
│                 │                              │                 │
│ Port 4001 (DHT) │      WebRTC/Bitswap         │ Port 4002 (DHT) │
│ Port 8081 (API) │[<]──────────────────────────[>]│ Port 8082 (API) │
│                 │                              │                 │
│ - Upload files  │     Payment Network          │ - Download files│
│ - Receive $$$   │[<]──────────────────────────[>]│ - Send $$$      │
└─────────────────┘                              └─────────────────┘
```

## Configuration

### Environment Variables

```bash
# Single machine
CHIRAL_NODE_ID=uploader_node
CHIRAL_DHT_PORT=4001
CHIRAL_API_PORT=8081
CHIRAL_STORAGE_DIR=/tmp/chiral-e2e/uploader
CHIRAL_WALLET_ADDRESS=0x1111...
CHIRAL_HEADLESS=true

# Cross-machine
E2E_CROSS_MACHINE=true
E2E_NODE_ROLE=uploader  # or 'downloader'
CHIRAL_BOOTSTRAP_NODES=/ip4/192.168.1.100/tcp/4001
```

### Node Configuration

Each node gets:
- Separate DHT port
- Separate API port  
- Separate storage directory
- Separate wallet address
- Bootstrap nodes (downloader connects to uploader)

## Timeouts

Real network operations need longer timeouts:

| Operation | Timeout |
|-----------|---------|
| Node startup | 60s |
| DHT connection | 5s |
| File upload | 30s |
| File search | 10s |
| Small file download (5MB) | 2min |
| Large file download (50MB) | 5min |
| Payment verification | 10s |

## Troubleshooting

### Nodes fail to connect
```bash
# Check if ports are available
lsof -i :4001
lsof -i :4002
lsof -i :8081
lsof -i :8082

# Check firewall settings
# Ensure DHT ports are open for local/network traffic
```

### Download times out
- Check network speed
- Increase test timeout
- Check node logs for errors

### Payment verification fails
- Ensure both nodes have wallet initialized
- Check transaction logs in node storage
- Verify wallet addresses are correct

## CI/CD Integration

### GitHub Actions Example

```yaml
name: Real E2E Tests

on: [push, pull_request]

jobs:
  real-e2e:
    runs-on: ubuntu-latest
    timeout-minutes: 30
    
    steps:
      - uses: actions/checkout@v3
      
      - name: Setup Node.js
        uses: actions/setup-node@v3
        with:
          node-version: '18'
      
      - name: Install dependencies
        run: npm ci
      
      - name: Build Tauri app
        run: npm run tauri build
      
      - name: Run real E2E tests
        run: npm run test:e2e:real
        timeout-minutes: 20
```

### Docker Compose Example

```yaml
version: '3.8'

services:
  uploader:
    build: .
    environment:
      - CHIRAL_NODE_ID=uploader_node
      - CHIRAL_DHT_PORT=4001
      - CHIRAL_API_PORT=8081
      - CHIRAL_HEADLESS=true
    ports:
      - "4001:4001"
      - "8081:8081"
  
  downloader:
    build: .
    environment:
      - CHIRAL_NODE_ID=downloader_node
      - CHIRAL_DHT_PORT=4002
      - CHIRAL_API_PORT=8082
      - CHIRAL_BOOTSTRAP_NODES=/ip4/uploader/tcp/4001
      - CHIRAL_HEADLESS=true
    ports:
      - "4002:4002"
      - "8082:8082"
    depends_on:
      - uploader
```

## Performance Benchmarks

Expected performance on typical hardware:

| Test | Duration | Network |
|------|----------|---------|
| 5MB WebRTC | ~30s | Local |
| 50MB WebRTC | ~2min | Local |
| 3MB Bitswap | ~20s | Local |
| 25MB Checkpoint | ~1min | Local |
| 5MB WebRTC | ~1min | Cross-machine (LAN) |

## Best Practices

1. **Clean state**: Each test run uses fresh temporary directories
2. **Proper cleanup**: Nodes are killed and temp files removed after tests
3. **Timeout handling**: All operations have appropriate timeouts
4. **Error logging**: Node output is captured and displayed
5. **Verification**: Downloaded files are byte-compared with originals

## Extending Tests

To add new real E2E tests:

```typescript
it("should test my scenario", async () => {
  // Create test file
  const testFile = framework.createTestFile("myfile.bin", 10);
  
  // Upload
  const fileHash = await framework.uploadFile(testFile, "WebRTC");
  
  // Search
  const metadata = await framework.searchFile(fileHash);
  
  // Download
  const downloadPath = await framework.downloadFile(
    fileHash,
    testFile.name,
    "WebRTC"
  );
  
  // Verify
  const verified = await framework.verifyDownloadedFile(downloadPath, testFile);
  expect(verified).toBe(true);
}, 120000); // 2 minute timeout
```

## Future Enhancements

- [ ] Network condition simulation (latency, packet loss)
- [ ] Multi-peer downloads (3+ nodes)
- [ ] Connection interruption tests
- [ ] Protocol fallback tests (WebRTC -> Bitswap)
- [ ] Real blockchain integration tests
- [ ] Performance profiling and metrics collection

