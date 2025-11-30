#!/bin/bash
# Docker E2E Test Runner
# Manages lifecycle of Dockerized P2P network for testing

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
COMPOSE_FILE="docker-compose.test.yml"
PROJECT_NAME="chiral-test"

log() {
    echo -e "${BLUE}[$(date +%H:%M:%S)]${NC} $1"
}

log_success() {
    echo -e "${GREEN}✅ $1${NC}"
}

log_error() {
    echo -e "${RED}❌ $1${NC}"
}

log_warning() {
    echo -e "${YELLOW}⚠️  $1${NC}"
}

# Check if Docker is running
check_docker() {
    if ! docker info > /dev/null 2>&1; then
        log_error "Docker is not running"
        exit 1
    fi
    log_success "Docker is running"
}

# Build the image
build_image() {
    log "Building chiral-network Docker image..."
    docker build -t chiral-network:test . || {
        log_error "Failed to build Docker image"
        exit 1
    }
    log_success "Docker image built successfully"
}

# Start the test network
start_network() {
    log "Starting test network..."
    docker-compose -f "$COMPOSE_FILE" -p "$PROJECT_NAME" up -d

    log "Waiting for nodes to initialize..."
    sleep 5

    log_success "Test network started"
}

# Stop the test network
stop_network() {
    log "Stopping test network..."
    docker-compose -f "$COMPOSE_FILE" -p "$PROJECT_NAME" down
    log_success "Test network stopped"
}

# Clean everything including volumes
clean_all() {
    log "Cleaning up all containers, networks, and volumes..."
    docker-compose -f "$COMPOSE_FILE" -p "$PROJECT_NAME" down -v
    docker system prune -f
    log_success "Cleanup complete"
}

# Show logs
show_logs() {
    local service=$1
    if [ -z "$service" ]; then
        docker-compose -f "$COMPOSE_FILE" -p "$PROJECT_NAME" logs -f
    else
        docker-compose -f "$COMPOSE_FILE" -p "$PROJECT_NAME" logs -f "$service"
    fi
}

# Get peer IDs from all nodes
get_peer_ids() {
    log "Retrieving peer IDs from all nodes..."

    echo ""
    echo "Bootstrap Node:"
    docker-compose -f "$COMPOSE_FILE" -p "$PROJECT_NAME" logs bootstrap 2>&1 | grep -oP 'PeerId\(".*?"\)' | head -1 || echo "Not found"

    echo ""
    echo "Seeder Node:"
    docker-compose -f "$COMPOSE_FILE" -p "$PROJECT_NAME" logs seeder 2>&1 | grep -oP 'PeerId\(".*?"\)' | head -1 || echo "Not found"

    echo ""
    echo "Downloader Node:"
    docker-compose -f "$COMPOSE_FILE" -p "$PROJECT_NAME" logs downloader 2>&1 | grep -oP 'PeerId\(".*?"\)' | head -1 || echo "Not found"
}

# Check DHT connectivity
check_connectivity() {
    log "Checking DHT connectivity..."

    echo ""
    echo "Bootstrap peer count:"
    docker-compose -f "$COMPOSE_FILE" -p "$PROJECT_NAME" logs bootstrap 2>&1 | grep "peer count" | tail -1 || echo "No peer count logs"

    echo ""
    echo "Seeder peer count:"
    docker-compose -f "$COMPOSE_FILE" -p "$PROJECT_NAME" logs seeder 2>&1 | grep "peer count" | tail -1 || echo "No peer count logs"

    echo ""
    echo "Downloader peer count:"
    docker-compose -f "$COMPOSE_FILE" -p "$PROJECT_NAME" logs downloader 2>&1 | grep "peer count" | tail -1 || echo "No peer count logs"
}

# Interactive shell into a container
shell() {
    local service=$1
    if [ -z "$service" ]; then
        log_error "Usage: $0 shell [bootstrap|seeder|downloader]"
        exit 1
    fi

    docker-compose -f "$COMPOSE_FILE" -p "$PROJECT_NAME" exec "$service" /bin/sh
}

# Show network topology
show_topology() {
    log "Network Topology:"
    echo ""
    echo "┌─────────────────────────────────────┐"
    echo "│         Bootstrap Node              │"
    echo "│  (Public Relay Server)              │"
    echo "│  172.20.0.10:4001                  │"
    echo "└──────────┬──────────────────────────┘"
    echo "           │"
    echo "           ├──────────────────────────┐"
    echo "           │                          │"
    echo "  ┌────────▼────────┐       ┌────────▼────────┐"
    echo "  │  Seeder Node    │       │ Downloader Node │"
    echo "  │  (NAT Network A)│       │  (NAT Network B)│"
    echo "  │  172.21.0.11    │       │   172.22.0.12   │"
    echo "  └─────────────────┘       └─────────────────┘"
    echo ""
    log_warning "Note: Direct seeder↔downloader communication is NOT blocked"
    log_warning "This is a known limitation of Docker networking"
    log_warning "For true NAT testing, use Rust integration tests instead"
}

# Main menu
show_menu() {
    echo ""
    echo "═══════════════════════════════════════"
    echo "  Chiral Network Docker Test Manager  "
    echo "═══════════════════════════════════════"
    echo ""
    echo "1. Build image"
    echo "2. Start network"
    echo "3. Stop network"
    echo "4. Clean all"
    echo "5. Show logs (all)"
    echo "6. Show logs (bootstrap)"
    echo "7. Show logs (seeder)"
    echo "8. Show logs (downloader)"
    echo "9. Get peer IDs"
    echo "10. Check connectivity"
    echo "11. Show topology"
    echo "12. Shell into container"
    echo "0. Exit"
    echo ""
}

# Main script
main() {
    check_docker

    if [ $# -eq 0 ]; then
        # Interactive mode
        while true; do
            show_menu
            read -p "Select option: " choice

            case $choice in
                1) build_image ;;
                2) start_network ;;
                3) stop_network ;;
                4) clean_all ;;
                5) show_logs ;;
                6) show_logs bootstrap ;;
                7) show_logs seeder ;;
                8) show_logs downloader ;;
                9) get_peer_ids ;;
                10) check_connectivity ;;
                11) show_topology ;;
                12)
                    read -p "Enter service name (bootstrap/seeder/downloader): " service
                    shell "$service"
                    ;;
                0) exit 0 ;;
                *) log_error "Invalid option" ;;
            esac
        done
    else
        # Command line mode
        case $1 in
            build) build_image ;;
            start) start_network ;;
            stop) stop_network ;;
            clean) clean_all ;;
            logs) show_logs "$2" ;;
            peers) get_peer_ids ;;
            check) check_connectivity ;;
            topology) show_topology ;;
            shell) shell "$2" ;;
            *)
                echo "Usage: $0 {build|start|stop|clean|logs|peers|check|topology|shell}"
                echo "Or run without arguments for interactive mode"
                exit 1
                ;;
        esac
    fi
}

main "$@"
