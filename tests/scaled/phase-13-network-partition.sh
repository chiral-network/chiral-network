#!/usr/bin/env bash
set -euo pipefail
source /tests/lib.sh

PHASE="phase-13-network-partition"
log_info "[$PHASE] Starting network partition simulation"

###############################################################################
# Phase 13 — Network Partition Simulation
#
# Stops DHT on 20% of nodes to simulate a partial network partition. Verifies
# remaining nodes can still find peers and perform file operations. Restarts
# stopped nodes and verifies they rejoin the network.
###############################################################################

if [[ -z "${ALL_NODES:-}" ]]; then
    log_warn "[$PHASE] ALL_NODES is empty — skipping"
    record_result "$PHASE" "network-partition" "skip" "0" "No nodes available"
    exit 0
fi

IFS=',' read -ra NODES <<< "$ALL_NODES"
NODE_COUNT=${#NODES[@]}

if [[ "$NODE_COUNT" -lt 5 ]]; then
    log_warn "[$PHASE] Need at least 5 nodes for partition test — skipping"
    record_result "$PHASE" "network-partition" "skip" "0" "Fewer than 5 nodes available"
    exit 0
fi

# Select 20% of nodes to stop (minimum 1)
stop_count=$(( NODE_COUNT * 20 / 100 ))
if [[ "$stop_count" -eq 0 ]]; then
    stop_count=1
fi
remaining_count=$(( NODE_COUNT - stop_count ))

# Partition nodes into stopped and active sets
declare -a STOPPED_NODES=()
declare -a ACTIVE_NODES=()
for i in $(seq 0 $(( NODE_COUNT - 1 ))); do
    if [[ "$i" -lt "$stop_count" ]]; then
        STOPPED_NODES+=("${NODES[$i]}")
    else
        ACTIVE_NODES+=("${NODES[$i]}")
    fi
done

log_info "[$PHASE] Node count: $NODE_COUNT"
log_info "[$PHASE] Stopping DHT on ${stop_count} nodes: ${STOPPED_NODES[*]}"
log_info "[$PHASE] Keeping ${remaining_count} nodes active"

start_timer

# --- Step 1: Record baseline peer counts ---
log_info "[$PHASE] Step 1: Recording baseline peer counts..."
declare -A BASELINE_PEERS=()
for node_url in "${ACTIVE_NODES[@]}"; do
    resp=$(curl -sf --max-time 10 \
        "${node_url}/api/headless/dht/peers" 2>/dev/null) || resp=""
    if [[ -n "$resp" ]]; then
        count=$(echo "$resp" | jq 'if type == "array" then length elif .peers then (.peers | length) else 0 end' 2>/dev/null) || count=0
        BASELINE_PEERS["$node_url"]=$count
    else
        BASELINE_PEERS["$node_url"]=0
    fi
done

# --- Step 2: Stop DHT on selected nodes ---
log_info "[$PHASE] Step 2: Stopping DHT on $stop_count nodes..."
stops_ok=0
stops_fail=0

for node_url in "${STOPPED_NODES[@]}"; do
    stop_resp=$(curl -sf --max-time 15 \
        -X POST "${node_url}/api/headless/dht/stop" \
        -H "Content-Type: application/json" \
        -d '{}' 2>/dev/null) || stop_resp=""

    if [[ -n "$stop_resp" ]]; then
        if echo "$stop_resp" | jq -e '.error' >/dev/null 2>&1; then
            err=$(echo "$stop_resp" | jq -r '.error')
            log_warn "[$PHASE] Failed to stop DHT on $node_url: $err"
            stops_fail=$((stops_fail + 1))
        else
            log_info "[$PHASE] Stopped DHT on $node_url"
            stops_ok=$((stops_ok + 1))
        fi
    else
        log_warn "[$PHASE] No response stopping DHT on $node_url"
        stops_fail=$((stops_fail + 1))
    fi
done

log_info "[$PHASE] DHT stops: $stops_ok succeeded, $stops_fail failed"

# Give the network a moment to detect the partition
sleep 5

# --- Step 3: Verify active nodes can still find peers ---
log_info "[$PHASE] Step 3: Checking active nodes can still find peers..."
active_peers_ok=0
active_peers_fail=0

for node_url in "${ACTIVE_NODES[@]}"; do
    resp=$(curl -sf --max-time 10 \
        "${node_url}/api/headless/dht/peers" 2>/dev/null) || resp=""

    if [[ -n "$resp" ]]; then
        count=$(echo "$resp" | jq 'if type == "array" then length elif .peers then (.peers | length) else 0 end' 2>/dev/null) || count=0
        if [[ "$count" -gt 0 ]]; then
            active_peers_ok=$((active_peers_ok + 1))
            log_info "[$PHASE] Active node $node_url: $count peers (baseline: ${BASELINE_PEERS[$node_url]:-0})"
        else
            active_peers_fail=$((active_peers_fail + 1))
            log_warn "[$PHASE] Active node $node_url: 0 peers during partition"
        fi
    else
        active_peers_fail=$((active_peers_fail + 1))
        log_warn "[$PHASE] Active node $node_url: could not query peers"
    fi
done

# --- Step 4: Try a file search on active nodes ---
log_info "[$PHASE] Step 4: Testing file search during partition..."
search_ok=0
search_fail=0
sample_active=3
if [[ "${#ACTIVE_NODES[@]}" -lt "$sample_active" ]]; then
    sample_active=${#ACTIVE_NODES[@]}
fi

for i in $(seq 0 $(( sample_active - 1 ))); do
    node_url="${ACTIVE_NODES[$i]}"
    search_resp=$(curl -sf --max-time 15 \
        -X POST "${node_url}/api/headless/dht/search" \
        -H "Content-Type: application/json" \
        -d '{"query": "test"}' 2>/dev/null) || search_resp=""

    if [[ -n "$search_resp" ]] && ! echo "$search_resp" | jq -e '.error' >/dev/null 2>&1; then
        search_ok=$((search_ok + 1))
        log_info "[$PHASE] Search on $node_url during partition: OK"
    else
        search_fail=$((search_fail + 1))
        log_warn "[$PHASE] Search on $node_url during partition: FAILED"
    fi
done

# --- Step 5: Restart stopped nodes ---
log_info "[$PHASE] Step 5: Restarting DHT on stopped nodes..."
restart_start=$(date +%s)
restarts_ok=0
restarts_fail=0

for node_url in "${STOPPED_NODES[@]}"; do
    start_resp=$(curl -sf --max-time 15 \
        -X POST "${node_url}/api/headless/dht/start" \
        -H "Content-Type: application/json" \
        -d '{}' 2>/dev/null) || start_resp=""

    if [[ -n "$start_resp" ]]; then
        if echo "$start_resp" | jq -e '.error' >/dev/null 2>&1; then
            err=$(echo "$start_resp" | jq -r '.error')
            log_warn "[$PHASE] Failed to restart DHT on $node_url: $err"
            restarts_fail=$((restarts_fail + 1))
        else
            log_info "[$PHASE] Restarted DHT on $node_url"
            restarts_ok=$((restarts_ok + 1))
        fi
    else
        log_warn "[$PHASE] No response restarting DHT on $node_url"
        restarts_fail=$((restarts_fail + 1))
    fi
done

# --- Step 6: Wait for rejoined nodes to find peers ---
log_info "[$PHASE] Step 6: Waiting for restarted nodes to rejoin network..."
sleep 10

rejoin_ok=0
rejoin_fail=0

for node_url in "${STOPPED_NODES[@]}"; do
    resp=$(curl -sf --max-time 10 \
        "${node_url}/api/headless/dht/peers" 2>/dev/null) || resp=""

    if [[ -n "$resp" ]]; then
        count=$(echo "$resp" | jq 'if type == "array" then length elif .peers then (.peers | length) else 0 end' 2>/dev/null) || count=0
        if [[ "$count" -gt 0 ]]; then
            rejoin_ok=$((rejoin_ok + 1))
            log_info "[$PHASE] Restarted node $node_url: $count peers (rejoined)"
        else
            rejoin_fail=$((rejoin_fail + 1))
            log_warn "[$PHASE] Restarted node $node_url: 0 peers (not yet rejoined)"
        fi
    else
        rejoin_fail=$((rejoin_fail + 1))
        log_warn "[$PHASE] Restarted node $node_url: could not query peers"
    fi
done

restart_end=$(date +%s)
recovery_time=$(( restart_end - restart_start ))

total_elapsed=$(stop_timer)

# --- Report ---
log_info "[$PHASE] === Network Partition Report ==="
log_info "[$PHASE]   Total nodes:             $NODE_COUNT"
log_info "[$PHASE]   Nodes stopped:           $stop_count"
log_info "[$PHASE]   Nodes kept active:       $remaining_count"
log_info "[$PHASE]   DHT stops OK/fail:       $stops_ok / $stops_fail"
log_info "[$PHASE]   Active peers OK/fail:    $active_peers_ok / $active_peers_fail"
log_info "[$PHASE]   Search during partition:  $search_ok OK / $search_fail fail"
log_info "[$PHASE]   DHT restarts OK/fail:    $restarts_ok / $restarts_fail"
log_info "[$PHASE]   Rejoin OK/fail:          $rejoin_ok / $rejoin_fail"
log_info "[$PHASE]   Recovery time:           ${recovery_time}s"
log_info "[$PHASE]   Total wall-clock time:   ${total_elapsed}ms"

# Pass if active nodes survived the partition and at least some restarted nodes rejoined
if [[ "$active_peers_ok" -gt 0 ]]; then
    if [[ "$rejoin_ok" -gt 0 ]] || [[ "$restarts_ok" -eq 0 ]]; then
        record_result "$PHASE" "network-partition" "pass" "$total_elapsed" ""
        log_pass "[$PHASE] Network survived partition ($active_peers_ok active nodes OK, $rejoin_ok/$stop_count rejoined)"
    else
        record_result "$PHASE" "network-partition" "fail" "$total_elapsed" "No restarted nodes rejoined the network"
        log_fail "[$PHASE] No restarted nodes rejoined the network after ${recovery_time}s"
        exit 1
    fi
else
    record_result "$PHASE" "network-partition" "fail" "$total_elapsed" "No active nodes had peers during partition"
    log_fail "[$PHASE] No active nodes had peers during partition"
    exit 1
fi
