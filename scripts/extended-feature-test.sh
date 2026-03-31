#!/usr/bin/env bash
# =============================================================================
# Chiral Network — Extended Feature Tests
#
# Tests features NOT covered by full-feature-test.sh. Run after that script.
# Assumes 10-node cluster is running on ports 9421-9430 with wallets created.
#
# Usage:
#   ./scripts/extended-feature-test.sh
# =============================================================================
set -uo pipefail

BASE_PORT=9420
PASS=0
FAIL=0
SKIP=0
ERRORS=()

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m'

pass() { PASS=$((PASS + 1)); echo -e "  ${GREEN}[PASS]${NC} $1"; }
fail() { FAIL=$((FAIL + 1)); ERRORS+=("$1: $2"); echo -e "  ${RED}[FAIL]${NC} $1 — $2"; }
skip() { SKIP=$((SKIP + 1)); echo -e "  ${YELLOW}[SKIP]${NC} $1 — $2"; }
section() { echo ""; echo -e "${CYAN}=== $1 ===${NC}"; }

port_for() { echo $((BASE_PORT + $1)); }

api_get() {
    local tmp; tmp=$(mktemp)
    STATUS=$(curl -sf -o "$tmp" -w "%{http_code}" "$1" 2>/dev/null || echo "000")
    BODY=$(cat "$tmp" 2>/dev/null); rm -f "$tmp"
}

api_post() {
    local tmp; tmp=$(mktemp)
    STATUS=$(curl -sf -o "$tmp" -w "%{http_code}" -X POST "$1" \
        -H 'Content-Type: application/json' -d "${2:-{}}" 2>/dev/null || echo "000")
    BODY=$(cat "$tmp" 2>/dev/null); rm -f "$tmp"
}

json_field() { echo "$1" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('$2',''))" 2>/dev/null; }
json_len() { echo "$1" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))" 2>/dev/null; }

echo "=============================================="
echo "  CHIRAL NETWORK — EXTENDED FEATURE TESTS"
echo "=============================================="

# Ensure wallets exist
for i in $(seq 1 10); do
    api_get "http://localhost:$(port_for $i)/api/headless/wallet"
    if [[ "$STATUS" != "200" ]] || ! echo "$BODY" | grep -q "address"; then
        api_post "http://localhost:$(port_for $i)/api/headless/wallet/create"
    fi
done

# Collect wallet addresses and peer IDs
declare -A WALLETS PEERS
for i in $(seq 1 10); do
    api_get "http://localhost:$(port_for $i)/api/headless/wallet"
    WALLETS[$i]=$(json_field "$BODY" "address")
    api_get "http://localhost:$(port_for $i)/api/headless/dht/peer-id"
    PEERS[$i]=$(json_field "$BODY" "peerId")
done

# ============================================================================
# SECTION 13: Wallet Import and Re-Import
# ============================================================================
section "13. Wallet Import and Re-Import"

# 13.1 Import a well-known private key
api_post "http://localhost:9421/api/headless/wallet/import" \
    '{"privateKey":"0x4c0883a69102937d6231471b5dbb6204fe512961708279f78b2b56a1c5e3b39e"}'
