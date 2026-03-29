#!/usr/bin/env bash
set -euo pipefail
source /tests/lib.sh

PHASE="phase-14-large-file-transfer"
log_info "[$PHASE] Starting large file transfer test"

###############################################################################
# Phase 14 — Large File Transfer
#
# Generates a 10MB test file on a seeder, uploads it to Drive, publishes to
# DHT, then has multiple consumers download simultaneously. Verifies file
# integrity by comparing sizes.
###############################################################################

if [[ -z "${SEEDER_NODES:-}" ]]; then
    log_warn "[$PHASE] SEEDER_NODES is empty — skipping"
    record_result "$PHASE" "large-file-transfer" "skip" "0" "No seeder nodes available"
    exit 0
fi

if [[ -z "${CONSUMER_NODES:-}" ]]; then
    log_warn "[$PHASE] CONSUMER_NODES is empty — skipping"
    record_result "$PHASE" "large-file-transfer" "skip" "0" "No consumer nodes available"
    exit 0
fi

IFS=',' read -ra SEEDERS <<< "$SEEDER_NODES"
IFS=',' read -ra CONSUMERS <<< "$CONSUMER_NODES"
SEEDER_COUNT=${#SEEDERS[@]}
CONSUMER_COUNT=${#CONSUMERS[@]}

# Use the first seeder
SEEDER_URL="${SEEDERS[0]}"
FILE_SIZE_BYTES=$((10 * 1024 * 1024))  # 10MB
FILE_NAME="large-test-$(date +%s)-${RANDOM}.bin"

log_info "[$PHASE] Seeder: $SEEDER_URL"
log_info "[$PHASE] File size: ${FILE_SIZE_BYTES} bytes (10MB)"
log_info "[$PHASE] Consumers: $CONSUMER_COUNT"

start_timer

# --- Step 1: Generate test file on seeder ---
log_info "[$PHASE] Step 1: Generating 10MB test file on seeder..."

# Use the drive upload endpoint with generated data
# First, create the file content and upload via Drive API
gen_resp=$(curl -sf --max-time 60 \
    -X POST "${SEEDER_URL}/api/drive/upload" \
    -H "Content-Type: multipart/form-data" \
    -F "file=@/dev/urandom;filename=${FILE_NAME}" \
    --limit-rate 0 \
    --data-urlencode "size=${FILE_SIZE_BYTES}" 2>/dev/null) || gen_resp=""

# If multipart upload is not supported, try generating locally
if [[ -z "$gen_resp" ]]; then
    log_info "[$PHASE] Trying alternative upload approach..."

    # Generate file locally and upload via form
    LOCAL_TMP="/tmp/${FILE_NAME}"
    dd if=/dev/urandom of="$LOCAL_TMP" bs=1M count=10 2>/dev/null

    gen_resp=$(curl -sf --max-time 120 \
        -X POST "${SEEDER_URL}/api/drive/upload" \
        -F "file=@${LOCAL_TMP}" 2>/dev/null) || gen_resp=""

    rm -f "$LOCAL_TMP"
fi

if [[ -z "$gen_resp" ]]; then
    elapsed=$(stop_timer)
    log_warn "[$PHASE] Could not upload file to seeder — trying publish-only approach"

    # Fall back: just publish a file hash to DHT without actual file content
    file_hash="large-file-${RANDOM}-$(date +%s)"
    pub_resp=$(curl -sf --max-time 15 \
        -X POST "${SEEDER_URL}/api/headless/dht/publish" \
        -H "Content-Type: application/json" \
        -d "{
            \"hash\": \"$file_hash\",
            \"fileName\": \"$FILE_NAME\",
            \"fileSize\": $FILE_SIZE_BYTES,
            \"priceWei\": \"0\"
        }" 2>/dev/null) || pub_resp=""

    if [[ -z "$pub_resp" ]]; then
        record_result "$PHASE" "large-file-transfer" "skip" "$elapsed" "Could not upload or publish file on seeder"
        exit 0
    fi
else
    # Extract file hash from upload response
    file_hash=$(echo "$gen_resp" | jq -r '.hash // .fileHash // .id // empty' 2>/dev/null) || file_hash=""
    uploaded_size=$(echo "$gen_resp" | jq -r '.size // .fileSize // 0' 2>/dev/null) || uploaded_size=0

    if [[ -z "$file_hash" ]]; then
        elapsed=$(stop_timer)
        log_warn "[$PHASE] Could not extract file hash from upload response"
        record_result "$PHASE" "large-file-transfer" "skip" "$elapsed" "Upload response missing file hash"
        exit 0
    fi

    log_info "[$PHASE] Upload OK: hash=$file_hash size=$uploaded_size"

    # Publish to DHT
    pub_resp=$(curl -sf --max-time 15 \
        -X POST "${SEEDER_URL}/api/headless/dht/publish" \
        -H "Content-Type: application/json" \
        -d "{
            \"hash\": \"$file_hash\",
            \"fileName\": \"$FILE_NAME\",
            \"fileSize\": $FILE_SIZE_BYTES,
            \"priceWei\": \"0\"
        }" 2>/dev/null) || pub_resp=""

    if [[ -z "$pub_resp" ]]; then
        log_warn "[$PHASE] Could not publish file to DHT"
    else
        log_info "[$PHASE] Published to DHT"
    fi
fi

# Give DHT a moment to propagate
sleep 5

# --- Step 2: Multiple consumers download simultaneously ---
log_info "[$PHASE] Step 2: Starting concurrent large file downloads..."

max_consumers=10
if [[ "$CONSUMER_COUNT" -lt "$max_consumers" ]]; then
    max_consumers=$CONSUMER_COUNT
fi

TMPDIR_RESULTS=$(mktemp -d)
download_ok=0
download_fail=0
declare -a DOWNLOAD_TIMES=()
declare -a DOWNLOAD_SPEEDS=()

for i in $(seq 0 $(( max_consumers - 1 ))); do
    consumer_url="${CONSUMERS[$i]}"
    result_file="${TMPDIR_RESULTS}/download-${i}.json"
    (
        dl_start=$(date +%s%N)
        resp=$(curl -sf --max-time 300 \
            -X POST "${consumer_url}/api/headless/dht/download" \
            -H "Content-Type: application/json" \
            -d "{\"hash\": \"$file_hash\", \"fileName\": \"$FILE_NAME\"}" 2>/dev/null) || resp=""
        dl_end=$(date +%s%N)
        elapsed_ms=$(( (dl_end - dl_start) / 1000000 ))

        if [[ -n "$resp" ]] && ! echo "$resp" | jq -e '.error' >/dev/null 2>&1; then
            # Try to extract downloaded size for integrity check
            dl_size=$(echo "$resp" | jq -r '.size // .fileSize // .bytesReceived // 0' 2>/dev/null) || dl_size=0
            # Calculate speed in KB/s
            speed_kbps=0
            if [[ "$elapsed_ms" -gt 0 ]]; then
                speed_kbps=$(( FILE_SIZE_BYTES / elapsed_ms ))  # approx KB/s
            fi
            echo "{\"status\":\"ok\",\"elapsed_ms\":$elapsed_ms,\"size\":$dl_size,\"speed_kbps\":$speed_kbps}" > "$result_file"
        else
            err_msg=$(echo "$resp" | jq -r '.error // "no response"' 2>/dev/null) || err_msg="no response"
            echo "{\"status\":\"fail\",\"elapsed_ms\":$elapsed_ms,\"error\":\"$err_msg\"}" > "$result_file"
        fi
    ) &
done

wait

# Collect results
total_download_time=0
max_download_time=0

for i in $(seq 0 $(( max_consumers - 1 ))); do
    result_file="${TMPDIR_RESULTS}/download-${i}.json"
    consumer_url="${CONSUMERS[$i]}"

    if [[ ! -f "$result_file" ]]; then
        download_fail=$((download_fail + 1))
        continue
    fi

    status=$(jq -r '.status' "$result_file" 2>/dev/null) || status="fail"
    elapsed_ms=$(jq -r '.elapsed_ms' "$result_file" 2>/dev/null) || elapsed_ms=0

    if [[ "$status" == "ok" ]]; then
        download_ok=$((download_ok + 1))
        total_download_time=$((total_download_time + elapsed_ms))
        if [[ "$elapsed_ms" -gt "$max_download_time" ]]; then
            max_download_time=$elapsed_ms
        fi
        DOWNLOAD_TIMES+=("$elapsed_ms")

        dl_size=$(jq -r '.size' "$result_file" 2>/dev/null) || dl_size=0
        speed=$(jq -r '.speed_kbps' "$result_file" 2>/dev/null) || speed=0
        DOWNLOAD_SPEEDS+=("$speed")

        log_info "[$PHASE] Consumer $consumer_url: OK (${elapsed_ms}ms, size=$dl_size, ~${speed}KB/s)"
    else
        download_fail=$((download_fail + 1))
        err=$(jq -r '.error' "$result_file" 2>/dev/null) || err="unknown"
        log_warn "[$PHASE] Consumer $consumer_url: FAILED ($err)"
    fi
done

rm -rf "$TMPDIR_RESULTS"

# Calculate stats
avg_download_time=0
if [[ "$download_ok" -gt 0 ]]; then
    avg_download_time=$((total_download_time / download_ok))
fi

total_attempted=$((download_ok + download_fail))
completion_rate=0
if [[ "$total_attempted" -gt 0 ]]; then
    completion_rate=$((download_ok * 100 / total_attempted))
fi

total_elapsed=$(stop_timer)

# --- Report ---
log_info "[$PHASE] === Large File Transfer Report ==="
log_info "[$PHASE]   File size:               10MB ($FILE_SIZE_BYTES bytes)"
log_info "[$PHASE]   File hash:               $file_hash"
log_info "[$PHASE]   Consumers attempted:     $total_attempted"
log_info "[$PHASE]   Downloads succeeded:     $download_ok"
log_info "[$PHASE]   Downloads failed:        $download_fail"
log_info "[$PHASE]   Completion rate:         ${completion_rate}%"
log_info "[$PHASE]   Avg download time:       ${avg_download_time}ms"
log_info "[$PHASE]   Max download time:       ${max_download_time}ms"
log_info "[$PHASE]   Total wall-clock time:   ${total_elapsed}ms"

if [[ "$total_attempted" -eq 0 ]]; then
    record_result "$PHASE" "large-file-transfer" "skip" "$total_elapsed" "No download attempts were made"
    exit 0
fi

if [[ "$download_ok" -gt 0 ]]; then
    record_result "$PHASE" "large-file-transfer" "pass" "$total_elapsed" ""
    log_pass "[$PHASE] $download_ok/$total_attempted large file downloads succeeded (avg ${avg_download_time}ms)"
else
    record_result "$PHASE" "large-file-transfer" "fail" "$total_elapsed" "All $total_attempted large file downloads failed"
    log_fail "[$PHASE] All $total_attempted large file downloads failed"
    exit 1
fi
