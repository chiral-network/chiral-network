# Download Retry Mechanism

## Purpose

Implement automatic retry logic for failed chunk downloads to improve the reliability and robustness of file transfers in the Chiral Network. Currently, when a chunk download fails (due to network issues, peer disconnection, or timeout), the entire download fails permanently, forcing users to manually restart. This feature will automatically retry failed chunks, switch to alternative peers, and recover from transient network failures.

## Problem Statement

### Current Behavior
- When a chunk download fails, the entire file download fails immediately
- No automatic recovery from temporary network issues
- No fallback to alternative peers when one peer becomes unavailable
- Poor user experience - manual intervention required for every network hiccup
- No distinction between recoverable errors (timeout) and permanent errors (file not found)

### Impact
- Downloads fail frequently in unstable network conditions
- Users must manually restart downloads, losing progress
- Multi-source downloads don't fully leverage available peers
- Wastes bandwidth re-downloading chunks that were already successful

## Proposed Solution

Implement a multi-layered retry mechanism with:

1. **Chunk-level retries**: Retry individual failed chunks up to N times
2. **Exponential backoff**: Increase delay between retries to avoid overwhelming failing peers
3. **Peer switching**: Automatically switch to alternative peers after repeated failures
4. **Partial progress preservation**: Track successfully downloaded chunks to avoid re-downloading
5. **Error categorization**: Distinguish between transient (retry) and permanent (fail) errors

## Plan

### Architecture

```
Download Flow with Retry:
1. Request chunk from Peer A
2. If success → Mark chunk complete
3. If failure → Categorize error
   - Transient (timeout, connection lost) → Retry
   - Permanent (file not found, corrupt) → Fail
4. On transient failure:
   - Retry #1: Wait 1s, retry same peer
   - Retry #2: Wait 2s, retry same peer
   - Retry #3: Wait 4s, switch to different peer
   - After 3 failures: Mark download as failed
5. Track retry count per chunk
6. If all chunks complete or max retries exceeded → Complete/Fail download
```

### Key Components

#### 1. Retry State Tracking
```rust
struct ChunkRetryState {
    chunk_index: u32,
    attempt_count: u32,
    last_attempt: Instant,
    last_peer: String,
    last_error: Option<String>,
}
```

#### 2. Error Classification
- **Transient Errors** (retry): Network timeout, connection reset, peer busy
- **Permanent Errors** (fail immediately): File not found, checksum mismatch, invalid chunk

#### 3. Retry Policy
- **Max attempts per chunk**: 3
- **Backoff strategy**: Exponential (1s, 2s, 4s)
- **Peer switching**: After 2 failures on same peer
- **Total timeout**: 5 minutes per chunk

#### 4. Peer Fallback Strategy
- Maintain list of available peers for each file
- On retry, select next best peer using peer selection service
- Skip peers that have failed repeatedly
- Update peer reputation on successful/failed retries

## Implementation Details

### Phase 1: Core Retry Logic (Priority 1)

**File**: `src-tauri/src/multi_source_download.rs`

**Changes**:
1. Add `ChunkRetryState` struct to track retry attempts
2. Modify `download_chunk_from_peer()` to return detailed error types
3. Implement `retry_failed_chunk()` function with exponential backoff
4. Add retry state to `MultiSourceDownload` struct
5. Implement error categorization logic

**New Functions**:
```rust
// Classify error as transient or permanent
fn is_transient_error(error: &str) -> bool;

// Calculate backoff delay based on attempt count
fn calculate_backoff_delay(attempt: u32) -> Duration;

// Retry a failed chunk with backoff and peer switching
async fn retry_chunk_download(
    chunk_index: u32,
    retry_state: &mut ChunkRetryState,
    available_peers: &[String],
) -> Result<Vec<u8>, String>;

// Select alternative peer for retry
fn select_retry_peer(
    exclude_peers: &[String],
    available_peers: &[String],
) -> Option<String>;
```

### Phase 2: Peer Management (Priority 2)

**Files**: 
- `src-tauri/src/multi_source_download.rs`
- `src-tauri/src/peer_selection.rs`

**Changes**:
1. Track peer failure count
2. Temporarily blacklist failing peers (5 minute cooldown)
3. Update peer reputation based on retry success/failure
4. Prefer peers with fewer recent failures

### Phase 3: Progress Persistence (Priority 3)

**Files**:
- `src-tauri/src/multi_source_download.rs`
- `src-tauri/src/file_transfer.rs`

