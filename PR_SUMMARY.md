# Add ED2K Core Hash Computation and File Validation

## Summary
This PR implements the foundational hash computation and file validation system for the ED2K (eDonkey2000) protocol, providing the building blocks needed for proper ED2K file integrity verification and multi-chunk file handling.

## Changes

### Hash Computation
- **`compute_md4_hash()`**: Calculate MD4 hash for arbitrary data
- **`compute_file_hash()`**: Generate ED2K file hash with proper handling:
  - Files ≤ 9.28MB: Returns direct MD4 hash of entire file
  - Files > 9.28MB: Returns MD4 hash of concatenated chunk hashes (root hash)
- **`compute_chunk_hashes()`**: Compute MD4 hashes for all 9.28MB ED2K chunks in a file

### Validation
- **`validate_file()`**: Verify entire file against expected ED2K hash
- **`validate_chunk()`**: Verify single chunk integrity with expected hash
- **`verify_md4_hash()`**: Core validation function with case-insensitive comparison

### File Operations
- **`create_file_info()`**: Generate complete ED2K metadata from local file:
  - File hash (MD4)
  - File size
  - File name
  - Chunk hashes (for multi-chunk files)
- **`split_into_chunks()`**: Split data into 9.28MB ED2K chunks
- **`get_chunk_count()`**: Calculate number of chunks needed for a file size
- **`get_chunk_size()`**: Get size of specific chunk (handles last chunk correctly)

### Testing
Added 27 comprehensive tests covering:
- MD4 hash computation (empty data, known values, case sensitivity)
- File hash computation (small files, multi-chunk files)
- Chunk operations (splitting, counting, size calculations)
- File validation (correct hash, incorrect hash, edge cases)
- File info creation from actual files

## Technical Details

### ED2K Hash System
The ED2K protocol uses MD4 hashing with a specific structure:
- **Chunk size**: 9,728,000 bytes (9.28 MB)
- **Small files** (≤ 9.28MB): Direct MD4 hash of file content
- **Large files** (> 9.28MB): MD4 hash of concatenated chunk hashes (Merkle-like structure)

### Integration
These functions integrate with the existing ED2K implementation in `multi_source_download.rs`, which handles:
- Downloading full 9.28MB ED2K chunks
- Splitting them into 256KB application chunks
- Storing individual chunks

This PR provides the hash computation and verification layer that ensures integrity at both the ED2K chunk level and full file level.

## Testing
All 27 tests pass successfully:
```
test result: ok. 27 passed; 0 failed; 0 ignored
```

Tests cover:
- Known MD4 hash values
- File operations with temporary files
- Chunk boundary conditions
- Empty and edge case inputs
- Case-insensitive hash comparison

## Why This Matters
ED2K's 9.28MB chunk structure is fundamentally different from HTTP's byte-range requests or BitTorrent's piece structure. This PR provides the correct cryptographic foundation for:
- Verifying downloaded ED2K chunks before storing
- Computing file hashes for sharing files via ED2K
- Validating complete file integrity after download
- Supporting the existing multi-source download infrastructure
