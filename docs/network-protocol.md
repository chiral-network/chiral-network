# Chiral Network Protocol Documentation

## Protocol Overview

The Chiral Network implements a multi-layered protocol stack combining blockchain consensus, distributed hash table routing, and peer-to-peer file transfer protocols. This document details the network protocols, message formats, and communication patterns.

## Protocol Stack

```
┌─────────────────────────────────────────┐
│         Application Layer               │
│   File Transfer | DHT | Mining          │
├─────────────────────────────────────────┤
│         Session Layer                   │
│   Authentication | Encryption | State   │
├─────────────────────────────────────────┤
│         Network Layer                   │
│   DHT Routing | Peer Discovery | NAT    │
├─────────────────────────────────────────┤
│         Transport Layer                 │
│   libp2p | TCP | UDP | QUIC | WebRTC    │
└─────────────────────────────────────────┘
```

## Core Protocols

### 1. Peer Discovery Protocol

#### Bootstrap Process (Current Implementation)

1. **Connect to the bootstrap DHT node**  
   Every node loads the configured `bootstrap_nodes` list (currently a single multiaddress such as `/ip4/bootstrap.chiral.network/tcp/4001/p2p/QmBootstrap`) and dials it on startup. That node runs the same libp2p/Kademlia behaviour as any other peer but is flagged with `is_bootstrap=true`, kept online permanently, and is configured not to publish provider records so it acts purely as a router.

2. **Seed the local routing table**  
   Once the connection succeeds, we add the bootstrap peer ID and address into the local Kademlia table. No custom “peer list” RPC is issued—the bootstrap node simply shares the peers it already knows through standard Kademlia gossip.

3. **Run the initial Kademlia bootstrap walk**  
   After the connection is established, the client immediately invokes `kademlia.bootstrap()`. This kicks off the built-in random walk (successive `FIND_NODE` queries) using the bootstrap node as the starting point, which populates the routing table with additional peers and providers.

4. **Keep the table fresh**  
   Non-bootstrap peers continue to run the periodic bootstrap loop (1 s interval) while the node is up, ensuring they resync with the network if entries age out. The dedicated bootstrap node disables this interval, but it remains available so new peers can repeat steps 1–3 at any time.

##### Operational Notes
- The bootstrap node exposes only the libp2p/DHT service (no extra REST endpoints) and listens on the same ports as any peer.
- Today the network relies on a single bootstrap address; adding secondary bootstrap nodes is recommended to avoid a single point of failure.

#### Message Format

```
PeerDiscovery Message {
  header: {
    version: u16,           // Protocol version (0x0001)
    message_type: u8,       // Message type enum
    request_id: u32,        // Request identifier
    timestamp: u64,         // Unix timestamp
  },
  sender: {
    node_id: [u8; 32],     // Node public key hash
    addresses: Vec<String>, // Multiaddresses
    capabilities: u32,      // Capability flags
  },
  payload: MessagePayload,  // Type-specific data
  signature: [u8; 64],     // Ed25519 signature
}
```

### 2. DHT Protocol (Kademlia)

#### Node ID Generation

```
Node ID = SHA256(public_key || nonce)
Distance = XOR(NodeID_A, NodeID_B)
```

#### Routing Table Structure

```
K-Buckets (k=20, b=160):
┌────────────────────────────────┐
│ Bucket 0: Distance 2^0        │ → [20 nodes max]
│ Bucket 1: Distance 2^1        │ → [20 nodes max]
│ ...                            │
│ Bucket 159: Distance 2^159    │ → [20 nodes max]
└────────────────────────────────┘
```

#### DHT Operations

##### PING

```
Request:
{
  type: "PING",
  sender_id: [u8; 20],
  random: [u8; 20]
}

Response:
{
  type: "PONG",
  sender_id: [u8; 20],
  echo: [u8; 20]
}
```

##### FIND_NODE

```
Request:
{
  type: "FIND_NODE",
  sender_id: [u8; 20],
  target_id: [u8; 20]
}

Response:
{
  type: "NODES",
  sender_id: [u8; 20],
  nodes: [{
    id: [u8; 20],
    ip: [u8; 4] | [u8; 16],
    port: u16
  }]
}
```

