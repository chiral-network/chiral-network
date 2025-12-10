# Chiral Network Architecture

## System Architecture Overview

The Chiral Network implements a layered architecture that separates concerns and enables modular development. Each layer communicates through well-defined interfaces, allowing for independent scaling and updates.

## Core Architecture Components

### 1. Blockchain Infrastructure

#### Ethereum Network Implementation

```
Network Parameters:
- Network ID: 98765
- Chain ID: 98765 (0x181cd)
- Genesis Block: Custom with pre-funded addresses
- Block Time: ~15 seconds
- Initial Difficulty: 0x400000
- Difficulty Adjustment: Dynamic adjustment per block
- Mining Algorithm: Ethash (Proof of Work)
- Gas Limit: 4,700,000 (0x47b760)
- Extra Data: "Keep on keeping on!" (0x4b656570206f6e206b656570696e67206f6e21)
```

#### Modifications from Ethereum Mainnet

- **Bootstrap Nodes**: Custom seed nodes for network discovery
- **Genesis Configuration**: Pre-allocated funds for initial distribution
- **Network Isolation**: Separate network/chain ID (98765) to prevent mainnet connection
- **Chain Parameters**: Adjusted block time and difficulty for network requirements

#### Transaction Model

```
Ethereum Transaction Structure:
{
  nonce: 0,
  gasPrice: 20000000000, // 20 Gwei
  gasLimit: 21000,
  to: "0x742d35Cc6634C0532925a3b8D0C9e0c8b346b983",
  value: 1000000000000000000, // 1 ETH in wei
  data: "0x",
  v: 28,
  r: "0x...",
  s: "0x..."
}
```

### 2. Distributed Storage System

#### DHT Implementation

The system uses a Kademlia-based DHT for distributed file indexing:

```
DHT Structure:
- Node ID: 160-bit identifier
- Routing Table: K-buckets (k=20)
- Lookup Complexity: O(log n)
```

#### File Storage Architecture

```
File Processing Pipeline:
1. File Input → SHA-256 Hash Generation
2. File Chunking → 256KB chunks
3. Chunk Encryption → AES-256
4. Chunking → 256 KB encrypted chunks
5. Chunk Distribution → Multiple storage nodes
6. DHT Registration → Hash-to-location mapping
```

#### Storage Node Structure
All nodes in the network are equal peers. 
Any node that stores a file becomes a seeder 
and can earn rewards when others download from them.
```
Storage Node:
{
  nodeId: "unique_peer_id",
  ip: "192.168.1.100",
  port: 8080,
  capacity: 1099511627776, // 1TB in bytes
  used: 549755813888, // 512GB in bytes
  uptime: 0.99,
  reputation: 4.5,
  seeding: ["file_hash_1", "file_hash_2", ...] // Files being shared
}
```

### 3. Peer Discovery Mechanism

#### DHT-Based Discovery (Fully Decentralized)

The system uses a fully decentralized approach for peer discovery via Kademlia DHT:

```
File Metadata Structure:
{
  file_hash: "sha256_hash",       // Content-based identifier
  file_name: "document.pdf",
  file_size: 1048576,             // bytes
  seeders: [                      // Direct peer multiaddresses
    "/ip4/192.168.1.100/tcp/8080/p2p/12D3KooW...",
    "/ip4/10.0.0.5/tcp/4001/p2p/12D3KooW..."
  ],
  created_at: 1640995200,
  mime_type: "application/pdf",
  total_chunks: 16
  price_per_mb: 0.001           // Chiral per MB (initially fixed, configurable)
}
```

#### DHT Operations

```
// File Registration (No central server)
dht.put_record(file_hash, metadata);

// Peer announces availability
dht.provider_record(file_hash, peer_multiaddr);

// File Discovery
metadata = dht.get_record(file_hash);
providers = dht.get_providers(file_hash);

// Direct peer connection
connect(providers[0]);  // Connect to seeders directly
```

#### Peer-to-Peer Compensation

When a user downloads a file, they pay the seeders who provide the chunks. Rewards are distributed via blockchain:

```rust
// Proof-of-Storage validation
struct StorageProof {
    file_hash: Hash,
    chunk_hashes: Vec<Hash>,
    merkle_proof: MerkleProof,
    timestamp: u64,
}

// Payment transaction for chunk delivery
struct ChunkPayment {
    from: Address,           // Downloader
    to: Address,             // Seeder
    file_hash: Hash,
    chunks_delivered: Vec<u32>,
    amount: u64,             // Chiral amount
}

```

### 4. Network Communication

#### P2P Protocol Stack

```
Protocol Layers:
┌─────────────────────┐
│   Application       │ ← File Transfer, DHT Queries
├─────────────────────┤
│   libp2p            │ ← Peer Discovery, Routing
├─────────────────────┤
│   Transport         │ ← TCP/UDP, WebRTC
├─────────────────────┤
│   Network           │ ← IP, NAT Traversal
└─────────────────────┘
```

#### Message Types

```protobuf
// Protocol Buffer Definitions
message FileRequest {
  string file_hash = 1;
  uint64 offset = 2;
  uint64 length = 3;
}

message FileResponse {
  bytes data = 1;
  bool is_last_chunk = 2;
  string next_chunk_hash = 3;
}

```

### 5. Client Architecture

#### Desktop Application Stack

```
Frontend Layer:
- Framework: Svelte + TypeScript
- UI Library: Tailwind CSS
- State Management: Svelte Stores
- Desktop Runtime: Tauri (Rust)

Backend Services:
- File Manager: Handles chunking/assembly
- Wallet Service: Transaction management
- Network Service: P2P communication
- Storage Service: Local cache management
```

#### Client Operations Flow

```
File Upload:
1. Select File → Generate Hash
2. Create Chunks → Encrypt
3. Chunking → 256 KB encrypted chunks
4. Query DHT → Find Nodes
5. Calculate Rewards → Create Transaction
6. Verify Chunks → Verify Storage
7. Register in DHT → Complete
8. Set Price → Configure per-MB rate

File Download:
1. Input Hash → Query DHT
2. Discover Storage Nodes → Select Available
3. Connect to Nodes → Initiate Transfer
4. Download Chunks → Verify Hashes
5. Reassemble File from Chunks → Decrypt
6. Reassemble File from Chunks
7. Distribute Rewards → Complete

File Download (BitTorrent-style):
1. Input Hash → Query DHT
2. Get Seeder List → Display list of available providers to user
3. User Selects Provider(s) → Choose which seeder(s) to download from
4. Establish Connections → Handshake with selected seeder(s)
5. Request Chunks → Request specific chunks from chosen seeder(s)
6. Receive Chunks → Download chunks from selected provider(s)
7. Track & Blacklist → Monitor performance, blacklist poor seeders
8. Verify Chunks → Check hashes against manifest
9. Reassemble & Decrypt → Rebuild original file
10. Distribute Payments → Send Chiral to seeders per chunk delivered


```
##### Seeder and Leecher Roles

##### Seeder: A peer who has the complete file and shares it with others. Earns Chiral when others download from them.
#### Leecher: A peer actively downloading a file but only has partial chunks. Once download completes, automatically becomes a seeder.
#### All nodes are peers: No distinction between "storage nodes" and "regular nodes" - anyone can seed and earn.

### 6. Security Architecture

#### Encryption Layers

```
File Encryption:
- Algorithm: AES-256-GCM
- Key Derivation: PBKDF2
- IV Generation: Cryptographically secure random

Network Encryption:
- Protocol: TLS 1.3
- Certificate: Self-signed for P2P
- Key Exchange: ECDHE

Transaction Security:
- Signature: ECDSA (secp256k1)
- Hash Function: SHA-256
- Address Format: Base58Check
```

#### Access Control

```
Permission Model:
- File Owner: Full control (read, write, delete, share)
- Storage Node: Read-only access to encrypted chunks
- Network Peer: No direct file access
- DHT Network: Metadata only (no file content)
```

### 7. Data Flow Architecture

#### Upload Data Flow

