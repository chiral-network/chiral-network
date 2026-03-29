#!/usr/bin/env bash
# Phase 04: Drive upload — generate test files and upload to seeder nodes via Drive API
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
source "${SCRIPT_DIR}/lib.sh"

PHASE="04-drive-upload"
WALLETS_FILE="${WALLETS_FILE:-/results/wallets.txt}"
DRIVE_FILES="${DRIVE_FILES:-/results/drive-files.txt}"
TEST_FILE_DIR="/tmp/chiral-test-files"

log_info "=== Phase 04: Drive Upload ==="

if [ -z "${SEEDER_NODES:-}" ]; then
    log_warn "SEEDER_NODES not set — using NODE_LIST"
    SEEDER_NODES="${NODE_LIST:?NODE_LIST env var required}"
fi

IFS=',' read -ra SEEDERS <<< "$SEEDER_NODES"
TOTAL=${#SEEDERS[@]}
log_info "Seeder nodes: ${SEEDERS[*]}"

# Helper: look up wallet address for a node
get_wallet() {
    local node="$1"
    grep "^${node}=" "$WALLETS_FILE" 2>/dev/null | cut -d'=' -f2 || echo "0x0000000000000000000000000000000000000001"
}

# ---- Step 1: Generate test files ----
log_info "Generating test files..."
mkdir -p "$TEST_FILE_DIR"

declare -A TEST_FILES
TEST_FILES["small-1k.bin"]=1024
TEST_FILES["medium-100k.bin"]=102400
TEST_FILES["large-1m.bin"]=1048576

for fname in "${!TEST_FILES[@]}"; do
    size=${TEST_FILES[$fname]}
    fpath="${TEST_FILE_DIR}/${fname}"
    if [ ! -f "$fpath" ]; then
        generate_test_file "$size" "$fpath"
        log_info "Generated ${fname} (${size} bytes)"
    fi
done

# Clear drive files record
mkdir -p "$(dirname "$DRIVE_FILES")"
> "$DRIVE_FILES"

# ---- Step 2: Upload files to each seeder node ----
for node in "${SEEDERS[@]}"; do
    owner=$(get_wallet "$node")
    log_info "Uploading files to ${node} (owner=${owner})..."

    for fname in "${!TEST_FILES[@]}"; do
        fpath="${TEST_FILE_DIR}/${fname}"
        size=${TEST_FILES[$fname]}

        start_timer
        # Upload via multipart form — Drive API expects X-Owner header
        tmp_resp=$(mktemp)
        http_code=$(curl -s -o "$tmp_resp" -w '%{http_code}' --max-time 30 \
            -X POST \
            -H "X-Owner: ${owner}" \
            -F "file=@${fpath};filename=${fname}" \
            "http://${node}:9419/api/drive/upload" 2>/dev/null) || http_code="000"
        LAST_STATUS="$http_code"
        resp=$(cat "$tmp_resp")
        rm -f "$tmp_resp"
        dur=$(stop_timer)

        assert_status "200" "${node}: upload ${fname}" "$dur"

        # Extract file ID from response
        file_id=$(echo "$resp" | jq -r '.id // .fileId // empty' 2>/dev/null) || file_id=""
        if [ -n "$file_id" ]; then
            echo "${node}|${file_id}|${fname}|${size}|${owner}" >> "$DRIVE_FILES"
            log_info "  ${fname} -> id=${file_id}"
        else
            log_warn "  ${fname}: could not extract file ID from response"
        fi
    done
done

# ---- Step 3: Verify uploads via list endpoint ----
log_info "Verifying uploaded files..."
for node in "${SEEDERS[@]}"; do
    owner=$(get_wallet "$node")

    start_timer
    resp=$(api_get "$node" "/api/drive/items?owner=${owner}")
    dur=$(stop_timer)
    assert_status "200" "${node}: list drive items" "$dur"

    item_count=$(echo "$resp" | jq 'if type == "array" then length else .items // [] | length end' 2>/dev/null) || item_count=0
    expected_count=${#TEST_FILES[@]}

    if [ "$item_count" -ge "$expected_count" ]; then
        log_pass "${node}: ${item_count} items found (expected >= ${expected_count})"
        PASS_COUNT=$(( PASS_COUNT + 1 ))
        record_result "$PHASE" "${node}: drive item count" "pass" "$dur" ""
    else
        log_fail "${node}: only ${item_count} items found (expected >= ${expected_count})"
        FAIL_COUNT=$(( FAIL_COUNT + 1 ))
        record_result "$PHASE" "${node}: drive item count" "fail" "$dur" "expected >= ${expected_count}, got ${item_count}"
    fi
done

# ---- Summary ----
file_count=$(wc -l < "$DRIVE_FILES")
log_info "Total files uploaded and recorded: ${file_count}"
print_summary
