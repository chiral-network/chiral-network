#!/usr/bin/env bash
# Chiral Network Relay - Bootstrap Script
# This script starts a dedicated Circuit Relay v2 daemon for Chiral Network

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Default configuration
RELAY_PORT="${RELAY_PORT:-4001}"
RELAY_DIR="${RELAY_DIR:-$HOME/.chiral-relay}"
IDENTITY_FILE="${IDENTITY_FILE:-$RELAY_DIR/identity.key}"
PID_FILE="${PID_FILE:-$RELAY_DIR/relay.pid}"
LOG_FILE="${LOG_FILE:-$RELAY_DIR/relay.log}"
METRICS_FILE="${METRICS_FILE:-$RELAY_DIR/metrics.json}"
MAX_RESERVATIONS="${MAX_RESERVATIONS:-128}"
MAX_CIRCUITS="${MAX_CIRCUITS:-16}"
EXTERNAL_ADDRESS="${EXTERNAL_ADDRESS:-}"
VERBOSE="${VERBOSE:-false}"
BINARY_PATH=""  # Will be set by check_binary()

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

# Check if relay is already running
check_if_running() {
    if [ -f "$PID_FILE" ]; then
        local pid=$(cat "$PID_FILE")
        if ps -p "$pid" > /dev/null 2>&1; then
            print_error "Relay daemon is already running (PID: $pid)"
            print_info "Use 'stop-relay.sh' to stop it first, or 'status-relay.sh' to check status"
            exit 1
        else
            print_warn "Stale PID file found, removing..."
            rm -f "$PID_FILE"
        fi
    fi
}

# Create directory structure
setup_directories() {
    if [ ! -d "$RELAY_DIR" ]; then
        print_info "Creating relay directory: $RELAY_DIR"
        mkdir -p "$RELAY_DIR"
    fi
}

# Check if binary exists
check_binary() {
    # Get the absolute path to the script's directory
    local script_dir="$(cd "$(dirname "$0")" && pwd)"
    local project_dir="$(cd "$script_dir/.." && pwd)"
    BINARY_PATH="$project_dir/target/release/chiral-relay"

    if [ ! -f "$BINARY_PATH" ]; then
        print_warn "Binary not found at $BINARY_PATH"
        print_info "Building relay daemon..."

        cd "$project_dir"
        cargo build --release

        if [ $? -ne 0 ]; then
            print_error "Failed to build relay daemon"
            exit 1
        fi

        print_info "Build successful!"
    fi
}

# Detect external IP
detect_external_ip() {
    if [ -z "$EXTERNAL_ADDRESS" ]; then
        print_info "Detecting external IP address..."

        # Try multiple services for reliability
        local external_ip=""

        external_ip=$(curl -s -4 https://ifconfig.me 2>/dev/null) || \
        external_ip=$(curl -s -4 https://icanhazip.com 2>/dev/null) || \
        external_ip=$(curl -s -4 https://api.ipify.org 2>/dev/null)

        if [ -n "$external_ip" ]; then
            EXTERNAL_ADDRESS="/ip4/$external_ip/tcp/$RELAY_PORT"
            print_info "Detected external address: $EXTERNAL_ADDRESS"
        else
            print_warn "Could not detect external IP. You may need to set it manually."
            print_warn "Use: export EXTERNAL_ADDRESS=/ip4/YOUR_IP/tcp/$RELAY_PORT"
        fi
    fi
}

# Build command line arguments
build_args() {
    local args=(
        --port "$RELAY_PORT"
        --identity-path "$IDENTITY_FILE"
        --pid-file "$PID_FILE"
        --metrics-file "$METRICS_FILE"
        --max-reservations "$MAX_RESERVATIONS"
        --max-circuits "$MAX_CIRCUITS"
    )

    if [ -n "$EXTERNAL_ADDRESS" ]; then
        args+=(--external-address "$EXTERNAL_ADDRESS")
    fi

    if [ "$VERBOSE" = "true" ]; then
        args+=(--verbose)
    fi

    echo "${args[@]}"
}

# Start the relay daemon
start_relay() {
    # Use the BINARY_PATH set by check_binary()
    local args=$(build_args)

    print_info "Starting Chiral Network Relay Daemon..."
    print_info "Port: $RELAY_PORT"
    print_info "Identity: $IDENTITY_FILE"
    print_info "PID file: $PID_FILE"
    print_info "Log file: $LOG_FILE"
    print_info "Metrics: $METRICS_FILE"
    print_info "Max reservations: $MAX_RESERVATIONS"
    print_info "Max circuits: $MAX_CIRCUITS"

    if [ -n "$EXTERNAL_ADDRESS" ]; then
        print_info "External address: $EXTERNAL_ADDRESS"
    fi

    # Start daemon in background
    nohup "$BINARY_PATH" $args > "$LOG_FILE" 2>&1 &
    local pid=$!

    # Wait a moment and verify it started
    sleep 3

    local http_port=$(grep -oP 'HTTP server started on port \K\d+' "$LOG_FILE" | tail -1)
    #adding for port rand 
    if [ -n "$http_port" ]; then
        print_info "✅ HTTP server listening on port: $http_port"
        echo "$http_port" > "$RELAY_DIR/http_port.txt"
    fi 
    #for port rand
    if ps -p "$pid" > /dev/null 2>&1; then
        print_info "✅ Relay daemon started successfully (PID: $pid)"
        print_info ""
        print_info "To view logs: tail -f $LOG_FILE"
        print_info "To check status: ./scripts/status-relay.sh"
        print_info "To stop relay: ./scripts/stop-relay.sh"
        print_info ""

        # Show initial log output
        print_info "Initial log output:"
        echo "---"
        tail -n 20 "$LOG_FILE" || true
        echo "---"

    else
        print_error "Failed to start relay daemon"
        print_error "Check logs at: $LOG_FILE"
        exit 1
    fi
}

# Main execution
main() {
    print_info "Chiral Network Relay - Bootstrap Script"
    print_info "========================================"

    check_if_running
    setup_directories
    check_binary
    detect_external_ip
    start_relay
}

# Run main function
main "$@"
