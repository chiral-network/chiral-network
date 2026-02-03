# V2 Chiral Network - Implementation Summary

## Overview

V2 Chiral Network is a complete rewrite of the decentralized peer-to-peer file sharing application that combines blockchain technology with distributed hash table (DHT) based file storage. It features a full libp2p implementation, Geth blockchain integration, and a modern Svelte 5 frontend.

## Completed Features

### 1. Wallet System

- **Wallet Creation Page** - Users can create a new wallet with:
  - 12-word mnemonic phrase generation using ethers.js
  - Copy, Regenerate, and Download as TXT buttons
  - Verification quiz asking for 2 random words from the phrase
  - Cancel option to return to wallet selection

- **Wallet Login Page** - Users can import existing wallets using:
  - Private key (with or without 0x prefix)
  - 12-word recovery phrase
  - Validation for both methods
  - Error handling and user feedback

### 2. Account Page (Complete Wallet Management)

- **Wallet Information Display**:
  - Balance display (in CHR) with auto-refresh
  - Wallet address (copyable, full address shown)
  - Private key (hideable/visible toggle, copyable)
  - Security warning for private key exposure

- **Send CHR Functionality**:
  - Send CHR to any valid Ethereum address
  - Two-step confirmation (form → confirm screen)
  - Input validation (address format, sufficient balance)
  - MAX button for sending entire balance
  - Loading states during transaction submission

- **Transaction History**:
  - List of recent transactions (last 100 blocks)
  - Incoming/outgoing transaction indicators
  - Color-coded display (green for received, red for sent)
  - Transaction details (amount, address, block number, timestamp)
  - Copy transaction hash functionality

- **Test Faucet**:
  - "Get Test CHR" button for new wallets
  - Provides 1 CHR from dev faucet for testing
  - Disabled when user already has balance

- **Export/Import**:
  - Export wallet to JSON file
  - Secure logout with confirmation

### 3. Mining Page

- **Geth Node Management**:
  - Download Core-Geth binary with progress tracking
  - Automatic platform detection (Linux, macOS, Windows)
  - Start/stop Geth blockchain node
  - Genesis initialization with custom chain parameters

- **Node Status Display**:
  - Current block height
  - Connected peer count
  - Chain ID verification
  - Sync status indicator

- **Mining Controls**:
  - Start/stop mining with single click
  - Configurable thread count (1 to max CPU cores)
  - Thread slider with live preview
  - Miner address auto-set to wallet address

- **Mining Stats**:
  - Live hash rate display (H/s, KH/s, MH/s, GH/s)
  - Miner address display
  - Mining status indicator (Mining/Idle)

### 4. Network & DHT (Full libp2p Implementation)

- **Backend (Rust)**:
  - Complete libp2p v0.54 integration
  - Kademlia DHT for distributed storage
  - mDNS for local peer discovery
  - Custom request-response protocols:
    - `/chiral/ping/1.0.0` - Peer ping protocol
    - `/chiral/file-transfer/1.0.0` - Direct file push
    - `/chiral/file-request/1.0.0` - File request by hash
  - Noise protocol for encrypted transport
  - Yamux for stream multiplexing
  - TCP transport on all interfaces

- **DHT Operations**:
  - Store/retrieve key-value pairs
  - File metadata publishing
  - Peer discovery and routing
  - Bootstrap on peer connection

- **Frontend**:
  - `dhtService.ts` for managing DHT connections
  - Auto-polling every 5 seconds for network updates
  - Network page showing connection status
  - Peer list with multiaddresses

### 5. Geth Blockchain Integration

- **Configuration** (in `geth.rs`):
  - Chain ID: 98765
  - Network ID: 98765
  - RPC Endpoint: http://127.0.0.1:8545
  - Low difficulty (0x100) for faster mining
  - EVM-compatible (all Ethereum forks enabled)

- **Bootstrap Nodes**:
  ```
  enode://ae987db6...@130.245.173.105:30303
  enode://b3ead5f0...@20.85.124.187:30303
  ```

- **Genesis Allocation**:
  - Dev faucet address: `0x0000000000000000000000000000000000001337`
  - Initial balance: 10,000 CHR for testing

