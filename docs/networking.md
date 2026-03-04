# Networking

## Swarm Configuration

The libp2p swarm is built in `dht.rs` with these transports and protocols:

- **Transport:** TCP with Noise encryption and Yamux multiplexing
- **Relay:** Circuit Relay v2 client for NAT traversal
- **Connection upgrade:** DCUtR for direct connections through relay
- **Idle timeout:** 3600 seconds

The swarm listens on dual-stack TCP (IPv4 `0.0.0.0:0` and IPv6 `:::0`) with random port assignment. It also establishes relay circuit reservations with configured relay nodes.

## DhtBehaviour

Ten sub-behaviours composed via `#[derive(NetworkBehaviour)]`:

| Field | Type | Protocol |
|-------|------|----------|
| `relay_client` | `relay::client::Behaviour` | Circuit Relay v2 |
| `dcutr` | `dcutr::Behaviour` | Direct Connection Upgrade |
| `kad` | `kad::Behaviour<MemoryStore>` | `/chiral/kad/1.0.0` |
| `mdns` | `mdns::tokio::Behaviour` | mDNS |
| `ping` | `ping::Behaviour` | libp2p ping |
| `identify` | `identify::Behaviour` | `/chiral/id/1.0.0` |
| `ping_protocol` | `request_response::cbor::Behaviour` | `/chiral/ping/1.0.0` |
| `file_transfer` | `cbor_codec::Behaviour` | `/chiral/file-transfer/1.0.0` |
| `file_request` | `cbor_codec::Behaviour` | `/chiral/file-request/3.0.0` |
| `echo_protocol` | `request_response::cbor::Behaviour` | `/chiral/echo/1.0.0` |

The `file_transfer` and `file_request` behaviours use a custom CBOR codec (`cbor_codec` module) with size limits: 1 MB for requests, 32 MB for responses.

## Bootstrap and Relay Nodes

Bootstrap nodes (used for Kademlia and initial peer discovery):

```
/ip4/130.245.173.73/tcp/4001/p2p/12D3KooWEfUVEbmkeH5C7TUNDn26hQTqs5TBYvKZgrCGMJroHRF1
/ip6/2002:82f5:ad49::1/tcp/4001/p2p/12D3KooWEfUVEbmkeH5C7TUNDn26hQTqs5TBYvKZgrCGMJroHRF1
/ip4/134.199.240.145/tcp/4001/p2p/12D3KooWFYTuQ2FY8tXRtFKfpXkTSipTF55mZkLntwtN1nHu83qE
```

Relay nodes (used for circuit relay reservations):

```
/ip4/130.245.173.73/tcp/4001/p2p/12D3KooWEfUVEbmkeH5C7TUNDn26hQTqs5TBYvKZgrCGMJroHRF1
/ip6/2002:82f5:ad49::1/tcp/4001/p2p/12D3KooWEfUVEbmkeH5C7TUNDn26hQTqs5TBYvKZgrCGMJroHRF1
```

On startup, the swarm dials all bootstrap nodes and opens relay circuit reservations. The relay set is a subset of bootstrap nodes.

## Connection Strategies

When downloading from a seeder, the system tries connections in this order:

1. **Already connected.** If the peer is in the swarm's connected set, the request is sent immediately.

2. **Direct dial via stored multiaddresses.** Seeder multiaddresses are published in DHT `FileMetadata`. The downloader retrieves these and uses `DialOpts::peer_id().addresses()` to attempt a direct TCP connection. Loopback addresses are filtered out.

3. **Relay circuit dial.** If direct dial fails or no multiaddresses are available, the system dials through each configured relay node using the address pattern `/relay_addr/p2p/relay_id/p2p-circuit/p2p/target_id`.

For mDNS-discovered local peers, connections are established automatically without explicit dialing.

## Swarm Command Loop

The swarm runs in a dedicated Tokio task. Tauri command handlers communicate with it via an MPSC channel sending `SwarmCommand` variants:

| Command | Purpose |
|---------|---------|
| `SendPing` | Ping a peer |
| `SendFile` | Direct file transfer (ChiralDrop) |
| `RequestFileInfo` | Start chunked download handshake |
| `PutDhtValue` | Store a DHT record |
| `GetDhtValue` | Retrieve a DHT record |
| `HealthCheck` | Get DHT health info |
| `CheckPeerConnected` | Check if peer is connected |
| `Echo` | Echo test |
| `GetListeningAddresses` | List swarm listener addresses |

Commands that need responses include a `oneshot::Sender` for the reply. Fire-and-forget commands (like `SendFile`) do not.

## Chunked File Transfer Protocol

Protocol: `/chiral/file-request/3.0.0`

### Message Types

**ChunkRequest** (downloader to seeder):
- `FileInfo { request_id, file_hash }` -- request file metadata
- `Chunk { request_id, file_hash, chunk_index }` -- request a specific chunk
- `PaymentProof { request_id, file_hash, payment_tx, payer_address }` -- prove payment

**ChunkResponse** (seeder to downloader):
- `FileInfo { request_id, file_hash, file_name, file_size, chunk_size, total_chunks, chunk_hashes, price_wei, wallet_address, error }` -- file metadata with per-chunk hashes
- `Chunk { request_id, file_hash, chunk_index, chunk_data, chunk_hash, error }` -- chunk data with hash
- `PaymentAck { request_id, file_hash, accepted, error }` -- payment acknowledgment

### Transfer Flow

1. Downloader sends `ChunkRequest::FileInfo` to get metadata.
2. Seeder responds with file size, chunk count, and a SHA-256 hash for each 256 KB chunk.
3. If the file is priced, the downloader sends payment and a `PaymentProof`.
4. Downloader requests each chunk sequentially with `ChunkRequest::Chunk`.
5. Each chunk is verified against its expected SHA-256 hash. Failed chunks are retried up to 3 times.
6. After all chunks are received, the assembled file is verified against the full file hash.
7. Speed tier delays are applied between chunk requests.

