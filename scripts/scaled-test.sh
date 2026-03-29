#!/usr/bin/env bash
# =============================================================================
# Chiral Network — Scaled Integration Test
#
# Spins up N headless daemon nodes and exercises all features end-to-end.
#
# Usage:
#   ./scripts/scaled-test.sh              # 10 nodes (default)
#   ./scripts/scaled-test.sh 50           # 50 nodes
#   ./scripts/scaled-test.sh 20 --build   # Rebuild images, 20 nodes
#   ./scripts/scaled-test.sh 10 --keep    # Don't tear down after test
# =============================================================================

set -euo pipefail

NODE_COUNT="${1:-10}"
REBUILD=false
KEEP=false

for arg in "${@:2}"; do
    case "$arg" in
        --build) REBUILD=true ;;
        --keep) KEEP=true ;;
    esac
done

COMPOSE_FILES="-f docker-compose.yml -f docker-compose.scaled-test.yml"
DC="docker compose $COMPOSE_FILES"

echo "=============================================="
echo "  Chiral Network Scaled Integration Test"
echo "  Nodes: $NODE_COUNT"
echo "=============================================="
echo ""

# ------------------------------------------------------------------
# Step 1: Build images
# ------------------------------------------------------------------
if $REBUILD; then
    echo "[1/6] Building Docker images..."
    $DC build
else
    echo "[1/6] Building Docker images (cached)..."
    $DC build --pull never 2>/dev/null || $DC build
fi

# ------------------------------------------------------------------
# Step 2: Start relay
# ------------------------------------------------------------------
echo "[2/6] Starting relay server..."
$DC up -d relay

echo "  Waiting for relay..."
for i in $(seq 1 60); do
    if $DC exec -T relay curl -sf http://localhost:8080/api/ratings/batch \
        -X POST -H 'Content-Type: application/json' -d '{"wallets":[]}' > /dev/null 2>&1; then
        echo "  Relay is ready!"
        break
    fi
    if [ "$i" = "60" ]; then
        echo "  ERROR: Relay failed to start after 60s"
        $DC logs relay | tail -20
        $DC down -v
        exit 1
    fi
    sleep 1
done

# ------------------------------------------------------------------
# Step 3: Start nodes
# ------------------------------------------------------------------
echo "[3/6] Starting $NODE_COUNT test nodes..."
$DC up -d --scale node=$NODE_COUNT
sleep 2

# ------------------------------------------------------------------
# Step 4: Wait for all nodes to be healthy
# ------------------------------------------------------------------
echo "[4/6] Waiting for nodes to be healthy..."
NODE_NAMES=$($DC ps node --format '{{.Name}}' 2>/dev/null | sort)

if [ -z "$NODE_NAMES" ]; then
    echo "  ERROR: No node containers found"
    $DC down -v
    exit 1
fi

HEALTHY=0
TOTAL=$(echo "$NODE_NAMES" | wc -l | tr -d ' ')

for attempt in $(seq 1 90); do
    HEALTHY=0
    while IFS= read -r name; do
        if docker exec "$name" curl -sf http://localhost:9419/api/health > /dev/null 2>&1; then
            HEALTHY=$((HEALTHY + 1))
        fi
    done <<< "$NODE_NAMES"

    echo "  Health: $HEALTHY/$TOTAL nodes ready (attempt $attempt/90)"

    if [ "$HEALTHY" = "$TOTAL" ]; then
        echo "  All nodes healthy!"
        break
    fi

    if [ "$attempt" = "90" ]; then
        echo "  WARNING: Only $HEALTHY/$TOTAL nodes healthy after 90s, proceeding anyway"
    fi

    sleep 1
done

# ------------------------------------------------------------------
# Step 5: Run test phases
# ------------------------------------------------------------------
echo "[5/6] Running scaled integration tests..."
echo ""

NODE_LIST=$(echo "$NODE_NAMES" | tr '\n' ' ')

$DC run --rm \
    -e NODE_LIST="$NODE_LIST" \
    -e NODE_COUNT="$TOTAL" \
    -e RELAY_URL="http://relay.chiral.local:8080" \
    scaled-test-runner

TEST_EXIT=$?

# ------------------------------------------------------------------
# Step 6: Results
# ------------------------------------------------------------------
echo ""
echo "=============================================="
if [ $TEST_EXIT -eq 0 ]; then
    echo "  ALL TESTS PASSED"
else
    echo "  SOME TESTS FAILED (exit code: $TEST_EXIT)"
    echo ""
    echo "  To inspect:"
    echo "    $DC logs node | tail -100"
    echo "    $DC exec <node-name> chiral daemon status --port 9419"
fi
echo "=============================================="

# Tear down unless --keep
if ! $KEEP; then
    echo ""
    echo "Tearing down..."
    $DC down -v
else
    echo ""
    echo "Containers kept running. Tear down with:"
    echo "  $DC down -v"
fi

exit $TEST_EXIT
