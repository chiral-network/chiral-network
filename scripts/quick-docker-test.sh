#!/bin/bash
# Quick Docker E2E Test - One command to run everything

set -e

echo "🚀 Chiral Network - Quick Docker Test"
echo "======================================"
echo ""

# Check if Docker is running
if ! docker info > /dev/null 2>&1; then
    echo "❌ Docker is not running. Please start Docker first."
    exit 1
fi

# Build image
echo "📦 Building Docker image..."
docker build -t chiral-network:test -q . || {
    echo "❌ Build failed"
    exit 1
}
echo "✅ Image built"

# Start network
echo ""
echo "🌐 Starting test network..."
docker-compose -f docker-compose.test.yml up -d

# Wait for initialization
echo "⏳ Waiting for nodes to initialize (15s)..."
sleep 15

# Get peer IDs
echo ""
echo "🔍 Peer IDs:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

BOOTSTRAP_ID=$(docker-compose -f docker-compose.test.yml logs bootstrap 2>&1 | grep -oP 'Local peer id: \K\w+' | head -1)
SEEDER_ID=$(docker-compose -f docker-compose.test.yml logs seeder 2>&1 | grep -oP 'Local peer id: \K\w+' | head -1)
DOWNLOADER_ID=$(docker-compose -f docker-compose.test.yml logs downloader 2>&1 | grep -oP 'Local peer id: \K\w+' | head -1)

echo "Bootstrap:   ${BOOTSTRAP_ID:-Not found}"
echo "Seeder:      ${SEEDER_ID:-Not found}"
echo "Downloader:  ${DOWNLOADER_ID:-Not found}"

# Check connectivity
echo ""
echo "📊 Network Status:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

BOOTSTRAP_PEERS=$(docker-compose -f docker-compose.test.yml logs bootstrap 2>&1 | grep -c "ConnectionEstablished" || echo "0")
SEEDER_PEERS=$(docker-compose -f docker-compose.test.yml logs seeder 2>&1 | grep -c "ConnectionEstablished" || echo "0")
DOWNLOADER_PEERS=$(docker-compose -f docker-compose.test.yml logs downloader 2>&1 | grep -c "ConnectionEstablished" || echo "0")

echo "Bootstrap connections:   $BOOTSTRAP_PEERS"
echo "Seeder connections:      $SEEDER_PEERS"
echo "Downloader connections:  $DOWNLOADER_PEERS"

echo ""
if [ "$BOOTSTRAP_PEERS" -gt "0" ] && [ "$SEEDER_PEERS" -gt "0" ] && [ "$DOWNLOADER_PEERS" -gt "0" ]; then
    echo "✅ Network appears healthy!"
else
    echo "⚠️  Some nodes may not be connected. Check logs for details."
fi

# Show next steps
echo ""
echo "📝 Next Steps:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "1. View logs:      docker-compose -f docker-compose.test.yml logs -f"
echo "2. Stop network:   docker-compose -f docker-compose.test.yml down"
echo "3. Clean all:      docker-compose -f docker-compose.test.yml down -v"
echo ""
echo "⚠️  Reminder: Docker testing has limitations for NAT traversal"
echo "    See DOCKER_TESTING.md for details"
echo "    For real testing, use: cargo test --test e2e_cross_network_transfer_test"
echo ""

# Ask if user wants to follow logs
read -p "Follow logs? (y/n): " -n 1 -r
echo
if [[ $REPLY =~ ^[Yy]$ ]]; then
    docker-compose -f docker-compose.test.yml logs -f
fi
