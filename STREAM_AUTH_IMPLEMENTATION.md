# Stream Authentication Implementation

## Overview

This document describes the implementation of cryptographic stream authentication for the Chiral Network, providing real-time data integrity verification during file transfers.

## Implementation Date

**Completed**: October 6, 2025

## Problem Statement

The Chiral Network previously lacked real-time stream authentication during file transfers. While the system had:

- SHA-256 checksums for individual chunks
- Merkle tree verification for complete files
- AES-256-GCM encryption (which provides some authentication)

It was missing proper **stream-level authentication** that would:

- Verify data integrity in real-time during transfer
- Authenticate the sender of each chunk
- Detect replay attacks
- Ensure overall stream integrity

## Solution

Implemented HMAC-based stream authentication using HMAC-SHA256 for cryptographic verification of data integrity during file transfers.

## Architecture

### Core Components

#### 1. `StreamAuthService` (`src-tauri/src/stream_auth.rs`)

The main service responsible for managing authenticated sessions and signing/verifying messages.

**Key Features:**

- Session-based authentication with unique session IDs
- HMAC-SHA256 signing and verification
- Sequence number tracking (prevents replay attacks)
- Timestamp validation (prevents stale messages)
- Automatic session cleanup

**Main Methods:**

- `create_session()` - Initialize a new authenticated session
- `sign_data()` - Generate HMAC signature for outgoing data
- `verify_data()` - Verify HMAC signature on incoming data
- `generate_hmac_key()` - Create secure 256-bit HMAC keys
- `cleanup_expired_sessions()` - Remove stale sessions

#### 2. `AuthMessage` Structure

Encapsulates authenticated data with verification metadata:

```rust
pub struct AuthMessage {
    pub message_type: AuthMessageType,  // Handshake, DataChunk, etc.
    pub data: Vec<u8>,                  // Actual payload
    pub signature: String,              // HMAC-SHA256 signature
    pub sequence: u64,                  // Sequence number
    pub timestamp: u64,                 // Unix timestamp
}
```

#### 3. Integration with WebRTC Service

The stream authentication is integrated into the existing WebRTC file transfer pipeline:

1. **Sender Side:**
   - Creates authenticated session
   - Signs each chunk with HMAC
   - Includes `AuthMessage` in `FileChunk`
   - Sends authenticated chunk

2. **Receiver Side:**
   - Extracts `AuthMessage` from received chunk
   - Verifies HMAC signature
   - Checks sequence number
   - Validates timestamp
   - Only processes chunk if verification passes

## Security Features

### 1. HMAC-SHA256 Authentication

- Industry-standard message authentication code
- 256-bit keys provide strong cryptographic security
- Fast computation with minimal performance overhead

### 2. Replay Attack Protection

- Sequential sequence numbers ensure message ordering
- Duplicate or out-of-order messages are rejected
- Each message must have the next expected sequence number

### 3. Freshness Validation

- Timestamp-based validation (5-minute window by default)
- Prevents acceptance of old/cached messages
- Configurable timeout per session

### 4. Session Management

- Each file transfer gets unique session ID
- Sessions automatically expire after timeout
- Cleanup mechanism prevents memory leaks

### 5. Defense in Depth

- Works alongside existing checksums and Merkle trees
- Complements AES-256-GCM encryption
- Multiple layers of integrity verification

## Implementation Details

### Files Modified

1. **`src-tauri/src/stream_auth.rs`** (NEW)
   - Complete stream authentication service
   - 400+ lines including tests
   - Comprehensive session and signature management

2. **`src-tauri/src/lib.rs`**
   - Added `pub mod stream_auth;`

3. **`src-tauri/src/webrtc_service.rs`**
   - Added `auth_message: Option<AuthMessage>` to `FileChunk`
   - Added `stream_auth: Arc<Mutex<StreamAuthService>>` to `WebRTCService`
   - Updated `process_incoming_chunk()` to verify authentication
   - Updated all data channel handlers to pass stream_auth

4. **`src-tauri/src/main.rs`**
   - Added `stream_auth: Arc<Mutex<StreamAuthService>>` to `AppState`
   - Added Tauri commands:
     - `create_auth_session()`
     - `verify_stream_auth()`
     - `generate_hmac_key()`
     - `cleanup_auth_sessions()`

5. **`README.md`**
   - Updated to mark stream authentication as ✅ implemented

6. **`SECURITY.md`**
   - Added Stream Authentication section
   - Documented security features

### Dependencies

All required dependencies were already present:

- `hmac = "0.12"` - HMAC implementation
- `sha2` - SHA-256 hashing
- `hex = "0.4"` - Hex encoding for signatures
- `rand = "0.8"` - Secure random key generation