##### STORE

```
Request:
{
  type: "STORE",
  sender_id: [u8; 20],
  key: [u8; 32],
  value: Vec<u8>,
  ttl: u32
}

Response:
{
  type: "STORED",
  sender_id: [u8; 20],
  key: [u8; 32],
  expires: u64
}
```

##### FIND_VALUE

```
Request:
{
  type: "FIND_VALUE",
  sender_id: [u8; 20],
  key: [u8; 32]
}

Response (if found):
{
  type: "VALUE",
  sender_id: [u8; 20],
  key: [u8; 32],
  value: Vec<u8>
}

Response (if not found):
{
  type: "NODES",
  sender_id: [u8; 20],
  nodes: [...]
}
```

### 3. File Transfer Protocol

#### File Metadata Exchange

```protobuf
message FileMetadata {
  string file_hash = 1;      // SHA-256 hash
  uint64 file_size = 2;      // Total size in bytes
  uint32 chunk_size = 3;     // Size of each chunk
  uint32 total_chunks = 4;   // Number of chunks
  string mime_type = 5;      // MIME type
  bool encrypted = 6;        // Encryption status
  repeated ChunkInfo chunks = 7;
}

message ChunkInfo {
  uint32 index = 1;          // Chunk index
  string hash = 2;           // Chunk hash
  uint32 size = 3;           // Chunk size
  repeated string nodes = 4; // Nodes storing chunk
}
```

#### Chunk Transfer Protocol

```
1. Request Chunk
   → REQUEST_CHUNK {
       file_hash: "sha256...",
       chunk_index: 5,
       offset: 0,
       length: 262144
     }

2. Receive Chunk
   ← CHUNK_DATA {
       file_hash: "sha256...",
       chunk_index: 5,
       data: [binary],
       proof: [merkle_proof]
     }

3. Verify Chunk
   - Hash verification
   - Merkle proof validation
   - Decrypt if needed

4. Acknowledge
   → CHUNK_ACK {
       file_hash: "sha256...",
       chunk_index: 5,
       status: "verified"
     }
```

#### Parallel Transfer Optimization

```
MaxParallelChunks = 10
WindowSize = 5

For chunks 0..n:
  While active_transfers < MaxParallelChunks:
    Request next chunk
    Track in flight

  On chunk received:
    Verify and store
    Request next chunk
    Update progress
```

### 4. Blockchain Protocol

#### Block Structure

```
EthereumBlock {
  header: {
    parentHash: [u8; 32],      // Previous block hash
    sha3Uncles: [u8; 32],      // Uncle blocks hash
    miner: [u8; 20],           // Coinbase address
    stateRoot: [u8; 32],       // State trie root
    transactionsRoot: [u8; 32], // Transaction trie root
    receiptsRoot: [u8; 32],    // Receipt trie root
    difficulty: U256,          // Block difficulty
    number: u64,               // Block number
    gasLimit: u64,             // Gas limit
    gasUsed: u64,              // Gas used
    timestamp: u64,            // Unix timestamp
    mixHash: [u8; 32],         // Ethash mix hash
    nonce: u64,                // Ethash nonce
  },
  transactions: Vec<Transaction>,
  uncles: Vec<BlockHeader>,
}
```

#### Transaction Format

```
EthereumTransaction {
  nonce: u64,
  gasPrice: U256,
  gasLimit: u64,
  to: Option<[u8; 20]>,       // Recipient address
  value: U256,                // Amount in wei
  data: Vec<u8>,              // Input data
  v: u64,                     // Recovery ID
  r: U256,                    // ECDSA signature r
  s: U256,                    // ECDSA signature s
}

TransactionTypes:
- Simple Transfer: to != null, data = []
- Contract Call: to != null, data != []
- Contract Creation: to = null, data = contract_code
- Storage Operation: encoded in data field
```

#### Consensus Messages

##### NewBlock

```
{
  type: "NEW_BLOCK",
  block: Block,
  total_difficulty: U256,
  sender: NodeId
}
```

##### GetBlockHeaders

