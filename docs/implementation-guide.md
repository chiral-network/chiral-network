# Chiral Network Implementation Guide

## Development Setup

### Prerequisites

- Node.js 18+ and npm
- Rust 1.70+ and Cargo
- Git
- Python 3.8+ (for build scripts)
- C++ compiler (for native modules)

### Repository Structure

```text
chiral-network/
├── blockchain/          # Ethereum-compatible implementation (Geth integration)
│   ├── src/            # Core blockchain code
│   ├── wallet/         # Wallet implementation
│   └── mining/         # Ethash mining implementation
├── p2p/                # P2P file sharing implementation
│   ├── dht/           # DHT implementation (Kademlia)
│   ├── chunks/        # Chunk management
│   └── api/           # Peer API
├── chiral-app/        # Desktop application
│   ├── src/           # Svelte frontend
│   ├── src-tauri/     # Tauri backend (Rust)
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

## Phase 2: P2P File Sharing Implementation

**Note**: All nodes are equal peers. This implementation allows any node to seed files, download files, and participate in DHT discovery.

### Step 1: DHT Implementation

Create `p2p/dht/dht.rs`:

```rust
use libp2p::{
    kad::{Kademlia, KademliaConfig, KademliaEvent},
    swarm::{Swarm, SwarmBuilder},
    PeerId, Transport,
};

pub struct DhtService {
    cmd_tx: mpsc::Sender<DhtCommand>,
    event_rx: Arc<Mutex<mpsc::Receiver<DhtEvent>>>,
    peer_id: String,
}

impl DhtService {
    pub async fn new(port: u16, bootstrap_nodes: Vec<String>) -> Result<Self, Box<dyn std::error::Error>> {
        let local_key = identity::Keypair::generate_ed25519();
        let local_peer_id = PeerId::from(local_key.public());
        let peer_id_str = local_peer_id.to_string();

        // Create Kademlia configuration
        let store = MemoryStore::new(local_peer_id);
        let mut kad_cfg = KademliaConfig::new(StreamProtocol::new("/chiral/kad/1.0.0"));
        kad_cfg.set_query_timeout(Duration::from_secs(10));

        let mut kademlia = Kademlia::with_config(local_peer_id, store, kad_cfg);
        kademlia.set_mode(Some(Mode::Server));

        // Create identify and mDNS behaviours
        let identify = identify::Behaviour::new(identify::Config::new(
            "/chiral/1.0.0".to_string(),
            local_key.public(),
        ));
        let mdns = Mdns::new(Default::default(), local_peer_id)?;

        let behaviour = DhtBehaviour { kademlia, identify, mdns };

        // Create swarm and spawn background task
        let mut swarm = SwarmBuilder::with_existing_identity(local_key)
            .with_tokio()
            .with_tcp(Default::default(), libp2p::noise::Config::new, libp2p::yamux::Config::default)?
            .with_behaviour(|_| behaviour)?
            .build();

        let listen_addr: Multiaddr = format!("/ip4/0.0.0.0/tcp/{}", port).parse()?;
        swarm.listen_on(listen_addr)?;

        let (cmd_tx, cmd_rx) = mpsc::channel(100);
        let (event_tx, event_rx) = mpsc::channel(100);
        let connected_peers = Arc::new(Mutex::new(HashSet::new()));

        // Spawn the DHT node task
        tokio::spawn(run_dht_node(swarm, local_peer_id, cmd_rx, event_tx, connected_peers));

        Ok(DhtService {
            cmd_tx,
            event_rx: Arc::new(Mutex::new(event_rx)),
            peer_id: peer_id_str,
        })
    }

    pub async fn publish_file(&self, metadata: FileMetadata) -> Result<(), String> {
        self.cmd_tx.send(DhtCommand::PublishFile(metadata)).await
            .map_err(|e| e.to_string())
    }

    pub async fn search_file(&self, file_hash: String) -> Result<(), String> {
        self.cmd_tx.send(DhtCommand::SearchFile(file_hash)).await
            .map_err(|e| e.to_string())
    }

    pub async fn connect_peer(&self, addr: String) -> Result<(), String> {
        self.cmd_tx.send(DhtCommand::ConnectPeer(addr)).await
            .map_err(|e| e.to_string())
    }

    pub async fn get_peer_id(&self) -> String {
        self.peer_id.clone()
    }
}

```

### Step 2: Chunk Manager

Create `p2p/chunks/manager.rs`:

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

### Step 3: Peer API

Create `p2p/api/server.rs`:

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

## Phase 3: Desktop Application

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
    // Search DHT for file metadata and available seeders
    const metadata = await this.searchDHT(hash);

    if (!metadata || metadata.seeders.length === 0) {
      throw new Error("No seeders found - file not available in network");
    }

    // Download chunks from seeders (any node can be a seeder)
    const chunks = await invoke<Uint8Array[]>("download_file", {
      hash,
      metadata,
    });

    // Combine chunks into blob
    const blob = new Blob(chunks);
    return blob;
  }

  private async searchDHT(hash: string): Promise<FileMetadata | null> {
    // Search DHT for file metadata without centralized market
    return await invoke<FileMetadata>("search_dht_metadata", { hash });
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

    // Start seeding chunks (make available to network)
    for chunk in chunks {
        publish_chunk_to_dht(&chunk).await?;
    }

    // Register in DHT
    register_in_dht(&file_hash, &chunks).await?;

    Ok(file_hash)
}

#[command]
pub async fn download_file(
    hash: String,
    seeder: String
) -> Result<Vec<Vec<u8>>, String> {
    // Get file metadata from DHT
    let metadata = get_file_metadata(&hash).await?;

    // Download chunks from seeder (any node can be a seeder)
    let mut chunks = Vec::new();
    for chunk_info in metadata.chunks {
        let chunk_data = download_chunk(&seeder, &chunk_info.hash).await?;
        chunks.push(chunk_data);
    }

    // Verify and decrypt chunks
    let decrypted = decrypt_chunks(chunks)?;

    // Distribute payment rewards to seeders
    distribute_rewards(&metadata.seeders, &file_hash).await?;

    Ok(decrypted)
}
```

