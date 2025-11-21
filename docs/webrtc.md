# WebRTC File Transfer Implementation

This document describes how WebRTC is implemented in Chiral Network for decentralized peer-to-peer file transfers.

## Overview

Chiral Network uses WebRTC (Web Real-Time Communication) for direct peer-to-peer file transfers. Unlike traditional client-server architectures, WebRTC enables browsers and applications to communicate directly without intermediary servers handling the actual data transfer.

## Why WebRTC for File Sharing?

### Decentralization
- **No Central Server**: Files transfer directly between peers, not through a central server
- **Scalability**: Network capacity grows with each new peer (BitTorrent-like model)
- **Censorship Resistance**: No single point of failure or control
- **Privacy**: Data doesn't pass through third-party servers

### Performance Benefits
- **Direct Connections**: Lowest possible latency between peers
- **NAT Traversal**: Built-in ICE/STUN/TURN for connecting peers behind firewalls
- **Efficient Protocol**: UDP-based transport optimized for real-time data
- **Parallel Downloads**: Can download from multiple peers simultaneously

### Large File Support
- **Streaming Writes**: Chunks written directly to disk (no memory overflow)
- **Flow Control**: ACK-based protocol prevents data channel overflow
- **Resume Support**: Checkpoints allow resuming interrupted downloads
- **Chunked Transfer**: 16KB chunks with integrity verification

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Downloader                                │
│  ┌─────────────┐  ┌──────────────┐  ┌────────────────────────┐  │
│  │  Frontend   │  │  Signaling   │  │  Streaming Download    │  │
│  │  WebRTC     │──│  Service     │  │  (Tauri Backend)       │  │
│  │  Session    │  │  (DHT/WS)    │  │  - Chunk writes        │  │
│  └─────────────┘  └──────────────┘  │  - Checkpoints         │  │
│         │                │          │  - Resume support      │  │
│         │                │          └────────────────────────┘  │
└─────────│────────────────│──────────────────────────────────────┘
          │                │
          │   Offer/Answer │ (SDP Exchange via DHT)
          │                │
┌─────────│────────────────│──────────────────────────────────────┐
│         │                │              Seeder                   │
│  ┌──────▼──────┐  ┌──────▼───────┐  ┌────────────────────────┐  │
│  │  Frontend   │  │  Signaling   │  │  File Transfer         │  │
│  │  WebRTC     │──│  Service     │  │  Service               │  │
│  │  Session    │  │  (DHT/WS)    │  │  - Chunk reading       │  │
│  └─────────────┘  └──────────────┘  │  - Flow control        │  │
│                                      │  - Encryption          │  │
│                                      └────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

## Connection Flow

### 1. Peer Discovery
```
Downloader                          DHT Network                         Seeder
    │                                    │                                 │
    │──── Search for file hash ─────────►│                                 │
    │                                    │◄──── File metadata published ───│
    │◄─── Return seeder peer IDs ────────│                                 │
```

### 2. Signaling (Offer/Answer Exchange)
```
Downloader                          SignalingService                    Seeder
    │                                    │                                 │
    │──── Create WebRTC Offer ──────────►│                                 │
    │                                    │───── Relay offer via DHT ──────►│
    │                                    │                                 │
    │                                    │◄──── Create & send Answer ──────│
    │◄─── Receive Answer ────────────────│                                 │
    │                                    │                                 │
    │◄─────────────── ICE Candidates Exchange ────────────────────────────►│
```

### 3. File Transfer
```
Downloader                          DataChannel                         Seeder
    │                                    │                                 │
    │──── ManifestRequest ──────────────►│────────────────────────────────►│
    │◄─── ManifestResponse ──────────────│◄────────────────────────────────│
    │                                    │                                 │
    │──── FileRequest ──────────────────►│────────────────────────────────►│
    │                                    │                                 │
    │◄─── FileChunk[0] ──────────────────│◄────────────────────────────────│
    │──── ChunkAck[0] ──────────────────►│────────────────────────────────►│
    │◄─── FileChunk[1] ──────────────────│◄────────────────────────────────│
    │──── ChunkAck[1] ──────────────────►│────────────────────────────────►│
    │            ...                     │            ...                  │
```

## Key Components

### Frontend (`src/lib/services/`)

#### `webrtcService.ts`
Creates and manages browser WebRTC connections:
- `RTCPeerConnection` lifecycle management
- `RTCDataChannel` for bidirectional data transfer
- ICE candidate handling
- Signaling integration

#### `signalingService.ts`
Handles WebRTC signaling via DHT or WebSocket:
- Offer/Answer exchange
- ICE candidate relay
- Peer discovery integration
- Automatic backend selection (DHT preferred)

#### `p2pFileTransfer.ts`
Orchestrates the file transfer process:
- Connection establishment with retry logic
- Manifest request/response handling
- Chunk reception and validation
- ACK protocol for flow control
- Checkpoint management for resume

### Backend (`src-tauri/src/`)

#### Streaming Download Commands
```rust
init_streaming_download     // Create temp file, allocate space
write_download_chunk        // Write chunk at correct offset
finalize_streaming_download // Rename temp to final destination
save_download_checkpoint    // Save progress for resume
resume_download_from_checkpoint // Load checkpoint, return missing chunks
```