```
{
  type: "GET_BLOCK_HEADERS",
  block: Either<u64, [u8; 32]>,  // Block number or hash
  maxHeaders: u32,
  skip: u32,
  reverse: bool
}
```

##### NewPooledTransactionHashes

```
{
  type: "NEW_POOLED_TRANSACTION_HASHES",
  hashes: Vec<[u8; 32]>
}
```

### 5. Provider Verification Protocol

#### On-Stream Chunk Validation

1) Requesters select providers via the DHT and local reputation cache, then stream chunks.
2) For every chunk received, the requester hashes the ciphertext and compares it to the expected chunk hash in the manifest before attempting decryption.
3) When a provider includes an Merkle path, the requester validates the chunk hash against the manifest’s Merkle root for end-to-end integrity.
4) Any failed verification aborts the transfer, blacklists the provider locally, and emits a reputation penalty signal to interested peers.
5) Providers that successfully deliver all requested chunks earn a positive reputation update and remain eligible for future download selection.


#### Proof Generation

```rust
fn generate_proof(challenge: Challenge) -> Proof {
    let mut proofs = Vec::new();

    for index in challenge.chunk_indices {
        let chunk = storage.get_chunk(challenge.file_hash, index);
        let hash = sha256(chunk.data);
        let merkle_proof = generate_merkle_proof(index);
        proofs.push((hash, merkle_proof));
    }

    Proof {
        file_hash: challenge.file_hash,
        proofs,
        timestamp: now(),
        signature: sign(proofs)
    }
}
```

### 6. Multi-Network Integration: Chiral and BitTorrent

To enhance file availability and download speed, the Chiral client integrates with the public BitTorrent network. This creates a hybrid system where the client can fetch file pieces from both the private, reputation-based Chiral P2P network and the massive, public BitTorrent swarm simultaneously.

#### Dual-Network Orchestration

The system does **not** merge the two networks. Instead, the `ProtocolManager` acts as an orchestrator, managing two parallel network handlers:

1.  **Chiral P2P Handler**: Interacts with the libp2p-based Kademlia DHT to find and communicate with other Chiral peers.
2.  **BitTorrent Handler**: Interacts with the public BitTorrent DHT (Mainline DHT) and public trackers to find and communicate with standard BitTorrent clients.

This allows the Chiral client to source file pieces from a high-reputation Chiral peer and a public torrent swarm at the same time, with the `multi_source_download` engine assembling the final file from all incoming pieces.

#### DHT Modifications for Torrent Discovery

To enable Chiral peers to discover torrent-based sources from each other, the Chiral DHT protocol is extended:

1.  **Extended DHT Record**: The metadata record stored in the Chiral DHT is modified to include an optional `info_hash` field. This allows a Chiral peer to associate a file's Content ID (CID) with a corresponding BitTorrent info hash.

    ```protobuf
    // Extended FileMetadata in Chiral DHT
    message FileMetadata {
      string file_hash = 1;      // SHA-256 hash (CID)
      // ... other fields
      optional string info_hash = 8; // BitTorrent info hash (SHA-1)
    }
    ```

2.  **Dual Lookup Logic**: The DHT interface is expanded with new lookup functions:
    *   `search_by_cid(cid: String)`: The standard lookup that resolves a file's CID to a list of Chiral peers.
    *   `search_by_infohash(info_hash: String)`: A new function that allows finding Chiral peers who have registered a specific torrent info hash.

3.  **Torrent Announcement**: A new DHT operation is added to allow a peer to advertise that it can serve a file via BitTorrent.

    ```rust
    // Pseudocode for announcing a torrent source
    fn announce_torrent(cid: String, info_hash: String) {
        // 1. Retrieve the existing metadata record for the CID.
        let mut metadata = dht.get(cid);
        // 2. Add or update the info_hash.
        metadata.info_hash = Some(info_hash);
        // 3. Store the updated record back into the DHT.
        dht.put(cid, metadata);
    }
    ```

These extensions enable a Chiral client to first discover a file via its CID, and then, from the returned metadata, discover that the file is *also* available on the public BitTorrent network via its info hash. This bridges the two networks, enabling the powerful multi-source downloading feature.

