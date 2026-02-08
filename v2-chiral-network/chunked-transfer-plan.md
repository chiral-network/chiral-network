# Plan: Chunked File Transfer Protocol

## Context

The current file transfer protocol sends entire files in a single CBOR message (`FileRequestResponse.file_data: Option<Vec<u8>>`). This loads the full file into memory on both sides, and any file exceeding ~10 MB hits the CBOR codec's size limit causing `UnexpectedEof` errors. We need a chunked protocol where the downloader requests individual 256 KB chunks, verifies each one via SHA-256, and reassembles the file — supporting files of any practical size without excessive memory usage.

## Protocol Design

### Message Types

Replace `FileRequestMessage`/`FileRequestResponse` with tagged enums reusing the same `file_request` behaviour:

```rust
const CHUNK_SIZE: usize = 256 * 1024; // 256 KB
const MAX_CHUNK_RETRIES: u8 = 3;

enum ChunkRequest {
    FileInfo { request_id, file_hash },           // "give me the manifest"
    Chunk { request_id, file_hash, chunk_index },  // "give me chunk N"
}

enum ChunkResponse {
    FileInfo { request_id, file_hash, file_name, file_size,
               chunk_size, total_chunks, chunk_hashes: Vec<String>, error },
    Chunk { request_id, file_hash, chunk_index,
            chunk_data: Option<Vec<u8>>, chunk_hash, error },
}
```

### Flow

```
Downloader                                Seeder
  |--- ChunkRequest::FileInfo ----------->|  compute chunk hashes
  |<-- ChunkResponse::FileInfo -----------|  manifest with chunk_hashes[]
  |                                       |
  |--- ChunkRequest::Chunk { 0 } -------->|  read 256KB from disk at offset
  |<-- ChunkResponse::Chunk { data } -----|  verify hash, append to file
  |    [rate-limit delay]                 |
  |--- ChunkRequest::Chunk { 1 } -------->|
  |<-- ChunkResponse::Chunk { data } -----|
  |    ... all chunks ...                 |
  |                                       |
  |  [SHA-256 full file == file_hash? done]
```

### Verification

- **Per-chunk**: SHA-256 of received data vs `chunk_hashes[i]` from manifest
- **Full-file**: SHA-256 of completed file vs the `file_hash` the user searched for
- Chunk hash mismatch -> retry (max 3 per chunk), then abort

### Rate Limiting

Delay between chunk requests instead of during disk writes:
- **Free (100 KB/s)**: ~2.6s between chunks
- **Standard (1 MB/s)**: ~0.25s between chunks
- **Premium**: no delay

## Files to Modify

### 1. `src-tauri/src/dht.rs` (primary - ~90% of changes)

**Remove:**
- `FileRequestMessage` struct
- `FileRequestResponse` struct
- Old `SwarmCommand::RequestFile` variant
- Old `FileRequest` event handlers (inbound request serving + outbound response handling)

**Add:**
- `CHUNK_SIZE` and `MAX_CHUNK_RETRIES` constants
- `ChunkRequest` and `ChunkResponse` enums (Serialize, Deserialize, Debug, Clone)
- `ActiveChunkedDownload` struct - tracks in-progress download state (chunk_hashes, received_chunks, output_path, retry_counts, peer_id, tier, bytes_written, start_time, current_chunk_index)
- `ActiveDownloadsMap = Arc<Mutex<HashMap<String, ActiveChunkedDownload>>>`
- `outbound_request_map: HashMap<OutboundRequestId, String>` in event loop scope - maps libp2p request IDs to our `request_id` for `OutboundFailure` correlation
- Helper: `compute_chunk_hashes(file_path, chunk_size) -> Result<Vec<String>>` - reads file in chunks, SHA-256 each
- `SharedFileInfo.chunk_hashes: Option<Vec<String>>` field - lazily computed, cached on first FileInfo request

**Update:**
- `DhtBehaviour.file_request`: type changes to `cbor_codec::Behaviour<ChunkRequest, ChunkResponse>`
- Protocol version: `/chiral/file-request/2.0.0`
- `SwarmCommand`: replace `RequestFile` with `RequestFileInfo { peer_id, request_id, file_hash }` and `RequestChunk { peer_id, request_id, file_hash, chunk_index }`
- `DhtService`: add `active_downloads: ActiveDownloadsMap` field, pass to event loop
- `request_file()` method: send `SwarmCommand::RequestFileInfo` instead of `RequestFile`

**New event handlers:**
- **Seeder inbound `ChunkRequest::FileInfo`**: look up file in `shared_files`, compute/cache chunk hashes, respond with manifest
- **Seeder inbound `ChunkRequest::Chunk`**: open file, seek to `chunk_index * CHUNK_SIZE`, read chunk, SHA-256 it, respond
- **Downloader `ChunkResponse::FileInfo`**: create `ActiveChunkedDownload`, create empty output file, request chunk 0
- **Downloader `ChunkResponse::Chunk`**: verify hash against manifest, append to file, emit `download-progress`, apply rate-limit delay via `tokio::time::sleep`, request next chunk OR finalize (SHA-256 full file, verify, emit complete/failed, cleanup)
- **`OutboundFailure`**: look up `request_id` via `outbound_request_map`, increment retry count, retry chunk or abort download

### 2. `src-tauri/src/speed_tiers.rs` (minor addition)

- Add `pub fn chunk_request_delay(chunk_size: u32, tier: &SpeedTier) -> Option<Duration>` - calculates how long to wait between chunk requests
- Keep existing `rate_limited_write()` for the local-cache code path

### 3. `src-tauri/src/lib.rs` (minimal)

- Update any references to removed `FileRequestMessage`/`FileRequestResponse` types if they exist
- `start_download` continues calling `dht.request_file()` - no signature changes needed

### 4. `cbor_codec` module in `dht.rs`

- Reduce `RESPONSE_SIZE_MAXIMUM` from 2 GB to 32 MB (FileInfo for a 100 GB file ~ 25 MB of chunk hashes; individual chunks are only 256 KB)

## Error Handling

| Error | Action |
|-------|--------|
| Chunk hash mismatch | Retry same chunk (max 3) |
| OutboundFailure (timeout/dial) | Retry same chunk (max 3) |
| "File not found" from seeder | Abort download, emit failed |
| 3 retries exceeded on any chunk | Abort download, delete partial file |
| Full-file hash mismatch | Emit failed, delete file |

## Backward Compatibility

Protocol bumps from `1.0.0` to `2.0.0`. Old/new nodes can't interoperate on file requests. At v0.1.0 stage this is fine - old nodes get `UnsupportedProtocols` which is already handled gracefully.

## Verification

1. `cargo build --lib` - compiles without errors
2. `cargo test` - tests pass (update serialization tests for new types)
3. Manual test: publish file on one node, download on another - chunks transfer, progress updates, file hash passes
