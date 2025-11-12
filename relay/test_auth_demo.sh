#!/bin/bash
# Demonstration script for relay authentication

set -e

RELAY_PORT=4002
RELAY_PID_FILE="/tmp/chiral-relay-test.pid"
RELAY_LOG="/tmp/chiral-relay-test.log"

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo "═══════════════════════════════════════"
echo "  Relay Authentication Test"
echo "═══════════════════════════════════════"
echo ""

# Cleanup function
cleanup() {
    if [ -f "$RELAY_PID_FILE" ]; then
        RELAY_PID=$(cat "$RELAY_PID_FILE")
        if ps -p "$RELAY_PID" > /dev/null 2>&1; then
            kill "$RELAY_PID" 2>/dev/null || true
            wait "$RELAY_PID" 2>/dev/null || true
        fi
        rm -f "$RELAY_PID_FILE"
    fi
}
trap cleanup EXIT

# Build
echo "Building..."
cd "$(dirname "$0")"
cargo build --release --bin chiral-relay --bin test_client > /dev/null 2>&1
echo "✓ Build complete"
echo ""

# Start relay
echo "Starting relay server..."
./target/release/chiral-relay \
    --port "$RELAY_PORT" \
    --external-address "/ip4/127.0.0.1/tcp/$RELAY_PORT" \
    --pid-file "$RELAY_PID_FILE" \
    > "$RELAY_LOG" 2>&1 &

RELAY_PID=$!
echo "$RELAY_PID" > "$RELAY_PID_FILE"
sleep 2

# Get relay peer ID
RELAY_PEER_ID=$(grep "Peer ID:" "$RELAY_LOG" | tail -1 | awk '{print $NF}')
RELAY_ADDR="/ip4/127.0.0.1/tcp/$RELAY_PORT/p2p/$RELAY_PEER_ID"
echo "✓ Relay started"
echo ""

# Test 1: Valid token
echo -n "Test 1: Valid token (mysecrettoken1) ... "
if ./target/release/test_client "$RELAY_ADDR" "mysecrettoken1" --timeout 10 > /dev/null 2>&1; then
    echo -e "${GREEN}✓ PASSED${NC}"
else
    echo -e "${RED}✗ FAILED${NC}"
    exit 1
fi

# Test 2: Invalid token
echo -n "Test 2: Invalid token (wrongtoken) ... "
if ./target/release/test_client "$RELAY_ADDR" "wrongtoken" --timeout 10 > /dev/null 2>&1; then
    echo -e "${RED}✗ FAILED (should reject)${NC}"
    exit 1
else
    echo -e "${GREEN}✓ PASSED (correctly rejected)${NC}"
fi

# Test 3: Another valid token
echo -n "Test 3: Valid token (mysecrettoken2) ... "
if ./target/release/test_client "$RELAY_ADDR" "mysecrettoken2" --timeout 10 > /dev/null 2>&1; then
    echo -e "${GREEN}✓ PASSED${NC}"
else
    echo -e "${RED}✗ FAILED${NC}"
    exit 1
fi

echo ""
echo "═══════════════════════════════════════"
echo -e "${GREEN}All tests passed!${NC}"
echo "═══════════════════════════════════════"