## Network Protocols

### 1. libp2p Integration

#### Protocol Multiplexing

```yaml
protocols:
  /chiral/kad/1.0.0: # Kademlia DHT
    handler: dht_handler
  /chiral/transfer/1.0.0: # File transfer
    handler: transfer_handler
  /chiral/dht/1.0.0: # DHT protocol
    handler: dht_handler
  /chiral/eth/1.0.0: # Ethereum-compatible sync
    handler: eth_handler
```

#### Stream Multiplexing

```
Connection
    ├── Stream 1: DHT queries
    ├── Stream 2: File transfer
    ├── Stream 3: Blockchain sync
    └── Stream 4: Control messages
```

### 2. NAT Traversal

#### STUN Protocol

```
STUN Request:
{
  type: "BINDING_REQUEST",
  transaction_id: [u8; 12],
  attributes: {
    USERNAME: "peer_id",
    MESSAGE_INTEGRITY: [u8; 20]
  }
}

STUN Response:
{
  type: "BINDING_RESPONSE",
  transaction_id: [u8; 12],
  attributes: {
    XOR_MAPPED_ADDRESS: "public_ip:port",
    SOFTWARE: "chiral/1.0.0"
  }
}
```

### 3. WebRTC Integration

#### Signaling Protocol

```javascript
// Offer
{
  type: "offer",
  sdp: "v=0\r\no=- ... ",
  ice_candidates: [
    {
      candidate: "candidate:1 1 UDP ...",
      sdpMLineIndex: 0
    }
  ]
}

// Answer
{
  type: "answer",
  sdp: "v=0\r\no=- ... ",
  ice_candidates: [...]
}
```

#### Data Channel Protocol

```
DataChannel Configuration:
{
  ordered: true,
  maxRetransmits: 3,
  maxPacketLifeTime: 5000,
  protocol: "chiral-transfer",
  negotiated: false
}
```

## Message Serialization

### Protocol Buffers Schema

```protobuf
syntax = "proto3";
package chiral;

message Envelope {
  uint32 version = 1;
  string message_type = 2;
  bytes payload = 3;
  uint64 timestamp = 4;
  bytes signature = 5;
}

message Node {
  bytes id = 1;
  repeated string addresses = 2;
  uint64 last_seen = 3;
  double reputation = 4;
}

message FileRequest {
  string hash = 1;
  uint32 chunk_index = 2;
  uint32 offset = 3;
  uint32 length = 4;
}

message FileResponse {
  string hash = 1;
  uint32 chunk_index = 2;
  bytes data = 3;
  repeated bytes merkle_proof = 4;
}
```

### MessagePack Format

```
Message Structure:
┌──────────┬──────────┬──────────┬──────────┐
│  Magic   │  Version │  Type    │  Length  │
│  2 bytes │  2 bytes │  2 bytes │  4 bytes │
├──────────┴──────────┴──────────┴──────────┤
│              Payload (variable)            │
├────────────────────────────────────────────┤
│            Checksum (4 bytes)              │
└────────────────────────────────────────────┘
```

## Protocol Negotiation

### Version Negotiation

```
Client: HELLO {
  versions: [0x0003, 0x0002, 0x0001],
  capabilities: ["serve", "mine"]
}

Server: HELLO_ACK {
  selected_version: 0x0002,
  capabilities: ["serve"],
  features: ["encryption", "compression"]
}
```

### Capability Discovery

```
Capabilities Bitmap:
Bit 0: Storage Node
Bit 1: Mining Node
Bit 3: DHT Node
Bit 4: Bootstrap Node
Bit 5: Archive Node
Bit 6-31: Reserved
```

## Network Topology

### Overlay Network Structure

```
Super Nodes (High Bandwidth/Storage)
    │
    ├── Regional Hubs
    │       │
    │       ├── Edge Nodes
    │               │
    │               └── Client Nodes
    │       
    │       
    │
    └── NAT-ed Clients (via SOCKS5 proxy)
```

### Routing Strategies

#### Iterative Routing

