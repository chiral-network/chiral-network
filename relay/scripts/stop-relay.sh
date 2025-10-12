#!/usr/bin/env bash
# Chiral Network Relay - Shutdown Script
# This script gracefully stops a running relay daemon

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Default configuration
RELAY_DIR="${RELAY_DIR:-$HOME/.chiral-relay}"
PID_FILE="${PID_FILE:-$RELAY_DIR/relay.pid}"
TIMEOUT="${TIMEOUT:-30}"
FORCE="${FORCE:-false}"

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

# Check if relay is running
check_if_running() {
    if [ ! -f "$PID_FILE" ]; then
        print_warn "PID file not found at $PID_FILE"
        print_info "Relay daemon is not running"
        exit 0
    fi

    local pid=$(cat "$PID_FILE")

    if ! ps -p "$pid" > /dev/null 2>&1; then
        print_warn "Process $pid from PID file is not running"
        print_info "Cleaning up stale PID file..."
        rm -f "$PID_FILE"
        exit 0
    fi

    echo "$pid"
}

# Graceful shutdown
graceful_shutdown() {
    local pid=$1
    print_info "Sending SIGINT to process $pid (graceful shutdown)..."

    kill -SIGINT "$pid" 2>/dev/null || {
        print_error "Failed to send SIGINT to process $pid"
        return 1
    }

    # Wait for process to exit
    local elapsed=0
    while ps -p "$pid" > /dev/null 2>&1; do
        if [ $elapsed -ge $TIMEOUT ]; then
            print_warn "Process did not exit within ${TIMEOUT}s"
            return 1
        fi

        sleep 1
        elapsed=$((elapsed + 1))
        if [ $((elapsed % 5)) -eq 0 ]; then
            print_info "Waiting for process to exit... (${elapsed}s elapsed)"
        fi
    done

    print_info "✅ Process exited gracefully"
    return 0
}

# Forceful shutdown
force_shutdown() {
    local pid=$1
    print_warn "Forcing shutdown with SIGKILL..."

    kill -SIGKILL "$pid" 2>/dev/null || {
        print_error "Failed to send SIGKILL to process $pid"
        return 1
    }

    sleep 1

    if ps -p "$pid" > /dev/null 2>&1; then
        print_error "Process $pid still running after SIGKILL"
        return 1
    fi

    print_info "✅ Process killed forcefully"
    return 0
}

# Clean up PID file
cleanup() {
    if [ -f "$PID_FILE" ]; then
        print_info "Removing PID file..."
        rm -f "$PID_FILE"
    fi
}

# Main execution
main() {
    print_info "Chiral Network Relay - Shutdown Script"
    print_info "======================================"

    # Parse command line arguments
    while [ $# -gt 0 ]; do
        case "$1" in
            --force|-f)
                FORCE=true
                shift
                ;;
            --timeout|-t)
                TIMEOUT="$2"
                shift 2
                ;;
            *)
                print_error "Unknown option: $1"
                echo "Usage: $0 [--force] [--timeout SECONDS]"
                exit 1
                ;;
        esac
    done

    local pid=$(check_if_running)

    if [ -z "$pid" ]; then
        exit 0
    fi

    print_info "Found relay daemon (PID: $pid)"

    if [ "$FORCE" = "true" ]; then
        force_shutdown "$pid" || exit 1
    else
        if ! graceful_shutdown "$pid"; then
            print_warn "Graceful shutdown failed, trying forceful shutdown..."
            force_shutdown "$pid" || exit 1
        fi
    fi

    cleanup
    print_info "✅ Relay daemon stopped successfully"
}

# Run main function
main "$@"
