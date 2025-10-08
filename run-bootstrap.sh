#!/bin/bash
# Chiral Network Bootstrap Node Runner
# Run this script on a server to act as a bootstrap node

echo "🚀 Chiral Network Bootstrap Node"
echo "================================"

# Default values
DHT_PORT=${DHT_PORT:-4001}
LOG_LEVEL=${LOG_LEVEL:-info}
ENABLE_GETH=${ENABLE_GETH:-false}
BOOTSTRAP_ADDRS=()
GETH_DATA_DIR=""
MINER_ADDRESS=""
SECRET=""
DISABLE_AUTONAT=false
AUTONAT_PROBE_INTERVAL=""
AUTONAT_SERVER=""
SHOW_MULTIADDR=false
SHOW_REACHABILITY=false
SHOW_DOWNLOADS=false

# Parse command line arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --port)
            DHT_PORT="$2"
            shift 2
            ;;
        --log-level)
            LOG_LEVEL="$2"
            shift 2
            ;;
        --enable-geth)
            ENABLE_GETH=true
            shift
            ;;
        --bootstrap)
            BOOTSTRAP_ADDRS+=("$2")
            shift 2
            ;;
        --geth-data-dir)
            GETH_DATA_DIR="$2"
            shift 2
            ;;
        --miner-address)
            MINER_ADDRESS="$2"
            shift 2
            ;;
        --secret)
            SECRET="$2"
            shift 2
            ;;
        --disable-autonat)
            DISABLE_AUTONAT=true
            shift
            ;;
        --autonat-probe-interval)
            AUTONAT_PROBE_INTERVAL="$2"
            shift 2
            ;;
        --autonat-server)
            AUTONAT_SERVER="$2"
            shift 2
            ;;
        --show-multiaddr)
            SHOW_MULTIADDR=true
            shift
            ;;
        --show-reachability)
            SHOW_REACHABILITY=true
            shift
            ;;
        --show-downloads)
            SHOW_DOWNLOADS=true
            shift
            ;;
        --help)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --port PORT                      DHT port to listen on (default: 4001)"
            echo "  --log-level LEVEL                Log level: trace, debug, info, warn, error (default: info)"
            echo "  --enable-geth                    Enable geth node alongside DHT"
            echo "  --bootstrap MULTIADDR            Bootstrap node multiaddr (can be used multiple times)"
            echo "  --geth-data-dir PATH             Geth data directory path"
            echo "  --miner-address ADDRESS          Ethereum miner address"
            echo "  --secret SECRET                  Peer ID secret"
            echo "  --disable-autonat                Disable AutoNAT"
            echo "  --autonat-probe-interval SECS    AutoNAT probe interval in seconds"
            echo "  --autonat-server MULTIADDR       AutoNAT server multiaddr"
            echo "  --show-multiaddr                 Show multiaddr information"
            echo "  --show-reachability              Show reachability status"
            echo "  --show-downloads                 Show download information"
            echo "  --help                           Show this help message"
            echo ""
            echo "Environment variables:"
            echo "  DHT_PORT            Same as --port"
            echo "  LOG_LEVEL           Same as --log-level"
            echo "  ENABLE_GETH         Set to 'true' to enable geth"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            echo "Use --help for usage information"
            exit 1
            ;;
    esac
done

# Change working directory to src-tauri
cd src-tauri

if [ ! -d "../dist" ]; then
    echo "Creating empty frontend dist folder..."
    mkdir -p ../dist
fi

# Build the application if needed
if [ ! -f "target/release/chiral-network" ]; then
    echo "📦 Building Chiral Network..."
    cargo build --release
fi

# Prepare the command - start with required flags (headless and is-bootstrap)
CMD="./target/release/chiral-network --headless --is-bootstrap"

# Add DHT port
CMD="$CMD --dht-port $DHT_PORT"

# Add log level
CMD="$CMD --log-level $LOG_LEVEL"

# Add optional flags only if they are set
for addr in "${BOOTSTRAP_ADDRS[@]}"; do
    CMD="$CMD --bootstrap $addr"
done

if [ "$ENABLE_GETH" = "true" ]; then
    CMD="$CMD --enable-geth"
fi

if [ -n "$GETH_DATA_DIR" ]; then
    CMD="$CMD --geth-data-dir $GETH_DATA_DIR"
fi

if [ -n "$MINER_ADDRESS" ]; then
    CMD="$CMD --miner-address $MINER_ADDRESS"
fi

if [ -n "$SECRET" ]; then
    CMD="$CMD --secret $SECRET"
fi

if [ "$DISABLE_AUTONAT" = true ]; then
    CMD="$CMD --disable-autonat"
fi

if [ -n "$AUTONAT_PROBE_INTERVAL" ]; then
    CMD="$CMD --autonat-probe-interval $AUTONAT_PROBE_INTERVAL"
fi

if [ -n "$AUTONAT_SERVER" ]; then
    CMD="$CMD --autonat-server $AUTONAT_SERVER"
fi

if [ "$SHOW_MULTIADDR" = true ]; then
    CMD="$CMD --show-multiaddr"
fi

if [ "$SHOW_REACHABILITY" = true ]; then
    CMD="$CMD --show-reachability"
fi

if [ "$SHOW_DOWNLOADS" = true ]; then
    CMD="$CMD --show-downloads"
fi

echo "📍 Starting bootstrap node on port $DHT_PORT"
echo "📊 Log level: $LOG_LEVEL"
echo ""

# Run the bootstrap node
exec $CMD