### Retry Logic

Each chunk has a retry counter. On `OutboundFailure` (timeout, connection error), the counter increments. After 3 failures for any single chunk, the download aborts. Request IDs are tracked in an `outbound_request_map` to correlate libp2p request/response IDs with download state.

### Active Download Tracking

The `ActiveChunkedDownload` struct tracks each in-progress download: file metadata, received chunks, chunk hashes, retry counts, and the request ID. Multiple concurrent downloads are supported, keyed by `request_id` in a `HashMap`.

## Speed Tiers

| Tier | Speed Limit | Cost per MB | Chunk Delay (256 KB) |
|------|------------|-------------|---------------------|
| Standard | 1 MB/s | 0.001 CHI | ~250 ms |
| Premium | 5 MB/s | 0.005 CHI | ~50 ms |
| Ultra | Unlimited | 0.01 CHI | None |

Tier payments are sent to a burn address before the download begins. The cost formula: `(file_size_bytes * cost_per_mb_wei + 999_999) / 1_000_000`, rounding up.

The rate-limited file write function in `speed_tiers.rs` writes in 8 KB sub-chunks with delays, emitting progress events every 64 KB.

## Direct File Transfer Protocol

Protocol: `/chiral/file-transfer/1.0.0`

Used for ChiralDrop peer-to-peer transfers. Sends the entire file in a single request/response message:

```
FileTransferRequest {
    transfer_id, file_name, file_data,
    price_wei, sender_wallet, file_hash, file_size
}
```

The recipient receives a `file-transfer-request` event and can accept or decline. On accept, the file is saved to the download directory.

## DHT Record Storage

All DHT records use Kademlia `put_record` and `get_record`. Values are JSON strings. Key patterns:

- `chiral_file_{hash}` -- `FileMetadata` with seeders list, each including peer_id, price, wallet, and multiaddrs
- `chiral_host_{peer_id}` -- Host advertisement (capacity, pricing, accepted types)
- `chiral_host_registry` -- Array of `HostRegistryEntry` (peer_id, wallet, timestamp)
- `chiral_agreement_{id}` -- Hosting agreement JSON
- `chiral_encryption_key_{peer_id}` -- X25519 public key hex

When publishing a file, the publisher's listening addresses are included in the `SeederInfo.multiaddrs` field so other peers can dial directly.

## Hosting Agreement Lifecycle

1. **Host advertises.** Publishes advertisement to DHT at `chiral_host_{peer_id}` and adds entry to `chiral_host_registry`.
2. **Browser discovers.** The Hosts page queries `chiral_host_registry` for available hosts and fetches individual advertisements.
3. **Proposer creates agreement.** Selects files from Drive, specifies duration and deposit. Agreement JSON stored at `chiral_agreement_{id}` in DHT and locally.
4. **Host accepts.** Updates agreement status. Begins seeding the agreed files using `register_shared_file`.
5. **Active hosting.** Host serves files via the chunked transfer protocol. Files are discoverable through DHT file metadata.
6. **Completion/cancellation.** Agreement status updated. `cleanup_agreement_files` removes hosted files and unregisters them from DHT.

## Local Gateway Server

An Axum HTTP server starts on port 9419 during app setup. Routes:

| Route | Purpose |
|-------|---------|
| `GET /health` | Health check |
| `GET /sites/{id}/*` | Serve hosted site files |
| `GET /api/drive/items` | List Drive items |
| `POST /api/drive/folders` | Create folder |
| `POST /api/drive/upload` | Upload file (multipart, 500 MB max) |
| `PUT /api/drive/items/:id` | Update item |
| `POST /api/drive/shares` | Create share link |
| `GET /api/drive/shares/:token` | Access shared file |

The server includes directory traversal protection: rejects `..`, null bytes, and paths that escape the site directory after canonicalization.

## Relay Share Proxying

For NAT traversal of shared Drive content and hosted sites:

1. **Registration.** The sharer calls `publish_drive_share` or `publish_site_to_relay`, which POSTs the share token and local origin URL to the relay server.
2. **Relay stores metadata.** The `RelayShareRegistry` maps tokens/site IDs to origin URLs. Persisted to `{data_dir}/chiral-relay-shares/registry.json`.
3. **Access.** When a visitor requests content via the relay URL, the relay checks for a WebSocket tunnel first. If available, it forwards the request through the tunnel. Otherwise, it reverse-proxies directly to the origin URL.
4. **URL rewriting.** If the origin URL uses localhost/0.0.0.0, the relay rewrites it using the client's real IP address.
5. **Tunnel protocol.** For NAT'd hosts, a WebSocket connection carries `TunnelRequest`/`TunnelResponse` messages with base64-encoded bodies. 30-second timeout per request.

## Encryption Protocol

File encryption uses X25519 ECDH + AES-256-GCM:

1. Sender generates an ephemeral X25519 keypair.
2. ECDH shared secret computed between ephemeral secret and recipient's public key.
3. HKDF-SHA256 derives a 256-bit AES key using the ephemeral public key as salt and `"chiral-network-v2-e2ee"` as info.
4. File encrypted with AES-256-GCM using a random 12-byte nonce.
5. Bundle sent: ephemeral public key + nonce + ciphertext (all hex-encoded).

Recipient's X25519 keypair is deterministically derived from their wallet private key via SHA-256. Public keys are published to DHT at `chiral_encryption_key_{peer_id}` for discovery.

All libp2p transport connections are secured with the Noise protocol (XX handshake pattern).
