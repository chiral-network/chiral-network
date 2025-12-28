# PR Summary: feat/ed2k-peer-serving

## Overview
Implements ED2K peer serving functionality, enabling seeders to respond to download requests from other peers. This completes the upload side of ED2K P2P file sharing.

## Problem Statement
The existing ED2K implementation had download capability (client can request chunks from peers) but no upload serving (no way to respond when peers request chunks from us). When peers tried to download, there was no TCP listener accepting connections or handler for `OP_REQUESTPARTS` packets.

## Changes Made

### Ed2kPeerServer - NEW (189 lines)
- `TcpListener` on port 4661 accepting incoming peer connections
- `share_file()` / `unshare_file()` - manage files available for serving
- `start()` - spawns async listener for peer connections
- `handle_peer_connection()` - processes requests from each peer
- `handle_request_parts()` - parses `OP_REQUESTPARTS`, validates file hash, reads chunk from disk
- Responds with `OP_SENDINGPART` containing requested chunk data
- Graceful shutdown via broadcast channel

### Integration with Ed2kProtocolHandler (27 lines)
- Added `peer_server` field to handler struct
- Start peer server automatically when first file is seeded (lazy init)
- Share each seeded file with peer server using MD4 hash from ed2k link
- Peer server runs for lifetime of application after first seed

## Technical Details

**Architecture:**
- `Arc<RwLock<HashMap<String, PathBuf>>>` tracks shared files (MD4 hash → file path)
- One async task spawned per peer connection
- On-demand file I/O (reads chunks when requested, not pre-cached)
- Reuses existing `Ed2kClient::send_packet()` / `receive_packet()` methods

**Protocol Flow:**
```
Downloader                     Seeder (Peer Server)
    |                                  |
    |-- TCP Connect to 4661 ---------->|
    |-- OP_REQUESTPARTS -------------->| (lookup file in HashMap)
    |                                  | (read chunk from disk)
    |<-- OP_SENDINGPART ----------------|
    |    [chunk_data]                  |
```

**Security:**
- ✅ Path traversal prevention (files tracked by hash, not paths)
- ✅ Graceful degradation (server start failure doesn't break seeding)
- ✅ Silent rejection (missing file requests ignored)
- ✅ **Connection limiting** - Max 100 concurrent connections (prevents resource exhaustion)
- ✅ **Chunk size validation** - Max 10MB per request (prevents memory exhaustion attacks)
- ✅ **Connection timeouts** - 5 minute max per connection (prevents slow loris attacks)
- ✅ **Hash format validation** - Validates 32-char hex MD4 hashes before processing
- ✅ **Bounds checking** - Validates offsets don't exceed file size
- ✅ **Active connection tracking** - Decrements count when connections close
- ⚠️ TODO: Rate limiting per IP address
- ⚠️ TODO: Bandwidth throttling per connection

## Testing
✅ **Build:** Compiles cleanly  
✅ **Manual Test:** Two instances successfully transferred file via ED2K  
✅ **Logs Verified:**
- `"ED2K peer server listening on 0.0.0.0:4661"`
- `"ED2K: Peer server started on port 4661"`
- `"ED2K: Now sharing file {hash} from {path}"`
- `"ED2K: Sent N bytes to peer for file {hash}"`

## What's Working Now
✅ Seeders accept incoming peer connections on port 4661  
✅ Handle `OP_REQUESTPARTS` from downloading peers  
✅ Serve file chunks via `OP_SENDINGPART` responses  
✅ Complete P2P file transfers between ED2K clients  
✅ Integration with manifest generation (feat/ed2k-upload-manifest)  
✅ Works with download validation (PRs #950/#952)

## What's Still Missing (Future Work)
❌ Rate limiting per IP address  
❌ Bandwidth throttling per connection  
❌ Upload bandwidth tracking/statistics  
❌ Integration tests for peer serving  
❌ Metrics for rejected connections  

## Files Changed
- `src-tauri/src/ed2k_client.rs` - Added `Ed2kPeerServer` struct (+189 lines)
- `src-tauri/src/protocols/ed2k.rs` - Integrated peer server with seeding (+27 lines)

**Total:** ~216 lines (under 300 line target ✅)

## Dependencies
- **Builds on:** feat/ed2k-upload-manifest (manifest generation)
- **Complements:** PRs #950/#952 (download validation), feat/ed2k-peer-protocol
- **Enables:** Complete bidirectional ED2K P2P file sharing
