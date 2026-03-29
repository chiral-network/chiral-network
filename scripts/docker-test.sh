#!/usr/bin/env bash
# =============================================================================
# Chiral Network — Docker-based scaled testing
#
# Usage:
#   ./scripts/docker-test.sh              # Run with 3 nodes (default)
#   ./scripts/docker-test.sh 10           # Run with 10 nodes
#   ./scripts/docker-test.sh 20 --build   # Rebuild images and run with 20 nodes
# =============================================================================

set -euo pipefail

NODE_COUNT="${1:-3}"
EXTRA_ARGS="${@:2}"

echo "=== Chiral Network Scaled Test ==="
echo "Nodes: $NODE_COUNT"
echo ""

# Build images
echo "[1/5] Building Docker images..."
docker compose build $EXTRA_ARGS

# Start relay first and wait for it
echo "[2/5] Starting relay server..."
docker compose up -d relay
echo "  Waiting for relay to be ready..."
for i in $(seq 1 30); do
    if docker compose exec relay curl -sf http://localhost:8080/api/ratings/batch -X POST -H 'Content-Type: application/json' -d '{"wallets":[]}' > /dev/null 2>&1; then
        echo "  Relay is ready!"
        break
    fi
    if [ "$i" = "30" ]; then
        echo "  ERROR: Relay failed to start"
        docker compose logs relay
        exit 1
    fi
    sleep 1
done

# Start seeder node
echo "[3/5] Starting seeder node..."
docker compose up -d seeder
sleep 3

# Scale up test nodes
echo "[4/5] Starting $NODE_COUNT test nodes..."
docker compose up -d --scale node=$NODE_COUNT
sleep 5

# Show status
echo ""
echo "=== Network Status ==="
docker compose ps
echo ""

# Run test suite
echo "[5/5] Running load tests..."
docker compose --profile test up test-runner --abort-on-container-exit

# Collect results
EXIT_CODE=$?
echo ""
echo "=== Test Complete ==="
echo "Exit code: $EXIT_CODE"

# Show logs if failed
if [ $EXIT_CODE -ne 0 ]; then
    echo ""
    echo "=== Failed container logs ==="
    docker compose logs --tail=50
fi

echo ""
echo "To inspect: docker compose exec node chiral daemon status"
echo "To tear down: docker compose down -v"

exit $EXIT_CODE
