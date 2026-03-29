#!/usr/bin/env bash
# Shared helper library for scaled test harness
# Source this file from phase scripts: source "$(dirname "$0")/lib.sh"

set -euo pipefail

# ---------------------------------------------------------------------------
# Globals
# ---------------------------------------------------------------------------
PASS_COUNT=0
FAIL_COUNT=0
SKIP_COUNT=0
RESULTS_FILE="${RESULTS_FILE:-/results/results.jsonl}"
LAST_STATUS=0
TIMER_START=0

# Ensure results directory exists
mkdir -p "$(dirname "$RESULTS_FILE")"

# ---------------------------------------------------------------------------
# Logging
# ---------------------------------------------------------------------------
_CLR_RESET='\033[0m'
_CLR_GREEN='\033[0;32m'
_CLR_RED='\033[0;31m'
_CLR_YELLOW='\033[0;33m'
_CLR_CYAN='\033[0;36m'

log_info()  { echo -e "${_CLR_CYAN}[INFO]${_CLR_RESET}  $*"; }
log_pass()  { echo -e "${_CLR_GREEN}[PASS]${_CLR_RESET}  $*"; }
log_fail()  { echo -e "${_CLR_RED}[FAIL]${_CLR_RESET}  $*"; }
log_warn()  { echo -e "${_CLR_YELLOW}[WARN]${_CLR_RESET}  $*"; }

# ---------------------------------------------------------------------------
# Timer helpers
# ---------------------------------------------------------------------------
start_timer() {
    TIMER_START=$(date +%s%N)
}

# Prints elapsed milliseconds since start_timer
stop_timer() {
    local now
    now=$(date +%s%N)
    echo $(( (now - TIMER_START) / 1000000 ))
}

# ---------------------------------------------------------------------------
# HTTP helpers
# ---------------------------------------------------------------------------
api_get() {
    local node="$1" path="$2"
    local url="http://${node}:9419${path}"
    local tmp
    tmp=$(mktemp)
    local body
    body=$(curl -s -o "$tmp" -w '%{http_code}' --max-time 10 "$url" 2>/dev/null) || true
    LAST_STATUS="$body"
    cat "$tmp"
    rm -f "$tmp"
}

api_post() {
    local node="$1" path="$2" body="${3:-{}}"
    local url="http://${node}:9419${path}"
    local tmp
    tmp=$(mktemp)
    local status
    status=$(curl -s -o "$tmp" -w '%{http_code}' --max-time 30 \
        -X POST -H 'Content-Type: application/json' -d "$body" "$url" 2>/dev/null) || true
    LAST_STATUS="$status"
    cat "$tmp"
    rm -f "$tmp"
}

relay_get() {
    local path="$1"
    local url="http://relay.chiral.local:8080${path}"
    local tmp
    tmp=$(mktemp)
    local status
    status=$(curl -s -o "$tmp" -w '%{http_code}' --max-time 10 "$url" 2>/dev/null) || true
    LAST_STATUS="$status"
    cat "$tmp"
    rm -f "$tmp"
}

relay_post() {
    local path="$1" body="${2:-{}}"
    local url="http://relay.chiral.local:8080${path}"
    local tmp
    tmp=$(mktemp)
    local status
    status=$(curl -s -o "$tmp" -w '%{http_code}' --max-time 30 \
        -X POST -H 'Content-Type: application/json' -d "$body" "$url" 2>/dev/null) || true
    LAST_STATUS="$status"
    cat "$tmp"
    rm -f "$tmp"
}

# ---------------------------------------------------------------------------
# Readiness polling
# ---------------------------------------------------------------------------
wait_for_ready() {
    local node="$1" timeout="${2:-60}"
    local deadline=$(( $(date +%s) + timeout ))
    log_info "Waiting for ${node} to become ready (timeout ${timeout}s)..."
    while [ "$(date +%s)" -lt "$deadline" ]; do
        local resp
        resp=$(api_get "$node" "/api/health") || true
        if [ "$LAST_STATUS" = "200" ]; then
            log_info "${node} is ready"
            return 0
        fi
        sleep 2
    done
    log_fail "${node} did not become ready within ${timeout}s"
    return 1
}

