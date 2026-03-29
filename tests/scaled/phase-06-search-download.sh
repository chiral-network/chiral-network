#!/usr/bin/env bash
set -euo pipefail
source /tests/lib.sh

PHASE="phase-06-search-download"
log_info "[$PHASE] Starting file search and download phase"

###############################################################################
# Phase 06 — File Search & Download (runs on consumer nodes)
#
# Reads published file metadata from /results/published-files.txt and attempts
# to discover them via DHT get, then requests downloads.
###############################################################################

PUBLISHED_FILE="/results/published-files.txt"

if [[ ! -f "$PUBLISHED_FILE" ]] || [[ ! -s "$PUBLISHED_FILE" ]]; then
    log_warn "[$PHASE] No published files found — skipping"
    record_result "$PHASE" "search-download" "skip" "0" "No published files available"
    exit 0
fi

if [[ -z "${CONSUMER_NODES:-}" ]]; then
    log_warn "[$PHASE] CONSUMER_NODES is empty — skipping"
    record_result "$PHASE" "search-download" "skip" "0" "No consumer nodes assigned"
    exit 0
fi

# Parse published files: each line is "fileHash|peerId|fileName|fileSize|seederUrl"
mapfile -t PUBLISHED_LINES < "$PUBLISHED_FILE"
PUBLISHED_COUNT=${#PUBLISHED_LINES[@]}

if [[ "$PUBLISHED_COUNT" -eq 0 ]]; then
    log_warn "[$PHASE] Published files list is empty — skipping"
    record_result "$PHASE" "search-download" "skip" "0" "Published files list empty"
    exit 0
fi

IFS=',' read -ra CONSUMERS <<< "$CONSUMER_NODES"

files_found=0
files_not_found=0
downloads_started=0
downloads_failed=0
downloads_completed=0

start_timer

for consumer_url in "${CONSUMERS[@]}"; do
    # Pick a random published file
    idx=$(( RANDOM % PUBLISHED_COUNT ))
    line="${PUBLISHED_LINES[$idx]}"

    IFS='|' read -r file_hash seeder_peer_id file_name file_size _seeder_url <<< "$line"

    log_info "[$PHASE] Consumer $consumer_url searching for file $file_hash"

    # --- DHT search: look up the file hash key ---
    search_resp=$(curl -sf --max-time 15 \
        -X POST "${consumer_url}/api/headless/dht/get" \
        -H "Content-Type: application/json" \
        -d "{\"key\": \"$file_hash\"}" 2>/dev/null) || search_resp=""

    if [[ -z "$search_resp" ]]; then
        log_warn "[$PHASE] Consumer $consumer_url: DHT get failed for $file_hash"
        files_not_found=$((files_not_found + 1))
        continue
    fi

    # Check if the response contains an error
    if echo "$search_resp" | jq -e '.error' >/dev/null 2>&1; then
        log_warn "[$PHASE] Consumer $consumer_url: DHT get error for $file_hash: $(echo "$search_resp" | jq -r '.error')"
        files_not_found=$((files_not_found + 1))
        continue
    fi

    # Verify the value contains seeder info
    dht_value=$(echo "$search_resp" | jq -r '.value // empty')
    if [[ -z "$dht_value" ]]; then
        log_warn "[$PHASE] Consumer $consumer_url: No DHT value for $file_hash"
        files_not_found=$((files_not_found + 1))
        continue
    fi

    log_info "[$PHASE] Consumer $consumer_url: Found file $file_hash in DHT"
    files_found=$((files_found + 1))

    # --- Request file download ---
    request_id="dl-${RANDOM}-$(date +%s)"
    dl_resp=$(curl -sf --max-time 15 \
        -X POST "${consumer_url}/api/headless/dht/request-file" \
        -H "Content-Type: application/json" \
        -d "{
            \"peerId\": \"$seeder_peer_id\",
            \"fileHash\": \"$file_hash\",
            \"requestId\": \"$request_id\"
        }" 2>/dev/null) || dl_resp=""

    if [[ -z "$dl_resp" ]]; then
        log_warn "[$PHASE] Consumer $consumer_url: Download request failed for $file_hash"
        downloads_failed=$((downloads_failed + 1))
        continue
    fi

    if echo "$dl_resp" | jq -e '.error' >/dev/null 2>&1; then
        log_warn "[$PHASE] Consumer $consumer_url: Download request error: $(echo "$dl_resp" | jq -r '.error')"
        downloads_failed=$((downloads_failed + 1))
        continue
    fi

    log_info "[$PHASE] Consumer $consumer_url: Download started for $file_hash (request=$request_id)"
    downloads_started=$((downloads_started + 1))
done

# --- Wait for downloads to complete (up to 60s) ---
if [[ "$downloads_started" -gt 0 ]]; then
    log_info "[$PHASE] Waiting up to 60s for $downloads_started download(s) to complete..."
    deadline=$((SECONDS + 60))

    while [[ $SECONDS -lt $deadline ]]; do
        all_idle=true
        for consumer_url in "${CONSUMERS[@]}"; do
            # Check if there are active/pending incoming transfers
            inbox_resp=$(curl -sf --max-time 5 \
                "${consumer_url}/api/headless/drop/inbox" 2>/dev/null) || continue
            pending=$(echo "$inbox_resp" | jq 'length' 2>/dev/null) || pending=0
            if [[ "$pending" -gt 0 ]]; then
                all_idle=false
                break
            fi
        done

        if [[ "$all_idle" == "true" ]]; then
            break
        fi
        sleep 3
    done

    # Count completed downloads — best effort: if inbox is empty, transfers finished
    for consumer_url in "${CONSUMERS[@]}"; do
        inbox_resp=$(curl -sf --max-time 5 \
            "${consumer_url}/api/headless/drop/inbox" 2>/dev/null) || continue
        pending_count=$(echo "$inbox_resp" | jq 'length' 2>/dev/null) || pending_count=0
        if [[ "$pending_count" -eq 0 ]]; then
            downloads_completed=$((downloads_completed + 1))
        fi
    done
fi

elapsed=$(stop_timer)

# --- Report ---
log_info "[$PHASE] === Search & Download Report ==="
log_info "[$PHASE]   Published files available: $PUBLISHED_COUNT"
log_info "[$PHASE]   Consumer nodes:            ${#CONSUMERS[@]}"
log_info "[$PHASE]   Files found via DHT:       $files_found"
log_info "[$PHASE]   Files not found:           $files_not_found"
log_info "[$PHASE]   Downloads started:         $downloads_started"
log_info "[$PHASE]   Downloads completed:       $downloads_completed"
log_info "[$PHASE]   Downloads failed:          $downloads_failed"

total_attempts=$(( files_found + files_not_found ))
if [[ "$total_attempts" -eq 0 ]]; then
    record_result "$PHASE" "search-download" "skip" "$elapsed" "No search attempts made"
    exit 0
fi

if [[ "$files_found" -gt 0 ]]; then
    record_result "$PHASE" "search-download" "pass" "$elapsed" ""
    log_pass "[$PHASE] Found $files_found/$total_attempts files; started $downloads_started downloads; completed $downloads_completed"
else
    record_result "$PHASE" "search-download" "fail" "$elapsed" "No files found in DHT out of $total_attempts attempts"
    log_fail "[$PHASE] No files found in DHT out of $total_attempts attempts"
    exit 1
fi
