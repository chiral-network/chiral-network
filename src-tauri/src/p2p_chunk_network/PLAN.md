# P2P Chunk Network Integration Plan

## Codebase Analysis Summary

### DHT API (dht.rs)
- `DhtCommand::GetProviders { file_hash, sender }` - Query providers for merkle_root
- `get_seeders_for_file(&str) -> Vec<String>` - Frontend method, returns peer IDs
- `start_providing(RecordKey)` - Announce as provider during PublishFile
- `DhtCommand::AnnounceTorrent { info_hash }` - BitTorrent provider announcement

### Download Systems
- **manager.rs**: ChunkInfo with index, hash, size, encrypted variants
- **multi_source_download.rs**: ActiveDownload with chunks, completed_chunks, failed_chunks
- **reassembly.rs**: Sparse writes with bitmap tracking for resume
- **HTTP**: 256KB chunks via Range requests
- **WebRTC**: 32KB binary frames with checksum

### Hash Systems
- **SHA-256**: Primary algorithm via `sha2::Sha256`
- **rs_merkle**: MerkleTree<Sha256Hasher> for root computation
- **CID**: `Cid::new_v1(RAW_CODEC, Code::Sha2_256.digest(data))`
- **RAW_CODEC**: 0x55 (IPFS raw content)

### Key Data Structures
```rust
// manager.rs
pub struct ChunkInfo {
    pub index: u32,
    pub hash: String,           // SHA-256 hex
    pub size: usize,
    pub encrypted_hash: String,
    pub encrypted_size: usize,
}

pub struct FileManifest {
    pub merkle_root: String,
    pub chunks: Vec<ChunkInfo>,
    pub encrypted_key_bundle: Option<...>,
}

// dht/models.rs
pub struct FileMetadata {
    pub merkle_root: String,
    pub manifest: Option<String>,  // Serialized FileManifest
    pub seeders: Vec<String>,
    pub cids: Option<Vec<Cid>>,
    // ...
}
```

## Integration Strategy

### Decision: Enhance existing systems, don't replace

The codebase already has:
1. Merkle tree computation in manager.rs
2. Multi-source downloading in multi_source_download.rs
3. Chunk tracking in reassembly.rs
4. DHT provider queries

We should wire them together, not duplicate.

## Phase 1: Merkle Verification Integration

### 1.1 Create p2p_chunk_network module
- New module that coordinates existing systems
- Uses manager.rs for merkle operations
- Uses dht.rs for provider operations

### 1.2 Verify merkle_root on recovery
- Load FileManifest from metadata
- Recompute merkle tree from chunk hashes
- Verify root matches stored merkle_root

### 1.3 Wire into download loop
- Hook into multi_source_download.rs chunk completion
- Compute hash of downloaded chunk
- Compare with ChunkInfo.hash from manifest

## Phase 2: DHT Provider Integration

### 2.1 Query providers on recovery
- Call `dht_service.get_seeders_for_file(merkle_root)`
- Add returned peer IDs to known_peers
- Use for chunk requests

### 2.2 Announce as provider
- When download completes, call `start_providing(merkle_root)`
- Store in DHT that we have this content

## Phase 3: Chunk Request Protocol

### 3.1 Use existing Bitswap
- dht.rs already has `DhtCommand::StoreBlock` and block fetching
- Bitswap protocol handles chunk requests
- CIDs identify chunks

### 3.2 Alternative: WebRTC chunks
- webrtc_service.rs already has chunk protocol
- Binary frame format with chunk_index, file_hash, checksum
- Can request specific chunks from peers

## Phase 4: Recovery Flow

### 4.1 On startup
- Scan for incomplete downloads (reassembly bitmaps)
- Load FileManifest from metadata
- Query DHT for providers
- Resume downloading missing chunks

### 4.2 Chunk verification
- Read downloaded chunks from disk
- Hash with SHA-256
- Compare with manifest chunk hashes
- Mark corrupted chunks for re-download

## Implementation Order

1. Create p2p_chunk_network.rs module
2. Add merkle verification using manager.rs
3. Add DHT provider queries
4. Hook into multi_source_download.rs
5. Add recovery scanning
6. Add chunk verification
7. Add corruption detection
8. Integration tests

## File Changes

### New Files
- src-tauri/src/p2p_chunk_network.rs - Main coordination module
- src-tauri/src/p2p_chunk_network/recovery.rs - Recovery logic
- src-tauri/src/p2p_chunk_network/verify.rs - Verification logic

### Modified Files
- src-tauri/src/lib.rs - Register module
- src-tauri/src/main.rs - Register Tauri commands
- src-tauri/src/multi_source_download.rs - Hook chunk completion
- src-tauri/src/dht.rs - Add chunk-level provider commands

## Success Criteria

1. merkle_root computed and verified from chunk hashes
2. DHT queries return real provider peer IDs
3. Can resume download from different peer
4. Corruption detected and chunks re-fetched
5. End-to-end test: crash mid-download, resume from new peer
