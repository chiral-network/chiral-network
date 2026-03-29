#!/usr/bin/env bash
set -euo pipefail
source /tests/lib.sh

PHASE="phase-16-dht-flood"
log_info "[$PHASE] Starting DHT flood stress test"

###############################################################################
# Phase 16 — DHT Flood
#
# Each node stores 10 unique key-value pairs via DHT put, then each node
# retrieves 10 random keys from other nodes via DHT get. Measures put/get
# success rates and latency. Tests with increasing concurrency.
###############################################################################

if [[ -z "${ALL_NODES:-}" ]]; then
    log_warn "[$PHASE] ALL_NODES is empty — skipping"
    record_result "$PHASE" "dht-flood" "skip" "0" "No nodes available"
    exit 0
fi

IFS=',' read -ra NODES <<< "$ALL_NODES"
NODE_COUNT=${#NODES[@]}

if [[ "$NODE_COUNT" -lt 2 ]]; then
    log_warn "[$PHASE] Need at least 2 nodes for DHT flood test — skipping"
    record_result "$PHASE" "dht-flood" "skip" "0" "Fewer than 2 nodes"
    exit 0
fi

KEYS_PER_NODE=10
GETS_PER_NODE=10

log_info "[$PHASE] Nodes: $NODE_COUNT, keys per node: $KEYS_PER_NODE, gets per node: $GETS_PER_NODE"

start_timer

# =========================================================================
# Step 1: Sequential puts — each node stores 10 unique keys
# =========================================================================
log_info "[$PHASE] Step 1: Sequential DHT puts ($((NODE_COUNT * KEYS_PER_NODE)) total)..."

put_ok=0
put_fail=0
put_total_ms=0
put_max_ms=0

# Track all keys we've stored for later retrieval
declare -a ALL_KEYS=()
declare -A KEY_VALUES=()

for i in $(seq 0 $(( NODE_COUNT - 1 ))); do
    node_url="${NODES[$i]}"
    for k in $(seq 1 "$KEYS_PER_NODE"); do
        key="flood-node${i}-key${k}-${RANDOM}"
        value="value-node${i}-key${k}-$(date +%s%N)"

        ALL_KEYS+=("$key")
        KEY_VALUES["$key"]="$value"

        req_start=$(date +%s%N)
        resp=$(curl -sf --max-time 15 \
            -X POST "${node_url}/api/headless/dht/put" \
            -H "Content-Type: application/json" \
            -d "{\"key\": \"$key\", \"value\": \"$value\"}" 2>/dev/null) || resp=""
        req_end=$(date +%s%N)
        elapsed_ms=$(( (req_end - req_start) / 1000000 ))

        if [[ -n "$resp" ]] && ! echo "$resp" | jq -e '.error' >/dev/null 2>&1; then
            put_ok=$((put_ok + 1))
            put_total_ms=$((put_total_ms + elapsed_ms))
            if [[ "$elapsed_ms" -gt "$put_max_ms" ]]; then
                put_max_ms=$elapsed_ms
            fi
        else
            put_fail=$((put_fail + 1))
        fi
    done
done

put_avg=0
if [[ "$put_ok" -gt 0 ]]; then
    put_avg=$((put_total_ms / put_ok))
fi

total_keys=${#ALL_KEYS[@]}
log_info "[$PHASE] Sequential puts: $put_ok OK, $put_fail fail (avg ${put_avg}ms, max ${put_max_ms}ms)"

# Give DHT a moment to propagate
sleep 3

# =========================================================================
# Step 2: Sequential gets — each node retrieves 10 random keys
# =========================================================================
log_info "[$PHASE] Step 2: Sequential DHT gets ($((NODE_COUNT * GETS_PER_NODE)) total)..."

get_ok=0
get_fail=0
get_consistent=0
get_inconsistent=0
get_total_ms=0
get_max_ms=0

for i in $(seq 0 $(( NODE_COUNT - 1 ))); do
    node_url="${NODES[$i]}"
    for g in $(seq 1 "$GETS_PER_NODE"); do
        # Pick a random key (avoid keys stored on this same node if possible)
        if [[ "$total_keys" -gt 0 ]]; then
            rand_idx=$(( RANDOM % total_keys ))
            key="${ALL_KEYS[$rand_idx]}"
            expected_value="${KEY_VALUES[$key]}"
        else
            continue
        fi

        req_start=$(date +%s%N)
        resp=$(curl -sf --max-time 15 \
            -X POST "${node_url}/api/headless/dht/get" \
            -H "Content-Type: application/json" \
            -d "{\"key\": \"$key\"}" 2>/dev/null) || resp=""
        req_end=$(date +%s%N)
        elapsed_ms=$(( (req_end - req_start) / 1000000 ))

        if [[ -n "$resp" ]] && ! echo "$resp" | jq -e '.error' >/dev/null 2>&1; then
            get_ok=$((get_ok + 1))
            get_total_ms=$((get_total_ms + elapsed_ms))
            if [[ "$elapsed_ms" -gt "$get_max_ms" ]]; then
                get_max_ms=$elapsed_ms
            fi

            # Check consistency: did we get back what we stored?
            actual_value=$(echo "$resp" | jq -r '.value // empty' 2>/dev/null) || actual_value=""
            if [[ "$actual_value" == "$expected_value" ]]; then
                get_consistent=$((get_consistent + 1))
            else
                get_inconsistent=$((get_inconsistent + 1))
            fi
        else
            get_fail=$((get_fail + 1))
        fi
    done
done

get_avg=0
if [[ "$get_ok" -gt 0 ]]; then
    get_avg=$((get_total_ms / get_ok))
fi
log_info "[$PHASE] Sequential gets: $get_ok OK, $get_fail fail, $get_consistent consistent, $get_inconsistent inconsistent"

# =========================================================================
# Step 3: Concurrent puts — all nodes put simultaneously
# =========================================================================
log_info "[$PHASE] Step 3: Concurrent DHT puts (all nodes at once)..."

TMPDIR_CPUT=$(mktemp -d)
cput_ok=0
cput_fail=0
cput_total_ms=0

for i in $(seq 0 $(( NODE_COUNT - 1 ))); do
    node_url="${NODES[$i]}"
    result_file="${TMPDIR_CPUT}/node-${i}.json"
    (
        node_ok=0
        node_fail=0
        node_ms=0
        for k in $(seq 1 "$KEYS_PER_NODE"); do
            key="cflood-node${i}-key${k}-${RANDOM}"
            value="cvalue-node${i}-key${k}-$(date +%s%N)"

            req_start=$(date +%s%N)
            resp=$(curl -sf --max-time 15 \
                -X POST "${node_url}/api/headless/dht/put" \
                -H "Content-Type: application/json" \
                -d "{\"key\": \"$key\", \"value\": \"$value\"}" 2>/dev/null) || resp=""
            req_end=$(date +%s%N)
            elapsed_ms=$(( (req_end - req_start) / 1000000 ))

            if [[ -n "$resp" ]] && ! echo "$resp" | jq -e '.error' >/dev/null 2>&1; then
                node_ok=$((node_ok + 1))
                node_ms=$((node_ms + elapsed_ms))
            else
                node_fail=$((node_fail + 1))
            fi
        done
        echo "{\"ok\":$node_ok,\"fail\":$node_fail,\"total_ms\":$node_ms}" > "$result_file"
    ) &
done
wait

for i in $(seq 0 $(( NODE_COUNT - 1 ))); do
    result_file="${TMPDIR_CPUT}/node-${i}.json"
    if [[ -f "$result_file" ]]; then
        ok=$(jq -r '.ok' "$result_file" 2>/dev/null) || ok=0
        fail=$(jq -r '.fail' "$result_file" 2>/dev/null) || fail=0
        ms=$(jq -r '.total_ms' "$result_file" 2>/dev/null) || ms=0
        cput_ok=$((cput_ok + ok))
        cput_fail=$((cput_fail + fail))
        cput_total_ms=$((cput_total_ms + ms))
    fi
done
rm -rf "$TMPDIR_CPUT"

cput_avg=0
if [[ "$cput_ok" -gt 0 ]]; then
    cput_avg=$((cput_total_ms / cput_ok))
fi
log_info "[$PHASE] Concurrent puts: $cput_ok OK, $cput_fail fail (avg ${cput_avg}ms)"

# =========================================================================
# Step 4: Concurrent gets — all nodes get simultaneously
# =========================================================================
log_info "[$PHASE] Step 4: Concurrent DHT gets (all nodes at once)..."

TMPDIR_CGET=$(mktemp -d)
cget_ok=0
cget_fail=0
cget_total_ms=0

for i in $(seq 0 $(( NODE_COUNT - 1 ))); do
    node_url="${NODES[$i]}"
    result_file="${TMPDIR_CGET}/node-${i}.json"
    (
        node_ok=0
        node_fail=0
        node_ms=0
        for g in $(seq 1 "$GETS_PER_NODE"); do
            if [[ "$total_keys" -gt 0 ]]; then
                rand_idx=$(( RANDOM % total_keys ))
                key="${ALL_KEYS[$rand_idx]}"
            else
                continue
            fi

            req_start=$(date +%s%N)
            resp=$(curl -sf --max-time 15 \
                -X POST "${node_url}/api/headless/dht/get" \
                -H "Content-Type: application/json" \
                -d "{\"key\": \"$key\"}" 2>/dev/null) || resp=""
            req_end=$(date +%s%N)
            elapsed_ms=$(( (req_end - req_start) / 1000000 ))

            if [[ -n "$resp" ]] && ! echo "$resp" | jq -e '.error' >/dev/null 2>&1; then
                node_ok=$((node_ok + 1))
                node_ms=$((node_ms + elapsed_ms))
            else
                node_fail=$((node_fail + 1))
            fi
        done
        echo "{\"ok\":$node_ok,\"fail\":$node_fail,\"total_ms\":$node_ms}" > "$result_file"
    ) &
done
wait

for i in $(seq 0 $(( NODE_COUNT - 1 ))); do
    result_file="${TMPDIR_CGET}/node-${i}.json"
    if [[ -f "$result_file" ]]; then
        ok=$(jq -r '.ok' "$result_file" 2>/dev/null) || ok=0
        fail=$(jq -r '.fail' "$result_file" 2>/dev/null) || fail=0
        ms=$(jq -r '.total_ms' "$result_file" 2>/dev/null) || ms=0
        cget_ok=$((cget_ok + ok))
        cget_fail=$((cget_fail + fail))
        cget_total_ms=$((cget_total_ms + ms))
    fi
done
rm -rf "$TMPDIR_CGET"

cget_avg=0
if [[ "$cget_ok" -gt 0 ]]; then
    cget_avg=$((cget_total_ms / cget_ok))
fi
log_info "[$PHASE] Concurrent gets: $cget_ok OK, $cget_fail fail (avg ${cget_avg}ms)"

total_elapsed=$(stop_timer)

# --- Report ---
total_put=$((put_ok + put_fail + cput_ok + cput_fail))
total_get=$((get_ok + get_fail + cget_ok + cget_fail))
total_all_ok=$((put_ok + get_ok + cput_ok + cget_ok))
total_all_fail=$((put_fail + get_fail + cput_fail + cget_fail))

log_info "[$PHASE] === DHT Flood Report ==="
log_info "[$PHASE]   Sequential put:          $put_ok OK / $put_fail fail (avg ${put_avg}ms, max ${put_max_ms}ms)"
log_info "[$PHASE]   Sequential get:          $get_ok OK / $get_fail fail (avg ${get_avg}ms, max ${get_max_ms}ms)"
log_info "[$PHASE]   Sequential consistency:  $get_consistent consistent / $get_inconsistent inconsistent"
log_info "[$PHASE]   Concurrent put:          $cput_ok OK / $cput_fail fail (avg ${cput_avg}ms)"
log_info "[$PHASE]   Concurrent get:          $cget_ok OK / $cget_fail fail (avg ${cget_avg}ms)"
log_info "[$PHASE]   Total puts:              $total_put"
log_info "[$PHASE]   Total gets:              $total_get"
log_info "[$PHASE]   Total wall-clock time:   ${total_elapsed}ms"

if [[ "$total_all_ok" -gt 0 ]]; then
    record_result "$PHASE" "dht-flood" "pass" "$total_elapsed" ""
    log_pass "[$PHASE] DHT flood: $total_all_ok operations succeeded ($total_all_fail failed)"
else
    record_result "$PHASE" "dht-flood" "fail" "$total_elapsed" "All DHT flood operations failed"
    log_fail "[$PHASE] All DHT flood operations failed"
    exit 1
fi
