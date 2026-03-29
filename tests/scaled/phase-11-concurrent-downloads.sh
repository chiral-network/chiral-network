#!/usr/bin/env bash
set -euo pipefail
source /tests/lib.sh

PHASE="phase-11-concurrent-downloads"
log_info "[$PHASE] Starting concurrent download stress test"

###############################################################################
# Phase 11 — Concurrent Downloads
#
# ALL consumer nodes attempt to download the SAME file simultaneously from the
# same seeder. Tests DHT under concurrent read load, seeder bandwidth handling,
# and chunked transfer protocol with many simultaneous connections.
###############################################################################

PUBLISHED_FILES="/results/published-files.txt"

if [[ -z "${CONSUMER_NODES:-}" ]]; then
    log_warn "[$PHASE] CONSUMER_NODES is empty — skipping"
    record_result "$PHASE" "concurrent-downloads" "skip" "0" "No consumer nodes available"
    exit 0
fi

if [[ ! -f "$PUBLISHED_FILES" ]] || [[ ! -s "$PUBLISHED_FILES" ]]; then
    log_warn "[$PHASE] No published files found at $PUBLISHED_FILES — skipping"
    record_result "$PHASE" "concurrent-downloads" "skip" "0" "No published files available"
    exit 0
fi

IFS=',' read -ra CONSUMERS <<< "$CONSUMER_NODES"
CONSUMER_COUNT=${#CONSUMERS[@]}

if [[ "$CONSUMER_COUNT" -lt 2 ]]; then
    log_warn "[$PHASE] Need at least 2 consumer nodes — skipping"
    record_result "$PHASE" "concurrent-downloads" "skip" "0" "Fewer than 2 consumer nodes"
    exit 0
fi

# Pick the first published file as our target
TARGET_FILE=$(head -1 "$PUBLISHED_FILES")
if [[ -z "$TARGET_FILE" ]]; then
    log_warn "[$PHASE] Could not read a published file entry — skipping"
    record_result "$PHASE" "concurrent-downloads" "skip" "0" "Empty published file entry"
    exit 0
fi

# Parse the published file entry — expect format: hash|filename|seeder_url or just hash
FILE_HASH=""
FILE_NAME=""
SEEDER_URL=""
if echo "$TARGET_FILE" | grep -q '|'; then
    IFS='|' read -r FILE_HASH FILE_NAME SEEDER_URL <<< "$TARGET_FILE"
else
    FILE_HASH="$TARGET_FILE"
    FILE_NAME="unknown"
fi

log_info "[$PHASE] Target file hash: $FILE_HASH"
log_info "[$PHASE] Target file name: $FILE_NAME"
log_info "[$PHASE] Consumer nodes: $CONSUMER_COUNT"

# --- Fire all downloads simultaneously ---
TMPDIR_RESULTS=$(mktemp -d)
download_successes=0
download_failures=0
declare -a ELAPSED_TIMES=()

start_timer

# Launch all download requests in parallel
for i in $(seq 0 $(( CONSUMER_COUNT - 1 ))); do
    consumer_url="${CONSUMERS[$i]}"
    result_file="${TMPDIR_RESULTS}/consumer-${i}.json"
    (
        req_start=$(date +%s%N)
        resp=$(curl -sf --max-time 120 \
            -X POST "${consumer_url}/api/headless/dht/download" \
            -H "Content-Type: application/json" \
            -d "{\"hash\": \"$FILE_HASH\", \"fileName\": \"$FILE_NAME\"}" 2>/dev/null) || resp=""
        req_end=$(date +%s%N)
        elapsed_ms=$(( (req_end - req_start) / 1000000 ))

        if [[ -n "$resp" ]] && ! echo "$resp" | jq -e '.error' >/dev/null 2>&1; then
            echo "{\"status\":\"ok\",\"elapsed_ms\":$elapsed_ms,\"node\":\"$consumer_url\"}" > "$result_file"
        else
            err_msg=$(echo "$resp" | jq -r '.error // "no response"' 2>/dev/null) || err_msg="no response"
            echo "{\"status\":\"fail\",\"elapsed_ms\":$elapsed_ms,\"node\":\"$consumer_url\",\"error\":\"$err_msg\"}" > "$result_file"
        fi
    ) &
done

# Wait for all background downloads to complete
wait

total_elapsed=$(stop_timer)

# Collect results
total_time=0
max_time=0

for i in $(seq 0 $(( CONSUMER_COUNT - 1 ))); do
    result_file="${TMPDIR_RESULTS}/consumer-${i}.json"
    if [[ ! -f "$result_file" ]]; then
        download_failures=$((download_failures + 1))
        continue
    fi

    status=$(jq -r '.status' "$result_file" 2>/dev/null) || status="fail"
    elapsed_ms=$(jq -r '.elapsed_ms' "$result_file" 2>/dev/null) || elapsed_ms=0
    node=$(jq -r '.node' "$result_file" 2>/dev/null) || node="unknown"

    if [[ "$status" == "ok" ]]; then
        download_successes=$((download_successes + 1))
        total_time=$((total_time + elapsed_ms))
        if [[ "$elapsed_ms" -gt "$max_time" ]]; then
            max_time=$elapsed_ms
        fi
        ELAPSED_TIMES+=("$elapsed_ms")
        log_info "[$PHASE] Consumer $node: download OK (${elapsed_ms}ms)"
    else
        download_failures=$((download_failures + 1))
        err=$(jq -r '.error' "$result_file" 2>/dev/null) || err="unknown"
        log_warn "[$PHASE] Consumer $node: download FAILED ($err)"
    fi
done

rm -rf "$TMPDIR_RESULTS"

# Calculate averages
avg_time=0
if [[ "$download_successes" -gt 0 ]]; then
    avg_time=$((total_time / download_successes))
fi

success_rate=0
total_attempted=$((download_successes + download_failures))
if [[ "$total_attempted" -gt 0 ]]; then
    success_rate=$((download_successes * 100 / total_attempted))
fi

# --- Report ---
log_info "[$PHASE] === Concurrent Download Report ==="
log_info "[$PHASE]   Target file:            $FILE_HASH"
log_info "[$PHASE]   Consumer nodes:          $CONSUMER_COUNT"
log_info "[$PHASE]   Downloads succeeded:     $download_successes"
log_info "[$PHASE]   Downloads failed:        $download_failures"
log_info "[$PHASE]   Success rate:            ${success_rate}%"
log_info "[$PHASE]   Avg download time:       ${avg_time}ms"
log_info "[$PHASE]   Max download time:       ${max_time}ms"
log_info "[$PHASE]   Total wall-clock time:   ${total_elapsed}ms"

if [[ "$total_attempted" -eq 0 ]]; then
    record_result "$PHASE" "concurrent-downloads" "skip" "$total_elapsed" "No download attempts were made"
    exit 0
fi

if [[ "$download_successes" -gt 0 ]]; then
    record_result "$PHASE" "concurrent-downloads" "pass" "$total_elapsed" ""
    log_pass "[$PHASE] $download_successes/$total_attempted concurrent downloads succeeded (avg ${avg_time}ms, max ${max_time}ms)"
else
    record_result "$PHASE" "concurrent-downloads" "fail" "$total_elapsed" "All $total_attempted concurrent downloads failed"
    log_fail "[$PHASE] All $total_attempted concurrent downloads failed"
    exit 1
fi
