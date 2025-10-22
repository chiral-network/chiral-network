# Chiral Network API Documentation

## Overview

The Chiral Network exposes RESTful and WebSocket APIs for client-server communication, peer-to-peer operations, and blockchain interactions. This document provides comprehensive API specifications for all network services.

## Base URLs

### Development

- REST API: `http://localhost:3000/api/v1`
- WebSocket: `ws://localhost:8547`
- RPC: `http://localhost:8546`
- P2P: `tcp://localhost:30304`

### Production

- REST API: `https://api.chiralnetwork.org/v1`
- WebSocket: `wss://ws.chiralnetwork.org`
- RPC: `https://rpc.chiralnetwork.org`
- P2P: `tcp://p2p.chiralnetwork.org:30304`

## Authentication

### API Key Authentication

```http
Authorization: Bearer <api_key>
```

### Ethereum Wallet Signature Authentication

```http
X-Wallet-Address: 0x742d35Cc6634C0532925a3b8D0C9e0c8b346b983
X-Signature: 0x8fe6d8ff73c51bc8f2f8f1f2c0db5a8b0d2c1e0f8e4d3c2b1a09876543210abcdef
X-Timestamp: 1234567890
```

## REST API Endpoints

### File Operations

#### Upload File

```http
POST /api/v1/files/upload
Content-Type: multipart/form-data
```

**Request Body:**

```
file: binary
encryption_key: string (optional)
```

**Response:**

```json
{
  "success": true,
  "file_hash": "0xa7d8f9e8c7b6a5d4f3e2d1c0b9a8d7f6e5d4c3b2a1098765432100abcdef123456",
  "chunks": [
    {
      "index": 0,
      "hash": "sha256_chunk_hash",
      "size": 262144,
      "nodes": ["node_id_1", "node_id_2", "node_id_3"]
    }
  ],
  "total_size": 10485760,
  "upload_time": 1234567890
}
```

#### Download File

```http
GET /api/v1/files/{file_hash}
```

**Path Parameters:**

- `file_hash`: SHA-256 hash of the file

**Query Parameters:**

- `chunk_start`: Starting chunk index (optional)
- `chunk_end`: Ending chunk index (optional)
- `stream`: Boolean for streaming response (optional)

**Response:**

```
Binary file data or chunked transfer encoding
```

#### Get File Info

```http
GET /api/v1/files/{file_hash}/info
```

**Response:**

```json
{
  "file_hash": "0xa7d8f9e8c7b6a5d4f3e2d1c0b9a8d7f6e5d4c3b2a1098765432100abcdef123456",
  "file_name": "document.pdf",
  "file_size": 10485760,
  "mime_type": "application/pdf",
  "chunk_count": 40,
  "chunk_size": 262144,
  "created_at": 1234567890,
  "owner": "0x1234567890abcdef",
  "encryption": {
    "algorithm": "AES-256-GCM",
    "encrypted": true
  },
  "availability": {
    "online_nodes": 15,
    "health_score": 0.95
  }
}
```

#### Delete File

```http
DELETE /api/v1/files/{file_hash}
```

**Response:**

```json
{
  "success": true,
  "message": "File deleted successfully",
  "chunks_removed": 40
}
```

#### List User Files

```http
GET /api/v1/files/list
```

**Query Parameters:**

- `page`: Page number (default: 1)
- `limit`: Items per page (default: 20, max: 100)
- `sort`: Sort field (name, size, date)
- `order`: Sort order (asc, desc)

**Response:**

```json
{
  "files": [
    {
      "file_hash": "0xa7d8f9e8c7b6a5d4f3e2d1c0b9a8d7f6e5d4c3b2a1098765432100abcdef123456",
      "name": "document.pdf",
      "size": 10485760,
      "uploaded_at": 1234567890,
      "status": "seeding"
    }
  ],
  "pagination": {
    "page": 1,
    "limit": 20,
    "total": 150,
    "pages": 8
  }
}
```

### Node Operations

**Note**: All nodes are equal in Chiral Network. Any node can seed files, download files, and participate in the network. There are no dedicated "storage nodes" - all nodes can perform any role.

#### Register Node

```http
POST /api/v1/node/register
```

**Request Body:**

```json
{
  "node_id": "12D3KooW...",
  "ip_address": "192.168.1.100",
  "port": 8080,
  "capacity": 1099511627776,
  "available": 549755813888,
  "bandwidth": {
    "upload": 104857600,
    "download": 104857600
  },
  "geolocation": {
    "country": "US",
    "region": "California",
    "city": "San Francisco"
  }
}
```

