#!/usr/bin/env bash
# Comprehensive test runner for P2P Chunk Network
# Runs all unit tests, integration tests, and end-to-end tests

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
TOTAL_PASS=0
TOTAL_FAIL=0

echo -e "${BLUE}╔════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║       P2P Chunk Network - Comprehensive Test Suite            ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════════╝${NC}"
echo ""

# =========================================================================
# Test 1: Cargo unit tests
# =========================================================================
echo -e "${YELLOW}═══ Running Cargo Unit Tests ═══${NC}"
cd "$PROJECT_DIR"

if cargo test --lib p2p_chunk --no-fail-fast -- --test-threads=1 2>&1 | tee /tmp/cargo_test_output.log | tail -20; then
    CARGO_TESTS=$(grep -c "test.*ok" /tmp/cargo_test_output.log 2>/dev/null || echo "0")
    echo -e "${GREEN}✓ Cargo tests passed: $CARGO_TESTS tests${NC}"
    TOTAL_PASS=$((TOTAL_PASS + 1))
else
    echo -e "${RED}✗ Cargo tests failed${NC}"
    TOTAL_FAIL=$((TOTAL_FAIL + 1))
fi
echo ""

# =========================================================================
# Test 2: Standalone integration test
# =========================================================================
echo -e "${YELLOW}═══ Running Standalone Integration Test ═══${NC}"

INTEGRATION_TEST="$PROJECT_DIR/tests/p2p_chunk_network_integration.rs"
if [ -f "$INTEGRATION_TEST" ]; then
    cd "$(dirname "$INTEGRATION_TEST")"
    if rustc "$(basename "$INTEGRATION_TEST")" -o /tmp/p2p_integration_test 2>&1 | tail -5; then
    if /tmp/p2p_integration_test 2>&1; then
        echo -e "${GREEN}✓ Standalone integration tests passed${NC}"
        TOTAL_PASS=$((TOTAL_PASS + 1))
    else
        echo -e "${RED}✗ Standalone integration tests failed${NC}"
        TOTAL_FAIL=$((TOTAL_FAIL + 1))
    fi
    else
        echo -e "${RED}✗ Failed to compile standalone test${NC}"
        TOTAL_FAIL=$((TOTAL_FAIL + 1))
    fi
else
    echo -e "${YELLOW}⚠ Standalone integration test not found at $INTEGRATION_TEST${NC}"
fi
echo ""

# =========================================================================
# Test 3: DHT integration test
# =========================================================================
echo -e "${YELLOW}═══ Running DHT Integration Test ═══${NC}"

if [ -x "$SCRIPT_DIR/test_dht_integration.sh" ]; then
    if "$SCRIPT_DIR/test_dht_integration.sh" 2>&1; then
        echo -e "${GREEN}✓ DHT integration tests passed${NC}"
        TOTAL_PASS=$((TOTAL_PASS + 1))
    else
        echo -e "${RED}✗ DHT integration tests failed${NC}"
        TOTAL_FAIL=$((TOTAL_FAIL + 1))
    fi
else
    echo -e "${YELLOW}⚠ DHT integration test not found${NC}"
fi
echo ""

# =========================================================================
# Test 4: REPL DHT operations test
# =========================================================================
echo -e "${YELLOW}═══ Running REPL DHT Operations Test ═══${NC}"

if [ -x "$SCRIPT_DIR/test_repl_dht_operations.sh" ]; then
    if "$SCRIPT_DIR/test_repl_dht_operations.sh" 2>&1; then
        echo -e "${GREEN}✓ REPL DHT operations tests passed${NC}"
        TOTAL_PASS=$((TOTAL_PASS + 1))
    else
        echo -e "${RED}✗ REPL DHT operations tests failed${NC}"
        TOTAL_FAIL=$((TOTAL_FAIL + 1))
    fi
else
    echo -e "${YELLOW}⚠ REPL operations test not found${NC}"
fi
echo ""

# =========================================================================
# Summary
# =========================================================================
echo -e "${BLUE}╔════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║                      Test Summary                              ║${NC}"
echo -e "${BLUE}╚════════════════════════════════════════════════════════════════╝${NC}"
echo ""
echo "  Test suites passed: $TOTAL_PASS"
echo "  Test suites failed: $TOTAL_FAIL"
echo ""

if [ "$TOTAL_FAIL" -eq 0 ]; then
    echo -e "${GREEN}╔════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${GREEN}║               All test suites passed!                          ║${NC}"
    echo -e "${GREEN}╚════════════════════════════════════════════════════════════════╝${NC}"
    exit 0
else
    echo -e "${RED}╔════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${RED}║               $TOTAL_FAIL test suite(s) failed                          ║${NC}"
    echo -e "${RED}╚════════════════════════════════════════════════════════════════╝${NC}"
    exit 1
fi
