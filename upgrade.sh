#!/bin/bash

# Chiral Network DHT Upgrade Script
# This script helps update existing installations with improved DHT networking

set -e

echo "========================================="
echo "Chiral Network DHT Upgrade Script"
echo "========================================="
echo ""

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check if we're in the right directory
if [ ! -f "package.json" ] || [ ! -d "src-tauri" ]; then
    echo -e "${RED}Error: This script must be run from the Chiral Network project root${NC}"
    exit 1
fi

echo -e "${YELLOW}This upgrade will:${NC}"
echo "  • Update DHT connection stability"
echo "  • Add protocol negotiation support"
echo "  • Improve keep-alive mechanisms"
echo "  • Extend connection timeouts"
echo ""

# Ask for confirmation
read -p "Do you want to continue? (y/N): " -n 1 -r
echo ""
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "Upgrade cancelled"
    exit 0
fi

echo ""
echo -e "${GREEN}Step 1: Stopping any running instances...${NC}"
# Kill any running chiral-network processes
pkill -f "chiral-network" 2>/dev/null || true
pkill -f "npm run tauri" 2>/dev/null || true

echo -e "${GREEN}Step 2: Backing up configuration...${NC}"
# Create backup directory
BACKUP_DIR="backup_$(date +%Y%m%d_%H%M%S)"
mkdir -p "$BACKUP_DIR"

# Backup important files if they exist
if [ -f "src-tauri/tauri.conf.json" ]; then
    cp "src-tauri/tauri.conf.json" "$BACKUP_DIR/"
fi

if [ -d "src-tauri/bin/geth-data" ]; then
    echo "  • Backing up geth data (this may take a moment)..."
    cp -r "src-tauri/bin/geth-data" "$BACKUP_DIR/" 2>/dev/null || true
fi

echo -e "${GREEN}Step 3: Cleaning build artifacts...${NC}"
rm -rf src-tauri/target/debug 2>/dev/null || true
rm -rf src-tauri/target/release 2>/dev/null || true
rm -rf dist 2>/dev/null || true

echo -e "${GREEN}Step 4: Installing dependencies...${NC}"
npm install

echo -e "${GREEN}Step 5: Building the application...${NC}"
echo "  • Building frontend..."
npm run build

echo "  • Building Tauri application (this may take several minutes)..."
npm run tauri build

echo ""
echo -e "${GREEN}✅ Upgrade complete!${NC}"
echo ""
echo "The DHT networking has been upgraded with:"
echo "  • Protocol negotiation via libp2p identify"
echo "  • Extended idle timeout (5 minutes)"
echo "  • Periodic Kademlia bootstrap (every 30 seconds)"
echo "  • Improved connection stability"
echo ""
echo "To run the application:"
echo "  • Development mode: npm run tauri dev"
echo "  • Production: ./src-tauri/target/release/chiral-network"
echo "  • Headless bootstrap: ./src-tauri/target/release/chiral-network --headless --dht-port 4001"
echo ""
echo "Your backup is stored in: $BACKUP_DIR"
echo ""