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
**upload → DHT search → HTTP Range download → payment (tx receipt)**.

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
npm run tauri dev
```

### 3) Downloader (laptop / local)

```bash
export CHIRAL_HEADLESS=true
export CHIRAL_E2E_API_PORT=8082
export CHIRAL_PRIVATE_KEY=0x...
export CHIRAL_RPC_ENDPOINT=http://...
npm run tauri dev
```

### 4) Run the test (Attach)

On the Downloader machine:

```bash
export E2E_ATTACH=true
export E2E_UPLOADER_API_URL=http://<VM_PUBLIC_IP>:8081
export E2E_DOWNLOADER_API_URL=http://127.0.0.1:8082
npm run test:e2e:real
```

### 5) Quick sanity checks

- Uploader: `GET /api/health`
- Downloader: `GET /api/health`
- DHT connectivity: Downloader `GET /api/dht/peers` should return a non-empty array

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
- **Small file (5MB)**: Full upload → search → download → payment flow
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
│  Uploader Node  │◄──────────────────────────►│ Downloader Node │
│                 │                              │                 │
│ Port 4001 (DHT) │      WebRTC/Bitswap         │ Port 4002 (DHT) │
│ Port 8081 (API) │◄──────────────────────────►│ Port 8082 (API) │
│                 │                              │                 │
│ - Upload files  │     Payment Network          │ - Download files│
│ - Receive $$$   │◄──────────────────────────►│ - Send $$$      │
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
- [ ] Protocol fallback tests (WebRTC → Bitswap)
- [ ] Real blockchain integration tests
- [ ] Performance profiling and metrics collection

