#!/bin/bash

# Chiral Network Bootstrap Node Runner
# Run this script on a server to act as a bootstrap node

echo "🚀 Chiral Network Bootstrap Node"
echo "================================"

# Default values
DHT_PORT=${DHT_PORT:-4001}
LOG_LEVEL=${LOG_LEVEL:-info}
ENABLE_GETH=${ENABLE_GETH:-false}

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
        --secret)
            SECRET="$2"
            shift 2
            ;;
        --help)
            echo "Usage: $0 [OPTIONS]"
            echo ""
            echo "Options:"
            echo "  --port PORT          DHT port to listen on (default: 4001)"
            echo "  --log-level LEVEL    Log level: trace, debug, info, warn, error (default: info)"
            echo "  --enable-geth        Enable geth node alongside DHT"
            echo "  --help              Show this help message"
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

# Build the application if needed
if [ ! -f "target/release/chiral-network" ]; then
    echo "📦 Building Chiral Network..."
    cargo build --release --manifest-path src-tauri/Cargo.toml
fi

# Prepare the command
CMD="./target/release/chiral-network --headless --is_bootstrap --dht-port $DHT_PORT --log-level $LOG_LEVEL --show-multiaddr"

if [ "$ENABLE_GETH" = "true" ]; then
    CMD="$CMD --enable-geth"
fi

echo "📍 Starting bootstrap node on port $DHT_PORT"
echo "📊 Log level: $LOG_LEVEL"
echo ""

# Run the bootstrap node
exec $CMD