**Response:**

```json
{
  "success": true,
  "node_id": "12D3KooW...",
  "registration_time": 1234567890,
  "certificate": "base64_encoded_cert"
}
```

#### Update Node Status

```http
PUT /api/v1/node/status
```

**Request Body:**

```json
{
  "node_id": "12D3KooW...",
  "available_space": 549755813888,
  "seeding_chunks": 15000,
  "bandwidth_used": {
    "upload": 52428800,
    "download": 26214400
  },
  "uptime": 0.99
}
```

#### Get Network Statistics

```http
GET /api/v1/network/stats
```

**Response:**

```json
{
  "total_nodes": 1000,
  "online_nodes": 950,
  "total_capacity": 1125899906842624,
  "used_capacity": 562949953421312,
  "average_uptime": 0.95,
  "geographic_distribution": {
    "NA": 400,
    "EU": 350,
    "ASIA": 200,
    "OTHER": 50
  }
}
```

### Peer Operations

#### Discover Peers

```http
GET /api/v1/peers/discover
```

**Query Parameters:**

- `limit`: Maximum number of peers (default: 20)
- `country`: Filter by country code
- `min_uptime`: Minimum uptime percentage

**Response:**

```json
{
  "peers": [
    {
      "peer_id": "12D3KooWExample",
      "multiaddr": "/ip4/192.168.1.100/tcp/30304",
      "reputation": 4.5,
      "uptime": 0.99,
      "last_seen": 1234567890,
      "capabilities": ["provider", "relay", "proxy"]
    }
  ]
}
```

#### Connect to Peer

```http
POST /api/v1/peers/connect
```

**Request Body:**

```json
{
  "peer_id": "12D3KooWExample",
  "multiaddr": "/ip4/192.168.1.100/tcp/30304"
}
```

**Response:**

```json
{
  "success": true,
  "peer_id": "12D3KooWExample",
  "connection_id": "conn_12345",
  "latency_ms": 25
}
```

#### Get Peer Info

```http
GET /api/v1/peers/{peer_id}
```

**Response:**

```json
{
  "peer_id": "12D3KooWExample",
  "addresses": ["/ip4/192.168.1.100/tcp/30304", "/ip6/::1/tcp/30304"],
  "protocols": ["/ipfs/1.0.0", "/libp2p/circuit/relay/0.1.0"],
  "agent_version": "chiral-network/1.0.0",
  "reputation": {
    "score": 4.5,
    "successful_transfers": 1000,
    "failed_transfers": 5,
    "total_data_served": 107374182400
  },
  "connected_since": 1234567890
}
```

### Blockchain Operations

#### Get Block by Number

```http
GET /api/v1/blockchain/block/{block_number}
```

**Response:**

```json
{
  "number": 12345,
  "hash": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
  "parentHash": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
  "timestamp": 1234567890,
  "miner": "0x742d35Cc6634C0532925a3b8D0C9e0c8b346b983",
  "difficulty": "0x20000",
  "gasLimit": "0x7A1200",
  "gasUsed": "0x5208",
  "transactions": [
    {
      "hash": "0xabcd1234567890abcd1234567890abcd1234567890abcd1234567890abcd1234",
      "from": "0x1234567890abcdef1234567890abcdef12345678",
      "to": "0x742d35Cc6634C0532925a3b8D0C9e0c8b346b983",
      "value": "1000000000000000000",
      "gas": 21000,
      "gasPrice": "20000000000",
      "nonce": 5
    }
  ]
}
```

#### Submit Transaction

```http
POST /api/v1/blockchain/transaction
```

**Request Body:**

```json
{
  "from": "0x1234567890abcdef1234567890abcdef12345678",
  "to": "0x742d35Cc6634C0532925a3b8D0C9e0c8b346b983",
  "value": "1000000000000000000",
  "gas": 21000,
  "gasPrice": "20000000000",
  "nonce": 5,
  "data": "0x",
  "v": 28,
  "r": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
  "s": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
}
```

**Response:**

```json
{
  "transaction_hash": "0x...",
  "status": "pending",
  "estimated_confirmation": 600
}
```

#### Get Balance

```http
GET /api/v1/blockchain/balance/{address}
```

**Response:**

```json
{
  "address": "0x...",
  "balance": "10000000000000000000",
  "balance_formatted": "10.0 Chiral",
  "pending_transactions": 2,
  "nonce": 5
}
```

