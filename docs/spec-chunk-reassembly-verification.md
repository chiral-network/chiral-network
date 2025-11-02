# Spec: Chunk Reassembly & Integrity Verification

## Summary

Given a set of downloaded chunks, reassemble them into the original file and verify integrity against the published Merkle root stored in the file metadata. Support streaming reassembly to avoid high memory usage.

## Why needed

Downloaded chunks must be reassembled into the original file binary and validated to ensure content hasn't been tampered with or corrupted in transit. This is essential for demo correctness and user trust.

## Changes

- Implement `reassemble_file` in `multi_source_download.rs` or a new `reassembly.rs` utility.

- Use Merkle-tree verification: verify each chunk's hash is a leaf in the published Merkle tree and reconstruct/verify root.

- Support streaming write directly to destination path to avoid memory spikes.

## API

- fn reassemble_file(chunks: Vec<PathBuf>, metadata: FileMetadata, output: PathBuf) -> Result<(), String>

- Behavior: Validate chunk count, verify each chunk's hash, write sequentially to `output`, and confirm final Merkle root matches metadata.

## Edge Cases & Errors

- Missing chunk(s): return Err with list of missing indices.

- Mismatched hash for a chunk: mark chunk as corrupted, remove from cache and retry download for that index.

## Tests

- Unit: small file split into 4 chunks, tamper one chunk and verify reassembly fails with expected error.

- Integration: full upload->download flow using local headless nodes.

## Files to modify

- `src-tauri/src/multi_source_download.rs`

- (optional) `src-tauri/src/reassembly.rs`

- Frontend: download completion UI to reflect verification success/failure.

---