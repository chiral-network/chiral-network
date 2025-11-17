# Proposal: Chunk Hash Verification for Multi-Source Downloads

## Executive Summary

This proposal outlines the implementation of cryptographic integrity verification for chunks downloaded through the multi-source download service. The feature ensures data integrity by validating SHA-256 hashes of downloaded chunks before they are stored or assembled into final files.

## Problem Statement

### Current State

The Chiral Network multi-source download service supports downloading files from multiple sources simultaneously (FTP, HTTP, WebRTC/P2P, ed2k) to improve download speed and reliability. However, the current implementation lacks integrity verification for downloaded chunks.

**Critical Gap**: At line 920 in `src-tauri/src/multi_source_download.rs`, there is a TODO comment:
```rust
// TODO: Add hash verification here once chunk hashes are properly calculated
// For now, we'll skip hash verification as it needs to be implemented in the chunk calculation
```

### Risks Without Verification

1. **Data Corruption**: Corrupted chunks from unreliable sources can be written to disk, resulting in unusable files
2. **Silent Failures**: Users may not discover corruption until attempting to use files, wasting time and bandwidth
3. **Network Trust**: Bad actors could intentionally serve corrupted data without detection
4. **Resource Waste**: Entire downloads must be retried when corruption is discovered late, rather than retrying only affected chunks
5. **Security Vulnerabilities**: Without integrity checks, malicious peers could inject corrupted data into the network

### Real-World Impact

- **User Experience**: Users download files that appear complete but are corrupted
- **Network Efficiency**: Bandwidth wasted on corrupted data that must be re-downloaded
- **Reputation System**: Cannot accurately track which peers provide reliable data
- **Future Features**: Blocks implementation of proof-of-delivery receipts (PoDR) which require verified chunk integrity

## Purpose & Goals

### Primary Goals

1. **Data Integrity**: Ensure all downloaded chunks match their expected cryptographic hashes
2. **Early Detection**: Reject corrupted chunks immediately after download, before storage
3. **Automatic Recovery**: Integrate with existing retry mechanisms to automatically fetch chunks from alternative sources
4. **Backward Compatibility**: Gracefully handle legacy chunk metadata that may not have proper hashes yet

### Secondary Goals

1. **Performance**: Minimize overhead of hash verification (target: <1ms per chunk)
2. **Observability**: Provide clear error messages and logging for debugging
3. **Extensibility**: Design verification system to work across all source types (FTP, HTTP, WebRTC, ed2k)

## Technical Approach

### Hash Format

- **Algorithm**: SHA-256 (256-bit, 64 hex characters)
- **Format**: Lowercase hexadecimal string
- **Source**: Chunk hashes are calculated during file upload/encryption and stored in `ChunkInfo.hash`

### Verification Flow

```
1. Download chunk data from source (FTP/HTTP/P2P/ed2k)
2. Validate chunk size matches expected size
3. Compute SHA-256 hash of downloaded data
4. Compare computed hash with expected hash from ChunkInfo
5. If match: Store chunk and emit ChunkCompleted event
6. If mismatch: Add to failed_chunks queue and emit ChunkFailed event
7. Retry mechanism automatically attempts failed chunks from alternative sources
```

### Implementation Strategy

#### Phase 1: Core Verification Functions (✅ Completed)

- `normalized_sha256_hex()`: Validates and normalizes hash format
- `verify_chunk_integrity()`: Performs hash computation and comparison

**Design Decisions**:
- Graceful degradation: Skip verification if hash format is invalid (maintains compatibility)
- Return detailed error information: Both expected and actual hashes for debugging

#### Phase 2: FTP Integration (✅ Completed)

- Integrated verification into `start_ftp_chunk_downloads()` function
- Added error handling and event emission
- Ensured failed chunks are queued for retry

#### Phase 3: Extend to Other Sources (Future Work)

- HTTP downloads: Add verification in HTTP chunk download handler
- WebRTC/P2P downloads: Add verification in WebRTC chunk receive handler
- ed2k downloads: Add verification in ed2k chunk download handler

#### Phase 4: Hash Metadata Migration (Future Work)

- Ensure all file uploads generate proper SHA-256 hashes for chunks
- Update chunk calculation to always include cryptographic hashes
- Migrate existing files to include proper hashes

## Implementation Plan

### Architecture