### Mining Operations

#### Start Mining

```http
POST /api/v1/mining/start
```

**Request Body:**

```json
{
  "miner_address": "0x...",
  "threads": 4,
  "intensity": 75
}
```

**Response:**

```json
{
  "success": true,
  "mining_id": "mining_session_12345",
  "estimated_hashrate": 50000
}
```

#### Get Mining Status

```http
GET /api/v1/mining/status
```

**Response:**

```json
{
  "active": true,
  "mining_id": "mining_session_12345",
  "hashrate": 48500,
  "shares_submitted": 100,
  "shares_accepted": 98,
  "blocks_found": 2,
  "total_earnings": "10000000000000000000",
  "uptime_seconds": 3600
}
```

#### Stop Mining

```http
POST /api/v1/mining/stop
```

**Response:**

```json
{
  "success": true,
  "session_stats": {
    "duration": 3600,
    "total_hashes": 174600000,
    "blocks_found": 2,
    "earnings": "10000000000000000000"
  }
}
```

### DHT Operations

#### DHT Put

```http
PUT /api/v1/dht/{key}
```

**Request Body:**

```json
{
  "value": "arbitrary_data",
  "ttl": 86400
}
```

**Response:**

```json
{
  "success": true,
  "key": "dht_key",
  "stored_at": ["node_1", "node_2", "node_3"],
  "expiry": 1234654290
}
```

#### DHT Get

```http
GET /api/v1/dht/{key}
```

**Response:**

```json
{
  "key": "dht_key",
  "value": "arbitrary_data",
  "found_at": "node_1",
  "ttl_remaining": 85000
}
```

#### Find Providers

```http
GET /api/v1/dht/providers/{content_hash}
```

**Response:**

```json
{
  "providers": [
    {
      "peer_id": "12D3KooWExample",
      "addresses": ["/ip4/192.168.1.100/tcp/8080"],
      "distance": 5,
      "last_seen": 1234567890
    }
  ]
}
```

## WebSocket API

### Connection

```javascript
const ws = new WebSocket("ws://localhost:8547");
ws.onopen = () => {
  ws.send(
    JSON.stringify({
      type: "subscribe",
      channels: ["blocks", "transactions", "peers", "files"],
    }),
  );
};
```

### Event Types

#### New Block Event

```json
{
  "type": "block",
  "data": {
    "number": 12345,
    "hash": "0x...",
    "timestamp": 1234567890,
    "transactions": 25,
    "miner": "0x..."
  }
}
```

#### Transaction Event

```json
{
  "type": "transaction",
  "data": {
    "hash": "0x...",
    "from": "0x...",
    "to": "0x...",
    "value": "1000000000000000000",
    "status": "confirmed",
    "block": 12345
  }
}
```

#### Peer Event

```json
{
  "type": "peer",
  "action": "connected" | "disconnected",
  "data": {
    "peer_id": "12D3KooWExample",
    "address": "/ip4/192.168.1.100/tcp/30304",
    "timestamp": 1234567890
  }
}
```

#### File Progress Event

```json
{
  "type": "file_progress",
  "data": {
    "file_hash": "0xa7d8f9e8c7b6a5d4f3e2d1c0b9a8d7f6e5d4c3b2a1098765432100abcdef123456",
    "action": "upload" | "download",
    "progress": 75.5,
    "speed": 1048576,
    "eta": 30,
    "peers": 5
  }
}
```

### Commands

#### Subscribe to Events

```json
{
  "type": "subscribe",
  "channels": ["blocks", "transactions", "peers", "files"],
  "filters": {
    "address": "0x..."
  }
}
```

#### Unsubscribe from Events

```json
{
  "type": "unsubscribe",
  "channels": ["transactions"]
}
```

#### Request File Chunk

```json
{
  "type": "request_chunk",
  "file_hash": "0xa7d8f9e8c7b6a5d4f3e2d1c0b9a8d7f6e5d4c3b2a1098765432100abcdef123456",
  "chunk_index": 5,
  "peer_id": "12D3KooWExample"
}
```

## RPC API (Ethereum-compatible)

### Standard Methods

#### eth_blockNumber

```json
{
  "jsonrpc": "2.0",
  "method": "eth_blockNumber",
  "params": [],
  "id": 1
}
```

#### eth_getBalance

```json
{
  "jsonrpc": "2.0",
  "method": "eth_getBalance",
  "params": ["0x...", "latest"],
  "id": 1
}
```

