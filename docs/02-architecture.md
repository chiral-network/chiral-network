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
- Replication Factor: 3
- Lookup Complexity: O(log n)
```

#### File Storage Architecture

```
File Processing Pipeline:
1. File Input → SHA-256 Hash Generation
2. File Chunking → 256KB chunks
3. Chunk Encryption → AES-256
4. Erasure Coding → 10 data, 4 parity shards
5. Chunk Distribution → Multiple storage nodes
6. DHT Registration → Hash-to-location mapping
```

#### Storage Node Structure

```
Storage Node:
{
  nodeId: "unique_peer_id",
  ip: "192.168.1.100",
  port: 8080,
  capacity: 1099511627776, // 1TB in bytes
  used: 549755813888, // 512GB in bytes
  rewardRate: 0.001, // algorithmic reward rate
  uptime: 0.99,
  reputation: 4.5
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

#### Decentralized Incentives

Rewards distributed via blockchain without centralized markets:

```rust
// Proof-of-Storage validation
struct StorageProof {
    file_hash: Hash,
    chunk_hashes: Vec<Hash>,
    merkle_proof: MerkleProof,
    timestamp: u64,
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

message StorageRequest {
  string file_hash = 1;
  uint64 duration = 2; // storage duration in seconds
}

message StorageResponse {
  uint64 reward_amount = 1; // algorithmic reward
  uint64 gas_cost = 2;
  bool accepted = 3;
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
3. Erasure Code Chunks → 10 data, 4 parity shards
4. Query DHT → Find Storage Nodes
5. Calculate Rewards → Create Transaction
6. Upload Shards → Verify Storage
7. Register in DHT → Complete

File Download:
1. Input Hash → Query DHT
2. Discover Storage Nodes → Select Available
3. Connect to Nodes → Initiate Transfer
4. Download Shards → Verify Hashes
5. Reassemble Chunks from Shards → Decrypt
6. Reassemble File from Chunks
7. Distribute Rewards → Complete
```

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
- Storage Node: Read-only access to encrypted shards
- Network Peer: No direct file access
- DHT Network: Metadata only (no file content)
```

### 7. Data Flow Architecture

#### Upload Data Flow

```mermaid
sequenceDiagram
    participant Client
    participant FileService
    participant StorageNode
    participant DHT
    participant Blockchain

    Client->>+FileService: Upload File
    FileService->>FileService: Generate Merkle Root from original chunk hashes
    FileService->>FileService: Chunk file into 256KB pieces
    FileService->>FileService: Apply 10+4 Reed-Solomon erasure coding to each chunk
    FileService->>FileService: Encrypt each of the 14 shards
    FileService->>+StorageNode: Upload encrypted shards
    StorageNode-->>-FileService: Confirm shard storage
    FileService->>+DHT: Register File Manifest (Merkle Root, Shard Hashes)
    DHT-->>-FileService: Confirm Registration
    FileService->>+Blockchain: Create Payment TX
    Blockchain-->>-FileService: TX Confirmed
    FileService-->>-Client: Upload Complete
```

#### Download Data Flow

```mermaid
sequenceDiagram
    participant Client
    participant FileService
    participant DHT
    participant StorageNode
    participant Blockchain

    Client->>+FileService: Request File (Merkle Root)
    FileService->>+DHT: Lookup File Manifest
    DHT-->>-FileService: Return Manifest (includes shard hashes)
    FileService->>+StorageNode: Request encrypted shards (needs 10 of 14)
    StorageNode-->>-FileService: Send available encrypted shards
    FileService->>FileService: Decrypt individual shards
    FileService->>FileService: Reconstruct original chunk via erasure coding
    FileService->>FileService: Verify chunk hash against original hash in manifest
    FileService->>FileService: Assemble file from verified chunks
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

#### Redundancy Mechanisms

```
File Redundancy:
- Replication Factor: 3 (minimum)
- Reed-Solomon Erasure Coding: The 10+4 configuration provides built-in fault tolerance.
- Geographic Distribution: Different regions
- Automatic Repair: Self-healing on node failure
```

#### Failure Recovery

```
Node Failure:
1. Detection: Heartbeat timeout (30 seconds)
2. Mark Offline: Update DHT records
3. Redirect: Route requests to replicas
4. Repair: Re-replicate to maintain redundancy
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
2. Mobile applications
3. Advanced features
4. Enterprise support

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
