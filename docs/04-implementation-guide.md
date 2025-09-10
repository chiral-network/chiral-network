# Chiral Network Implementation Guide

## Development Setup

### Prerequisites

- Node.js 18+ and npm
- Rust 1.70+ and Cargo
- Git
- Python 3.8+ (for build scripts)
- C++ compiler (for native modules)

### Repository Structure

```
chiral-network/
├── blockchain/          # Ethereum-compatible implementation
│   ├── src/            # Core blockchain code
│   ├── wallet/         # Wallet implementation
│   └── mining/         # Ethash mining implementation
├── storage/            # Storage node implementation
│   ├── dht/           # DHT implementation
│   ├── chunks/        # Chunk management
│   └── api/           # Storage API
├── market/            # Market server
│   ├── server/        # Market API server
│   ├── database/      # Market database
│   └── contracts/     # Smart contracts (optional)
├── chiral-app/        # Desktop application
│   ├── src/           # Svelte frontend
│   ├── src-tauri/     # Tauri backend
│   └── docs/          # Documentation
└── scripts/           # Build and deployment scripts
```

## Phase 1: Blockchain Setup

### Step 1: Setup Ethereum Node

```bash
# Install Geth (Go-Ethereum)
# macOS
brew tap ethereum/ethereum
brew install ethereum

# Linux
sudo add-apt-repository -y ppa:ethereum/ethereum
sudo apt-get update
sudo apt-get install ethereum

# Or build from source
git clone https://github.com/ethereum/go-ethereum.git
cd go-ethereum
make geth
```

### Step 2: Create Genesis Configuration

Create `genesis.json` (Geth-compatible format):

```json
{
  "config": {
    "chainId": 98765,
    "homesteadBlock": 0,
    "eip150Block": 0,
    "eip155Block": 0,
    "eip158Block": 0,
    "byzantiumBlock": 0,
    "constantinopleBlock": 0,
    "petersburgBlock": 0,
    "istanbulBlock": 0,
    "berlinBlock": 0,
    "londonBlock": 0
  },
  "difficulty": "0x400000",
  "gasLimit": "0x47b760",
  "alloc": {},
  "coinbase": "0x0000000000000000000000000000000000000000",
  "extraData": "0x4b656570206f6e206b656570696e67206f6e21",
  "nonce": "0x0000000000000042",
  "mixhash": "0x0000000000000000000000000000000000000000000000000000000000000000",
  "parentHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
  "timestamp": "0x68b3b2ca"
}
```

### Step 3: Initialize Blockchain

```bash
# Initialize the genesis block
geth init genesis.json --datadir ./chiral-data

# Create new account for mining
geth account new --datadir ./chiral-data

# Save the account address for mining rewards
```

### Step 4: Start the Network

```bash
# Start the first node (bootnode)
geth --datadir ./chiral-data \
     --networkid 98765 \
     --port 30304 \
     --http --http.port 8546 \
     --ws --ws.port 8547 \
     --mine --miner.threads 2 \
     --miner.etherbase 0xYourMinerAddress \
     --allow-insecure-unlock \
     console

```

## Phase 2: Storage Implementation

### Step 1: DHT Implementation

Create `storage/dht/dht.rs`:

```rust
use libp2p::{
    kad::{Kademlia, KademliaConfig, KademliaEvent},
    swarm::{Swarm, SwarmBuilder},
    PeerId, Transport,
};

pub struct DHTNode {
    swarm: Swarm<Kademlia<MemoryStore>>,
    peer_id: PeerId,
}

impl DHTNode {
    pub fn new() -> Result<Self, Box<dyn Error>> {
        let local_key = identity::Keypair::generate_ed25519();
        let peer_id = PeerId::from(local_key.public());

        let transport = libp2p::development_transport(local_key)?;

        let mut cfg = KademliaConfig::default();
        cfg.set_query_timeout(Duration::from_secs(60));

        let store = MemoryStore::new(peer_id);
        let kademlia = Kademlia::with_config(peer_id, store, cfg);

        let swarm = SwarmBuilder::new(transport, kademlia, peer_id)
            .executor(Box::new(|fut| {
                tokio::spawn(fut);
            }))
            .build();

        Ok(DHTNode { swarm, peer_id })
    }

    pub async fn store(&mut self, key: Vec<u8>, value: Vec<u8>) -> Result<(), Error> {
        self.swarm.behaviour_mut().put_record(
            Record::new(key, value),
            Quorum::One
        )?;
        Ok(())
    }

    pub async fn retrieve(&mut self, key: &[u8]) -> Result<Vec<u8>, Error> {
        let query_id = self.swarm.behaviour_mut().get_record(key.into());
        // Wait for result...
    }
}
```

### Step 2: Chunk Manager

Create `storage/chunks/manager.rs`:

```rust
use sha2::{Sha256, Digest};
use aes_gcm::{Aes256Gcm, Key, Nonce};

pub struct ChunkManager {
    chunk_size: usize,
    storage_path: PathBuf,
}

impl ChunkManager {
    pub fn new(storage_path: PathBuf) -> Self {
        ChunkManager {
            chunk_size: 256 * 1024, // 256KB
            storage_path,
        }
    }

    pub fn chunk_file(&self, file_path: &Path) -> Result<Vec<ChunkInfo>, Error> {
        let mut file = File::open(file_path)?;
        let mut chunks = Vec::new();
        let mut buffer = vec![0u8; self.chunk_size];
        let mut index = 0;

        loop {
            let bytes_read = file.read(&mut buffer)?;
            if bytes_read == 0 { break; }

            let chunk_data = &buffer[..bytes_read];
            let chunk_hash = self.hash_chunk(chunk_data);
            let encrypted = self.encrypt_chunk(chunk_data)?;

            chunks.push(ChunkInfo {
                index,
                hash: chunk_hash,
                size: bytes_read,
                encrypted_size: encrypted.len(),
            });

            self.save_chunk(&chunk_hash, &encrypted)?;
            index += 1;
        }

        Ok(chunks)
    }

    fn encrypt_chunk(&self, data: &[u8]) -> Result<Vec<u8>, Error> {
        let key = Key::from_slice(b"encryption_key_32_bytes_long!!!!");
        let cipher = Aes256Gcm::new(key);
        let nonce = Nonce::from_slice(b"unique_nonce");

        cipher.encrypt(nonce, data).map_err(|e| e.into())
    }

    fn hash_chunk(&self, data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }
}
```

### Step 3: Storage Node API

Create `storage/api/server.rs`:

```rust
use warp::{Filter, Reply};

pub fn storage_api() -> impl Filter<Extract = impl Reply> + Clone {
    store_chunk()
        .or(retrieve_chunk())
        .or(list_chunks())
        .or(delete_chunk())
}

fn store_chunk() -> impl Filter<Extract = impl Reply> + Clone {
    warp::path!("chunks")
        .and(warp::post())
        .and(warp::body::bytes())
        .and_then(|body: Bytes| async move {
            // Store chunk implementation
            Ok::<_, warp::Rejection>(warp::reply::json(&json!({
                "hash": "chunk_hash",
                "size": body.len()
            })))
        })
}

fn retrieve_chunk() -> impl Filter<Extract = impl Reply> + Clone {
    warp::path!("chunks" / String)
        .and(warp::get())
        .and_then(|hash: String| async move {
            // Retrieve chunk implementation
            Ok::<_, warp::Rejection>(warp::reply::with_status(
                "chunk_data",
                StatusCode::OK
            ))
        })
}
```

## Phase 3: Market Server

### Step 1: Database Schema

Create `market/database/schema.sql`:

```sql
CREATE DATABASE chiral_market;
USE chiral_market;

CREATE TABLE files (
    file_hash VARCHAR(64) PRIMARY KEY,
    file_name VARCHAR(255),
    file_size BIGINT,
    mime_type VARCHAR(100),
    upload_date TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    total_chunks INT,
    INDEX idx_upload_date (upload_date)
);

CREATE TABLE suppliers (
    id INT AUTO_INCREMENT PRIMARY KEY,
    supplier_id VARCHAR(64) NOT NULL,
    file_hash VARCHAR(64) NOT NULL,
    ip_address VARCHAR(45) NOT NULL,
    port INT NOT NULL,
    price_per_mb DECIMAL(10, 8),
    bandwidth_limit INT,
    reputation DECIMAL(3, 2),
    last_seen TIMESTAMP DEFAULT CURRENT_TIMESTAMP ON UPDATE CURRENT_TIMESTAMP,
    expires_at TIMESTAMP,
    FOREIGN KEY (file_hash) REFERENCES files(file_hash),
    INDEX idx_file_hash (file_hash),
    INDEX idx_expires (expires_at)
);

CREATE TABLE transactions (
    id INT AUTO_INCREMENT PRIMARY KEY,
    tx_hash VARCHAR(64) UNIQUE,
    buyer_address VARCHAR(64),
    supplier_id VARCHAR(64),
    file_hash VARCHAR(64),
    amount DECIMAL(18, 8),
    status ENUM('pending', 'confirmed', 'failed'),
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    confirmed_at TIMESTAMP NULL,
    INDEX idx_status (status)
);
```

### Step 2: Market API Server

Create `market/server/index.js`:

```javascript
const express = require("express");
const mysql = require("mysql2/promise");
const app = express();

// Database connection pool
const pool = mysql.createPool({
  host: "localhost",
  user: "chiral",
  password: "password",
  database: "chiral_market",
  waitForConnections: true,
  connectionLimit: 10,
});

// Register as supplier
app.post("/api/v1/market/supply", async (req, res) => {
  const { file_hash, ip, port, price, bandwidth } = req.body;

  try {
    // Check if file exists
    const [files] = await pool.execute(
      "SELECT file_hash FROM files WHERE file_hash = ?",
      [file_hash],
    );

    if (files.length === 0) {
      return res.status(404).json({ error: "File not found" });
    }

    // Insert or update supplier
    await pool.execute(
      `INSERT INTO suppliers 
             (supplier_id, file_hash, ip_address, port, price_per_mb, bandwidth_limit, expires_at)
             VALUES (?, ?, ?, ?, ?, ?, DATE_ADD(NOW(), INTERVAL 1 HOUR))
             ON DUPLICATE KEY UPDATE
             ip_address = VALUES(ip_address),
             port = VALUES(port),
             price_per_mb = VALUES(price_per_mb),
             bandwidth_limit = VALUES(bandwidth_limit),
             last_seen = NOW(),
             expires_at = DATE_ADD(NOW(), INTERVAL 1 HOUR)`,
      [req.user.id, file_hash, ip, port, price, bandwidth],
    );

    res.json({ success: true, expires_in: 3600 });
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
});

// Query suppliers for file
app.get("/api/v1/market/lookup/:hash", async (req, res) => {
  const { hash } = req.params;

  try {
    const [suppliers] = await pool.execute(
      `SELECT supplier_id, ip_address, port, price_per_mb, bandwidth_limit, reputation
             FROM suppliers
             WHERE file_hash = ? AND expires_at > NOW()
             ORDER BY price_per_mb ASC, reputation DESC`,
      [hash],
    );

    res.json(suppliers);
  } catch (error) {
    res.status(500).json({ error: error.message });
  }
});

app.listen(3000, () => {
  console.log("Market server running on port 3000");
});
```

## Phase 4: Desktop Application

### Step 1: Frontend Setup

```bash
# Create Svelte + Tauri app
npm create tauri-app@latest chiral-app
cd chiral-app

# Install dependencies
npm install
npm install @tauri-apps/api
npm install tailwindcss lucide-svelte
```

### Step 2: File Service Implementation

Create `src/lib/services/fileService.ts`:

```typescript
import { invoke } from "@tauri-apps/api/tauri";
import type { FileItem } from "../stores";

export class FileService {
  async uploadFile(file: File): Promise<string> {
    // Read file as array buffer
    const buffer = await file.arrayBuffer();
    const bytes = new Uint8Array(buffer);

    // Call Rust backend to process file
    const hash = await invoke<string>("upload_file", {
      name: file.name,
      data: Array.from(bytes),
      size: file.size,
    });

    return hash;
  }

  async downloadFile(hash: string): Promise<Blob> {
    // Query market for suppliers
    const suppliers = await this.queryMarket(hash);

    if (suppliers.length === 0) {
      throw new Error("File not found in network");
    }

    // Select best supplier (lowest price)
    const supplier = suppliers[0];

    // Download chunks
    const chunks = await invoke<Uint8Array[]>("download_file", {
      hash,
      supplier: supplier.id,
    });

    // Combine chunks into blob
    const blob = new Blob(chunks);
    return blob;
  }

  private async queryMarket(hash: string): Promise<Supplier[]> {
    const response = await fetch(`${MARKET_URL}/api/v1/market/lookup/${hash}`);
    return response.json();
  }
}
```

### Step 3: Tauri Backend

Create `src-tauri/src/file_handler.rs`:

```rust
use tauri::command;

#[command]
pub async fn upload_file(
    name: String,
    data: Vec<u8>,
    size: usize
) -> Result<String, String> {
    // Create chunk manager
    let chunk_manager = ChunkManager::new(get_storage_path());

    // Save temporary file
    let temp_path = save_temp_file(&name, &data)?;

    // Chunk the file
    let chunks = chunk_manager.chunk_file(&temp_path)
        .map_err(|e| e.to_string())?;

    // Calculate file hash
    let file_hash = calculate_file_hash(&data);

    // Upload chunks to storage nodes
    for chunk in chunks {
        upload_chunk_to_network(&chunk).await?;
    }

    // Register in DHT
    register_in_dht(&file_hash, &chunks).await?;

    // Register in market
    register_in_market(&file_hash, &name, size).await?;

    Ok(file_hash)
}

#[command]
pub async fn download_file(
    hash: String,
    supplier: String
) -> Result<Vec<Vec<u8>>, String> {
    // Get file metadata from DHT
    let metadata = get_file_metadata(&hash).await?;

    // Download chunks from supplier
    let mut chunks = Vec::new();
    for chunk_info in metadata.chunks {
        let chunk_data = download_chunk(&supplier, &chunk_info.hash).await?;
        chunks.push(chunk_data);
    }

    // Verify and decrypt chunks
    let decrypted = decrypt_chunks(chunks)?;

    // Make payment
    make_payment(&supplier, metadata.total_price).await?;

    Ok(decrypted)
}
```