#### WebRTC Service (`webrtc_service.rs`)
Rust-side WebRTC for seeder functionality:
- File chunk reading and sending
- Flow control (batch sending with ACK wait)
- Encryption support (AES-256-GCM)
- HMAC authentication for integrity

## Protocol Messages

### ManifestRequest
```json
{
  "type": "ManifestRequest",
  "file_hash": "sha256:abc123..."
}
```

### ManifestResponse
```json
{
  "type": "ManifestResponse",
  "file_hash": "sha256:abc123...",
  "manifest_json": "{\"chunks\": [...], \"file_size\": 1234567}"
}
```

### FileChunk
```json
{
  "type": "file_chunk",
  "file_hash": "sha256:abc123...",
  "chunk_index": 42,
  "total_chunks": 1000,
  "data": [/* byte array */],
  "checksum": "sha256:..."
}
```

### ChunkAck (Flow Control)
```json
{
  "type": "ChunkAck",
  "file_hash": "sha256:abc123...",
  "chunk_index": 42,
  "ready_for_more": true
}
```

## Flow Control

To prevent data channel overflow when transferring large files:

1. **Batch Sending**: Seeder sends chunks in batches of 10
2. **ACK Tracking**: Each chunk receives an ACK from downloader
3. **Backpressure**: Seeder pauses when >20 chunks are unacked
4. **Timeout**: 5-second timeout prevents deadlock

```
Seeder                                              Downloader
  │                                                      │
  │──── Chunk[0-9] (batch) ─────────────────────────────►│
  │                                                      │
  │◄─── ACK[0], ACK[1], ... ACK[9] ──────────────────────│
  │                                                      │
  │──── Chunk[10-19] (next batch) ──────────────────────►│
  │            ...                                       │
```

## Streaming Downloads

For files larger than 1MB, chunks are written directly to disk:

1. **Pre-allocation**: Temp file created with full size allocated
2. **Random Writes**: Each chunk written at `offset = chunk_index * 16KB`
3. **No Memory Accumulation**: Only metadata kept in memory
4. **Atomic Finalization**: Temp file renamed to final destination

```
┌─────────────────────────────────────────────────────────┐
│                    .chiral_partial file                  │
├────────┬────────┬────────┬────────┬────────┬───────────┤
│ Chunk0 │ Chunk1 │ Chunk2 │  ...   │ChunkN-1│  ChunkN   │
│ 16KB   │ 16KB   │ 16KB   │        │ 16KB   │ <=16KB    │
└────────┴────────┴────────┴────────┴────────┴───────────┘
                          ↓
               (on completion, rename to)
                          ↓
┌─────────────────────────────────────────────────────────┐
│                     final_file.ext                       │
└─────────────────────────────────────────────────────────┘
```

## Resume Support

Downloads can be resumed after interruption:

### Checkpoint File (`.checkpoint`)
```json
{
  "file_hash": "sha256:abc123...",
  "file_name": "large_file.zip",
  "file_size": 1073741824,
  "output_path": "/downloads/large_file.zip",
  "total_chunks": 65536,
  "chunk_size": 16384,
  "received_chunks": [0, 1, 2, 5, 6, 7, ...],
  "temp_path": "/downloads/large_file.zip.chiral_partial"
}
```

### Resume Flow
1. Load checkpoint file
2. Verify temp file exists
3. Calculate missing chunks: `total - received`
4. Request only missing chunks from seeder
5. Continue writing to existing temp file

## Connection Recovery

Automatic retry with exponential backoff:

```
Attempt 1: Immediate retry with next seeder
Attempt 2: Wait 2s, try all seeders
Attempt 3: Wait 4s, try all seeders
Attempt 4: Wait 8s, try all seeders
Attempt 5: Wait 16s, try all seeders
Failure: Save checkpoint for manual resume
```

On each retry:
1. Save checkpoint (preserve progress)
2. Clean up failed WebRTC session
3. Try next available seeder
4. Resume from last received chunk

## Security Features

### Encryption
- **AES-256-GCM**: Per-chunk encryption with authenticated encryption
- **ECIES Key Wrapping**: Secure key exchange per chunk
- **PBKDF2**: Key derivation from passwords

### Integrity
- **SHA-256 Checksums**: Each chunk verified on receipt
- **HMAC Authentication**: Stream integrity for unencrypted transfers
- **Corrupted Chunk Re-request**: Automatic retry for failed chunks

## Comparison with Traditional Downloads

| Feature | Traditional HTTP | Chiral WebRTC |
|---------|-----------------|---------------|
| Server dependency | Required | None (P2P) |
| Scalability | Limited by server | Grows with peers |
| Resume support | Range requests | Checkpoint-based |
| Multi-source | No | Yes (planned) |
| Privacy | Server sees all | Direct peer transfer |
| NAT traversal | N/A | ICE/STUN/TURN |
| Large files | Memory issues | Streaming writes |

## Future Improvements

### Phase 3 (Performance)
- [ ] Parallel chunk requests (sliding window)
- [ ] Multi-source downloads from multiple peers
- [ ] DCUtR for better NAT hole punching
- [ ] Intelligent peer selection using reputation

### Phase 4 (Advanced)
- [ ] WebAssembly for crypto operations
- [ ] IPFS compatibility layer
- [ ] Adaptive bitrate based on network conditions
