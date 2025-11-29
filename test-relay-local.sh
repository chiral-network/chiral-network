#!/bin/bash
# Quick local relay testing script
# This runs 3 nodes locally to test relay functionality

set -e

echo "🧪 Chiral Network Local Relay Test"
echo "===================================="
echo ""
echo "This script will:"
echo "1. Start a bootstrap node with relay on port 4001"
echo "2. Wait for you to start 2 more nodes manually"
echo "3. You can test connections between them via relay"
echo ""

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Build if needed
if [ ! -f "src-tauri/target/release/chiral-network" ]; then
    echo "📦 Building Chiral Network..."
    cd src-tauri
    cargo build --release
    cd ..
fi

# Create empty dist if needed
if [ ! -d "dist" ]; then
    echo "Creating empty dist folder..."
    mkdir -p dist
fi

# Generate a test secret
SECRET="local-relay-test-secret-$(date +%s)"

echo ""
echo -e "${GREEN}Step 1: Starting Bootstrap Node with Relay${NC}"
echo "==========================================="
echo ""

cd src-tauri

# Start bootstrap in background and capture output
./target/release/chiral-network \
    --headless \
    --is-bootstrap \
    --dht-port 4001 \
    --secret "$SECRET" \
    --show-multiaddr \
    --show-reachability 2>&1 | tee /tmp/chiral-bootstrap.log &

BOOTSTRAP_PID=$!

echo "Bootstrap PID: $BOOTSTRAP_PID"
echo "Waiting for bootstrap to start..."
sleep 5

# Extract Peer ID from log
echo ""
echo "Extracting bootstrap Peer ID..."
PEER_ID=$(grep -oP '(?<=p2p/)[A-Za-z0-9]+' /tmp/chiral-bootstrap.log | head -1)

if [ -z "$PEER_ID" ]; then
    echo "❌ Could not extract Peer ID. Check /tmp/chiral-bootstrap.log"
    kill $BOOTSTRAP_PID 2>/dev/null || true
    exit 1
fi

echo -e "${GREEN}✓ Bootstrap node started${NC}"
echo ""
echo "Bootstrap Multiaddr:"
echo -e "${BLUE}/ip4/127.0.0.1/tcp/4001/p2p/$PEER_ID${NC}"
echo ""

# Check if relay is enabled
if grep -q "Relay server enabled" /tmp/chiral-bootstrap.log; then
    echo -e "${GREEN}✓ Relay server is enabled${NC}"
else
    echo -e "${YELLOW}⚠ Warning: Could not confirm relay server is enabled${NC}"
fi

echo ""
echo -e "${GREEN}Step 2: Start Additional Nodes${NC}"
echo "================================"
echo ""
echo "Open 2 more terminal windows and run these commands:"
echo ""
echo -e "${BLUE}Terminal 2 (Node on port 4002):${NC}"
echo "cd $(pwd)"
echo "./target/release/chiral-network \\"
echo "    --headless \\"
echo "    --dht-port 4002 \\"
echo "    --bootstrap \"/ip4/127.0.0.1/tcp/4001/p2p/$PEER_ID\" \\"
echo "    --show-multiaddr"
echo ""
echo -e "${BLUE}Terminal 3 (Node on port 4003):${NC}"
echo "cd $(pwd)"
echo "./target/release/chiral-network \\"
echo "    --headless \\"
echo "    --dht-port 4003 \\"
echo "    --bootstrap \"/ip4/127.0.0.1/tcp/4001/p2p/$PEER_ID\" \\"
echo "    --show-multiaddr"
echo ""
echo -e "${GREEN}Step 3: Test Relay Connection${NC}"
echo "=============================="
echo ""
echo "After starting nodes 2 and 3:"
echo "1. Note the Peer IDs from each node's output"
echo "2. Open the GUI application"
echo "3. Go to Network page"
echo "4. Try connecting node 2 to node 3 (or vice versa)"
echo "5. Check the logs for relay connection messages"
echo ""
echo "Expected log messages:"
echo "  - '🔍 Detected private IP address'"
echo "  - '🔄 Will attempt relay connection'"
echo "  - '✓ Relay connection requested successfully'"
echo ""
echo -e "${YELLOW}Bootstrap node is running (PID: $BOOTSTRAP_PID)${NC}"
echo "Press Ctrl+C to stop the bootstrap node"
echo ""
echo "Logs are in: /tmp/chiral-bootstrap.log"
echo ""

# Trap Ctrl+C to cleanup
trap "echo ''; echo 'Stopping bootstrap node...'; kill $BOOTSTRAP_PID 2>/dev/null || true; exit 0" INT

# Keep running
tail -f /tmp/chiral-bootstrap.log
