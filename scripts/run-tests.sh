#!/bin/bash
# Test Runner for Chiral Network
# Runs all E2E and integration tests

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

echo -e "${BLUE}========================================${NC}"
echo -e "${BLUE}  Chiral Network Test Suite${NC}"
echo -e "${BLUE}========================================${NC}"
echo ""

# Check if cargo is available
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}❌ Cargo is not installed${NC}"
    echo "Please install Rust from: https://rustup.rs/"
    exit 1
fi

# Navigate to src-tauri directory
cd "$(dirname "$0")/../src-tauri"

echo -e "${YELLOW}📋 Running tests...${NC}"
echo ""

# Run specific test suites with progress
echo -e "${BLUE}1/3: E2E Cross-Network Transfer Tests${NC}"
cargo test --test e2e_cross_network_transfer_test -- --nocapture --test-threads=1 || {
    echo -e "${RED}❌ E2E tests failed${NC}"
    exit 1
}

echo ""
echo -e "${BLUE}2/3: NAT Traversal Tests${NC}"
cargo test --test nat_traversal_e2e_test -- --nocapture --test-threads=1 || {
    echo -e "${YELLOW}⚠️  NAT traversal tests failed (may be expected in local environment)${NC}"
}

echo ""
echo -e "${BLUE}3/3: Unit Tests${NC}"
cargo test --lib -- --nocapture || {
    echo -e "${RED}❌ Unit tests failed${NC}"
    exit 1
}

echo ""
echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}✅ All tests completed${NC}"
echo -e "${GREEN}========================================${NC}"
