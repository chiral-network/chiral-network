#!/usr/bin/env bash
set -euo pipefail
source /tests/lib.sh

PHASE="phase-12-rapid-publish-search"
log_info "[$PHASE] Starting rapid publish-then-search cycle"

###############################################################################
# Phase 12 — Rapid Publish-Search Cycle
#
# Tests DHT propagation speed by having seeders publish new files, then having
# all consumers immediately search for them. Measures time from publish to
# first successful search hit.
###############################################################################

if [[ -z "${SEEDER_NODES:-}" ]]; then
    log_warn "[$PHASE] SEEDER_NODES is empty — skipping"
    record_result "$PHASE" "rapid-publish-search" "skip" "0" "No seeder nodes available"
    exit 0
fi

if [[ -z "${CONSUMER_NODES:-}" ]]; then
    log_warn "[$PHASE] CONSUMER_NODES is empty — skipping"
    record_result "$PHASE" "rapid-publish-search" "skip" "0" "No consumer nodes available"
    exit 0
fi

IFS=',' read -ra SEEDERS <<< "$SEEDER_NODES"
IFS=',' read -ra CONSUMERS <<< "$CONSUMER_NODES"
SEEDER_COUNT=${#SEEDERS[@]}
CONSUMER_COUNT=${#CONSUMERS[@]}

if [[ "$SEEDER_COUNT" -eq 0 ]] || [[ "$CONSUMER_COUNT" -eq 0 ]]; then
    log_warn "[$PHASE] Need at least 1 seeder and 1 consumer — skipping"
    record_result "$PHASE" "rapid-publish-search" "skip" "0" "Insufficient nodes"
    exit 0
fi

ROUNDS=5
MAX_SEARCH_WAIT=30  # seconds to wait for propagation per round

total_publishes=0
total_publish_ok=0
total_searches=0
total_search_hits=0
total_search_misses=0
declare -a PROPAGATION_TIMES=()

start_timer

for round in $(seq 1 "$ROUNDS"); do
    # Pick a seeder for this round (cycle through available seeders)
    seeder_idx=$(( (round - 1) % SEEDER_COUNT ))
    seeder_url="${SEEDERS[$seeder_idx]}"

    # Generate a unique file hash for this round
    unique_hash="stress-test-$(date +%s%N)-round${round}-${RANDOM}"
    unique_name="rapid-test-round${round}-${RANDOM}.dat"

    log_info "[$PHASE] Round $round/$ROUNDS: Publishing '$unique_name' (hash=$unique_hash) from $seeder_url"

    # Publish the file to DHT
    publish_start=$(date +%s%N)
    pub_resp=$(curl -sf --max-time 15 \
        -X POST "${seeder_url}/api/headless/dht/publish" \
        -H "Content-Type: application/json" \
        -d "{
            \"hash\": \"$unique_hash\",
            \"fileName\": \"$unique_name\",
            \"fileSize\": 1024,
            \"priceWei\": \"0\"
        }" 2>/dev/null) || pub_resp=""

    total_publishes=$((total_publishes + 1))

    if [[ -z "$pub_resp" ]]; then
        log_warn "[$PHASE] Round $round: publish failed (no response)"
        continue
    fi

    if echo "$pub_resp" | jq -e '.error' >/dev/null 2>&1; then
        err_msg=$(echo "$pub_resp" | jq -r '.error')
        log_warn "[$PHASE] Round $round: publish error: $err_msg"
        continue
    fi

    total_publish_ok=$((total_publish_ok + 1))
    log_info "[$PHASE] Round $round: publish OK, searching from $CONSUMER_COUNT consumers..."

    # All consumers search for the newly published file simultaneously
    TMPDIR_SEARCH=$(mktemp -d)
    for ci in $(seq 0 $(( CONSUMER_COUNT - 1 ))); do
        consumer_url="${CONSUMERS[$ci]}"
        search_result_file="${TMPDIR_SEARCH}/consumer-${ci}.json"
        (
            found=false
            first_found_ms=0
            search_start=$(date +%s%N)
            deadline=$(( $(date +%s) + MAX_SEARCH_WAIT ))

            # Poll for the file to appear in search results
            while [[ "$(date +%s)" -lt "$deadline" ]]; do
                search_resp=$(curl -sf --max-time 10 \
                    -X POST "${consumer_url}/api/headless/dht/search" \
                    -H "Content-Type: application/json" \
                    -d "{\"query\": \"$unique_name\"}" 2>/dev/null) || search_resp=""

                if [[ -n "$search_resp" ]]; then
                    # Check if our hash appears in results
                    match=$(echo "$search_resp" | jq -r "
                        if type == \"array\" then
                            (.[] | select(.hash == \"$unique_hash\" or .fileHash == \"$unique_hash\") | .hash // .fileHash) // \"\"
                        elif type == \"object\" and (.results // null) != null then
                            (.results[] | select(.hash == \"$unique_hash\" or .fileHash == \"$unique_hash\") | .hash // .fileHash) // \"\"
                        else
                            \"\"
                        end
                    " 2>/dev/null) || match=""

                    if [[ -n "$match" ]] && [[ "$match" != "null" ]]; then
                        now=$(date +%s%N)
                        first_found_ms=$(( (now - publish_start) / 1000000 ))
                        found=true
                        break
                    fi
                fi
                sleep 2
            done

            if [[ "$found" == "true" ]]; then
                echo "{\"status\":\"found\",\"propagation_ms\":$first_found_ms}" > "$search_result_file"
            else
                echo "{\"status\":\"missed\",\"propagation_ms\":0}" > "$search_result_file"
            fi
        ) &
    done

    wait

    # Collect round search results
    round_hits=0
    round_misses=0
    for ci in $(seq 0 $(( CONSUMER_COUNT - 1 ))); do
        search_result_file="${TMPDIR_SEARCH}/consumer-${ci}.json"
        total_searches=$((total_searches + 1))

        if [[ ! -f "$search_result_file" ]]; then
            round_misses=$((round_misses + 1))
            total_search_misses=$((total_search_misses + 1))
            continue
        fi

        status=$(jq -r '.status' "$search_result_file" 2>/dev/null) || status="missed"
        prop_ms=$(jq -r '.propagation_ms' "$search_result_file" 2>/dev/null) || prop_ms=0

        if [[ "$status" == "found" ]]; then
            round_hits=$((round_hits + 1))
            total_search_hits=$((total_search_hits + 1))
            PROPAGATION_TIMES+=("$prop_ms")
        else
            round_misses=$((round_misses + 1))
            total_search_misses=$((total_search_misses + 1))
        fi
    done

    rm -rf "$TMPDIR_SEARCH"
    log_info "[$PHASE] Round $round: ${round_hits}/${CONSUMER_COUNT} consumers found the file"
done

total_elapsed=$(stop_timer)

# Calculate propagation time stats
avg_prop=0
max_prop=0
min_prop=0
if [[ ${#PROPAGATION_TIMES[@]} -gt 0 ]]; then
    sum=0
    min_prop=${PROPAGATION_TIMES[0]}
    max_prop=${PROPAGATION_TIMES[0]}
    for t in "${PROPAGATION_TIMES[@]}"; do
        sum=$((sum + t))
        if [[ "$t" -gt "$max_prop" ]]; then max_prop=$t; fi
        if [[ "$t" -lt "$min_prop" ]]; then min_prop=$t; fi
    done
    avg_prop=$((sum / ${#PROPAGATION_TIMES[@]}))
fi

# --- Report ---
log_info "[$PHASE] === Rapid Publish-Search Report ==="
log_info "[$PHASE]   Rounds:                  $ROUNDS"
log_info "[$PHASE]   Publishes attempted:     $total_publishes"
log_info "[$PHASE]   Publishes succeeded:     $total_publish_ok"
log_info "[$PHASE]   Total searches:          $total_searches"
log_info "[$PHASE]   Search hits:             $total_search_hits"
log_info "[$PHASE]   Search misses:           $total_search_misses"
log_info "[$PHASE]   Propagation avg:         ${avg_prop}ms"
log_info "[$PHASE]   Propagation min:         ${min_prop}ms"
log_info "[$PHASE]   Propagation max:         ${max_prop}ms"
log_info "[$PHASE]   Total wall-clock time:   ${total_elapsed}ms"

if [[ "$total_publishes" -eq 0 ]]; then
    record_result "$PHASE" "rapid-publish-search" "skip" "$total_elapsed" "No publishes attempted"
    exit 0
fi

if [[ "$total_search_hits" -gt 0 ]]; then
    record_result "$PHASE" "rapid-publish-search" "pass" "$total_elapsed" ""
    log_pass "[$PHASE] $total_search_hits/$total_searches searches found published files (avg propagation ${avg_prop}ms)"
else
    record_result "$PHASE" "rapid-publish-search" "fail" "$total_elapsed" "All $total_searches searches missed published files"
    log_fail "[$PHASE] All $total_searches searches missed published files"
    exit 1
fi