```
1. Query α closest nodes
2. Wait for responses
3. Query α next closest
4. Repeat until target found
```

#### Recursive Routing

```
1. Query closest node
2. Node forwards query
3. Continues recursively
4. Response returns via path
```

## Quality of Service

### Priority Levels

```
enum Priority {
  Critical = 0,  // System messages
  High = 1,      // Financial transactions
  Normal = 2,    // File transfers
  Low = 3,       // Background sync
}
```

### Bandwidth Allocation

```
Total Bandwidth = 100 Mbps
- Critical: 10% reserved
- High: 30% guaranteed
- Normal: 50% shared
- Low: 10% best effort
```

### Flow Control

```
Window-based Flow Control:
- Initial window: 64 KB
- Maximum window: 1 MB
- Increment: 32 KB per RTT
- Backoff: 50% on congestion
```

## Protocol Security

### Message Authentication

```
HMAC-SHA256(key, message) where:
- key = shared_secret
- message = header || payload
```

### Replay Attack Prevention

```
Requirements:
1. Timestamp within 5 minutes
2. Nonce not in recent set
3. Sequence number increments
```

### Protocol Fuzzing

```yaml
fuzzing_targets:
  - message_parsing
  - state_transitions
  - error_handling
  - boundary_conditions
```

## Performance Metrics

### Latency Targets

| Operation         | Target | Maximum |
| ----------------- | ------ | ------- |
| Ping              | 50ms   | 200ms   |
| DHT Lookup        | 500ms  | 2s      |
| Chunk Request     | 100ms  | 1s      |
| Block Propagation | 1s     | 5s      |

### Throughput Targets

| Operation      | Target  | Minimum |
| -------------- | ------- | ------- |
| File Upload    | 10 MB/s | 1 MB/s  |
| File Download  | 20 MB/s | 2 MB/s  |
| DHT Operations | 100/s   | 10/s    |
| Transactions   | 100/s   | 10/s    |

## Protocol Extensions

### Custom Protocol Registration

```typescript
interface ProtocolHandler {
  name: string;
  version: string;
  handler: (stream: Stream) => Promise<void>;
}

network.registerProtocol({
  name: "/chiral/custom/1.0.0",
  version: "1.0.0",
  handler: async (stream) => {
    // Handle protocol
  },
});
```

### Protocol Upgrade Path

```
Version 1.0.0 → 1.1.0:
- Backward compatible
- New optional fields
- Deprecation warnings

Version 1.x → 2.0.0:
- Breaking changes
- Migration period
- Dual-stack support
```

## Debugging & Monitoring

### Protocol Tracing

```
TRACE [2024-01-01 00:00:00] DHT FIND_NODE
  → Target: 0x1234...
  ← Nodes: 20
  Duration: 150ms

DEBUG [2024-01-01 00:00:01] FILE_TRANSFER
  → Request chunk 5 of file 0xabcd...
  ← Received 262144 bytes
  Verification: OK
```

### Network Diagnostics

```bash
# Test connectivity
chiral-cli network ping <peer_id>

# Trace route
chiral-cli network trace <file_hash>

# Protocol statistics
chiral-cli network stats --protocol=dht
```

## Protocol Compliance

### Standards Compliance

- libp2p Specification v1.0
- Ethereum Wire Protocol (RLPx)
- Ethereum DevP2P Protocol
- WebRTC RFC 8825
- JSON-RPC 2.0 (Ethereum-compatible)

### Testing Suite

```yaml
test_categories:
  conformance:
    - message_format
    - state_machine
    - error_codes
  interoperability:
    - version_compatibility
    - cross_platform
    - network_conditions
  performance:
    - throughput
    - latency
    - scalability
```

## Future Protocol Enhancements

### Planned Features

1. **QUIC Transport:** Lower latency connections
2. **GraphSync:** Efficient graph synchronization
3. **Bitswap:** Content exchange protocol
4. **Gossipsub:** Pub/sub messaging
5. **Noise Protocol:** Modern crypto handshake

### Research Areas

- Quantum-resistant protocols
- Machine learning optimization
- Satellite communication
- Mesh networking
- Edge computing integration
