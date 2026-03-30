#!/usr/bin/env bash
# =============================================================================
# Chiral Network — Local Test Cluster (no Docker)
#
# Runs N headless daemon instances directly on the host.
#
# Usage:
#   ./scripts/local-test-cluster.sh start 10    # Start 10 nodes
#   ./scripts/local-test-cluster.sh status       # Check status
#   ./scripts/local-test-cluster.sh stop         # Stop all nodes
# =============================================================================
set -euo pipefail

DAEMON_BIN="src-tauri/target/release/chiral_daemon"
CLI_BIN="src-tauri/target/release/chiral"
BASE_PORT=9420
DATA_BASE="/tmp/chiral-test-cluster"
LOG_DIR="$DATA_BASE/logs"
NODE_COUNT="${2:-10}"
ACTION="${1:-status}"

if [[ ! -f "$DAEMON_BIN" ]]; then
    echo "Building binaries..."
    cargo build --manifest-path src-tauri/Cargo.toml --release --bin chiral_daemon --bin chiral
fi

start_nodes() {
    echo "Starting $NODE_COUNT test nodes..."
    mkdir -p "$LOG_DIR"

    for i in $(seq 1 "$NODE_COUNT"); do
        local port=$((BASE_PORT + i))
        local data_dir="$DATA_BASE/node-$i"
        local pid_file="$data_dir/daemon.pid"
        local log_file="$LOG_DIR/node-$i.log"

        # Skip if already running
        if [[ -f "$pid_file" ]] && kill -0 "$(cat "$pid_file")" 2>/dev/null; then
            echo "  Node $i already running (port $port, PID $(cat "$pid_file"))"
            continue
        fi

        mkdir -p "$data_dir/chiral-network"

        XDG_DATA_HOME="$data_dir" "$DAEMON_BIN" \
            --port "$port" \
            --pid-file "$pid_file" \
            --auto-start-dht \
            > "$log_file" 2>&1 &

        local pid=$!
        echo "$pid" > "$pid_file"
        echo "  Node $i started: port=$port PID=$pid data=$data_dir"
    done

    echo ""
    echo "Waiting for nodes to be ready..."
    sleep 3

    local healthy=0
    for i in $(seq 1 "$NODE_COUNT"); do
        local port=$((BASE_PORT + i))
        if curl -sf "http://localhost:$port/api/health" > /dev/null 2>&1; then
            healthy=$((healthy + 1))
        fi
    done

    echo "  $healthy/$NODE_COUNT nodes healthy"
    echo ""
    echo "Node ports: $((BASE_PORT + 1)) - $((BASE_PORT + NODE_COUNT))"
    echo "Logs: $LOG_DIR/"
    echo ""
    echo "Test commands:"
    echo "  curl http://localhost:$((BASE_PORT + 1))/api/health"
    echo "  curl http://localhost:$((BASE_PORT + 1))/api/headless/runtime"
    echo "  curl http://localhost:$((BASE_PORT + 1))/api/headless/dht/peers"
    echo ""
    echo "Stop with: $0 stop"
}

stop_nodes() {
    echo "Stopping all test nodes..."
    local stopped=0

    for pid_file in "$DATA_BASE"/node-*/daemon.pid; do
        if [[ -f "$pid_file" ]]; then
            local pid
            pid=$(cat "$pid_file")
            if kill -0 "$pid" 2>/dev/null; then
                kill "$pid" 2>/dev/null || true
                stopped=$((stopped + 1))
            fi
            rm -f "$pid_file"
        fi
    done

    echo "  Stopped $stopped nodes"
    echo ""
    echo "To clean up data: rm -rf $DATA_BASE"
}

show_status() {
    echo "=== Chiral Test Cluster Status ==="
    echo ""

    local running=0
    local total=0

    for pid_file in "$DATA_BASE"/node-*/daemon.pid; do
        if [[ -f "$pid_file" ]]; then
            total=$((total + 1))
            local pid
            pid=$(cat "$pid_file")
            local node_name
            node_name=$(basename "$(dirname "$pid_file")")
            local node_num="${node_name#node-}"
            local port=$((BASE_PORT + node_num))

            if kill -0 "$pid" 2>/dev/null; then
                local health="DOWN"
                if curl -sf "http://localhost:$port/api/health" > /dev/null 2>&1; then
                    health="HEALTHY"
                fi

                local dht_info
                dht_info=$(curl -sf "http://localhost:$port/api/headless/runtime" 2>/dev/null || echo '{}')
                local dht_running
                dht_running=$(echo "$dht_info" | grep -o '"dhtRunning":true' | head -1)
                local peer_id
                peer_id=$(echo "$dht_info" | grep -o '"peerId":"[^"]*"' | head -1 | cut -d'"' -f4 | head -c20)

                printf "  %-8s port=%-5s PID=%-7s %s  DHT=%s  peer=%s...\n" \
                    "$node_name" "$port" "$pid" "$health" \
                    "${dht_running:+ON}" "${peer_id:-?}"
                running=$((running + 1))
            else
                printf "  %-8s STOPPED (stale PID file)\n" "$node_name"
            fi
        fi
    done

    if [[ $total -eq 0 ]]; then
        echo "  No nodes found. Start with: $0 start 10"
    else
        echo ""
        echo "  $running/$total nodes running"
    fi
}

case "$ACTION" in
    start)  start_nodes ;;
    stop)   stop_nodes ;;
    status) show_status ;;
    restart)
        stop_nodes
        sleep 2
        start_nodes
        ;;
    *)
        echo "Usage: $0 {start|stop|status|restart} [node_count]"
        exit 1
        ;;
esac
