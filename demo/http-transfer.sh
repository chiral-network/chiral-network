#!/usr/bin/env bash
#
# HTTP Transfer Demo Script
# Tests the pause/resume download functionality
#
# This script demonstrates:
# 1. Node A (seeder) starts HTTP server
# 2. Node B (downloader) starts download
# 3. Download pauses at ~50%
# 4. Node B restarts
# 5. Download resumes from saved offset
# 6. Final SHA-256 verification
#

set -e  # Exit on error

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
DEMO_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$DEMO_DIR")"
TEST_FILE="$DEMO_DIR/test-file.bin"
TEST_FILE_SIZE=$((10 * 1024 * 1024))  # 10 MB
NODE_A_PORT=8080
NODE_B_DIR="$DEMO_DIR/node-b-downloads"
EXPECTED_SHA256=""

log() {
    echo -e "${BLUE}[DEMO]${NC} $1"
}

success() {
    echo -e "${GREEN}[OK]${NC} $1"
}

error() {
    echo -e "${RED}[X]${NC} $1"
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

# Cleanup function
cleanup() {
    log "Cleaning up..."

    # Kill Node A (HTTP server)
    if [ -n "$NODE_A_PID" ] && kill -0 "$NODE_A_PID" 2>/dev/null; then
        kill "$NODE_A_PID" 2>/dev/null || true
    fi

    # Also kill any other processes on port 8080
    lsof -ti :$NODE_A_PORT 2>/dev/null | xargs kill 2>/dev/null || true

    # Optional: Clean up test files (comment out to preserve for inspection)
    # rm -rf "$TEST_FILE" "$NODE_B_DIR"

    success "Cleanup complete"
}

trap cleanup EXIT

# Pre-cleanup: Kill any existing processes on port 8080
log "Checking for existing processes on port $NODE_A_PORT..."
if lsof -ti :$NODE_A_PORT >/dev/null 2>&1; then
    warn "Port $NODE_A_PORT is in use. Killing existing processes..."
    lsof -ti :$NODE_A_PORT | xargs kill 2>/dev/null || true
    sleep 1
    success "Port $NODE_A_PORT is now free"
fi

# Step 1: Create test file
log "Step 1: Creating test file ($TEST_FILE_SIZE bytes)..."
mkdir -p "$DEMO_DIR"
dd if=/dev/urandom of="$TEST_FILE" bs=1024 count=$((TEST_FILE_SIZE / 1024)) 2>/dev/null
EXPECTED_SHA256=$(shasum -a 256 "$TEST_FILE" | awk '{print $1}')
success "Test file created: $TEST_FILE"
log "Expected SHA-256: $EXPECTED_SHA256"

# Step 2: Start Node A (HTTP seeder)
log "Step 2: Starting Node A HTTP server on port $NODE_A_PORT..."

# Simple Python HTTP server with Range support
python3 -m http.server $NODE_A_PORT --directory "$DEMO_DIR" &
NODE_A_PID=$!

sleep 5

if ! kill -0 "$NODE_A_PID" 2>/dev/null; then
    error "Failed to start HTTP server"
    exit 1
fi

# Wait for server to be ready (health check with extended timeout for CI)
log "Waiting for HTTP server to be ready..."
MAX_RETRIES=30
RETRY_COUNT=0
while [ $RETRY_COUNT -lt $MAX_RETRIES ]; do
    if curl -s -f -m 5 "http://localhost:$NODE_A_PORT/test-file.bin" -o /dev/null 2>/dev/null; then
        break
    fi
    RETRY_COUNT=$((RETRY_COUNT + 1))
    if [ $((RETRY_COUNT % 5)) -eq 0 ]; then
        log "Still waiting... (attempt $RETRY_COUNT/$MAX_RETRIES)"
    fi
    sleep 1
done

if [ $RETRY_COUNT -eq $MAX_RETRIES ]; then
    error "HTTP server failed to respond after $MAX_RETRIES attempts"
    exit 1
fi

success "Node A HTTP server running (PID: $NODE_A_PID)"
log "URL: http://localhost:$NODE_A_PORT/test-file.bin"

# Step 3: Start Node B (downloader)
log "Step 3: Starting Node B downloader..."
mkdir -p "$NODE_B_DIR"

DOWNLOAD_URL="http://localhost:$NODE_A_PORT/test-file.bin"
DOWNLOAD_DEST="$NODE_B_DIR/test-file.bin"

# For now, we'll use a simple curl-based download simulation
# In a real scenario, this would invoke the Chiral Network app
log "Simulating download start..."

# Download first 50% synchronously (not in background)
cd "$NODE_B_DIR"
HALF_SIZE=$((TEST_FILE_SIZE / 2))

# Use curl with Range header and check the response
log "Requesting bytes 0-$((HALF_SIZE - 1))..."
HTTP_CODE=$(curl -s -w "%{http_code}" -r "0-$((HALF_SIZE - 1))" "$DOWNLOAD_URL" -o "$DOWNLOAD_DEST.part")

if [ "$HTTP_CODE" = "206" ]; then
    success "Server supports Range requests (HTTP 206)"
elif [ "$HTTP_CODE" = "200" ]; then
    warn "Server returned HTTP 200 (full file). Truncating to 50%..."
    # Truncate the file to 50%
    dd if="$DOWNLOAD_DEST.part" of="$DOWNLOAD_DEST.part.tmp" bs=1 count=$HALF_SIZE 2>/dev/null
    mv "$DOWNLOAD_DEST.part.tmp" "$DOWNLOAD_DEST.part"
fi

# Create metadata file
cat > "$DOWNLOAD_DEST.meta.json" <<EOF
{
  "version": 1,
  "download_id": "demo-download-123",
  "url": "$DOWNLOAD_URL",
  "etag": null,
  "expected_size": $TEST_FILE_SIZE,
  "bytes_downloaded": $HALF_SIZE,
  "last_modified": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "sha256_final": null
}
EOF

ACTUAL_SIZE=$(stat -f%z "$DOWNLOAD_DEST.part" 2>/dev/null || stat -c%s "$DOWNLOAD_DEST.part" 2>/dev/null || echo "0")
success "Download started and paused at ~50%"
log "Downloaded: $ACTUAL_SIZE bytes (expected: $HALF_SIZE)"

# Verify we got the right amount
if [ "$ACTUAL_SIZE" -ne "$HALF_SIZE" ]; then
    warn "Downloaded size ($ACTUAL_SIZE) doesn't match expected ($HALF_SIZE)"
fi

cd "$DEMO_DIR"

# Step 4: Simulate Node B restart
log "Step 4: Simulating Node B restart..."
sleep 1
success "Node B stopped (simulated restart)"

# Step 5: Resume download
log "Step 5: Resuming download from saved offset..."

# Read metadata
BYTES_DOWNLOADED=$(grep -o '"bytes_downloaded": [0-9]*' "$DOWNLOAD_DEST.meta.json" | grep -o '[0-9]*')
log "Resuming from byte offset: $BYTES_DOWNLOADED"

# Resume download from offset
cd "$NODE_B_DIR"
HTTP_CODE=$(curl -s -w "%{http_code}" -r "$BYTES_DOWNLOADED-" "$DOWNLOAD_URL" -o "$DOWNLOAD_DEST.part.tmp")

if [ "$HTTP_CODE" = "206" ]; then
    # Server supports resume, append the partial content
    cat "$DOWNLOAD_DEST.part.tmp" >> "$DOWNLOAD_DEST.part"
    rm "$DOWNLOAD_DEST.part.tmp"
    success "Download resumed with Range support (HTTP 206)"
elif [ "$HTTP_CODE" = "200" ]; then
    # Server doesn't support resume, extract the remaining bytes
    warn "Server returned HTTP 200 (full file). Extracting remaining bytes..."
    dd if="$DOWNLOAD_DEST.part.tmp" of="$DOWNLOAD_DEST.part.tmp2" bs=1 skip=$BYTES_DOWNLOADED 2>/dev/null
    cat "$DOWNLOAD_DEST.part.tmp2" >> "$DOWNLOAD_DEST.part"
    rm "$DOWNLOAD_DEST.part.tmp" "$DOWNLOAD_DEST.part.tmp2"
    success "Download resumed (extracted remaining bytes)"
fi

FINAL_SIZE=$(stat -f%z "$DOWNLOAD_DEST.part" 2>/dev/null || stat -c%s "$DOWNLOAD_DEST.part")
log "Final size: $FINAL_SIZE bytes (expected: $TEST_FILE_SIZE)"

cd "$DEMO_DIR"
success "Download resumed and completed"

# Step 6: Finalize and verify
log "Step 6: Finalizing download and verifying SHA-256..."

# Move .part to final destination
mv "$DOWNLOAD_DEST.part" "$DOWNLOAD_DEST"

# Verify hash
ACTUAL_SHA256=$(shasum -a 256 "$DOWNLOAD_DEST" | awk '{print $1}')

if [ "$EXPECTED_SHA256" = "$ACTUAL_SHA256" ]; then
    success "SHA-256 verification PASSED"
    log "Expected: $EXPECTED_SHA256"
    log "Actual:   $ACTUAL_SHA256"
else
    error "SHA-256 verification FAILED"
    log "Expected: $EXPECTED_SHA256"
    log "Actual:   $ACTUAL_SHA256"
    exit 1
fi

# Final summary
echo ""
echo -e "${GREEN}╔════════════════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║                   DEMO SUCCESSFUL                          ║${NC}"
echo -e "${GREEN}╚════════════════════════════════════════════════════════════╝${NC}"
echo ""
log "Summary:"
log "  • Test file size: $TEST_FILE_SIZE bytes"
log "  • Download paused at: ~50%"
log "  • Resume successful: Yes"
log "  • Final hash match: Yes"
log "  • Downloaded file: $DOWNLOAD_DEST"
echo ""

# Demo complete - cleanup will run via trap
success "Demo completed successfully. Cleaning up..."