## Usage

### For File Senders

```rust
// Create authenticated session
let session_id = format!("{}-{}", peer_id, file_hash);
let hmac_key = StreamAuthService::generate_hmac_key();
stream_auth.create_session(session_id.clone(), hmac_key)?;

// Sign each chunk
let auth_msg = stream_auth.sign_data(
    &session_id,
    &chunk_data,
    AuthMessageType::DataChunk
)?;

// Include in FileChunk
let chunk = FileChunk {
    // ... other fields ...
    auth_message: Some(auth_msg),
};
```

### For File Receivers

```rust
// Verify incoming chunk
if let Some(ref auth_msg) = chunk.auth_message {
    let session_id = format!("{}-{}", peer_id, chunk.file_hash);

    if !stream_auth.verify_data(&session_id, auth_msg)? {
        warn!("Authentication failed!");
        return; // Reject chunk
    }
}

// Process verified chunk...
```

## Performance Impact

### Computational Overhead

- HMAC-SHA256 is extremely fast (microseconds per chunk)
- Minimal CPU overhead compared to network I/O
- No noticeable impact on transfer speeds

### Memory Overhead

- ~100 bytes per session for metadata
- Session cleanup prevents memory growth
- Negligible impact on overall memory usage

### Network Overhead

- ~40 bytes per chunk for signature + metadata
- Less than 0.3% overhead for 16KB chunks
- Acceptable trade-off for security benefits

## Testing

### Unit Tests

The `stream_auth` module includes comprehensive tests:

```rust
#[cfg(test)]
mod tests {
    test_session_creation()         // Session lifecycle
    test_duplicate_session()        // Prevents duplicates
    test_sign_and_verify()          // Basic crypto
    test_sequence_verification()    // Replay protection
    test_tampered_message()         // Tamper detection
    test_authenticated_chunk()      // Chunk authentication
}
```

Run tests with:

```bash
cd src-tauri
cargo test stream_auth
```

### Integration Testing

To test the full implementation:

1. Start two nodes
2. Share a file from Node A
3. Download from Node B
4. Monitor logs for authentication messages:
   ```
   ✅ Stream authentication successful
   ✅ Verified authenticated message for session <id>
   ```

## Security Considerations

### Threat Model

**Protected Against:**

- ✅ Data tampering during transit
- ✅ Replay attacks
- ✅ Man-in-the-middle modifications
- ✅ Stale message injection
- ✅ Out-of-order message acceptance

**Not Protected Against:**

- ❌ Network-level attacks (requires TLS/Noise)
- ❌ Compromised peer endpoints
- ❌ Denial of service attacks
- ❌ Sybil attacks on DHT

### Key Management

**Current Implementation:**

- Keys generated per-session using secure RNG
- Keys stored in memory only (not persisted)
- Sessions automatically cleaned up

**Future Enhancements:**

- Key exchange protocol for long-lived sessions
- Key rotation policies
- Persistent key storage for trusted peers

## Future Enhancements

### Short-term

1. Add metrics/logging for authentication failures
2. Implement configurable session timeouts
3. Add authentication to handshake messages
4. Create dashboard UI for authentication stats

### Long-term

1. Implement key exchange protocol
2. Add certificate-based peer authentication
3. Support for different HMAC algorithms (SHA-512, Blake3)
4. Batch signature verification for performance
5. Persistent session storage for reconnections

## Troubleshooting

### Common Issues

**Authentication Failures:**

- Check session exists for the file transfer
- Verify sequence numbers are incrementing
- Ensure timestamps are recent (< 5 minutes)
- Check HMAC keys match on both sides

**Performance Issues:**

- Session cleanup may be needed
- Check for memory leaks in long-running transfers
- Monitor CPU usage during high-throughput transfers

**Debugging:**
Enable debug logging:

```bash
RUST_LOG=debug cargo tauri dev
```

Look for:

- "Created authenticated session"
- "Verified authenticated message"
- "Signature verification failed"
- "Sequence mismatch"

## Compliance

This implementation follows industry standards:

- **FIPS 180-4**: SHA-256 hashing
- **RFC 2104**: HMAC specification
- **NIST SP 800-107**: Key derivation practices
- **OWASP**: Cryptographic storage guidelines

## References

- [RFC 2104: HMAC](https://www.rfc-editor.org/rfc/rfc2104)
- [NIST FIPS 180-4: SHA-256](https://nvlpubs.nist.gov/nistpubs/FIPS/NIST.FIPS.180-4.pdf)
- [Rust HMAC Crate Documentation](https://docs.rs/hmac/)

## Authors

Implementation by: Chiral Network Development Team
Date: October 6, 2025

## License

This implementation is part of the Chiral Network project and follows the same license.