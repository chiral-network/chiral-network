#!/usr/bin/env bash
set -euo pipefail
source /tests/lib.sh

###############################################################################
# Master Orchestrator — Scaled Test Harness
#
# Parses NODE_LIST, assigns roles, runs all phases (01-10) sequentially,
# generates a report, and exits with appropriate code.
###############################################################################

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RESULTS_DIR="/results"
mkdir -p "$RESULTS_DIR"

# --- Parse NODE_LIST into array ---
# NODE_LIST is a comma-separated list of node base URLs (e.g., http://node1:9419,http://node2:9419)
if [[ -z "${NODE_LIST:-}" ]]; then
    log_fail "NODE_LIST environment variable is not set"
    echo "Usage: NODE_LIST=http://node1:9419,http://node2:9419 $0"
    exit 1
fi

IFS=',' read -ra ALL_NODES_ARRAY <<< "$NODE_LIST"
TOTAL_NODES=${#ALL_NODES_ARRAY[@]}

if [[ "$TOTAL_NODES" -eq 0 ]]; then
    log_fail "NODE_LIST is empty"
    exit 1
fi

log_info "Total nodes: $TOTAL_NODES"

# --- Assign roles: 20% miners, 30% seeders, rest consumers ---
miner_count=$(( TOTAL_NODES * 20 / 100 ))
seeder_count=$(( TOTAL_NODES * 30 / 100 ))

# Ensure at least 1 of each role when we have enough nodes
if [[ "$TOTAL_NODES" -ge 3 ]]; then
    [[ "$miner_count" -eq 0 ]] && miner_count=1
    [[ "$seeder_count" -eq 0 ]] && seeder_count=1
elif [[ "$TOTAL_NODES" -eq 2 ]]; then
    miner_count=1
    seeder_count=1
elif [[ "$TOTAL_NODES" -eq 1 ]]; then
    # Single node gets all roles
    miner_count=1
    seeder_count=0
fi

consumer_start=$(( miner_count + seeder_count ))

# Build comma-separated role lists
MINER_NODES=""
SEEDER_NODES=""
CONSUMER_NODES=""

for (( i=0; i<TOTAL_NODES; i++ )); do
    url="${ALL_NODES_ARRAY[$i]}"
    if [[ $i -lt $miner_count ]]; then
        [[ -n "$MINER_NODES" ]] && MINER_NODES+=","
        MINER_NODES+="$url"
    elif [[ $i -lt $consumer_start ]]; then
        [[ -n "$SEEDER_NODES" ]] && SEEDER_NODES+=","
        SEEDER_NODES+="$url"
    else
        [[ -n "$CONSUMER_NODES" ]] && CONSUMER_NODES+=","
        CONSUMER_NODES+="$url"
    fi
done

export ALL_NODES="$NODE_LIST"
export MINER_NODES
export SEEDER_NODES
export CONSUMER_NODES

log_info "Role assignment:"
log_info "  Miners ($miner_count):    $MINER_NODES"
log_info "  Seeders ($seeder_count):   $SEEDER_NODES"
log_info "  Consumers ($(( TOTAL_NODES - consumer_start ))): $CONSUMER_NODES"

# Clear previous results
> "$RESULTS_DIR/results.jsonl"

# --- Phase runner ---
phases_run=0
phases_passed=0
phases_failed=0
phases_skipped=0
failed_phases=()

run_phase() {
    local phase_num="$1"
    local phase_script="$2"
    local phase_name
    phase_name=$(basename "$phase_script" .sh)

    if [[ ! -f "$phase_script" ]]; then
        log_warn "Phase script not found: $phase_script — skipping"
        phases_skipped=$((phases_skipped + 1))
        return 0
    fi

    if [[ ! -x "$phase_script" ]]; then
        chmod +x "$phase_script"
    fi

    log_info "=========================================="
    log_info "PHASE $phase_num: $phase_name — START"
    log_info "=========================================="

    local start_time=$SECONDS
    local exit_code=0

    bash "$phase_script" || exit_code=$?

    local elapsed=$(( SECONDS - start_time ))
    phases_run=$((phases_run + 1))

    if [[ "$exit_code" -eq 0 ]]; then
        log_pass "PHASE $phase_num: $phase_name — PASSED (${elapsed}s)"
        phases_passed=$((phases_passed + 1))
    else
        log_fail "PHASE $phase_num: $phase_name — FAILED (exit=$exit_code, ${elapsed}s)"
        phases_failed=$((phases_failed + 1))
        failed_phases+=("$phase_name")
    fi

    return 0  # Always continue to next phase
}

# --- Run all phases in sequence ---
run_phase "01" "${SCRIPT_DIR}/phase-01-health.sh"
run_phase "02" "${SCRIPT_DIR}/phase-02-wallets.sh"
run_phase "03" "${SCRIPT_DIR}/phase-03-mining.sh"
run_phase "04" "${SCRIPT_DIR}/phase-04-drive-upload.sh"
run_phase "05" "${SCRIPT_DIR}/phase-05-file-publish.sh"
run_phase "06" "${SCRIPT_DIR}/phase-06-search-download.sh"
run_phase "07" "${SCRIPT_DIR}/phase-07-chiraldrop.sh"
run_phase "08" "${SCRIPT_DIR}/phase-08-payments.sh"
run_phase "09" "${SCRIPT_DIR}/phase-09-reputation.sh"
run_phase "10" "${SCRIPT_DIR}/phase-10-drive-crud.sh"

# --- Stress test phases ---
log_info "=========================================="
log_info "Running stress test phases (11-17)..."
log_info "=========================================="

run_phase "11" "${SCRIPT_DIR}/phase-11-concurrent-downloads.sh"
run_phase "12" "${SCRIPT_DIR}/phase-12-rapid-publish-search.sh"
run_phase "13" "${SCRIPT_DIR}/phase-13-network-partition.sh"
run_phase "14" "${SCRIPT_DIR}/phase-14-large-file-transfer.sh"
run_phase "15" "${SCRIPT_DIR}/phase-15-rapid-wallet-ops.sh"
run_phase "16" "${SCRIPT_DIR}/phase-16-dht-flood.sh"
run_phase "17" "${SCRIPT_DIR}/phase-17-long-running-stability.sh"

# --- Generate report ---
log_info "=========================================="
log_info "Generating report..."
log_info "=========================================="

if [[ -f "${SCRIPT_DIR}/report.sh" ]]; then
    bash "${SCRIPT_DIR}/report.sh" || log_warn "Report generation had errors"
fi

# --- Final summary ---
echo ""
echo "============================================"
echo "        SCALED TEST HARNESS — SUMMARY"
echo "============================================"
echo ""
echo "  Total nodes:      $TOTAL_NODES"
echo "  Miners:           $miner_count"
echo "  Seeders:          $seeder_count"
echo "  Consumers:        $(( TOTAL_NODES - consumer_start ))"
echo ""
echo "  Phases run:       $phases_run"
echo "  Phases passed:    $phases_passed"
echo "  Phases failed:    $phases_failed"
echo "  Phases skipped:   $phases_skipped"
echo ""

if [[ ${#failed_phases[@]} -gt 0 ]]; then
    echo "  Failed phases:"
    for fp in "${failed_phases[@]}"; do
        echo "    - $fp"
    done
    echo ""
fi

if [[ "$phases_failed" -eq 0 ]]; then
    echo "  Result: ALL PHASES PASSED"
    echo "============================================"
    exit 0
else
    echo "  Result: $phases_failed PHASE(S) FAILED"
    echo "============================================"
    exit 1
fi
