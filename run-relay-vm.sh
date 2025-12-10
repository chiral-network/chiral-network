#!/bin/bash
# Script to run Chiral Network relay server on VM (Google Cloud, AWS, etc.)
# This script automatically enables AutoNAT and relay server functionality

set -e

echo "🚀 Chiral Network Relay Server - VM Mode"
echo "=========================================="
echo ""

# Check if we're in the right directory
if [ ! -d "src-tauri" ]; then
    echo "❌ Error: Please run this script from the project root directory"
    exit 1
fi

# Build the application if needed
if [ ! -f "src-tauri/target/release/chiral-network" ]; then
    echo "📦 Building Chiral Network..."
    cd src-tauri
    cargo build --release
    cd ..
fi

echo "✅ Binary found: src-tauri/target/release/chiral-network"
echo ""

# Set environment variable to enable AutoNAT (for VM mode)
export CHIRAL_ENABLE_AUTONAT=1
echo "🔧 Environment variable set: CHIRAL_ENABLE_AUTONAT=1"
echo ""

# Optional: Set VM mode flag (alternative way)
# export CHIRAL_VM_MODE=1

# Optional: Set Google Cloud project (if running on GCP)
# This will auto-detect and enable AutoNAT
# export GOOGLE_CLOUD_PROJECT=$(gcloud config get-value project 2>/dev/null || echo "")

echo "🚀 Starting Chiral Network in headless mode..."
echo "   - AutoNAT will be enabled automatically"
echo "   - Relay server will be created in standby mode"
echo "   - When public IP is detected, relay service will be advertised in DHT"
echo ""
echo "Press Ctrl+C to stop"
echo ""

# Run the application in headless mode
exec src-tauri/target/release/chiral-network \
    --headless \
    --dht-port 4001 \
    --log-level info \
    --show-multiaddr \
    --show-reachability