**Changes**:
1. Save download progress to disk periodically
2. Resume interrupted downloads from last checkpoint
3. Track which chunks are complete vs. in-progress vs. failed

### Phase 4: UI Feedback (Priority 4)

**Files**:
- `src/pages/Download.svelte`
- Backend events from `multi_source_download.rs`

**Changes**:
1. Emit retry events to frontend
2. Display retry count in download progress
3. Show "Retrying chunk X (attempt Y/3)" status
4. Display peer switches

## Configuration

Add to application settings (`AppState` or config file):

```rust
struct RetryConfig {
    max_chunk_retries: u32,           // Default: 3
    initial_backoff_ms: u64,          // Default: 1000 (1 second)
    backoff_multiplier: u32,          // Default: 2 (exponential)
    max_backoff_ms: u64,              // Default: 8000 (8 seconds)
    peer_failure_threshold: u32,      // Default: 3 (blacklist after 3 failures)
    peer_blacklist_duration_secs: u64, // Default: 300 (5 minutes)
    chunk_timeout_secs: u64,          // Default: 60
}
```

## Error Handling

### Transient Errors (Retry)
- Network timeout
- Connection reset by peer
- Peer temporarily unavailable
- Rate limit exceeded

### Permanent Errors (Fail)
- File not found on any peer
- Chunk checksum verification failed
- Invalid chunk index
- Peer explicitly rejected request
- All retry attempts exhausted

## Success Metrics

1. **Reduced download failure rate**: Track % of downloads that complete successfully
2. **Improved resilience**: Measure recovery from transient network failures
3. **Bandwidth efficiency**: Ensure retries don't waste excessive bandwidth
4. **User satisfaction**: Fewer manual restarts required

## Testing Strategy

### Unit Tests
- Test exponential backoff calculation
- Test error classification (transient vs permanent)
- Test peer selection for retries
- Test retry state tracking

### Integration Tests
- Simulate network timeouts and verify retries
- Simulate peer disconnection and verify peer switching
- Test with multiple failing peers
- Test max retry limit enforcement

### Manual Testing
- Download large files over unstable network
- Kill peers mid-download and verify recovery
- Test with slow/fast peers mixed
- Verify progress preservation across app restarts

## Timeline

- **Documentation & Approval**: Week 9 (current)
- **Phase 1 (Core Retry)**: Week 10 (3-4 hours)
- **Phase 2 (Peer Management)**: Week 10 (2-3 hours)
- **Phase 3 (Progress Persistence)**: Week 11 (4-5 hours)
- **Phase 4 (UI Feedback)**: Week 11 (2-3 hours)
- **Testing & Refinement**: Week 12

**Total estimated effort**: 12-16 hours

## Dependencies

- Existing peer selection service (`src-tauri/src/peer_selection.rs`)
- Multi-source download infrastructure (`src-tauri/src/multi_source_download.rs`)
- WebRTC service for peer communication (`src-tauri/src/webrtc_service.rs`)
- DHT service for peer discovery (`src-tauri/src/dht.rs`)

## Future Enhancements

1. **Adaptive retry policy**: Adjust retry count based on network conditions
2. **Smart peer selection**: Learn which peers are most reliable over time
3. **Parallel retries**: Try multiple peers simultaneously for critical chunks
4. **User notifications**: Alert user when downloads are recovering from failures
5. **Analytics**: Track retry statistics for network health monitoring

## Compatibility

- **Backward compatible**: Existing downloads without retry will continue to work
- **Protocol changes**: None required (retry is client-side only)
- **UI changes**: Optional status display, no breaking changes

## Security Considerations

- Prevent retry amplification attacks (limit total retries across all chunks)
- Validate chunk checksums even on retries
- Rate limit requests to prevent peer abuse
- Don't retry on authentication failures (could indicate malicious peer)

## Alternative Approaches Considered

1. **Restart entire download on any failure**: Simpler but wastes bandwidth
2. **Manual retry only**: Simpler but poor UX
3. **Infinite retries**: Could hang forever on permanent failures
4. **No backoff**: Could overwhelm failing peers

**Chosen approach balances reliability, efficiency, and user experience.**

---

## References

- Multi-source download implementation: `src-tauri/src/multi_source_download.rs`
- Peer selection strategies: `src-tauri/src/peer_selection.rs`
- WebRTC chunk transfer: `src-tauri/src/webrtc_service.rs`
- Current roadmap: `docs/roadmap.md` (Phase 3)
