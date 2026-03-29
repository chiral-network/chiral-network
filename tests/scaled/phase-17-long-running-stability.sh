#!/usr/bin/env bash
set -euo pipefail
source /tests/lib.sh

PHASE="phase-17-long-running-stability"
log_info "[$PHASE] Starting long-running stability test"

###############################################################################
# Phase 17 — Long-Running Stability
#
# Monitors the network over 2 minutes with health checks every 5 seconds.
# Tracks: nodes dropping off, DHT peer count trends, runtime info (memory),
# and mining activity (if miners are active).
###############################################################################

if [[ -z "${ALL_NODES:-}" ]]; then
    log_warn "[$PHASE] ALL_NODES is empty — skipping"
    record_result "$PHASE" "long-running-stability" "skip" "0" "No nodes available"
    exit 0
fi

IFS=',' read -ra NODES <<< "$ALL_NODES"
NODE_COUNT=${#NODES[@]}

DURATION_SECS=120
CHECK_INTERVAL=5
TOTAL_CHECKS=$(( DURATION_SECS / CHECK_INTERVAL ))

log_info "[$PHASE] Nodes: $NODE_COUNT, duration: ${DURATION_SECS}s, interval: ${CHECK_INTERVAL}s, checks: $TOTAL_CHECKS"

# Optionally check miners
declare -a MINERS=()
if [[ -n "${MINER_NODES:-}" ]]; then
    IFS=',' read -ra MINERS <<< "$MINER_NODES"
fi

start_timer

# Track state across checks
declare -a HEALTH_HISTORY=()      # "check_num:healthy_count"
declare -a PEER_COUNT_HISTORY=()  # "check_num:total_peers"
nodes_ever_down=0
anomalies=0
anomaly_messages=()

# Record initial state
initial_healthy=0
for node_url in "${NODES[@]}"; do
    resp=$(curl -sf --max-time 5 "${node_url}/api/health" 2>/dev/null) || resp=""
    if [[ -n "$resp" ]]; then
        initial_healthy=$((initial_healthy + 1))
    fi
done
log_info "[$PHASE] Initial health: $initial_healthy/$NODE_COUNT nodes healthy"

# --- Main monitoring loop ---
for check in $(seq 1 "$TOTAL_CHECKS"); do
    check_start=$(date +%s)

    healthy_count=0
    unhealthy_nodes=()
    total_peer_count=0

    # Health check all nodes
    for i in $(seq 0 $(( NODE_COUNT - 1 ))); do
        node_url="${NODES[$i]}"

        # Health check
        health_resp=$(curl -sf --max-time 5 "${node_url}/api/health" 2>/dev/null) || health_resp=""
        if [[ -n "$health_resp" ]]; then
            healthy_count=$((healthy_count + 1))
        else
            unhealthy_nodes+=("$node_url")
        fi

        # Peer count (sample a few nodes to avoid overwhelming the network)
        if [[ $(( i % 3 )) -eq 0 ]] || [[ "$NODE_COUNT" -le 5 ]]; then
            peer_resp=$(curl -sf --max-time 5 \
                "${node_url}/api/headless/dht/peers" 2>/dev/null) || peer_resp=""
            if [[ -n "$peer_resp" ]]; then
                pcount=$(echo "$peer_resp" | jq 'if type == "array" then length elif .peers then (.peers | length) else 0 end' 2>/dev/null) || pcount=0
                total_peer_count=$((total_peer_count + pcount))
            fi
        fi
    done

    HEALTH_HISTORY+=("${check}:${healthy_count}")
    PEER_COUNT_HISTORY+=("${check}:${total_peer_count}")

    # Detect nodes dropping off
    if [[ "$healthy_count" -lt "$initial_healthy" ]]; then
        dropped=$(( initial_healthy - healthy_count ))
        nodes_ever_down=$((nodes_ever_down + dropped))
        log_warn "[$PHASE] Check $check/$TOTAL_CHECKS: $dropped node(s) unhealthy: ${unhealthy_nodes[*]}"
        anomalies=$((anomalies + 1))
        anomaly_messages+=("Check $check: $dropped node(s) dropped off")
    fi

    # Log periodic status
    if [[ $(( check % 4 )) -eq 0 ]] || [[ "$check" -eq 1 ]]; then
        log_info "[$PHASE] Check $check/$TOTAL_CHECKS: $healthy_count/$NODE_COUNT healthy, total peer connections: $total_peer_count"
    fi

    # Check runtime/memory on a sample node (once every 6 checks to reduce noise)
    if [[ $(( check % 6 )) -eq 0 ]]; then
        sample_node="${NODES[0]}"
        runtime_resp=$(curl -sf --max-time 5 \
            "${sample_node}/api/headless/runtime" 2>/dev/null) || runtime_resp=""

        if [[ -n "$runtime_resp" ]]; then
            mem_mb=$(echo "$runtime_resp" | jq -r '.memory_mb // .memoryMb // .rss_mb // empty' 2>/dev/null) || mem_mb=""
            uptime=$(echo "$runtime_resp" | jq -r '.uptime // .uptime_seconds // empty' 2>/dev/null) || uptime=""
            if [[ -n "$mem_mb" ]]; then
                log_info "[$PHASE] Runtime sample ($sample_node): memory=${mem_mb}MB, uptime=${uptime:-unknown}s"
                # Flag if memory seems very high (>2GB as a rough threshold)
                high_mem=$(awk -v m="$mem_mb" 'BEGIN { print (m > 2048) ? "yes" : "no" }' 2>/dev/null) || high_mem="no"
                if [[ "$high_mem" == "yes" ]]; then
                    anomalies=$((anomalies + 1))
                    anomaly_messages+=("Check $check: High memory on $sample_node (${mem_mb}MB)")
                    log_warn "[$PHASE] High memory detected on $sample_node: ${mem_mb}MB"
                fi
            fi
        fi
    fi

    # Check mining status (once every 8 checks)
    if [[ $(( check % 8 )) -eq 0 ]] && [[ ${#MINERS[@]} -gt 0 ]]; then
        miner_url="${MINERS[0]}"
        mining_resp=$(curl -sf --max-time 5 \
            "${miner_url}/api/headless/mining/status" 2>/dev/null) || mining_resp=""

        if [[ -n "$mining_resp" ]]; then
            mining_active=$(echo "$mining_resp" | jq -r '.mining // .active // false' 2>/dev/null) || mining_active="unknown"
            block_num=$(echo "$mining_resp" | jq -r '.blockNumber // .currentBlock // empty' 2>/dev/null) || block_num=""
            log_info "[$PHASE] Mining sample ($miner_url): active=$mining_active, block=${block_num:-unknown}"
        fi
    fi

    # Wait for next check interval
    check_end=$(date +%s)
    check_duration=$(( check_end - check_start ))
    sleep_time=$(( CHECK_INTERVAL - check_duration ))
    if [[ "$sleep_time" -gt 0 ]]; then
        sleep "$sleep_time"
    fi
done

total_elapsed=$(stop_timer)

# --- Analyze trends ---
# Check if peer count declined over time
first_peers=0
last_peers=0
if [[ ${#PEER_COUNT_HISTORY[@]} -ge 2 ]]; then
    first_peers=$(echo "${PEER_COUNT_HISTORY[0]}" | cut -d: -f2)
    last_peers=$(echo "${PEER_COUNT_HISTORY[-1]}" | cut -d: -f2)
fi

peer_trend="stable"
if [[ "$first_peers" -gt 0 ]] && [[ "$last_peers" -gt 0 ]]; then
    # Decline of more than 30% is concerning
    threshold=$(( first_peers * 70 / 100 ))
    if [[ "$last_peers" -lt "$threshold" ]]; then
        peer_trend="declining"
        anomalies=$((anomalies + 1))
        anomaly_messages+=("Peer count declined from $first_peers to $last_peers (>${30}% drop)")
    fi
fi

# Check final health
final_healthy=0
for node_url in "${NODES[@]}"; do
    resp=$(curl -sf --max-time 5 "${node_url}/api/health" 2>/dev/null) || resp=""
    if [[ -n "$resp" ]]; then
        final_healthy=$((final_healthy + 1))
    fi
done

# --- Report ---
log_info "[$PHASE] === Long-Running Stability Report ==="
log_info "[$PHASE]   Duration:                ${DURATION_SECS}s ($TOTAL_CHECKS checks)"
log_info "[$PHASE]   Nodes:                   $NODE_COUNT"
log_info "[$PHASE]   Initial healthy:         $initial_healthy"
log_info "[$PHASE]   Final healthy:           $final_healthy"
log_info "[$PHASE]   Peer count trend:        $peer_trend (first=$first_peers, last=$last_peers)"
log_info "[$PHASE]   Anomalies detected:      $anomalies"
log_info "[$PHASE]   Total wall-clock time:   ${total_elapsed}ms"

if [[ ${#anomaly_messages[@]} -gt 0 ]]; then
    log_info "[$PHASE]   Anomaly details:"
    for msg in "${anomaly_messages[@]}"; do
        log_info "[$PHASE]     - $msg"
    done
fi

# Pass if most nodes are still healthy and no critical anomalies
if [[ "$final_healthy" -eq 0 ]]; then
    record_result "$PHASE" "long-running-stability" "fail" "$total_elapsed" "All nodes unhealthy at end of stability test"
    log_fail "[$PHASE] All nodes unhealthy at end of stability test"
    exit 1
fi

# Allow some degradation — fail only if we lost more than half the nodes
half_nodes=$(( NODE_COUNT / 2 ))
if [[ "$final_healthy" -lt "$half_nodes" ]]; then
    record_result "$PHASE" "long-running-stability" "fail" "$total_elapsed" "Only $final_healthy/$NODE_COUNT nodes healthy after ${DURATION_SECS}s"
    log_fail "[$PHASE] Only $final_healthy/$NODE_COUNT nodes healthy after ${DURATION_SECS}s"
    exit 1
fi

record_result "$PHASE" "long-running-stability" "pass" "$total_elapsed" ""
log_pass "[$PHASE] Stability OK: $final_healthy/$NODE_COUNT nodes healthy after ${DURATION_SECS}s ($anomalies anomalies, peer trend: $peer_trend)"