```
┌─────────────────────────────────────────────────────────┐
│              Multi-Source Download Service               │
└─────────────────────────────────────────────────────────┘
                          │
                          ▼
        ┌─────────────────────────────────────┐
        │   Download Chunk from Source        │
        │   (FTP/HTTP/WebRTC/ed2k)           │
        └─────────────────────────────────────┘
                          │
                          ▼
        ┌─────────────────────────────────────┐
        │   Validate Chunk Size               │
        └─────────────────────────────────────┘
                          │
                          ▼
        ┌─────────────────────────────────────┐
        │   verify_chunk_integrity()          │
        │   - Compute SHA-256 of data         │
        │   - Compare with ChunkInfo.hash     │
        └─────────────────────────────────────┘
                          │
            ┌─────────────┴─────────────┐
            │                           │
            ▼                           ▼
    ┌──────────────┐          ┌──────────────────┐
    │ Hash Match   │          │ Hash Mismatch    │
    └──────────────┘          └──────────────────┘
            │                           │
            ▼                           ▼
    ┌──────────────┐          ┌──────────────────┐
    │ Store Chunk  │          │ Emit ChunkFailed │
    │ Emit Success │          │ Queue for Retry  │
    └──────────────┘          └──────────────────┘
```

### Code Structure

```rust
// Helper function: Normalize and validate hash format
fn normalized_sha256_hex(hash: &str) -> Option<String>

// Core verification function
fn verify_chunk_integrity(
    chunk: &ChunkInfo, 
    data: &[u8]
) -> Result<(), (String, String)>

// Integration point in download flow
async fn start_ftp_chunk_downloads(...) {
    // ... download chunk ...
    if let Err((expected, actual)) = verify_chunk_integrity(&chunk, &data) {
        // Handle verification failure
    }
    // ... store chunk ...
}
```

### Error Handling

- **Hash Mismatch**: Emit `ChunkFailed` event with detailed error message
- **Invalid Hash Format**: Skip verification (backward compatibility)
- **Missing Hash**: Skip verification (graceful degradation)

### Performance Considerations

- **SHA-256 Computation**: ~0.1-1ms per chunk (negligible for typical chunk sizes)
- **Memory**: No additional memory overhead (hash computed in-place)
- **CPU**: Minimal impact, can be optimized with hardware acceleration if needed

## Testing Strategy

### Unit Tests

- ✅ Valid hash acceptance
- ✅ Hash mismatch detection
- ✅ Legacy hash format handling
- ✅ Edge cases (empty data, very large chunks)

### Integration Tests

- ✅ FTP download with hash verification
- ✅ Error event emission
- ✅ Retry mechanism integration
- ⏳ HTTP download verification (future)
- ⏳ WebRTC download verification (future)

### Performance Tests

- ⏳ Benchmark hash computation overhead
- ⏳ Measure impact on download throughput

## Success Metrics

### Immediate Metrics

- **Verification Coverage**: % of chunks verified (target: 100% for chunks with valid hashes)
- **Corruption Detection Rate**: Number of corrupted chunks detected and rejected
- **False Positive Rate**: Should be 0% (no valid chunks rejected)

### Long-Term Metrics

- **Download Success Rate**: Should improve as corrupted chunks are rejected early
- **Network Trust**: Reputation system can track which peers provide verified data
- **User Satisfaction**: Reduced reports of corrupted downloads

## Risks & Mitigation

### Risk 1: Performance Impact

**Risk**: Hash computation slows down downloads
**Mitigation**: 
- SHA-256 is fast (~1ms per chunk)
- Can be parallelized if needed
- Hardware acceleration available on modern CPUs

### Risk 2: Backward Compatibility

**Risk**: Breaking existing downloads that use placeholder hashes
**Mitigation**: 
- Graceful degradation: Skip verification for invalid hash formats
- No breaking changes to existing APIs

### Risk 3: False Positives

**Risk**: Valid chunks incorrectly rejected
**Mitigation**: 
- Comprehensive unit tests
- Careful hash normalization (case-insensitive, whitespace trimming)
- Detailed error logging for debugging

## Future Enhancements

1. **Proof-of-Delivery Receipts (PoDR)**: Use verified chunk hashes as cryptographic proof of successful delivery
2. **Reputation Weighting**: Weight peer reputation scores based on verified vs. unverified chunks
3. **Parallel Verification**: Verify multiple chunks concurrently to improve throughput
4. **Hardware Acceleration**: Use CPU SHA-256 instructions for faster verification
5. **Metrics Dashboard**: Track verification statistics in network analytics

## Conclusion

Chunk hash verification is a critical security and reliability feature that ensures data integrity in the Chiral Network. The implementation is designed to be:

- **Non-breaking**: Gracefully handles legacy hash formats
- **Performant**: Minimal overhead on download performance
- **Extensible**: Easy to extend to all source types
- **Observable**: Clear error messages and logging

This feature lays the foundation for future enhancements like proof-of-delivery receipts and improved reputation systems, while immediately improving the reliability and trustworthiness of the network.

