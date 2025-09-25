# Chiral Network Technical Specifications

## Network Specifications

### Blockchain Parameters

| Parameter                 | Value                 | Description                          |
| ------------------------- | --------------------- | ------------------------------------ |
| **Network ID**            | 98765                 | Unique identifier for Chiral Network |
| **Chain ID**              | 98765                 | EIP-155 chain identifier             |
| **Block Time**            | ~15 seconds           | Target time between blocks           |
| **Gas Limit**             | 4,700,000 (0x47b760)  | Maximum gas per block                |
| **Initial Difficulty**    | 0x400000              | Starting mining difficulty           |
| **Difficulty Adjustment** | Per block             | Dynamic difficulty adjustment        |
| **Mining Algorithm**      | Ethash                | ASIC-resistant proof-of-work         |
| **Initial Reward**        | 2 ETH                 | Block reward initially               |
| **Max Supply**            | No limit              | Inflationary supply model            |
| **Precision**             | 18 decimals           | Smallest unit: 1 wei (10^-18 ETH)    |
| **Coinbase**              | 0x0000...0000         | Initial coinbase address             |
| **Extra Data**            | "Keep on keeping on!" | Genesis message (0x4b656570...)      |

### Network Ports

| Service           | Port  | Protocol   | Description                |
| ----------------- | ----- | ---------- | -------------------------- |
| **P2P**           | 30304 | TCP/UDP    | Peer-to-peer communication |
| **RPC**           | 8546  | HTTP       | JSON-RPC interface         |
| **WebSocket**     | 8547  | WS         | Real-time updates          |
| **File Transfer** | 8080  | HTTP/HTTPS | File chunk transfers       |
| **DHT**           | 4001  | UDP        | DHT routing                |

### Node Requirements

#### Minimum Requirements

| Component   | Specification                         |
| ----------- | ------------------------------------- |
| **CPU**     | 2 cores @ 2.0 GHz                     |
| **RAM**     | 4 GB                                  |
| **Storage** | 100 GB SSD                            |
| **Network** | 10 Mbps symmetric                     |
| **OS**      | Windows 10, macOS 10.15, Ubuntu 20.04 |

#### Recommended Requirements

| Component   | Specification         |
| ----------- | --------------------- |
| **CPU**     | 4 cores @ 3.0 GHz     |
| **RAM**     | 8 GB                  |
| **Storage** | 500 GB SSD            |
| **Network** | 100 Mbps symmetric    |
| **OS**      | Latest stable version |

#### Storage Node Requirements

| Component   | Specification      |
| ----------- | ------------------ |
| **CPU**     | 4 cores @ 2.5 GHz  |
| **RAM**     | 16 GB              |
| **Storage** | 2 TB+ HDD/SSD      |
| **Network** | 100 Mbps symmetric |
| **Uptime**  | >95% availability  |

## File Storage Specifications

### File Processing

| Aspect                 | Specification    |
| ---------------------- | ---------------- |
| **Hash Algorithm**     | SHA-256          |
| **Chunk Size**         | 256 KB           |
| **Encryption**         | AES-256-GCM      |
| **Compression**        | Optional (zstd)  |
| **Max File Size**      | 10 GB (per file) |
| **Replication Factor** | 3 (configurable) |

### Chunk Structure

```
Chunk Format:
┌─────────────────────────────────────┐
│ Header (64 bytes)                   │
├─────────────────────────────────────┤
│ - Magic Number (4 bytes): 0x43484E4B│
│ - Version (2 bytes): 0x0001         │
│ - Chunk Index (4 bytes)             │
│ - Total Chunks (4 bytes)            │
│ - File Hash (32 bytes)              │
│ - Chunk Hash (32 bytes)             │
├─────────────────────────────────────┤
│ Metadata (256 bytes)                │
├─────────────────────────────────────┤
│ - Encryption IV (16 bytes)          │
│ - Compression Type (1 byte)         │
│ - Original Size (8 bytes)           │
│ - Compressed Size (8 bytes)         │
│ - Timestamp (8 bytes)               │
│ - Reserved (215 bytes)              │
├─────────────────────────────────────┤
│ Data (variable, max 256KB)          │
├─────────────────────────────────────┤
│ Checksum (32 bytes)                 │
└─────────────────────────────────────┘
```

### File Metadata Structure