# ---------------------------------------------------------------------------
# Assertions
# ---------------------------------------------------------------------------
assert_status() {
    local expected="$1" test_name="$2"
    local duration="${3:-0}"
    if [ "$LAST_STATUS" = "$expected" ]; then
        log_pass "${test_name} (HTTP ${LAST_STATUS})"
        PASS_COUNT=$(( PASS_COUNT + 1 ))
        record_result "" "$test_name" "pass" "$duration" ""
    else
        log_fail "${test_name} (expected HTTP ${expected}, got ${LAST_STATUS})"
        FAIL_COUNT=$(( FAIL_COUNT + 1 ))
        record_result "" "$test_name" "fail" "$duration" "expected HTTP ${expected}, got ${LAST_STATUS}"
    fi
}

assert_json_field() {
    local response="$1" field="$2" expected="$3" test_name="$4"
    local duration="${5:-0}"
    local actual
    actual=$(echo "$response" | jq -r "$field" 2>/dev/null) || actual="(jq error)"
    if [ "$actual" = "$expected" ]; then
        log_pass "${test_name} (${field}=${actual})"
        PASS_COUNT=$(( PASS_COUNT + 1 ))
        record_result "" "$test_name" "pass" "$duration" ""
    else
        log_fail "${test_name} (${field}: expected '${expected}', got '${actual}')"
        FAIL_COUNT=$(( FAIL_COUNT + 1 ))
        record_result "" "$test_name" "fail" "$duration" "${field}: expected '${expected}', got '${actual}'"
    fi
}

assert_not_empty() {
    local response="$1" field="$2" test_name="$3"
    local duration="${4:-0}"
    local val
    val=$(echo "$response" | jq -r "$field" 2>/dev/null) || val=""
    if [ -n "$val" ] && [ "$val" != "null" ] && [ "$val" != "" ]; then
        log_pass "${test_name} (${field} is non-empty)"
        PASS_COUNT=$(( PASS_COUNT + 1 ))
        record_result "" "$test_name" "pass" "$duration" ""
    else
        log_fail "${test_name} (${field} is empty or null)"
        FAIL_COUNT=$(( FAIL_COUNT + 1 ))
        record_result "" "$test_name" "fail" "$duration" "${field} is empty or null"
    fi
}

# ---------------------------------------------------------------------------
# Result recording
# ---------------------------------------------------------------------------
record_result() {
    local phase="$1" test_name="$2" status="$3" duration="$4" error_msg="${5:-}"
    local ts
    ts=$(date -u +%Y-%m-%dT%H:%M:%SZ)
    local json
    json=$(jq -nc \
        --arg phase "$phase" \
        --arg test "$test_name" \
        --arg status "$status" \
        --arg duration "$duration" \
        --arg error "$error_msg" \
        --arg ts "$ts" \
        '{timestamp: $ts, phase: $phase, test: $test, status: $status, duration_ms: ($duration | tonumber), error: $error}')
    echo "$json" >> "$RESULTS_FILE"
}

# ---------------------------------------------------------------------------
# Test file generation
# ---------------------------------------------------------------------------
generate_test_file() {
    local size_bytes="$1" path="$2"
    local dir
    dir=$(dirname "$path")
    mkdir -p "$dir"
    dd if=/dev/urandom of="$path" bs=1 count="$size_bytes" 2>/dev/null
}

# ---------------------------------------------------------------------------
# Summary
# ---------------------------------------------------------------------------
print_summary() {
    local total=$(( PASS_COUNT + FAIL_COUNT + SKIP_COUNT ))
    echo ""
    echo "========================================"
    echo " Results: ${PASS_COUNT} passed, ${FAIL_COUNT} failed, ${SKIP_COUNT} skipped (${total} total)"
    echo "========================================"
    echo ""
    if [ "$FAIL_COUNT" -gt 0 ]; then
        return 1
    fi
    return 0
}

# ---------------------------------------------------------------------------
# Node list helper — reads NODE_LIST env var (comma-separated) into array
# ---------------------------------------------------------------------------
get_node_list() {
    local IFS=','
    read -ra NODES <<< "${NODE_LIST:-}"
    echo "${NODES[@]}"
}
