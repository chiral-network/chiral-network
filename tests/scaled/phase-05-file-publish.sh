#!/usr/bin/env bash
# Phase 05: File publish — register uploaded files in the DHT for peer discovery
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "${SCRIPT_DIR}/lib.sh"

PHASE="05-file-publish"
WALLETS_FILE="${WALLETS_FILE:-/results/wallets.txt}"
DRIVE_FILES="${DRIVE_FILES:-/results/drive-files.txt}"
PUBLISHED_FILES="${PUBLISHED_FILES:-/results/published-files.txt}"

log_info "=== Phase 05: File Publish to DHT ==="

if [ ! -s "$DRIVE_FILES" ]; then
    log_fail "No drive files found in ${DRIVE_FILES} — run phase-04 first"
    FAIL_COUNT=$(( FAIL_COUNT + 1 ))
    record_result "$PHASE" "drive files exist" "fail" "0" "${DRIVE_FILES} is empty or missing"
    print_summary
    exit 1
fi

# Clear published files record
mkdir -p "$(dirname "$PUBLISHED_FILES")"
> "$PUBLISHED_FILES"

# Read drive files: NODE|FILE_ID|FILENAME|SIZE|OWNER
PUBLISH_COUNT=0
while IFS='|' read -r node file_id fname size owner; do
    [ -z "$node" ] && continue

    log_info "Publishing ${fname} from ${node} to DHT..."

    # Generate a deterministic file hash from the file_id (sha256 hex for test purposes)
    file_hash=$(echo -n "${node}:${file_id}:${fname}" | sha256sum | awk '{print $1}')

    # The file path inside the node container — Drive stores files under the chiral-drive directory
    file_path="/root/.local/share/chiral-network/chiral-drive/${file_id}"

    start_timer
    body=$(jq -nc \
        --arg hash "$file_hash" \
        --arg path "$file_path" \
        --arg name "$fname" \
        --argjson size "$size" \
        --arg price "0" \
        --arg wallet "$owner" \
        '{
            file_hash: $hash,
            file_path: $path,
            file_name: $name,
            file_size: $size,
            price_wei: $price,
            wallet_address: $wallet
        }')
    resp=$(api_post "$node" "/api/headless/dht/register-shared-file" "$body")
    dur=$(stop_timer)

    assert_status "200" "${node}: publish ${fname}" "$dur"

    # Check response for success
    status_field=$(echo "$resp" | jq -r '.status // empty' 2>/dev/null) || status_field=""
    if [ "$status_field" = "ok" ]; then
        echo "${node}|${file_hash}|${fname}|${size}|${owner}" >> "$PUBLISHED_FILES"
        PUBLISH_COUNT=$(( PUBLISH_COUNT + 1 ))
        log_pass "${node}: published ${fname} (hash=${file_hash:0:16}...)"
        PASS_COUNT=$(( PASS_COUNT + 1 ))
        record_result "$PHASE" "${node}: publish ${fname}" "pass" "$dur" ""
    else
        log_fail "${node}: publish ${fname} — unexpected response: ${resp}"
        FAIL_COUNT=$(( FAIL_COUNT + 1 ))
        record_result "$PHASE" "${node}: publish ${fname}" "fail" "$dur" "response: ${resp}"
    fi

done < "$DRIVE_FILES"

# ---- Summary ----
log_info "Published ${PUBLISH_COUNT} file(s) to DHT"
if [ -s "$PUBLISHED_FILES" ]; then
    log_info "Published files:"
    while IFS='|' read -r node hash fname size owner; do
        log_info "  ${node}: ${fname} (hash=${hash:0:16}...)"
    done < "$PUBLISHED_FILES"
fi

print_summary