```json
{
  "version": "1.0",
  "file_hash": "sha256_hash_of_complete_file",
  "file_name": "example.pdf",
  "file_size": 10485760,
  "mime_type": "application/pdf",
  "chunk_size": 262144,
  "total_chunks": 40,
  "chunks": [
    {
      "index": 0,
      "hash": "chunk_sha256_hash",
      "size": 262144,
      "storage_nodes": ["node_id_1", "node_id_2", "node_id_3"]
    }
  ],
  "encryption": {
    "algorithm": "AES-256-GCM",
    "key_derivation": "PBKDF2",
    "iterations": 100000
  },
  "timestamps": {
    "created": 1234567890,
    "modified": 1234567890,
    "accessed": 1234567890
  },
  "permissions": {
    "owner": "wallet_address",
    "public": false,
    "shared_with": []
  }
}
```

## DHT Specifications

### Kademlia Parameters

| Parameter            | Value    | Description           |
| -------------------- | -------- | --------------------- |
| **K**                | 20       | Bucket size           |
| **α**                | 3        | Concurrency parameter |
| **Key Space**        | 160 bits | Node ID size          |
| **Replication**      | 20       | Number of replicas    |
| **Refresh Interval** | 3600s    | Bucket refresh time   |
| **Expiration**       | 86400s   | Record expiration     |

### DHT Operations

| Operation      | Timeout | Retries | Description     |
| -------------- | ------- | ------- | --------------- |
| **PING**       | 5s      | 3       | Liveness check  |
| **FIND_NODE**  | 10s     | 3       | Node discovery  |
| **FIND_VALUE** | 10s     | 5       | Value lookup    |
| **STORE**      | 10s     | 3       | Store key-value |

### DHT Message Format

```
Message Structure:
┌──────────────────────────┐
│ Header (20 bytes)        │
├──────────────────────────┤
│ - Version (2 bytes)      │
│ - Message Type (2 bytes) │
│ - Transaction ID (4 bytes)│
│ - Sender ID (20 bytes)   │
├──────────────────────────┤
│ Payload (variable)       │
└──────────────────────────┘
```

## API Specifications

### REST API Endpoints

#### File Operations

| Endpoint                    | Method | Description           |
| --------------------------- | ------ | --------------------- |
| `/api/v1/files/upload`      | POST   | Upload a new file     |
| `/api/v1/files/{hash}`      | GET    | Retrieve file by hash |
| `/api/v1/files/{hash}/info` | GET    | Get file metadata     |
| `/api/v1/files/{hash}`      | DELETE | Delete file           |
| `/api/v1/files/list`        | GET    | List user's files     |

#### Node Operations

| Endpoint              | Method  | Description          |
| --------------------- | ------- | -------------------- |
| `/api/v1/node/status` | GET     | Node status          |
| `/api/v1/node/peers`  | GET     | List connected peers |
| `/api/v1/node/stats`  | GET     | Node statistics      |
| `/api/v1/node/config` | GET/PUT | Node configuration   |

### WebSocket Events

```javascript
// Client -> Server
{
  "type": "subscribe",
  "channels": ["blocks", "transactions", "files"]
}

// Server -> Client
{
  "type": "block",
  "data": {
    "height": 12345,
    "hash": "0x...",
    "transactions": 25
  }
}

{
  "type": "file_progress",
  "data": {
    "file_hash": "sha256_hash",
    "progress": 75.5,
    "speed": 1048576,
    "eta": 30
  }
}
```

## Cryptographic Specifications

### Algorithms

| Purpose                | Algorithm   | Parameters                |
| ---------------------- | ----------- | ------------------------- |
| **File Hashing**       | Keccak-256  | 256-bit output            |
| **File Encryption**    | AES-256-GCM | 256-bit key, 96-bit nonce |
| **Key Derivation**     | PBKDF2      | SHA-256, 100k iterations  |
| **Digital Signatures** | ECDSA       | secp256k1 curve           |
| **Account Addresses**  | Keccak-256  | Last 20 bytes of hash     |
| **Random Generation**  | CSPRNG      | System entropy            |

### Key Management

```
Key Hierarchy:
Master Key (from mnemonic)
    ├── Wallet Keys (m/44'/98765'/0'/0/*)
    ├── File Encryption Keys (m/44'/98765'/1'/0/*)
    └── Identity Keys (m/44'/98765'/2'/0/*)
```

## Transaction Specifications

### Transaction Types