- **RPC APIs Enabled**:
  - eth, net, web3, personal, debug, miner, admin, txpool

- **Tauri Commands**:
  - `is_geth_installed` - Check if Geth binary exists
  - `download_geth` - Download Core-Geth with progress
  - `start_geth` - Start Geth with mining address
  - `stop_geth` - Stop Geth process
  - `get_geth_status` - Get sync/block/peer status
  - `start_mining` - Start CPU mining
  - `stop_mining` - Stop mining
  - `get_mining_status` - Get hash rate and miner info
  - `set_miner_address` - Set coinbase address

### 6. File Sharing System

- **Upload (Seeding)**:
  - Drag-and-drop file upload
  - SHA-256 hash computation (Merkle root)
  - File metadata publishing to DHT
  - Torrent file export for sharing
  - Continuous seeding from original location

- **Download**:
  - Search by file hash
  - Multi-seeder discovery via DHT
  - Direct peer-to-peer file transfer
  - Progress tracking and completion events
  - Auto-save to Downloads folder

- **Tauri Commands**:
  - `publish_file` - Hash and publish file to DHT
  - `search_file` - Look up file metadata by hash
  - `start_download` - Initiate download from seeders
  - `register_shared_file` - Re-register on startup
  - `parse_torrent_file` - Parse .torrent files
  - `export_torrent_file` - Generate .torrent

### 7. Wallet/Transaction Backend

- **Balance Queries**:
  - `get_wallet_balance` - Query via eth_getBalance RPC
  - Returns balance in CHR and wei
  - Connects to local Geth node

- **Transactions**:
  - `send_transaction` - Send CHR between addresses
  - Auto-import private key to Geth keystore
  - Account unlock before sending
  - Gas price set to 0 for test network

- **Transaction History**:
  - `get_transaction_history` - Scan last 100 blocks
  - Returns transactions involving address
  - Includes block number, timestamp, gas used

- **Test Faucet**:
  - `request_faucet` - Get 1 CHR from dev address
  - Uses pre-allocated genesis balance

### 8. Navigation & Layout

- **Navbar Component** with pages:
  - Download - File download management
  - Upload - File sharing/seeding
  - ChiralDrop - Direct peer transfers
  - Account - Wallet management
  - Network - P2P network status
  - Mining - Geth & mining controls
  - Settings - App configuration

- **Features**:
  - Logout button with confirmation
  - Connection status indicator
  - Active page highlighting
  - Responsive design

### 9. Additional Services

- **Toast Notifications** (`toastStore.ts`):
  - Success, error, info, warning types
  - Auto-dismiss with timeout
  - Stack multiple toasts

- **ChiralDrop** (`chiralDropStore.ts`):
  - Direct peer-to-peer file transfers
  - Transfer history tracking

- **Encrypted History** (`encryptedHistoryService.ts`):
  - Secure storage of sensitive data

## Project Structure

```
v2-chiral-network/
├── src/
│   ├── lib/
│   │   ├── components/
│   │   │   ├── Navbar.svelte
│   │   │   ├── WalletCreation.svelte
│   │   │   ├── WalletLogin.svelte
│   │   │   ├── GethStatus.svelte
│   │   │   └── Toast.svelte
│   │   ├── services/
│   │   │   ├── walletService.ts
│   │   │   └── gethService.ts
│   │   ├── stores.ts
│   │   ├── walletService.ts
│   │   ├── dhtService.ts
│   │   ├── toastStore.ts
│   │   ├── chiralDropStore.ts
│   │   ├── encryptedHistoryService.ts
│   │   ├── aliasService.ts
│   │   └── utils.ts
│   ├── pages/
│   │   ├── Wallet.svelte
│   │   ├── Download.svelte
│   │   ├── Upload.svelte
│   │   ├── ChiralDrop.svelte
│   │   ├── Account.svelte
│   │   ├── Network.svelte
│   │   ├── Mining.svelte
│   │   └── Settings.svelte
│   ├── App.svelte
│   └── main.ts
├── src-tauri/
│   └── src/
│       ├── dht.rs          # libp2p DHT service
│       ├── geth.rs         # Geth management
│       ├── file_transfer.rs # File transfer service
│       ├── lib.rs          # Tauri commands
│       └── main.rs
└── package.json
```