## Phase 5: Integration Testing

### Test Network Setup

```bash
# Start blockchain node
chiral-node --chain chiral-spec.json --config chiral-config.toml

# Start storage nodes
cd storage && cargo run --bin storage-node -- --port 8080 --dht-port 4001

# Start market server
cd market && npm start

# Start desktop app
cd chiral-app && npm run tauri dev
```

### Integration Tests

Create `tests/integration.test.ts`:

```typescript
describe("Chiral Network Integration", () => {
  let fileService: FileService;
  let walletService: WalletService;

  beforeAll(async () => {
    fileService = new FileService();
    walletService = new WalletService();
    await walletService.initialize();
  });

  test("Upload and download file", async () => {
    // Create test file
    const content = "Hello, Chiral Network!";
    const file = new File([content], "test.txt");

    // Upload file
    const hash = await fileService.uploadFile(file);
    expect(hash).toMatch(/^[a-f0-9]{64}$/);

    // Wait for propagation
    await new Promise((resolve) => setTimeout(resolve, 5000));

    // Download file
    const downloaded = await fileService.downloadFile(hash);
    const text = await downloaded.text();
    expect(text).toBe(content);
  });

  test("Market discovery", async () => {
    const suppliers = await fileService.queryMarket("test_hash");
    expect(suppliers.length).toBeGreaterThan(0);
    expect(suppliers[0]).toHaveProperty("price");
  });
});
```

## Deployment

### Production Build

```bash
# Build blockchain
cd blockchain && cargo build --release

# Build storage node
cd storage && cargo build --release

# Build market server
cd market && npm run build

# Build desktop app
cd chiral-app && npm run tauri build
```

### Docker Deployment

Create `docker-compose.yml`:

```yaml
version: "3.8"

services:
  blockchain:
    image: chiral/blockchain:latest
    ports:
      - "30304:30304"
      - "8546:8546"
    volumes:
      - blockchain-data:/data
    command: --chain /config/chiral-spec.json --config /config/chiral-config.toml

  storage:
    image: chiral/storage:latest
    ports:
      - "8080:8080"
      - "4001:4001"
    volumes:
      - storage-data:/storage
    environment:
      - DHT_BOOTSTRAP=/ip4/bootstrap.chiral.network/tcp/4001

  market:
    image: chiral/market:latest
    ports:
      - "3000:3000"
    environment:
      - DB_HOST=postgres
      - DB_PASSWORD=${DB_PASSWORD}
    depends_on:
      - postgres

  postgres:
    image: postgres:15
    environment:
      - POSTGRES_DB=chiral_market
      - POSTGRES_PASSWORD=${DB_PASSWORD}
    volumes:
      - postgres-data:/var/lib/postgresql/data

volumes:
  blockchain-data:
  storage-data:
  postgres-data:
```

## Monitoring

### Metrics Collection

```javascript
// Prometheus metrics
const prometheus = require("prom-client");

const metrics = {
  filesUploaded: new prometheus.Counter({
    name: "chiral_files_uploaded_total",
    help: "Total number of files uploaded",
  }),

  bytesTransferred: new prometheus.Counter({
    name: "chiral_bytes_transferred_total",
    help: "Total bytes transferred",
  }),

  activeNodes: new prometheus.Gauge({
    name: "chiral_active_nodes",
    help: "Number of active nodes",
  }),
};
```

## Troubleshooting

### Common Issues

#### Issue: Blockchain not syncing

```bash
# Check peer connections
curl -X POST -H "Content-Type: application/json" \
  --data '{"jsonrpc":"2.0","method":"net_peerCount","params":[],"id":1}' \
  http://localhost:8546

# Add manual peer
curl -X POST -H "Content-Type: application/json" \
  --data '{"jsonrpc":"2.0","method":"admin_addPeer","params":["enode://..."],"id":1}' \
  http://localhost:8546

# Check sync status
curl -X POST -H "Content-Type: application/json" \
  --data '{"jsonrpc":"2.0","method":"eth_syncing","params":[],"id":1}' \
  http://localhost:8546
```

#### Issue: DHT lookup failures

```bash
# Check DHT status
curl http://localhost:8080/api/dht/status

# Bootstrap DHT
curl -X POST http://localhost:8080/api/dht/bootstrap

# Check routing table
curl http://localhost:8080/api/dht/peers
```

#### Issue: File upload failures

```bash
# Check storage node logs
tail -f storage-node.log

# Verify chunk storage
ls -la /storage/chunks/

# Test chunk upload manually
curl -X POST http://localhost:8080/chunks \
  -H "Content-Type: application/octet-stream" \
  --data-binary @test-chunk.bin
```