| Type                | Description          | Base Gas Cost |
| ------------------- | -------------------- | ------------- |
| **Transfer**        | Send coins           | 21,000 gas    |
| **Storage Request** | Request file storage | 100,000 gas   |
| **Storage Proof**   | Prove file storage   | 50,000 gas    |
| **File Access**     | Access stored file   | 30,000 gas    |

### Transaction Structure

```
Transaction {
  nonce: u64,
  gasPrice: U256,
  gasLimit: u64,
  to: Option<Address>,
  value: U256,
  data: Vec<u8>,
  v: u64,
  r: U256,
  s: U256
}
```

## Performance Specifications

### Throughput Targets

| Metric                     | Target  | Current |
| -------------------------- | ------- | ------- |
| **TPS (Transactions/sec)** | 100     | 10      |
| **File Upload Speed**      | 10 MB/s | 5 MB/s  |
| **File Download Speed**    | 20 MB/s | 10 MB/s |
| **DHT Lookup Time**        | <2s     | <5s     |
| **Block Propagation**      | <3s     | <5s     |

### Scalability Limits

| Resource                 | Soft Limit | Hard Limit |
| ------------------------ | ---------- | ---------- |
| **Files per Node**       | 100,000    | 1,000,000  |
| **Peers per Node**       | 100        | 1,000      |
| **Concurrent Transfers** | 50         | 200        |
| **DHT Entries**          | 10,000     | 100,000    |

## Protocol Versions

### Version Compatibility

| Version | Release Date | Status | Breaking Changes   |
| ------- | ------------ | ------ | ------------------ |
| 0.1.0   | 2024-01-01   | Alpha  | Initial release    |
| 0.2.0   | 2024-03-01   | Beta   | DHT implementation |
| 0.3.0   | 2024-06-01   | Beta   | Storage protocol   |
| 1.0.0   | 2024-12-01   | Stable | Production ready   |

### Protocol Negotiation

```
Handshake {
  version: "1.0.0",
  network_id: 9001,
  capabilities: ["storage", "dht", "relay"],
  timestamp: 1234567890,
  nonce: "random_bytes"
}
```

## Data Formats

### File Hash Format

```
Format: cn1<version><hash>
Example: cn1a7d8f9e8c7b6a5d4f3e2d1c0b9a8d7f6e5d4c3b2a1
- Prefix: "cn1" (3 chars)
- Version: "a" (1 char)
- Hash: SHA-256 in hex (64 chars)
```

### Address Format

```
Format: Ethereum-style hexadecimal addresses
Example: 0x742d35Cc6634C0532925a3b8D0C9e0c8b346b983
- Prefix: "0x" (2 chars)
- Address: Last 20 bytes of Keccak-256 hash of public key (40 hex chars)
- Checksum: EIP-55 mixed-case checksum encoding
```

## Error Codes

### System Error Codes

| Code | Name               | Description                   |
| ---- | ------------------ | ----------------------------- |
| 1000 | NETWORK_ERROR      | Network connectivity issue    |
| 1001 | TIMEOUT            | Operation timed out           |
| 1002 | INVALID_HASH       | Invalid file hash format      |
| 1003 | FILE_NOT_FOUND     | File not in network           |
| 1004 | INSUFFICIENT_FUNDS | Not enough balance            |
| 1005 | PERMISSION_DENIED  | Access not authorized         |
| 1006 | STORAGE_FULL       | Node storage capacity reached |
| 1007 | INVALID_CHUNK      | Chunk verification failed     |
| 1008 | DHT_TIMEOUT        | DHT lookup timeout            |
| 1009 | PEER_UNREACHABLE   | Cannot connect to peer        |

## Quality of Service

### SLA Targets

| Metric           | Target  | Measurement           |
| ---------------- | ------- | --------------------- |
| **Availability** | 99.9%   | Monthly uptime        |
| **Durability**   | 99.999% | Annual data loss      |
| **Latency**      | <100ms  | P95 response time     |
| **Throughput**   | >1MB/s  | Average transfer rate |

### Priority Levels

| Level | Name     | Description        | Multiplier |
| ----- | -------- | ------------------ | ---------- |
| 0     | Low      | Best effort        | 1.0x       |
| 1     | Normal   | Standard service   | 1.5x       |
| 2     | High     | Priority handling  | 2.0x       |
| 3     | Critical | Guaranteed service | 3.0x       |