## Technologies Used

- **Frontend**: Svelte 5, TypeScript, Vite, Tailwind CSS
- **Router**: @mateothegreat/svelte5-router
- **Wallet**: ethers.js for mnemonic and key management
- **Backend**: Tauri 2, Rust, Tokio
- **P2P**: libp2p 0.54 (Rust)
  - Kademlia DHT
  - mDNS discovery
  - Noise encryption
  - Yamux multiplexing
  - Request-response protocols
- **Blockchain**: Core-Geth (Ethereum-compatible)
- **Icons**: lucide-svelte
- **HTTP**: reqwest (Rust)

## Running the Application

### Development

```bash
cd v2-chiral-network
npm install
npm run tauri:dev
```

### Production Build

```bash
npm run tauri:build
```

### Starting Geth Manually (Optional)

The app handles Geth automatically, but for manual control:

```bash
# Download Geth (if not installed)
# The app will download to bin/geth automatically

# Initialize genesis (first run)
./bin/geth --datadir ~/.local/share/chiral-network/geth init genesis.json

# Start Geth
./bin/geth --datadir ~/.local/share/chiral-network/geth \
  --networkid 98765 \
  --http --http.addr 127.0.0.1 --http.port 8545 \
  --http.api eth,net,web3,personal,debug,miner,admin,txpool \
  --http.corsdomain "*" \
  --syncmode snap \
  --miner.etherbase YOUR_ADDRESS
```

### Mining

1. Go to Mining page
2. Download Geth if not installed
3. Start Geth node
4. Adjust thread count (1-N cores)
5. Click "Start Mining"

### Getting Test CHR

Option 1: Use the faucet
- Go to Account page
- Click "Get Test CHR" button (if balance is 0)

Option 2: Mine blocks
- Start mining on Mining page
- Block rewards go to your wallet address

## Key Improvements Over V1

1. **Full libp2p Implementation** - Real P2P networking (not placeholder)
2. **Geth Integration** - Real blockchain with mining support
3. **Complete Account Page** - Send/receive CHR, transaction history
4. **Mining Support** - CPU mining with configurable threads
5. **Better State Management** - Svelte 5 runes and stores
6. **Type Safety** - Full TypeScript implementation
7. **Modern Architecture** - Clean separation of concerns
8. **Production Ready** - Proper error handling throughout

## Configuration

### Chain Parameters

| Parameter | Value |
|-----------|-------|
| Chain ID | 98765 |
| Network ID | 98765 |
| Block Gas Limit | 30,000,000 |
| Initial Difficulty | 256 (0x100) |
| RPC Port | 8545 |
| P2P Port | 30303 |

### Genesis Allocations

| Address | Balance |
|---------|---------|
| 0x0000...001337 (Faucet) | 10,000 CHR |

## Next Steps (Future Iterations)

- [ ] WebRTC for NAT traversal
- [ ] Circuit Relay v2 support
- [ ] Multi-source parallel downloads
- [ ] Bandwidth scheduling
- [ ] File encryption (AES-256-GCM)
- [ ] Geographic peer distribution map
- [ ] Mining rewards tracking/history
- [ ] Hardware wallet support
- [ ] Mobile app version

## Troubleshooting

### Geth Won't Start

1. Check if Geth is downloaded: Mining page shows "Not Installed"
2. Check data directory permissions: `~/.local/share/chiral-network/geth`
3. Check if port 8545 is available
4. Check Geth logs: `~/.local/share/chiral-network/geth/geth.log`

### Can't Get Balance

1. Ensure Geth is running (Mining page shows "Running")
2. Wait for Geth to sync (check block height)
3. Verify RPC is responding: `curl http://127.0.0.1:8545`

### No Peers Connecting

1. Check firewall allows port 30303
2. Ensure bootstrap nodes are reachable
3. Check Network page for mDNS discovered peers

### Transaction Fails

1. Ensure sufficient balance (including gas)
2. Check Geth is running and synced
3. Verify recipient address format (0x + 40 hex chars)
