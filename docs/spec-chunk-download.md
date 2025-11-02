# Spec: Chunk Download & P2P Transfer

## Summary

Implement reliable chunk request/transfer from seeders. Support Bitswap (blockstore) and HTTP fallback. Provide a clear API to request chunks, progress events, retries and error modes.

## Why needed

Without a concrete chunk transfer implementation, downloads cannot fetch data from seeders discovered via the DHT. This is critical for end-to-end upload→discover→download demo.

## Changes

- Implement chunk request handlers in `file_transfer.rs` and integrate with `dht.rs` / `multi_source_download.rs`.

- Add a small Bitswap HTTP gateway fallback (if Bitswap block unavailable, try HTTP URL in metadata).

- Emit progress events to frontend: `download_chunk_started`, `download_chunk_progress`, `download_chunk_complete`, `download_chunk_failed`.

## API (Rust backend)

### Command: start_chunk_download

- Signature: async fn start_chunk_download(peer: String, cid: String, file_hash: String, chunk_index: u32, output_path: String) -> Result<(), String>

- Behavior: Attempts to fetch a chunk identified by `cid` from the `peer`. If peer supports Bitswap, use Bitswap request. If Bitswap fails within a short timeout, attempt HTTP to `peer_http_url` (if present in metadata).

- Events:

  - emit `download_chunk_started` { file_hash, chunk_index, cid, peer }

  - emit `download_chunk_progress` { file_hash, chunk_index, bytes_received, total_bytes }

  - emit `download_chunk_complete` { file_hash, chunk_index, path }

  - emit `download_chunk_failed` { file_hash, chunk_index, error }

### Errors

- Retries: default 3 attempts per chunk per peer with exponential backoff (250ms, 500ms, 1000ms).

- If all retries against a peer fail, return Err and let coordinator pick another peer.

## Data Shapes

```json
{ "download_chunk_started": {"file_hash":"...","chunk_index":0,"cid":"...","peer":"..."} }
```

## Integration points

- `multi_source_download.rs` must call `start_chunk_download` for each selected peer.

- `file_transfer.rs` should reuse existing blockstore/bitswap client.

- Store successful chunks into the local chunk cache (existing blockstore) to avoid re-download.

## Tests

- Unit test: mock a Bitswap peer that serves a block; verify event sequence and file written.

- Integration: start a local headless node as seeder, upload a small file, then download from another node.

## Success criteria

- Chunks can be downloaded from Bitswap-capable peers.

- HTTP fallback works when Bitswap fails.

- Events are emitted and consumable by frontend.

## Files to modify

- `src-tauri/src/file_transfer.rs`

- `src-tauri/src/multi_source_download.rs`

- `src-tauri/src/dht.rs` (for peer capabilities)

- Frontend: `src/lib/dht.ts` (listen to events), downloader UI components

---