## Phase 4: Integration Testing

### NAT reachability instrumentation

- AutoNAT v2 client and server behaviours are mounted alongside Kademlia. The
  probe interval defaults to 30 s and can be adjusted with
  `--autonat-probe-interval`; disable probing entirely via `--disable-autonat`.
- Additional AutoNAT servers can be supplied with repeated
  `--autonat-server` flags. `--show-reachability` streams a periodic summary in
  headless mode (state, confidence, observed addresses, last error) so ops can
  validate reachability without the GUI.
- The Network → DHT page now surfaces a “Reachability” card that mirrors Kubo’s
  vocabulary (Direct/Relayed/Unknown), confidence badge, observed external
  addresses with copy affordances, and the last few probe summaries. Toasts are
  emitted when reachability changes (restored, degraded, reset) to give desktop
  operators immediate feedback.

### AutoRelay & reservation management

- AutoRelay behavior is enabled by default in GUI mode to automatically discover
  and use relay servers for NAT traversal.
- The relay client listens for Circuit Relay v2 reservations from discovered
  relay candidates (bootstrap nodes by default, or custom relays via `--relay`).
- When a peer is identified as a relay candidate (via `identify` protocol), the
  node attempts to listen on a relay circuit address to enable inbound connections
  from NAT-restricted peers.
- Relay reservation events (accepted, renewed, evicted) are tracked in metrics:
  - `active_relay_peer_id`: PeerId of the current relay server
  - `relay_reservation_status`: Current reservation state (accepted/pending/failed)
  - `reservation_renewals`: Counter for successful reservation renewals
  - `reservation_evictions`: Counter for reservations lost/evicted
- The Network → DHT page displays a "Relay Status" card showing:
  - AutoRelay enabled/disabled badge
  - Active relay peer ID (truncated for display)
  - Reservation status and renewal counters
  - Last successful reservation and eviction timestamps
- Headless mode supports:
  - `--disable-autorelay` to turn off AutoRelay behavior
  - `--relay <multiaddr>` to specify preferred relay servers (repeatable)
- Info-level log messages emit when relay reservations are accepted or circuits
  are established, including relay peer IDs for debugging.

### DCUtR hole-punching

- DCUtR (Direct Connection Upgrade through Relay) behavior is automatically
  enabled when AutoNAT is active, allowing peers behind NATs to coordinate
  simultaneous hole-punching attempts via relay servers.
- The `--show-dcutr` flag prints periodic DCUtR metrics in headless mode,
  including hole-punch attempts, successes, failures, and success rate.
- The Network → DHT page displays a "DCUtR Hole-Punching" card showing real-time
  metrics: total attempts, successes (green), failures (red), success rate
  percentage, enabled/disabled badge, and timestamps for last success/failure.
- DCUtR events are logged at the `info` level with peer IDs and relay addresses,
  and emit `DhtEvent::Info` or `DhtEvent::Warning` messages for UI feedback.

### Local download resilience & tracing

The desktop runtime now performs up to three local download attempts with an
exponential backoff (250 ms → 500 ms → 1 s). Each attempt emits a
`download_attempt` tracing span carrying `hash`, `attempt`, `max_attempts`, and
`duration_ms` fields. When debugging failed transfers, filter logs on
`download_succeeded`/`download_failed` events to see whether the file was missing
or a transient write error occurred. The final error returned to the UI remains
the last failure string so existing user messaging stays unchanged.

In addition to spans, the Tauri shell now broadcasts a `download_attempt`
frontend event and exposes a `get_download_metrics` command returning aggregate
success/failure counters and the last 20 attempts. Headless mode gains a
`--show-downloads` flag that prints the same snapshot at startup so operators can
confirm retry behaviour without the GUI.
### Test Network Setup

```bash
# Start blockchain node
chiral-node --chain chiral-spec.json --config chiral-config.toml

# Start peer nodes (for file sharing)
cd p2p && cargo run --bin peer-node -- --port 8080 --dht-port 4001

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

  test("DHT peer discovery", async () => {
    const metadata = await fileService.searchDHT("test_hash");
    expect(metadata).toBeTruthy();
    expect(metadata.seeders.length).toBeGreaterThan(0);
  });
});
```

## Deployment

### Production Build

```bash
# Build blockchain
cd blockchain && cargo build --release

# Build peer node
cd p2p && cargo build --release

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

  peer:
    image: chiral/peer:latest
    ports:
      - "8080:8080"
      - "4001:4001"
    volumes:
      - peer-data:/data
    environment:
      - DHT_BOOTSTRAP=/ip4/bootstrap.chiral.network/tcp/4001

volumes:
  blockchain-data:
  peer-data:
```

## Monitoring

### Metrics Collection

```javascript
// Prometheus metrics
const prometheus = require("prom-client");

const metrics = {
  filesShared: new prometheus.Counter({
    name: "chiral_files_shared_total",
    help: "Total number of files shared (seeding)",
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

#### Issue: File sharing failures

```bash
# Check peer node logs
tail -f peer-node.log

# Verify seeding files
ls -la /data/files/chunks/

# Test chunk availability manually
curl -X POST http://localhost:8080/chunks \
  -H "Content-Type: application/octet-stream" \
  --data-binary @test-chunk.bin
```