#### eth_sendTransaction

```json
{
  "jsonrpc": "2.0",
  "method": "eth_sendTransaction",
  "params": [
    {
      "from": "0x...",
      "to": "0x...",
      "value": "0xde0b6b3a7640000",
      "gas": "0x5208",
      "gasPrice": "0x3b9aca00"
    }
  ],
  "id": 1
}
```

### Custom Methods

#### chiral_getFileInfo

```json
{
  "jsonrpc": "2.0",
  "method": "chiral_getFileInfo",
  "params": [
    "0xa7d8f9e8c7b6a5d4f3e2d1c0b9a8d7f6e5d4c3b2a1098765432100abcdef123456"
  ],
  "id": 1
}
```

#### chiral_getNodeStats

```json
{
  "jsonrpc": "2.0",
  "method": "chiral_getNodeStats",
  "params": [],
  "id": 1
}
```

## Error Codes

### HTTP Status Codes

| Code | Description           |
| ---- | --------------------- |
| 200  | Success               |
| 201  | Created               |
| 400  | Bad Request           |
| 401  | Unauthorized          |
| 403  | Forbidden             |
| 404  | Not Found             |
| 409  | Conflict              |
| 429  | Too Many Requests     |
| 500  | Internal Server Error |
| 503  | Service Unavailable   |

### Application Error Codes

| Code | Name               | Description                   |
| ---- | ------------------ | ----------------------------- |
| 1000 | NETWORK_ERROR      | Network connectivity issue    |
| 1001 | TIMEOUT            | Operation timed out           |
| 1002 | INVALID_HASH       | Invalid file hash format      |
| 1003 | FILE_NOT_FOUND     | File not in network           |
| 1004 | INSUFFICIENT_FUNDS | Not enough balance            |
| 1005 | PERMISSION_DENIED  | Access not authorized         |
| 1006 | STORAGE_FULL       | Node provider capacity reached |
| 1007 | INVALID_CHUNK      | Chunk verification failed     |
| 1008 | PEER_UNREACHABLE   | Cannot connect to peer        |
| 1009 | INVALID_SIGNATURE  | Transaction signature invalid |
| 1010 | NONCE_TOO_LOW      | Transaction nonce too low     |

## Rate Limiting

### Default Limits

- Anonymous: 100 requests/minute
- Authenticated: 1000 requests/minute
- WebSocket: 100 messages/second

### Headers

```http
X-RateLimit-Limit: 1000
X-RateLimit-Remaining: 999
X-RateLimit-Reset: 1234567890
```

## Pagination

### Request Parameters

- `page`: Page number (starts at 1)
- `limit`: Items per page (max 100)
- `sort`: Sort field
- `order`: asc/desc

### Response Format

```json
{
  "data": [...],
  "pagination": {
    "page": 1,
    "limit": 20,
    "total": 150,
    "pages": 8,
    "has_next": true,
    "has_prev": false
  }
}
```

## Versioning

### API Version in URL

```
https://api.chiralnetwork.org/v1/...
https://api.chiralnetwork.org/v2/...
```

### Version Header

```http
API-Version: 1.0.0
```

## SDK Examples

### JavaScript/TypeScript

```typescript
import { ChiralClient } from "@chiral/sdk";

const client = new ChiralClient({
  apiKey: "your_api_key",
  network: "mainnet",
});

// Upload file
const result = await client.files.upload(file, {
  encryption: true,
});

// Download file
const data = await client.files.download(fileHash);

// Start mining
await client.mining.start({
  threads: 4,
  intensity: 75,
});
```

### Python

```python
from chiral import ChiralClient

client = ChiralClient(
    api_key='your_api_key',
    network='mainnet'
)

# Upload file
result = client.files.upload(
    file_path='document.pdf',
    encryption=True,
)

# Get file info
info = client.files.get_info(file_hash)

# Connect to peer
client.peers.connect(peer_id, multiaddr)
```

### Go

```go
package main

import "github.com/chiral-network/go-sdk"

func main() {
    client := chiral.NewClient("your_api_key", "mainnet")

    // Upload file
    result, err := client.Files.Upload("document.pdf", &chiral.UploadOptions{
        Encryption:  true,
    })

    // Start mining
    err := client.Mining.Start(&chiral.MiningOptions{
        Threads:   4,
        Intensity: 75,
    })
}
```

## Testing
Testing API documentation to be added here later.
