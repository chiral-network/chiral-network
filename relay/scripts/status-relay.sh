#!/usr/bin/env bash
# Chiral Network Relay - Status Check Script
# This script checks the status of a running relay daemon and displays metrics

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# Default configuration
RELAY_DIR="${RELAY_DIR:-$HOME/.chiral-relay}"
PID_FILE="${PID_FILE:-$RELAY_DIR/relay.pid}"
METRICS_FILE="${METRICS_FILE:-$RELAY_DIR/metrics.json}"
LOG_FILE="${LOG_FILE:-$RELAY_DIR/relay.log}"

# Print functions
print_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

print_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

print_header() {
    echo -e "${CYAN}$1${NC}"
}

print_field() {
    local key="$1"
    local value="$2"
    printf "  ${BLUE}%-25s${NC} %s\n" "$key:" "$value"
}

# Check if relay is running
check_if_running() {
    if [ ! -f "$PID_FILE" ]; then
        print_error "PID file not found at $PID_FILE"
        print_info "Relay daemon is not running"
        print_info "Start it with: ./scripts/start-relay.sh"
        exit 1
    fi

    local pid=$(cat "$PID_FILE")

    if ! ps -p "$pid" > /dev/null 2>&1; then
        print_error "Process $pid from PID file is not running"
        print_warn "Found stale PID file"
        print_info "Clean up with: rm $PID_FILE"
        exit 1
    fi

    echo "$pid"
}

# Get process information
get_process_info() {
    local pid=$1

    # Get process start time and uptime
    if command -v ps > /dev/null 2>&1; then
        # macOS/BSD ps
        if ps -o comm= -p "$pid" | grep -q chiral-relay 2>/dev/null; then
            local start_time=$(ps -o lstart= -p "$pid" 2>/dev/null || echo "Unknown")
            local cpu=$(ps -o %cpu= -p "$pid" 2>/dev/null | tr -d ' ' || echo "N/A")
            local mem=$(ps -o %mem= -p "$pid" 2>/dev/null | tr -d ' ' || echo "N/A")
            local rss=$(ps -o rss= -p "$pid" 2>/dev/null | tr -d ' ' || echo "N/A")

            echo "$start_time|$cpu|$mem|$rss"
        else
            echo "Unknown||||"
        fi
    else
        echo "Unknown||||"
    fi
}

# Parse and display metrics
display_metrics() {
    if [ ! -f "$METRICS_FILE" ]; then
        print_warn "Metrics file not found at $METRICS_FILE"
        return 1
    fi

    if ! command -v jq > /dev/null 2>&1; then
        print_warn "jq not installed, showing raw metrics:"
        cat "$METRICS_FILE"
        return 0
    fi

    local metrics=$(cat "$METRICS_FILE")

    print_header "📊 Relay Metrics"
    echo ""

    # Parse JSON fields
    local peer_id=$(echo "$metrics" | jq -r '.peer_id // "N/A"')
    local connected_peers=$(echo "$metrics" | jq -r '.connected_peers // "N/A"')
    local uptime=$(echo "$metrics" | jq -r '.uptime_seconds // "N/A"')
    local reservations=$(echo "$metrics" | jq -r '.relay_reservations // "N/A"')
    local circuits=$(echo "$metrics" | jq -r '.relay_circuits // "N/A"')
    local listen_addrs=$(echo "$metrics" | jq -r '.listen_addresses[]? // "N/A"' | tr '\n' ',' | sed 's/,$//')

    print_field "Peer ID" "$peer_id"
    print_field "Connected Peers" "$connected_peers"
    print_field "Uptime (seconds)" "$uptime"
    print_field "Active Reservations" "$reservations"
    print_field "Active Circuits" "$circuits"
    print_field "Listen Addresses" "$listen_addrs"

    echo ""
}

# Display recent log entries
display_logs() {
    print_header "📝 Recent Logs (last 10 lines)"
    echo ""

    if [ ! -f "$LOG_FILE" ]; then
        print_warn "Log file not found at $LOG_FILE"
        return 1
    fi

    tail -n 10 "$LOG_FILE" | while IFS= read -r line; do
        echo "  $line"
    done

    echo ""
}

# Format uptime
format_uptime() {
    local seconds=$1
    local days=$((seconds / 86400))
    local hours=$(( (seconds % 86400) / 3600 ))
    local minutes=$(( (seconds % 3600) / 60 ))
    local secs=$((seconds % 60))

    if [ $days -gt 0 ]; then
        echo "${days}d ${hours}h ${minutes}m ${secs}s"
    elif [ $hours -gt 0 ]; then
        echo "${hours}h ${minutes}m ${secs}s"
    elif [ $minutes -gt 0 ]; then
        echo "${minutes}m ${secs}s"
    else
        echo "${secs}s"
    fi
}

# Main execution
main() {
    print_header "═══════════════════════════════════════════"
    print_header " Chiral Network Relay - Status Check"
    print_header "═══════════════════════════════════════════"
    echo ""

    local pid=$(check_if_running)

    print_header "✅ Relay Daemon Status"
    echo ""

    print_field "Status" "${GREEN}Running${NC}"
    print_field "PID" "$pid"

    # Get process info
    IFS='|' read -r start_time cpu mem rss <<< "$(get_process_info "$pid")"

    if [ -n "$start_time" ] && [ "$start_time" != "Unknown" ]; then
        print_field "Started At" "$start_time"
    fi

    if [ -n "$cpu" ] && [ "$cpu" != "N/A" ]; then
        print_field "CPU Usage" "${cpu}%"
    fi

    if [ -n "$mem" ] && [ "$mem" != "N/A" ]; then
        print_field "Memory Usage" "${mem}%"
    fi

    if [ -n "$rss" ] && [ "$rss" != "N/A" ]; then
        local rss_mb=$((rss / 1024))
        print_field "Memory (RSS)" "${rss_mb} MB"
    fi
    # Show HTTP port if available
    if [ -f "$RELAY_DIR/http_port.txt" ]; then
        local http_port=$(cat "$RELAY_DIR/http_port.txt")
        print_field "HTTP Server Port" "$http_port"
        print_info "  Health check: http://127.0.0.1:$http_port/health"
    fi
    echo ""

    # Display metrics if available
    display_metrics

    # Display recent logs
    display_logs

    print_header "═══════════════════════════════════════════"
    echo ""
    print_info "Files:"
    print_field "PID file" "$PID_FILE"
    print_field "Metrics" "$METRICS_FILE"
    print_field "Logs" "$LOG_FILE"
    echo ""
    print_info "Commands:"
    print_info "  View logs: tail -f $LOG_FILE"
    print_info "  Stop relay: ./scripts/stop-relay.sh"
    echo ""
}

# Run main function
main "$@"