```mermaid
sequenceDiagram
    participant Client
    participant FileService
    participant DHT

    Client->>+FileService: Upload File
    FileService->>FileService: Chunk and/or Encrypt chunk file into 256 KB pieces
    FileService->>FileService: Generate Merkle Root from original chunk hashes
    FileService->>+DHT: Register File Manifest (Merkle Root, Chunk Hashes)
    DHT-->>-FileService: Confirm Registration
    FileService-->>-Client: Upload Complete
```

#### Download Data Flow

```mermaid
sequenceDiagram
    participant Client
    participant FileService
    participant DHT
    participant ProviderNode
    participant Blockchain

    Client->>+FileService: Request File (Merkle Root)
    FileService->>+DHT: Lookup File Manifest
    DHT-->>-FileService: Return Manifest (includes chunk hashes)
    FileService->>+ProviderNode: Request encrypted chunks
    ProviderNode-->>-FileService: Send available encrypted chunks
    FileService->>FileService: Decrypt individual chunks
    FileService->>FileService: Verify chunk hash against original hash in manifest
    FileService->>FileService: Assemble original file from verified chunks
    FileService->>+Blockchain: Send Payment
    Blockchain-->>-FileService: Payment Confirmed
    FileService-->>-Client: File Ready
```

### 8. Scalability Design

#### Horizontal Scaling

- **Storage**: Add more storage nodes
- **DHT**: Distributed peer discovery with no central servers
- **Blockchain**: Increase block size or use sidechains
- **DHT**: Automatic scaling with node count

#### Vertical Scaling

- **Node Capacity**: Increase individual storage limits
- **Bandwidth**: Upgrade network connections
- **Processing**: More powerful hardware for mining

#### Optimization Strategies

```
Caching:
- L1: Memory cache (hot files)
- L2: SSD cache (frequently accessed)
- L3: HDD storage (cold storage)

Load Balancing:
- Geographic distribution
- Latency-based routing
- Bandwidth availability
- Reward optimization
```

### 9. Fault Tolerance



#### Node Recovery

```
Node Failure:
1. Detection: Heartbeat timeout (30 seconds)
2. Mark Offline: Update DHT records
3. Redirect: Route requests to alternative providers
5. Cleanup: Remove after grace period

Network Partition:
1. Detection: Consensus split detection
2. Isolation: Prevent double-spending
3. Resolution: Longest chain rule
4. Merge: Reconcile when healed
```

### 10. Performance Optimization

#### Parallel Processing

```
Concurrent Operations:
- Multi-threaded chunking
- Parallel uploads to different nodes
- Concurrent chunk downloads
- Async transaction processing
```

#### Network Optimization

```
Techniques:
- Connection pooling
- Request batching
- Compression (gzip/brotli)
- CDN for popular files
- Predictive prefetching
```

## Implementation Priorities

### Phase 1: MVP

1. Basic blockchain with wallet
2. Simple file upload/download
3. Fully decentralized DHT discovery
4. Desktop GUI

### Phase 2: Decentralization

1. Full DHT implementation
2. DHT-based peer discovery
3. Enhanced encryption
4. Reputation system

### Phase 3: Optimization

1. Performance improvements
2. Advanced features
3. Enterprise support

## Architecture Decisions Log

### Decision: Use Ethereum-Compatible Network

**Rationale**: Account-based model suits storage payments, extensive tooling, smart contract capability
**Alternative**: Build from scratch or use Bitcoin fork
**Trade-off**: More complex but more flexible for application needs

### Decision: Fully Decentralized DHT

**Rationale**: Faster development, easier debugging
**Alternative**: Fully decentralized from start
**Trade-off**: Temporary centralization for faster iteration

### Decision: 256KB Chunk Size

**Rationale**: Balance between overhead and parallelism
**Alternative**: Variable chunk sizes
**Trade-off**: Simplicity over optimization

### Decision: Ethash Mining Algorithm

**Rationale**: ASIC-resistant, proven by Ethereum, fair distribution
**Alternative**: SHA-256, Scrypt, RandomX
**Trade-off**: Memory-hard algorithm prevents centralization
