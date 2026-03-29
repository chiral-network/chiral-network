#!/usr/bin/env bash
# Phase 01: Health checks — verify all nodes are up, DHT running, peers discovered
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "${SCRIPT_DIR}/lib.sh"

PHASE="01-health"
PEER_RETRY_ATTEMPTS="${PEER_RETRY_ATTEMPTS:-10}"
PEER_RETRY_DELAY="${PEER_RETRY_DELAY:-5}"

log_info "=== Phase 01: Health Checks ==="

IFS=',' read -ra NODES <<< "${NODE_LIST:?NODE_LIST env var required (comma-separated node hostnames)}"
TOTAL=${#NODES[@]}
HEALTHY=0

# ---- Step 1: /api/health for each node ----
log_info "Checking /api/health on ${TOTAL} nodes..."
for node in "${NODES[@]}"; do
    start_timer
    resp=$(api_get "$node" "/api/health")
    dur=$(stop_timer)
    assert_status "200" "${node}: /api/health" "$dur"
    if [ "$LAST_STATUS" = "200" ]; then
        HEALTHY=$(( HEALTHY + 1 ))
    fi
done
log_info "Health: ${HEALTHY}/${TOTAL} nodes returned 200"

# ---- Step 2: /api/headless/runtime — check dhtRunning ----
log_info "Checking DHT runtime status..."
for node in "${NODES[@]}"; do
    start_timer
    resp=$(api_get "$node" "/api/headless/runtime")
    dur=$(stop_timer)
    assert_status "200" "${node}: /api/headless/runtime status" "$dur"
    assert_json_field "$resp" ".dhtRunning" "true" "${node}: DHT is running" "$dur"
done

# ---- Step 3: /api/headless/dht/peers — at least 1 peer (with retries) ----
log_info "Checking peer discovery (retrying up to ${PEER_RETRY_ATTEMPTS} times with ${PEER_RETRY_DELAY}s delay)..."
for node in "${NODES[@]}"; do
    found_peers=false
    for attempt in $(seq 1 "$PEER_RETRY_ATTEMPTS"); do
        start_timer
        resp=$(api_get "$node" "/api/headless/dht/peers")
        dur=$(stop_timer)

        if [ "$LAST_STATUS" != "200" ]; then
            log_warn "${node}: peers endpoint returned HTTP ${LAST_STATUS} (attempt ${attempt}/${PEER_RETRY_ATTEMPTS})"
            sleep "$PEER_RETRY_DELAY"
            continue
        fi

        peer_count=$(echo "$resp" | jq 'if type == "array" then length else .peers // [] | length end' 2>/dev/null) || peer_count=0
        if [ "$peer_count" -gt 0 ]; then
            log_pass "${node}: discovered ${peer_count} peer(s) (attempt ${attempt})"
            PASS_COUNT=$(( PASS_COUNT + 1 ))
            record_result "$PHASE" "${node}: peer discovery" "pass" "$dur" ""
            found_peers=true
            break
        fi

        log_warn "${node}: 0 peers (attempt ${attempt}/${PEER_RETRY_ATTEMPTS}), retrying..."
        sleep "$PEER_RETRY_DELAY"
    done

    if [ "$found_peers" = false ]; then
        log_fail "${node}: no peers found after ${PEER_RETRY_ATTEMPTS} attempts"
        FAIL_COUNT=$(( FAIL_COUNT + 1 ))
        record_result "$PHASE" "${node}: peer discovery" "fail" "0" "no peers after ${PEER_RETRY_ATTEMPTS} attempts"
    fi
done

# ---- Summary ----
log_info "Phase 01 complete: ${HEALTHY}/${TOTAL} nodes healthy"
print_summary
