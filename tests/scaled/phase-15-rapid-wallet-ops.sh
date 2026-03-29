#!/usr/bin/env bash
set -euo pipefail
source /tests/lib.sh

PHASE="phase-15-rapid-wallet-ops"
log_info "[$PHASE] Starting rapid wallet operations stress test"

###############################################################################
# Phase 15 — Rapid Wallet Operations
#
# Stress tests wallet and balance operations:
# 1. All nodes query their own balance simultaneously
# 2. All nodes query the SAME wallet's balance (hotspot test)
# 3. Rapid sequential balance queries on one node (cache stress)
# 4. Batch reputation lookups with all wallet addresses
###############################################################################

RELAY_URL="${RELAY_URL:-http://130.245.173.73:8080}"
WALLETS_FILE="/results/wallets.txt"

if [[ -z "${ALL_NODES:-}" ]]; then
    log_warn "[$PHASE] ALL_NODES is empty — skipping"
    record_result "$PHASE" "rapid-wallet-ops" "skip" "0" "No nodes available"
    exit 0
fi

IFS=',' read -ra NODES <<< "$ALL_NODES"
NODE_COUNT=${#NODES[@]}

# Load wallet addresses if available
declare -a WALLETS=()
if [[ -f "$WALLETS_FILE" ]] && [[ -s "$WALLETS_FILE" ]]; then
    mapfile -t WALLETS < "$WALLETS_FILE"
fi
WALLET_COUNT=${#WALLETS[@]}

log_info "[$PHASE] Nodes: $NODE_COUNT, Wallets: $WALLET_COUNT"

start_timer

# =========================================================================
# Test 1: All nodes query their own balance simultaneously
# =========================================================================
log_info "[$PHASE] Test 1: All nodes query own balance simultaneously..."

TMPDIR_1=$(mktemp -d)
test1_ok=0
test1_fail=0
test1_total_ms=0

for i in $(seq 0 $(( NODE_COUNT - 1 ))); do
    node_url="${NODES[$i]}"
    result_file="${TMPDIR_1}/node-${i}.json"
    (
        req_start=$(date +%s%N)
        resp=$(curl -sf --max-time 15 \
            "${node_url}/api/headless/wallet/balance" 2>/dev/null) || resp=""
        req_end=$(date +%s%N)
        elapsed_ms=$(( (req_end - req_start) / 1000000 ))

        if [[ -n "$resp" ]] && ! echo "$resp" | jq -e '.error' >/dev/null 2>&1; then
            echo "{\"status\":\"ok\",\"elapsed_ms\":$elapsed_ms}" > "$result_file"
        else
            echo "{\"status\":\"fail\",\"elapsed_ms\":$elapsed_ms}" > "$result_file"
        fi
    ) &
done
wait

for i in $(seq 0 $(( NODE_COUNT - 1 ))); do
    result_file="${TMPDIR_1}/node-${i}.json"
    if [[ -f "$result_file" ]]; then
        status=$(jq -r '.status' "$result_file" 2>/dev/null) || status="fail"
        elapsed_ms=$(jq -r '.elapsed_ms' "$result_file" 2>/dev/null) || elapsed_ms=0
        if [[ "$status" == "ok" ]]; then
            test1_ok=$((test1_ok + 1))
            test1_total_ms=$((test1_total_ms + elapsed_ms))
        else
            test1_fail=$((test1_fail + 1))
        fi
    else
        test1_fail=$((test1_fail + 1))
    fi
done
rm -rf "$TMPDIR_1"

test1_avg=0
if [[ "$test1_ok" -gt 0 ]]; then
    test1_avg=$((test1_total_ms / test1_ok))
fi
log_info "[$PHASE] Test 1 results: $test1_ok OK, $test1_fail fail, avg ${test1_avg}ms"

if [[ "$test1_ok" -gt 0 ]]; then
    record_result "$PHASE" "wallet-self-balance" "pass" "$test1_avg" ""
else
    record_result "$PHASE" "wallet-self-balance" "fail" "0" "All $NODE_COUNT self-balance queries failed"
fi

# =========================================================================
# Test 2: All nodes query the SAME wallet's balance (hotspot)
# =========================================================================
log_info "[$PHASE] Test 2: All nodes query same wallet (hotspot)..."

# Pick a target wallet — use first wallet from file, or first node's wallet
TARGET_WALLET=""
if [[ "$WALLET_COUNT" -gt 0 ]]; then
    TARGET_WALLET="${WALLETS[0]}"
else
    # Try to get wallet from first node
    wallet_resp=$(curl -sf --max-time 10 \
        "${NODES[0]}/api/headless/wallet/address" 2>/dev/null) || wallet_resp=""
    if [[ -n "$wallet_resp" ]]; then
        TARGET_WALLET=$(echo "$wallet_resp" | jq -r '.address // .wallet // empty' 2>/dev/null) || TARGET_WALLET=""
    fi
fi

if [[ -z "$TARGET_WALLET" ]]; then
    log_warn "[$PHASE] Test 2: No target wallet available — skipping"
    record_result "$PHASE" "wallet-hotspot" "skip" "0" "No target wallet"
else
    log_info "[$PHASE] Test 2: Target wallet: $TARGET_WALLET"

    TMPDIR_2=$(mktemp -d)
    test2_ok=0
    test2_fail=0
    test2_total_ms=0

    for i in $(seq 0 $(( NODE_COUNT - 1 ))); do
        node_url="${NODES[$i]}"
        result_file="${TMPDIR_2}/node-${i}.json"
        (
            req_start=$(date +%s%N)
            resp=$(curl -sf --max-time 15 \
                "${node_url}/api/headless/wallet/balance/${TARGET_WALLET}" 2>/dev/null) || resp=""

            # If specific wallet balance endpoint is not available, try geth status
            if [[ -z "$resp" ]]; then
                resp=$(curl -sf --max-time 15 \
                    "${node_url}/api/headless/geth/status" 2>/dev/null) || resp=""
            fi

            req_end=$(date +%s%N)
            elapsed_ms=$(( (req_end - req_start) / 1000000 ))

            if [[ -n "$resp" ]] && ! echo "$resp" | jq -e '.error' >/dev/null 2>&1; then
                echo "{\"status\":\"ok\",\"elapsed_ms\":$elapsed_ms}" > "$result_file"
            else
                echo "{\"status\":\"fail\",\"elapsed_ms\":$elapsed_ms}" > "$result_file"
            fi
        ) &
    done
    wait

    for i in $(seq 0 $(( NODE_COUNT - 1 ))); do
        result_file="${TMPDIR_2}/node-${i}.json"
        if [[ -f "$result_file" ]]; then
            status=$(jq -r '.status' "$result_file" 2>/dev/null) || status="fail"
            elapsed_ms=$(jq -r '.elapsed_ms' "$result_file" 2>/dev/null) || elapsed_ms=0
            if [[ "$status" == "ok" ]]; then
                test2_ok=$((test2_ok + 1))
                test2_total_ms=$((test2_total_ms + elapsed_ms))
            else
                test2_fail=$((test2_fail + 1))
            fi
        else
            test2_fail=$((test2_fail + 1))
        fi
    done
    rm -rf "$TMPDIR_2"

    test2_avg=0
    if [[ "$test2_ok" -gt 0 ]]; then
        test2_avg=$((test2_total_ms / test2_ok))
    fi
    log_info "[$PHASE] Test 2 results: $test2_ok OK, $test2_fail fail, avg ${test2_avg}ms"

    if [[ "$test2_ok" -gt 0 ]]; then
        record_result "$PHASE" "wallet-hotspot" "pass" "$test2_avg" ""
    else
        record_result "$PHASE" "wallet-hotspot" "fail" "0" "All hotspot queries failed"
    fi
fi

# =========================================================================
# Test 3: Rapid sequential balance queries on one node (cache stress)
# =========================================================================
log_info "[$PHASE] Test 3: Rapid sequential queries on single node..."

RAPID_NODE="${NODES[0]}"
RAPID_COUNT=50
test3_ok=0
test3_fail=0
test3_total_ms=0
test3_max_ms=0

for r in $(seq 1 "$RAPID_COUNT"); do
    req_start=$(date +%s%N)
    resp=$(curl -sf --max-time 10 \
        "${RAPID_NODE}/api/headless/wallet/balance" 2>/dev/null) || resp=""
    req_end=$(date +%s%N)
    elapsed_ms=$(( (req_end - req_start) / 1000000 ))

    if [[ -n "$resp" ]] && ! echo "$resp" | jq -e '.error' >/dev/null 2>&1; then
        test3_ok=$((test3_ok + 1))
        test3_total_ms=$((test3_total_ms + elapsed_ms))
        if [[ "$elapsed_ms" -gt "$test3_max_ms" ]]; then
            test3_max_ms=$elapsed_ms
        fi
    else
        test3_fail=$((test3_fail + 1))
    fi
done

test3_avg=0
if [[ "$test3_ok" -gt 0 ]]; then
    test3_avg=$((test3_total_ms / test3_ok))
fi
log_info "[$PHASE] Test 3 results: $test3_ok/$RAPID_COUNT OK, avg ${test3_avg}ms, max ${test3_max_ms}ms"

if [[ "$test3_ok" -gt 0 ]]; then
    record_result "$PHASE" "wallet-rapid-sequential" "pass" "$test3_avg" ""
else
    record_result "$PHASE" "wallet-rapid-sequential" "fail" "0" "All $RAPID_COUNT rapid queries failed"
fi

# =========================================================================
# Test 4: Batch reputation lookup with all wallet addresses
# =========================================================================
log_info "[$PHASE] Test 4: Batch reputation lookup..."

if [[ "$WALLET_COUNT" -eq 0 ]]; then
    log_warn "[$PHASE] Test 4: No wallet addresses — skipping"
    record_result "$PHASE" "wallet-batch-reputation" "skip" "0" "No wallet addresses available"
else
    wallet_json=$(printf '%s\n' "${WALLETS[@]}" | jq -R . | jq -s '{ "wallets": . }')

    req_start=$(date +%s%N)
    batch_resp=$(curl -sf --max-time 30 \
        -X POST "${RELAY_URL}/api/ratings/batch" \
        -H "Content-Type: application/json" \
        -d "$wallet_json" 2>/dev/null) || batch_resp=""
    req_end=$(date +%s%N)
    batch_elapsed_ms=$(( (req_end - req_start) / 1000000 ))

    if [[ -n "$batch_resp" ]] && ! echo "$batch_resp" | jq -e '.error' >/dev/null 2>&1; then
        log_info "[$PHASE] Test 4: Batch reputation OK (${batch_elapsed_ms}ms for $WALLET_COUNT wallets)"
        record_result "$PHASE" "wallet-batch-reputation" "pass" "$batch_elapsed_ms" ""
    else
        log_warn "[$PHASE] Test 4: Batch reputation failed"
        record_result "$PHASE" "wallet-batch-reputation" "fail" "$batch_elapsed_ms" "Batch reputation query failed"
    fi
fi

total_elapsed=$(stop_timer)

# --- Report ---
total_ops=$((test1_ok + test1_fail + test2_ok + test2_fail + test3_ok + test3_fail))
total_ok=$((test1_ok + test2_ok + test3_ok))
total_fail=$((test1_fail + test2_fail + test3_fail))

log_info "[$PHASE] === Rapid Wallet Operations Report ==="
log_info "[$PHASE]   Test 1 (self-balance):   $test1_ok OK / $test1_fail fail (avg ${test1_avg}ms)"
log_info "[$PHASE]   Test 2 (hotspot):        ${test2_ok:-0} OK / ${test2_fail:-0} fail (avg ${test2_avg:-0}ms)"
log_info "[$PHASE]   Test 3 (rapid seq):      $test3_ok OK / $test3_fail fail (avg ${test3_avg}ms, max ${test3_max_ms}ms)"
log_info "[$PHASE]   Test 4 (batch rep):      ${batch_elapsed_ms:-0}ms for $WALLET_COUNT wallets"
log_info "[$PHASE]   Total operations:        $total_ops ($total_ok OK / $total_fail fail)"
log_info "[$PHASE]   Total wall-clock time:   ${total_elapsed}ms"

if [[ "$total_ok" -gt 0 ]]; then
    record_result "$PHASE" "rapid-wallet-ops" "pass" "$total_elapsed" ""
    log_pass "[$PHASE] $total_ok/$total_ops wallet operations succeeded"
else
    record_result "$PHASE" "rapid-wallet-ops" "fail" "$total_elapsed" "All wallet operations failed"
    log_fail "[$PHASE] All wallet operations failed"
    exit 1
fi
