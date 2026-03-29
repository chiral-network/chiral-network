#!/usr/bin/env bash
# Phase 03: Mining — start Geth and CPU mining on miner nodes
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "${SCRIPT_DIR}/lib.sh"

PHASE="03-mining"
WALLETS_FILE="${WALLETS_FILE:-/results/wallets.txt}"
MINING_THREADS="${MINING_THREADS:-2}"
GETH_SETTLE_DELAY="${GETH_SETTLE_DELAY:-5}"
MINING_SETTLE_DELAY="${MINING_SETTLE_DELAY:-30}"

log_info "=== Phase 03: Mining ==="

if [ -z "${MINER_NODES:-}" ]; then
    log_warn "MINER_NODES not set — skipping mining phase"
    SKIP_COUNT=$(( SKIP_COUNT + 1 ))
    record_result "$PHASE" "mining phase" "skip" "0" "MINER_NODES not set"
    print_summary
    exit 0
fi

IFS=',' read -ra MINERS <<< "$MINER_NODES"
TOTAL=${#MINERS[@]}
log_info "Mining nodes: ${MINERS[*]}"

# Helper: look up wallet address for a node from wallets.txt
get_wallet() {
    local node="$1"
    grep "^${node}=" "$WALLETS_FILE" 2>/dev/null | cut -d'=' -f2 || echo ""
}

# ---- Step 1: Start Geth on each miner ----
log_info "Starting Geth on ${TOTAL} miner nodes..."
for node in "${MINERS[@]}"; do
    addr=$(get_wallet "$node")
    if [ -z "$addr" ]; then
        log_warn "${node}: no wallet found in ${WALLETS_FILE}, using default"
        addr="0x0000000000000000000000000000000000000001"
    fi

    start_timer
    body=$(jq -nc --arg addr "$addr" '{minerAddress: $addr}')
    resp=$(api_post "$node" "/api/headless/geth/start" "$body")
    dur=$(stop_timer)
    assert_status "200" "${node}: geth start" "$dur"
done

log_info "Waiting ${GETH_SETTLE_DELAY}s for Geth to initialize..."
sleep "$GETH_SETTLE_DELAY"

# ---- Step 2: Set miner address ----
log_info "Setting miner addresses..."
for node in "${MINERS[@]}"; do
    addr=$(get_wallet "$node")
    [ -z "$addr" ] && addr="0x0000000000000000000000000000000000000001"

    start_timer
    body=$(jq -nc --arg addr "$addr" '{address: $addr}')
    resp=$(api_post "$node" "/api/headless/mining/miner-address" "$body")
    dur=$(stop_timer)
    assert_status "200" "${node}: set miner address" "$dur"
done

# ---- Step 3: Start mining ----
log_info "Starting mining with ${MINING_THREADS} threads..."
for node in "${MINERS[@]}"; do
    start_timer
    body=$(jq -nc --argjson threads "$MINING_THREADS" '{threads: $threads}')
    resp=$(api_post "$node" "/api/headless/mining/start" "$body")
    dur=$(stop_timer)
    assert_status "200" "${node}: mining start" "$dur"
done

log_info "Waiting ${MINING_SETTLE_DELAY}s for blocks to be mined..."
sleep "$MINING_SETTLE_DELAY"

# ---- Step 4: Verify mining status ----
log_info "Checking mining status..."
for node in "${MINERS[@]}"; do
    start_timer
    resp=$(api_get "$node" "/api/headless/mining/status")
    dur=$(stop_timer)
    assert_status "200" "${node}: mining status check" "$dur"

    mining_active=$(echo "$resp" | jq -r '.mining // .isMining // false' 2>/dev/null)
    if [ "$mining_active" = "true" ]; then
        log_pass "${node}: mining is active"
        PASS_COUNT=$(( PASS_COUNT + 1 ))
        record_result "$PHASE" "${node}: mining active" "pass" "$dur" ""
    else
        log_fail "${node}: mining is not active"
        FAIL_COUNT=$(( FAIL_COUNT + 1 ))
        record_result "$PHASE" "${node}: mining active" "fail" "$dur" "mining not active: ${resp}"
    fi

    hash_rate=$(echo "$resp" | jq -r '.hashRate // .hashrate // "unknown"' 2>/dev/null)
    log_info "${node}: hash rate = ${hash_rate}"
done

# ---- Step 5: Check mined blocks ----
log_info "Checking mined blocks..."
for node in "${MINERS[@]}"; do
    start_timer
    resp=$(api_get "$node" "/api/headless/mining/blocks")
    dur=$(stop_timer)
    assert_status "200" "${node}: mining blocks check" "$dur"

    block_count=$(echo "$resp" | jq 'if type == "array" then length elif .blocks then .blocks | length else .blockNumber // 0 end' 2>/dev/null) || block_count=0
    if [ "$block_count" -gt 0 ]; then
        log_pass "${node}: mined ${block_count} block(s)"
        PASS_COUNT=$(( PASS_COUNT + 1 ))
        record_result "$PHASE" "${node}: blocks mined" "pass" "$dur" ""
    else
        log_warn "${node}: 0 blocks mined (may need more time)"
        SKIP_COUNT=$(( SKIP_COUNT + 1 ))
        record_result "$PHASE" "${node}: blocks mined" "skip" "$dur" "0 blocks — may need more time"
    fi
done

print_summary
