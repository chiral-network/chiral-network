#!/usr/bin/env bash
# Phase 02: Wallet creation — generate deterministic wallet addresses for each node
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "${SCRIPT_DIR}/lib.sh"

PHASE="02-wallets"
WALLETS_FILE="${WALLETS_FILE:-/results/wallets.txt}"

log_info "=== Phase 02: Wallet Creation ==="

IFS=',' read -ra NODES <<< "${NODE_LIST:?NODE_LIST env var required (comma-separated node hostnames)}"
TOTAL=${#NODES[@]}

# Clear existing wallets file
mkdir -p "$(dirname "$WALLETS_FILE")"
> "$WALLETS_FILE"

# Generate deterministic wallet addresses from node index.
# These are synthetic addresses for test identification purposes.
# Format: 0x0000...{index padded to 40 hex chars}
for i in "${!NODES[@]}"; do
    node="${NODES[$i]}"
    index=$(( i + 1 ))

    start_timer

    # Generate a 40-char hex address: pad the index with leading zeros
    addr=$(printf "0x%040x" "$index")
    # Generate a 64-char hex private key
    privkey=$(printf "%064x" "$index")

    echo "${node}=${addr}" >> "$WALLETS_FILE"

    dur=$(stop_timer)
    log_pass "${node}: wallet ${addr}"
    PASS_COUNT=$(( PASS_COUNT + 1 ))
    record_result "$PHASE" "${node}: wallet created" "pass" "$dur" ""
done

# Verify wallet file
wallet_count=$(wc -l < "$WALLETS_FILE")
log_info "Generated ${wallet_count} wallets in ${WALLETS_FILE}"

if [ "$wallet_count" -eq "$TOTAL" ]; then
    log_pass "All ${TOTAL} wallets created"
else
    log_fail "Expected ${TOTAL} wallets, got ${wallet_count}"
    FAIL_COUNT=$(( FAIL_COUNT + 1 ))
    record_result "$PHASE" "wallet count check" "fail" "0" "expected ${TOTAL}, got ${wallet_count}"
fi

# Print wallet list for debugging
log_info "Wallet assignments:"
while IFS='=' read -r name addr; do
    log_info "  ${name} -> ${addr}"
done < "$WALLETS_FILE"

print_summary