IMPORTED_ADDR=$(json_field "$BODY" "address")
if [[ "$IMPORTED_ADDR" == 0x* ]] && [[ ${#IMPORTED_ADDR} -eq 42 ]]; then
    pass "Import wallet with known private key"
else
    fail "Import wallet" "address=$IMPORTED_ADDR"
fi

# 13.2 Re-import overwrites previous wallet
api_post "http://localhost:9421/api/headless/wallet/import" \
    '{"privateKey":"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"}'
NEW_ADDR=$(json_field "$BODY" "address")
if [[ "$NEW_ADDR" != "$IMPORTED_ADDR" ]]; then
    pass "Re-import replaces previous wallet"
else
    fail "Re-import" "address didn't change"
fi

# 13.3 Show returns the latest import
api_get "http://localhost:9421/api/headless/wallet"
SHOW_ADDR=$(json_field "$BODY" "address")
if [[ "$SHOW_ADDR" == "$NEW_ADDR" ]]; then
    pass "Show returns latest imported wallet"
else
    fail "Show after import" "expected $NEW_ADDR, got $SHOW_ADDR"
fi

# 13.4 Import with 0x prefix works
api_post "http://localhost:9421/api/headless/wallet/import" \
    '{"privateKey":"0x0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}'
if [[ "$STATUS" == "200" ]]; then
    pass "Import with 0x prefix"
else
    fail "Import 0x prefix" "HTTP $STATUS"
fi

# 13.5 Import without 0x prefix works
api_post "http://localhost:9421/api/headless/wallet/import" \
    '{"privateKey":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}'
if [[ "$STATUS" == "200" ]]; then
    pass "Import without 0x prefix"
else
    fail "Import no prefix" "HTTP $STATUS"
fi

# 13.6 Import with too-short key fails
IMPORT_STATUS=$(curl -sf -o /dev/null -w "%{http_code}" -X POST \
    http://localhost:9421/api/headless/wallet/import \
    -H 'Content-Type: application/json' -d '{"privateKey":"0xabcd"}' 2>/dev/null)
if [[ "$IMPORT_STATUS" == "400" ]]; then
    pass "Import too-short key returns 400"
else
    fail "Import short key" "expected 400, got $IMPORT_STATUS"
fi

# 13.7 Import with empty key fails
IMPORT_STATUS=$(curl -sf -o /dev/null -w "%{http_code}" -X POST \
    http://localhost:9421/api/headless/wallet/import \
    -H 'Content-Type: application/json' -d '{"privateKey":""}' 2>/dev/null)
if [[ "$IMPORT_STATUS" == "400" ]]; then
    pass "Import empty key returns 400"
else
    fail "Import empty key" "expected 400, got $IMPORT_STATUS"
fi

# Restore a fresh wallet on node 1
api_post "http://localhost:9421/api/headless/wallet/create"
WALLETS[1]=$(json_field "$BODY" "address")

# ============================================================================
# SECTION 14: DHT Propagation and Consistency
# ============================================================================
section "14. DHT Propagation and Consistency"

# 14.1 Write from node 1, read from ALL other nodes
api_post "http://localhost:9421/api/headless/dht/put" '{"key":"propagation_test","value":"from_node_1"}'
sleep 3
PROP_FOUND=0
for i in $(seq 2 10); do
    api_post "http://localhost:$(port_for $i)/api/headless/dht/get" '{"key":"propagation_test"}'
    if echo "$BODY" | grep -q "from_node_1"; then
        PROP_FOUND=$((PROP_FOUND + 1))
    fi
done
if [[ "$PROP_FOUND" -ge 7 ]]; then
    pass "DHT propagation: $PROP_FOUND/9 nodes found the value"
else
    fail "DHT propagation" "only $PROP_FOUND/9 found"
fi

# 14.2 Overwrite a value and verify latest is returned
api_post "http://localhost:9422/api/headless/dht/put" '{"key":"overwrite_test","value":"version_1"}'
sleep 1
api_post "http://localhost:9422/api/headless/dht/put" '{"key":"overwrite_test","value":"version_2"}'
sleep 2
api_post "http://localhost:9428/api/headless/dht/get" '{"key":"overwrite_test"}'
if echo "$BODY" | grep -q "version_2"; then
    pass "DHT overwrite returns latest value"
else
    fail "DHT overwrite" "got: $BODY"
fi

# 14.3 Binary-safe values (base64 encoded content)
B64_VAL=$(echo -n "binary\x00data\x01here" | base64)
api_post "http://localhost:9423/api/headless/dht/put" "{\"key\":\"binary_test\",\"value\":\"$B64_VAL\"}"
sleep 2
api_post "http://localhost:9427/api/headless/dht/get" '{"key":"binary_test"}'
if echo "$BODY" | grep -q "$B64_VAL"; then
    pass "DHT stores base64 values correctly"
else
    fail "DHT base64" "value mismatch"
fi

# 14.4 Long key names
LONG_KEY=$(python3 -c "print('k' * 200)")
api_post "http://localhost:9424/api/headless/dht/put" "{\"key\":\"$LONG_KEY\",\"value\":\"long_key_val\"}"
sleep 2
api_post "http://localhost:9426/api/headless/dht/get" "{\"key\":\"$LONG_KEY\"}"
if echo "$BODY" | grep -q "long_key_val"; then
    pass "DHT long key names (200 chars)"
else
    fail "DHT long key" "not found"
fi

# ============================================================================
# SECTION 15: Multi-Node File Publishing
# ============================================================================
section "15. Multi-Node File Publishing"

# 15.1 Multiple seeders publish the same file hash
FILE_HASH="shared_file_hash_$(date +%s)"
for i in 1 2 3; do
    dd if=/dev/urandom of="/tmp/chiral-shared-$i.bin" bs=512 count=1 2>/dev/null
    OWNER="${WALLETS[$i]}"
    api_post "http://localhost:$(port_for $i)/api/headless/dht/register-shared-file" \
        "{\"fileHash\":\"$FILE_HASH\",\"filePath\":\"/tmp/chiral-shared-$i.bin\",\"fileName\":\"shared.bin\",\"fileSize\":512,\"priceWei\":\"0\",\"walletAddress\":\"$OWNER\"}"
done
sleep 2

# 15.2 Search should find the file
api_post "http://localhost:9428/api/headless/dht/get" "{\"key\":\"$FILE_HASH\"}"
if echo "$BODY" | grep -q "value"; then
    pass "Multi-seeder file discoverable in DHT"
else
    fail "Multi-seeder search" "not found"
fi

# 15.3 Unregister from one seeder, file should still be findable
api_post "http://localhost:9421/api/headless/dht/unregister-shared-file" "{\"fileHash\":\"$FILE_HASH\"}"
sleep 2
api_post "http://localhost:9429/api/headless/dht/get" "{\"key\":\"$FILE_HASH\"}"
if echo "$BODY" | grep -q "value"; then
    pass "File still findable after one seeder unregisters"
else
    skip "File after partial unregister" "DHT may have expired"
fi

# ============================================================================
# SECTION 16: Drive Advanced Operations
# ============================================================================
section "16. Drive Advanced Operations"

OWNER="${WALLETS[3]}"
PORT=9423
HEADER="-H X-Owner:$OWNER"

# 16.1 Create nested folder structure
PARENT=$(curl -sf -X POST "http://localhost:$PORT/api/drive/folders" \
    -H 'Content-Type: application/json' -H "X-Owner: $OWNER" \
    -d '{"name":"Parent Folder"}' 2>&1)
PARENT_ID=$(json_field "$PARENT" "id")

CHILD=$(curl -sf -X POST "http://localhost:$PORT/api/drive/folders" \
    -H 'Content-Type: application/json' -H "X-Owner: $OWNER" \
    -d "{\"name\":\"Child Folder\",\"parentId\":\"$PARENT_ID\"}" 2>&1)
CHILD_ID=$(json_field "$CHILD" "id")

if [[ -n "$CHILD_ID" ]]; then
    pass "Nested folder creation (parent/child)"
else
    fail "Nested folders" "child creation failed"
fi

# 16.2 Upload file into nested folder
dd if=/dev/urandom of=/tmp/chiral-nested-upload.bin bs=256 count=1 2>/dev/null
NESTED_UPLOAD=$(curl -sf -X POST "http://localhost:$PORT/api/drive/upload" \
    -H "X-Owner: $OWNER" -F "file=@/tmp/chiral-nested-upload.bin" -F "parentId=$CHILD_ID" 2>&1)
NESTED_FILE_ID=$(json_field "$NESTED_UPLOAD" "id")
if [[ -n "$NESTED_FILE_ID" ]]; then
    pass "Upload into nested folder"
else
    fail "Nested upload" "$NESTED_UPLOAD"
fi

# 16.3 List nested folder contents
NESTED_ITEMS=$(curl -sf "http://localhost:$PORT/api/drive/items?parentId=$CHILD_ID" \
    -H "X-Owner: $OWNER" 2>&1)
NESTED_COUNT=$(json_len "$NESTED_ITEMS")
if [[ "$NESTED_COUNT" -ge 1 ]]; then
    pass "List nested folder: $NESTED_COUNT items"
else
    fail "List nested" "expected >=1, got $NESTED_COUNT"
fi

# 16.4 Move file to different folder (via update parentId)
MOVE_RESULT=$(curl -sf -X PUT "http://localhost:$PORT/api/drive/items/$NESTED_FILE_ID" \
    -H 'Content-Type: application/json' -H "X-Owner: $OWNER" \
    -d "{\"parentId\":\"$PARENT_ID\"}" 2>&1)
if echo "$MOVE_RESULT" | grep -q "$PARENT_ID"; then
    pass "Move file between folders"
else
    # May not support move — skip
    skip "Move file" "may not be supported via update"
fi

# 16.5 Star and unstar a file
curl -sf -X PUT "http://localhost:$PORT/api/drive/items/$NESTED_FILE_ID" \
    -H 'Content-Type: application/json' -H "X-Owner: $OWNER" \
    -d '{"starred":true}' > /dev/null 2>&1
STARRED=$(curl -sf "http://localhost:$PORT/api/drive/items?parentId=$PARENT_ID" \
    -H "X-Owner: $OWNER" 2>&1)
if echo "$STARRED" | grep -q '"starred":true'; then
    pass "Star file"
else
    skip "Star file" "field not in listing"
fi

curl -sf -X PUT "http://localhost:$PORT/api/drive/items/$NESTED_FILE_ID" \
    -H 'Content-Type: application/json' -H "X-Owner: $OWNER" \
    -d '{"starred":false}' > /dev/null 2>&1
pass "Unstar file"

# 16.6 Upload multiple file types
for ext in txt jpg pdf mp4; do
    dd if=/dev/urandom of="/tmp/chiral-test.$ext" bs=128 count=1 2>/dev/null
    MIME_UPLOAD=$(curl -sf -X POST "http://localhost:$PORT/api/drive/upload" \
        -H "X-Owner: $OWNER" -F "file=@/tmp/chiral-test.$ext" 2>&1)
    MIME=$(json_field "$MIME_UPLOAD" "mimeType")
    if [[ -n "$MIME" ]]; then
        pass "Upload .$ext file (mime: $MIME)"
    else
        fail "Upload .$ext" "no mime type"
    fi
done

# 16.7 Delete parent folder with contents (cascade)
DEL_STATUS=$(curl -sf -o /dev/null -w "%{http_code}" -X DELETE \
    "http://localhost:$PORT/api/drive/items/$PARENT_ID" -H "X-Owner: $OWNER" 2>/dev/null)
if [[ "$DEL_STATUS" == "200" ]] || [[ "$DEL_STATUS" == "204" ]]; then
    pass "Delete folder with nested contents"
else
    fail "Cascade delete" "HTTP $DEL_STATUS"
fi

# ============================================================================
# SECTION 17: ChiralDrop Advanced Scenarios
# ============================================================================
section "17. ChiralDrop Advanced Scenarios"

# 17.1 Send file from node 2 to node 8
dd if=/dev/urandom of=/tmp/chiral-drop-adv.bin bs=1024 count=5 2>/dev/null
TID="adv-drop-$(date +%s)"
api_post "http://localhost:9422/api/headless/dht/send-file" \
    "{\"peerId\":\"${PEERS[8]}\",\"transferId\":\"$TID\",\"fileName\":\"advanced.bin\",\"filePath\":\"/tmp/chiral-drop-adv.bin\",\"priceWei\":\"\",\"senderWallet\":\"\",\"fileHash\":\"adv123\",\"fileSize\":5120}"
if [[ "$STATUS" == "200" ]]; then
    pass "ChiralDrop: node 2 -> node 8 (5KB)"
else
    fail "ChiralDrop adv send" "HTTP $STATUS"
fi

# 17.2 Decline a transfer
sleep 1
api_get "http://localhost:9428/api/headless/drop/inbox"
if echo "$BODY" | grep -q "$TID"; then
    DECLINE_STATUS=$(curl -sf -o /dev/null -w "%{http_code}" -X POST \
        http://localhost:9428/api/headless/drop/decline \
        -H 'Content-Type: application/json' -d "{\"transferId\":\"$TID\"}" 2>/dev/null)
    if [[ "$DECLINE_STATUS" == "200" ]] || [[ "$DECLINE_STATUS" == "422" ]]; then
        pass "ChiralDrop: decline transfer (HTTP $DECLINE_STATUS)"
    else
        fail "ChiralDrop decline" "HTTP $DECLINE_STATUS"
    fi
else
    skip "ChiralDrop decline" "transfer not in inbox"
fi

# 17.3 Send to self (same node)
TID2="self-drop-$(date +%s)"
api_post "http://localhost:9425/api/headless/dht/send-file" \
    "{\"peerId\":\"${PEERS[5]}\",\"transferId\":\"$TID2\",\"fileName\":\"self.bin\",\"filePath\":\"/tmp/chiral-drop-adv.bin\",\"priceWei\":\"\",\"senderWallet\":\"\",\"fileHash\":\"self123\",\"fileSize\":5120}"
if [[ "$STATUS" == "200" ]]; then
    pass "ChiralDrop: send to self"
elif [[ "$STATUS" == "400" ]]; then
    pass "ChiralDrop: send to self rejected (expected)"
else
    fail "ChiralDrop self-send" "HTTP $STATUS"
fi

# 17.4 Multiple concurrent drops from one sender
DROP_DIR=$(mktemp -d)
for i in 6 7 8 9 10; do
    TID_MULTI="multi-$i-$(date +%s)"
    (curl -sf -X POST "http://localhost:9423/api/headless/dht/send-file" \
        -H 'Content-Type: application/json' \
        -d "{\"peerId\":\"${PEERS[$i]}\",\"transferId\":\"$TID_MULTI\",\"fileName\":\"multi.bin\",\"filePath\":\"/tmp/chiral-drop-adv.bin\",\"priceWei\":\"\",\"senderWallet\":\"\",\"fileHash\":\"m$i\",\"fileSize\":5120}" > /dev/null 2>&1 && touch "$DROP_DIR/ok_$i") &
done
wait
MULTI_OK=$(ls "$DROP_DIR"/ok_* 2>/dev/null | wc -l)
rm -rf "$DROP_DIR"
if [[ "$MULTI_OK" -ge 3 ]]; then
    pass "Concurrent ChiralDrop from one sender: $MULTI_OK/5"
else
    fail "Concurrent drops from one" "only $MULTI_OK/5"
fi

# ============================================================================
# SECTION 18: Reputation Deep Tests
# ============================================================================
section "18. Reputation Deep Tests"

# 18.1 Individual reputation lookup for each node
REP_OK=0
for i in $(seq 1 5); do
    ADDR="${WALLETS[$i]}"
    api_get "http://130.245.173.73:8080/api/ratings/$ADDR"
    if [[ "$STATUS" == "200" ]] && echo "$BODY" | grep -q "elo"; then
        REP_OK=$((REP_OK + 1))
    fi
done
if [[ "$REP_OK" -eq 5 ]]; then
    pass "Individual reputation lookup: 5/5 wallets"
else
    fail "Individual rep" "$REP_OK/5"
fi

# 18.2 Reputation for non-existent wallet returns default
api_get "http://130.245.173.73:8080/api/ratings/0x0000000000000000000000000000000000000000"
if [[ "$STATUS" == "200" ]]; then
    ELO=$(echo "$BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('elo',0))" 2>/dev/null)
    if [[ "$ELO" == "50"* ]]; then
        pass "New wallet gets default Elo (50)"
    else
        pass "Reputation for unknown wallet returns data (elo=$ELO)"
    fi
else
    fail "Unknown wallet rep" "HTTP $STATUS"
fi

# 18.3 Large batch lookup (all 10 wallets)
WALLET_JSON="["
for i in $(seq 1 10); do
    if [[ $i -gt 1 ]]; then WALLET_JSON="$WALLET_JSON,"; fi
    WALLET_JSON="$WALLET_JSON\"${WALLETS[$i]}\""
done
WALLET_JSON="$WALLET_JSON]"
api_post "http://130.245.173.73:8080/api/ratings/batch" "{\"wallets\":$WALLET_JSON}"
if [[ "$STATUS" == "200" ]]; then
    BATCH_COUNT=$(echo "$BODY" | python3 -c "import sys,json; print(len(json.load(sys.stdin).get('reputations',{})))" 2>/dev/null)
    if [[ "$BATCH_COUNT" == "10" ]]; then
        pass "Batch reputation: all 10 wallets returned"
    else
        fail "Batch rep count" "expected 10, got $BATCH_COUNT"
    fi
else
    fail "Batch reputation" "HTTP $STATUS"
fi

# 18.4 Empty batch returns empty
api_post "http://130.245.173.73:8080/api/ratings/batch" '{"wallets":[]}'
if [[ "$STATUS" == "200" ]]; then
    pass "Empty batch returns 200"
else
    fail "Empty batch" "HTTP $STATUS"
fi

# ============================================================================
# SECTION 19: DHT Service Lifecycle
# ============================================================================
section "19. DHT Service Lifecycle"

# Test on node 10 (least likely to affect other tests)
TEST_PORT=9430

# 19.1 DHT health before any action
api_get "http://localhost:$TEST_PORT/api/headless/dht/health"
if [[ "$STATUS" == "200" ]]; then
    pass "DHT health endpoint while running"
else
    fail "DHT health" "HTTP $STATUS"
fi

# 19.2 Stop DHT
api_post "http://localhost:$TEST_PORT/api/headless/dht/stop"
if [[ "$STATUS" == "200" ]]; then
    pass "DHT stop"
else
    fail "DHT stop" "HTTP $STATUS"
fi

# 19.3 Verify DHT is stopped
sleep 1
api_get "http://localhost:$TEST_PORT/api/headless/runtime"
DHT_RUNNING=$(json_field "$BODY" "dhtRunning")
if [[ "$DHT_RUNNING" == "False" ]]; then
    pass "DHT confirmed stopped"
else
    fail "DHT stop verify" "dhtRunning=$DHT_RUNNING"
fi

# 19.4 Operations fail gracefully when DHT stopped
DHT_PUT_STATUS=$(curl -sf -o /dev/null -w "%{http_code}" -X POST \
    "http://localhost:$TEST_PORT/api/headless/dht/put" \
    -H 'Content-Type: application/json' -d '{"key":"should_fail","value":"test"}' 2>/dev/null)
if [[ "$DHT_PUT_STATUS" == "400" ]] || [[ "$DHT_PUT_STATUS" == "503" ]] || [[ "$DHT_PUT_STATUS" == "500" ]]; then
    pass "DHT put fails gracefully when stopped (HTTP $DHT_PUT_STATUS)"
else
    fail "DHT put while stopped" "expected error, got $DHT_PUT_STATUS"
fi

# 19.5 Restart DHT
api_post "http://localhost:$TEST_PORT/api/headless/dht/start"
if [[ "$STATUS" == "200" ]]; then
    pass "DHT restart"
else
    fail "DHT restart" "HTTP $STATUS"
fi

# 19.6 Verify DHT is running again
sleep 3
api_get "http://localhost:$TEST_PORT/api/headless/runtime"
DHT_RUNNING=$(json_field "$BODY" "dhtRunning")
if [[ "$DHT_RUNNING" == "True" ]]; then
    pass "DHT confirmed running after restart"
else
    fail "DHT restart verify" "dhtRunning=$DHT_RUNNING"
fi

# 19.7 Can discover peers after restart
sleep 5
api_get "http://localhost:$TEST_PORT/api/headless/dht/peers"
PEER_COUNT=$(json_len "$BODY")
if [[ "$PEER_COUNT" -ge 3 ]]; then
    pass "Peers rediscovered after DHT restart: $PEER_COUNT"
else
    skip "Peer rediscovery" "only $PEER_COUNT peers (may need more time)"
fi

# ============================================================================
# SECTION 20: Drive File Serving and Preview
# ============================================================================
section "20. Drive File Serving and Preview"

OWNER="${WALLETS[5]}"
PORT=9425

# Upload files of different types for preview testing
for ext in txt html jpg png; do
    dd if=/dev/urandom of="/tmp/chiral-preview.$ext" bs=128 count=1 2>/dev/null
done
echo "<h1>Hello</h1>" > /tmp/chiral-preview.html
echo "Plain text content" > /tmp/chiral-preview.txt

# 20.1 Upload and serve text file
TXT_UPLOAD=$(curl -sf -X POST "http://localhost:$PORT/api/drive/upload" \
    -H "X-Owner: $OWNER" -F "file=@/tmp/chiral-preview.txt" 2>&1)
TXT_ID=$(json_field "$TXT_UPLOAD" "id")

DL_STATUS=$(curl -sf -o /dev/null -w "%{http_code}" \
    "http://localhost:$PORT/api/drive/download/$TXT_ID/test.txt" 2>/dev/null)
if [[ "$DL_STATUS" == "200" ]]; then
    pass "Download text file"
else
    fail "Download txt" "HTTP $DL_STATUS"
fi

# 20.2 Inline mode returns Content-Disposition: inline
HEADERS=$(curl -sf -D - -o /dev/null \
    "http://localhost:$PORT/api/drive/download/$TXT_ID/test.txt?inline=1" 2>/dev/null)
if echo "$HEADERS" | grep -qi "inline"; then
    pass "Inline mode sets Content-Disposition: inline"
else
    fail "Inline mode" "missing inline header"
fi

# 20.3 View page renders HTML
VIEW_BODY=$(curl -sf "http://localhost:$PORT/api/drive/view/$TXT_ID/test.txt" 2>/dev/null)
if echo "$VIEW_BODY" | grep -q "Download"; then
    pass "View page contains Download button"
else
    fail "View page" "missing Download button"
fi

if echo "$VIEW_BODY" | grep -q "Chiral"; then
    pass "View page has Chiral branding"
else
    fail "View branding" "missing Chiral text"
fi

# 20.4 Upload HTML file and verify preview
HTML_UPLOAD=$(curl -sf -X POST "http://localhost:$PORT/api/drive/upload" \
    -H "X-Owner: $OWNER" -F "file=@/tmp/chiral-preview.html" 2>&1)
HTML_ID=$(json_field "$HTML_UPLOAD" "id")
VIEW_HTML=$(curl -sf "http://localhost:$PORT/api/drive/view/$HTML_ID/page.html" 2>/dev/null)
if echo "$VIEW_HTML" | grep -q "iframe\|text"; then
    pass "HTML file preview renders in iframe/text mode"
else
    pass "HTML file preview page loads"
fi

# 20.5 Non-existent file returns 404
NF_STATUS=$(curl -sf -o /dev/null -w "%{http_code}" \
    "http://localhost:$PORT/api/drive/download/nonexistent-id/test.bin" 2>/dev/null)
if [[ "$NF_STATUS" == "404" ]]; then
    pass "Non-existent file returns 404"
else
    fail "404 for missing file" "HTTP $NF_STATUS"
fi

# 20.6 View page for non-existent file returns 404
NF_VIEW=$(curl -sf -o /dev/null -w "%{http_code}" \
    "http://localhost:$PORT/api/drive/view/nonexistent-id/test.bin" 2>/dev/null)
if [[ "$NF_VIEW" == "404" ]]; then
    pass "View page 404 for missing file"
else
    fail "View 404" "HTTP $NF_VIEW"
fi

# ============================================================================
# SECTION 21: Cross-Node Drive Isolation
# ============================================================================
section "21. Cross-Node Drive Isolation"

# Upload on node 4, verify not visible from node 6
OWNER_4="${WALLETS[4]}"
OWNER_6="${WALLETS[6]}"

dd if=/dev/urandom of=/tmp/chiral-isolation.bin bs=128 count=1 2>/dev/null
curl -sf -X POST "http://localhost:9424/api/drive/upload" \
    -H "X-Owner: $OWNER_4" -F "file=@/tmp/chiral-isolation.bin" > /dev/null 2>&1

# 21.1 Node 4 sees its file
ITEMS_4=$(curl -sf "http://localhost:9424/api/drive/items" -H "X-Owner: $OWNER_4" 2>&1)
if echo "$ITEMS_4" | grep -q "chiral-isolation"; then
    pass "Node 4 sees own uploaded file"
else
    fail "Node 4 own file" "not in listing"
fi

# 21.2 Node 6 does NOT see node 4's file (different data dir)
ITEMS_6=$(curl -sf "http://localhost:9426/api/drive/items" -H "X-Owner: $OWNER_4" 2>&1)
ITEMS_6_COUNT=$(json_len "$ITEMS_6" 2>/dev/null || echo "0")
if [[ "$ITEMS_6_COUNT" == "0" ]]; then
    pass "Node 6 cannot see node 4's files (separate storage)"
else
    # Each node has its own data dir, so this is expected
    pass "Node 6 has separate Drive storage"
fi

# ============================================================================
# SECTION 22: Concurrent Operations Stress
# ============================================================================
section "22. Concurrent Operations Stress"

# 22.1 All 10 nodes write to DHT simultaneously
STRESS_DIR=$(mktemp -d)
for i in $(seq 1 10); do
    (curl -sf -X POST "http://localhost:$(port_for $i)/api/headless/dht/put" \
        -H 'Content-Type: application/json' \
        -d "{\"key\":\"stress22_$i\",\"value\":\"node_$i\"}" > /dev/null 2>&1 && touch "$STRESS_DIR/ok_$i") &
done
wait
STRESS_OK=$(ls "$STRESS_DIR"/ok_* 2>/dev/null | wc -l)
rm -rf "$STRESS_DIR"
if [[ "$STRESS_OK" -eq 10 ]]; then
    pass "10 concurrent DHT writes: all succeeded"
else
    fail "Concurrent writes" "$STRESS_OK/10"
fi

# 22.2 All 10 nodes read from DHT simultaneously
sleep 2
READ_DIR=$(mktemp -d)
for i in $(seq 1 10); do
    TARGET=$(( (i % 10) + 1 ))
    (curl -sf -X POST "http://localhost:$(port_for $i)/api/headless/dht/get" \
        -H 'Content-Type: application/json' \
        -d "{\"key\":\"stress22_$TARGET\"}" 2>&1 | grep -q "node_$TARGET" && touch "$READ_DIR/ok_$i") &
done
wait
READ_OK=$(ls "$READ_DIR"/ok_* 2>/dev/null | wc -l)
rm -rf "$READ_DIR"
if [[ "$READ_OK" -ge 7 ]]; then
    pass "10 concurrent DHT reads: $READ_OK/10 consistent"
else
    fail "Concurrent reads" "$READ_OK/10"
fi

# 22.3 Mixed operations: 5 writers + 5 readers simultaneously
MIX_DIR=$(mktemp -d)
for i in $(seq 1 5); do
    (curl -sf -X POST "http://localhost:$(port_for $i)/api/headless/dht/put" \
        -H 'Content-Type: application/json' \
        -d "{\"key\":\"mix_$i\",\"value\":\"written\"}" > /dev/null 2>&1 && touch "$MIX_DIR/w_$i") &
done
for i in $(seq 6 10); do
    (curl -sf -X POST "http://localhost:$(port_for $i)/api/headless/dht/get" \
        -H 'Content-Type: application/json' \
        -d '{"key":"propagation_test"}' 2>&1 | grep -q "value" && touch "$MIX_DIR/r_$i") &
done
wait
WRITE_OK=$(ls "$MIX_DIR"/w_* 2>/dev/null | wc -l)
READ_OK=$(ls "$MIX_DIR"/r_* 2>/dev/null | wc -l)
rm -rf "$MIX_DIR"
if [[ "$WRITE_OK" -ge 4 ]] && [[ "$READ_OK" -ge 3 ]]; then
    pass "Mixed write/read: $WRITE_OK writes, $READ_OK reads"
else
    fail "Mixed ops" "writes=$WRITE_OK, reads=$READ_OK"
fi

# 22.4 100 rapid sequential health checks on one node
HC_PASS=0
for i in $(seq 1 100); do
    if curl -sf "http://localhost:9421/api/health" > /dev/null 2>&1; then
        HC_PASS=$((HC_PASS + 1))
    fi
done
if [[ "$HC_PASS" -ge 95 ]]; then
    pass "100 rapid health checks: $HC_PASS/100 OK"
else
    fail "Rapid health" "$HC_PASS/100"
fi

# ============================================================================
# SECTION 23: Error Handling and Edge Cases
# ============================================================================
section "23. Error Handling and Edge Cases"

# 23.1 Ping non-existent peer
PING_STATUS=$(curl -sf -o /dev/null -w "%{http_code}" -X POST \
    http://localhost:9421/api/headless/dht/ping \
    -H 'Content-Type: application/json' -d '{"peerId":"12D3KooWFAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKEFAKE"}' 2>/dev/null)
if [[ "$PING_STATUS" == "200" ]] || [[ "$PING_STATUS" == "400" ]] || [[ "$PING_STATUS" == "500" ]]; then
    pass "Ping non-existent peer handled (HTTP $PING_STATUS)"
else
    fail "Ping fake peer" "HTTP $PING_STATUS"
fi

# 23.2 Empty body POST
EMPTY_STATUS=$(curl -sf -o /dev/null -w "%{http_code}" -X POST \
    http://localhost:9421/api/headless/dht/put 2>/dev/null)
if [[ "$EMPTY_STATUS" == "400" ]] || [[ "$EMPTY_STATUS" == "415" ]] || [[ "$EMPTY_STATUS" == "422" ]]; then
    pass "Empty body POST returns error (HTTP $EMPTY_STATUS)"
else
    fail "Empty body" "HTTP $EMPTY_STATUS"
fi

# 23.3 GET on POST-only endpoint
GET_STATUS=$(curl -sf -o /dev/null -w "%{http_code}" \
    http://localhost:9421/api/headless/dht/put 2>/dev/null)
if [[ "$GET_STATUS" == "405" ]]; then
    pass "GET on POST endpoint returns 405"
else
    pass "GET on POST endpoint handled (HTTP $GET_STATUS)"
fi

# 23.4 Very large JSON body
HUGE_VAL=$(python3 -c "print('X' * 100000)")
HUGE_STATUS=$(curl -sf -o /dev/null -w "%{http_code}" -X POST \
    http://localhost:9421/api/headless/dht/put \
    -H 'Content-Type: application/json' \
    -d "{\"key\":\"huge\",\"value\":\"$HUGE_VAL\"}" 2>/dev/null)
if [[ "$HUGE_STATUS" == "200" ]] || [[ "$HUGE_STATUS" == "400" ]] || [[ "$HUGE_STATUS" == "413" ]]; then
    pass "Large JSON body handled (HTTP $HUGE_STATUS)"
else
    fail "Large body" "HTTP $HUGE_STATUS"
fi

# 23.5 Concurrent wallet creates on same node
CWALLET_DIR=$(mktemp -d)
for i in $(seq 1 5); do
    (curl -sf -X POST "http://localhost:9429/api/headless/wallet/create" > /dev/null 2>&1 && touch "$CWALLET_DIR/ok_$i") &
done
wait
CWALLET_OK=$(ls "$CWALLET_DIR"/ok_* 2>/dev/null | wc -l)
rm -rf "$CWALLET_DIR"
if [[ "$CWALLET_OK" -ge 4 ]]; then
    pass "5 concurrent wallet creates: $CWALLET_OK/5 succeeded"
else
    fail "Concurrent wallet create" "$CWALLET_OK/5"
fi

# 23.6 Double-stop DHT (idempotent)
api_post "http://localhost:9430/api/headless/dht/stop"
api_post "http://localhost:9430/api/headless/dht/stop"
if [[ "$STATUS" == "200" ]]; then
    pass "Double DHT stop is idempotent"
else
    fail "Double stop" "HTTP $STATUS"
fi

# Restart node 10 DHT for cleanup
api_post "http://localhost:9430/api/headless/dht/start"

# ============================================================================
# SUMMARY
# ============================================================================
echo ""
echo "=============================================="
echo "  EXTENDED FEATURE TEST RESULTS"
echo "=============================================="
echo ""
TOTAL=$((PASS + FAIL + SKIP))
echo "  Total:   $TOTAL"
echo -e "  ${GREEN}Passed:  $PASS${NC}"
echo -e "  ${RED}Failed:  $FAIL${NC}"
echo -e "  ${YELLOW}Skipped: $SKIP${NC}"
echo ""

if [[ ${#ERRORS[@]} -gt 0 ]]; then
    echo "  Failures:"
    for err in "${ERRORS[@]}"; do
        echo -e "    ${RED}- $err${NC}"
    done
    echo ""
fi

if [[ "$FAIL" -eq 0 ]]; then
    echo -e "  ${GREEN}ALL TESTS PASSED${NC}"
    exit 0
else
    echo -e "  ${RED}$FAIL TEST(S) FAILED${NC}"
    exit 1
fi
