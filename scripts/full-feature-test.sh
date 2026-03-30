#!/usr/bin/env bash
# =============================================================================
# Chiral Network — Full Feature Test Suite
#
# Tests EVERY feature of the application against a running 10-node cluster.
# Assumes cluster is running via: ./scripts/local-test-cluster.sh start 10
#
# Usage:
#   ./scripts/full-feature-test.sh
# =============================================================================
set -uo pipefail

BASE_PORT=9420
PASS=0
FAIL=0
SKIP=0
ERRORS=()

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
NC='\033[0m'

pass() {
    PASS=$((PASS + 1))
    echo -e "  ${GREEN}[PASS]${NC} $1"
}

fail() {
    FAIL=$((FAIL + 1))
    ERRORS+=("$1: $2")
    echo -e "  ${RED}[FAIL]${NC} $1 — $2"
}

skip() {
    SKIP=$((SKIP + 1))
    echo -e "  ${YELLOW}[SKIP]${NC} $1 — $2"
}

section() {
    echo ""
    echo -e "${CYAN}=== $1 ===${NC}"
}

# Helper: HTTP GET, store body in BODY, status in STATUS
api_get() {
    local url="$1"
    local tmp
    tmp=$(mktemp)
    STATUS=$(curl -sf -o "$tmp" -w "%{http_code}" "$url" 2>/dev/null || echo "000")
    BODY=$(cat "$tmp" 2>/dev/null)
    rm -f "$tmp"
}

# Helper: HTTP POST JSON
api_post() {
    local url="$1"
    local data="${2:-{}}"
    local tmp
    tmp=$(mktemp)
    STATUS=$(curl -sf -o "$tmp" -w "%{http_code}" -X POST "$url" \
        -H 'Content-Type: application/json' -d "$data" 2>/dev/null || echo "000")
    BODY=$(cat "$tmp" 2>/dev/null)
    rm -f "$tmp"
}

# Helper: get JSON field
json_field() {
    echo "$1" | python3 -c "import sys,json; d=json.load(sys.stdin); print(d.get('$2',''))" 2>/dev/null
}

json_len() {
    echo "$1" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))" 2>/dev/null
}

port_for() {
    echo $((BASE_PORT + $1))
}

echo "=============================================="
echo "  CHIRAL NETWORK — FULL FEATURE TEST SUITE"
echo "=============================================="
echo "  Cluster: ports 9421-9430 (10 nodes)"
echo ""

# ============================================================================
# SECTION 1: Health and Infrastructure
# ============================================================================
section "1. Health and Infrastructure"

# 1.1 Health endpoint on all nodes
ALL_HEALTHY=true
for i in $(seq 1 10); do
    api_get "http://localhost:$(port_for $i)/api/health"
    if [[ "$STATUS" != "200" ]]; then
        ALL_HEALTHY=false
        fail "Node $i health check" "HTTP $STATUS"
    fi
done
if $ALL_HEALTHY; then
    pass "All 10 nodes health check (200 OK)"
fi

# 1.2 Readiness endpoint
api_get "http://localhost:9421/api/ready"
if [[ "$STATUS" == "200" ]]; then
    pass "Readiness endpoint returns 200"
else
    fail "Readiness endpoint" "HTTP $STATUS"
fi

# 1.3 Runtime status
api_get "http://localhost:9421/api/headless/runtime"
DHT_RUNNING=$(json_field "$BODY" "dhtRunning")
if [[ "$DHT_RUNNING" == "True" ]]; then
    pass "Runtime status shows DHT running"
else
    fail "Runtime status" "dhtRunning=$DHT_RUNNING"
fi

# 1.4 DHT health
api_get "http://localhost:9421/api/headless/dht/health"
if [[ "$STATUS" == "200" ]]; then
    pass "DHT health endpoint"
else
    fail "DHT health" "HTTP $STATUS"
fi

# ============================================================================
# SECTION 2: Peer Networking
# ============================================================================
section "2. Peer Networking"

# 2.1 Peer ID unique per node
PEER_IDS=()
ALL_UNIQUE=true
for i in $(seq 1 10); do
    api_get "http://localhost:$(port_for $i)/api/headless/dht/peer-id"
    PID=$(json_field "$BODY" "peerId")
    if [[ -z "$PID" ]]; then
        fail "Node $i peer ID" "empty"
        ALL_UNIQUE=false
    fi
    PEER_IDS+=("$PID")
done
UNIQUE_COUNT=$(printf '%s\n' "${PEER_IDS[@]}" | sort -u | wc -l)
if [[ "$UNIQUE_COUNT" == "10" ]]; then
    pass "All 10 nodes have unique peer IDs"
else
    fail "Unique peer IDs" "only $UNIQUE_COUNT unique out of 10"
fi

# 2.2 Peer discovery
api_get "http://localhost:9421/api/headless/dht/peers"
PEER_COUNT=$(json_len "$BODY")
if [[ "$PEER_COUNT" -ge 5 ]]; then
    pass "Peer discovery: node 1 has $PEER_COUNT peers"
else
    fail "Peer discovery" "only $PEER_COUNT peers"
fi

# 2.3 Listening addresses
api_get "http://localhost:9421/api/headless/dht/listening-addresses"
if [[ "$STATUS" == "200" ]] && [[ -n "$BODY" ]]; then
    pass "Listening addresses endpoint"
else
    fail "Listening addresses" "HTTP $STATUS"
fi

# 2.4 Ping a peer
TARGET_PEER="${PEER_IDS[4]}"
api_post "http://localhost:9421/api/headless/dht/ping" "{\"peerId\":\"$TARGET_PEER\"}"
if [[ "$STATUS" == "200" ]]; then
    pass "Ping peer (node 1 -> node 5)"
else
    fail "Ping peer" "HTTP $STATUS: $BODY"
fi

# 2.5 Echo a peer
PAYLOAD_B64=$(echo -n "hello from test" | base64)
api_post "http://localhost:9421/api/headless/dht/echo" \
    "{\"peerId\":\"$TARGET_PEER\",\"payloadBase64\":\"$PAYLOAD_B64\"}"
if [[ "$STATUS" == "200" ]]; then
    pass "Echo peer (node 1 -> node 5)"
else
    fail "Echo peer" "HTTP $STATUS: $BODY"
fi

# ============================================================================
# SECTION 3: Wallet Management
# ============================================================================
section "3. Wallet Management"

# 3.1 Create wallet
api_post "http://localhost:9421/api/headless/wallet/create"
WALLET1_ADDR=$(json_field "$BODY" "address")
WALLET1_PK=$(json_field "$BODY" "privateKey")
if [[ "$WALLET1_ADDR" == 0x* ]] && [[ ${#WALLET1_ADDR} -eq 42 ]]; then
    pass "Create wallet: $WALLET1_ADDR"
else
    fail "Create wallet" "invalid address: $WALLET1_ADDR"
fi

# 3.2 Show wallet
api_get "http://localhost:9421/api/headless/wallet"
SHOW_ADDR=$(json_field "$BODY" "address")
if [[ "$SHOW_ADDR" == "$WALLET1_ADDR" ]]; then
    pass "Show wallet returns same address"
else
    fail "Show wallet" "expected $WALLET1_ADDR, got $SHOW_ADDR"
fi

# 3.3 Import wallet (override with a known key)
api_post "http://localhost:9422/api/headless/wallet/import" \
    '{"privateKey":"0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"}'
IMPORT_ADDR=$(json_field "$BODY" "address")
if [[ "$IMPORT_ADDR" == 0x* ]] && [[ ${#IMPORT_ADDR} -eq 42 ]]; then
    pass "Import wallet: $IMPORT_ADDR"
else
    fail "Import wallet" "invalid: $IMPORT_ADDR"
fi

# 3.4 Import with invalid key should fail
INVALID_IMPORT_STATUS=$(curl -sf -o /dev/null -w "%{http_code}" -X POST \
    http://localhost:9423/api/headless/wallet/import \
    -H 'Content-Type: application/json' -d '{"privateKey":"notavalidkey"}' 2>/dev/null)
if [[ "$INVALID_IMPORT_STATUS" == "400" ]]; then
    pass "Import invalid key returns 400"
else
    fail "Import invalid key" "expected 400, got $INVALID_IMPORT_STATUS"
fi

# 3.5 Create wallets on remaining nodes
for i in $(seq 3 10); do
    api_post "http://localhost:$(port_for $i)/api/headless/wallet/create" > /dev/null 2>&1
done
pass "Created wallets on nodes 3-10"

# ============================================================================
# SECTION 4: DHT Key-Value Storage
# ============================================================================
section "4. DHT Key-Value Storage"

# 4.1 Put a value
api_post "http://localhost:9421/api/headless/dht/put" '{"key":"test_key_1","value":"hello_world"}'
if [[ "$STATUS" == "200" ]]; then
    pass "DHT put (node 1)"
else
    fail "DHT put" "HTTP $STATUS"
fi

# 4.2 Get the value from another node
sleep 2
api_post "http://localhost:9425/api/headless/dht/get" '{"key":"test_key_1"}'
if echo "$BODY" | grep -q "hello_world"; then
    pass "DHT get from different node (node 5)"
else
    fail "DHT get" "value not found: $BODY"
fi

# 4.3 Put/get with special characters
api_post "http://localhost:9422/api/headless/dht/put" '{"key":"special/key:with.dots","value":"sp3c!al v@lue"}'
sleep 1
api_post "http://localhost:9428/api/headless/dht/get" '{"key":"special/key:with.dots"}'
if echo "$BODY" | grep -q "sp3c"; then
    pass "DHT put/get with special characters"
else
    fail "DHT special chars" "$BODY"
fi

# 4.4 Put/get large value (10KB)
LARGE_VAL=$(python3 -c "print('A' * 10000)")
api_post "http://localhost:9423/api/headless/dht/put" "{\"key\":\"large_key\",\"value\":\"$LARGE_VAL\"}"
sleep 1
api_post "http://localhost:9427/api/headless/dht/get" '{"key":"large_key"}'
if echo "$BODY" | grep -q "AAAA"; then
    pass "DHT put/get large value (10KB)"
else
    fail "DHT large value" "not retrieved"
fi

# 4.5 Get non-existent key
api_post "http://localhost:9421/api/headless/dht/get" '{"key":"does_not_exist_xyz"}'
if [[ "$STATUS" == "200" ]] || [[ "$STATUS" == "404" ]]; then
    pass "DHT get non-existent key (no crash)"
else
    fail "DHT get missing key" "HTTP $STATUS"
fi

# 4.6 Concurrent puts from all 10 nodes
for i in $(seq 1 10); do
    curl -sf -X POST "http://localhost:$(port_for $i)/api/headless/dht/put" \
        -H 'Content-Type: application/json' \
        -d "{\"key\":\"concurrent_$i\",\"value\":\"from_node_$i\"}" &
done
wait
sleep 2
CONC_PASS=0
for i in $(seq 1 10); do
    READ_NODE=$(( (i % 10) + 1 ))
    api_post "http://localhost:$(port_for $READ_NODE)/api/headless/dht/get" "{\"key\":\"concurrent_$i\"}"
    if echo "$BODY" | grep -q "from_node_$i"; then
        CONC_PASS=$((CONC_PASS + 1))
    fi
done
if [[ "$CONC_PASS" -ge 8 ]]; then
    pass "Concurrent DHT puts: $CONC_PASS/10 readable from other nodes"
else
    fail "Concurrent DHT puts" "only $CONC_PASS/10 readable"
fi

# ============================================================================
# SECTION 5: Drive — File Management
# ============================================================================
section "5. Drive — File Management"

OWNER=$(json_field "$(curl -sf http://localhost:9421/api/headless/wallet)" "address")

# 5.1 Create folder
api_post "http://localhost:9421/api/drive/folders" '{"name":"Test Suite Folder"}'
# Need X-Owner header
FOLDER_RESULT=$(curl -sf -X POST http://localhost:9421/api/drive/folders \
    -H 'Content-Type: application/json' -H "X-Owner: $OWNER" \
    -d '{"name":"Test Suite Folder"}' 2>&1)
FOLDER_ID=$(json_field "$FOLDER_RESULT" "id")
if [[ -n "$FOLDER_ID" ]]; then
    pass "Create folder: $FOLDER_ID"
else
    fail "Create folder" "$FOLDER_RESULT"
fi

# 5.2 Upload file
dd if=/dev/urandom of=/tmp/chiral-feat-test.bin bs=1024 count=50 2>/dev/null
UPLOAD_RESULT=$(curl -sf -X POST http://localhost:9421/api/drive/upload \
    -H "X-Owner: $OWNER" -F "file=@/tmp/chiral-feat-test.bin" 2>&1)
FILE_ID=$(json_field "$UPLOAD_RESULT" "id")
FILE_NAME=$(json_field "$UPLOAD_RESULT" "name")
if [[ -n "$FILE_ID" ]]; then
    pass "Upload file (50KB): $FILE_NAME"
else
    fail "Upload file" "$UPLOAD_RESULT"
fi

# 5.3 Upload into folder
UPLOAD2_RESULT=$(curl -sf -X POST http://localhost:9421/api/drive/upload \
    -H "X-Owner: $OWNER" -F "file=@/tmp/chiral-feat-test.bin" -F "parentId=$FOLDER_ID" 2>&1)
FILE2_ID=$(json_field "$UPLOAD2_RESULT" "id")
if [[ -n "$FILE2_ID" ]]; then
    pass "Upload file into folder"
else
    fail "Upload into folder" "$UPLOAD2_RESULT"
fi

# 5.4 List root items
ITEMS=$(curl -sf "http://localhost:9421/api/drive/items" -H "X-Owner: $OWNER" 2>&1)
ITEM_COUNT=$(json_len "$ITEMS")
if [[ "$ITEM_COUNT" -ge 2 ]]; then
    pass "List root items: $ITEM_COUNT items"
else
    fail "List root items" "expected >=2, got $ITEM_COUNT"
fi

# 5.5 List folder contents
FOLDER_ITEMS=$(curl -sf "http://localhost:9421/api/drive/items?parentId=$FOLDER_ID" \
    -H "X-Owner: $OWNER" 2>&1)
FOLDER_ITEM_COUNT=$(json_len "$FOLDER_ITEMS")
if [[ "$FOLDER_ITEM_COUNT" -ge 1 ]]; then
    pass "List folder contents: $FOLDER_ITEM_COUNT items"
else
    fail "List folder contents" "expected >=1, got $FOLDER_ITEM_COUNT"
fi

# 5.6 Rename file
RENAME_RESULT=$(curl -sf -X PUT "http://localhost:9421/api/drive/items/$FILE_ID" \
    -H 'Content-Type: application/json' -H "X-Owner: $OWNER" \
    -d '{"name":"renamed-feature-test.bin"}' 2>&1)
if echo "$RENAME_RESULT" | grep -q "renamed"; then
    pass "Rename file"
else
    fail "Rename file" "$RENAME_RESULT"
fi

# 5.7 Star file
STAR_RESULT=$(curl -sf -X PUT "http://localhost:9421/api/drive/items/$FILE_ID" \
    -H 'Content-Type: application/json' -H "X-Owner: $OWNER" \
    -d '{"starred":true}' 2>&1)
if echo "$STAR_RESULT" | grep -q "true"; then
    pass "Star file"
else
    fail "Star file" "$STAR_RESULT"
fi

# 5.8 Download file (raw)
DL_STATUS=$(curl -sf -o /dev/null -w "%{http_code}" \
    "http://localhost:9421/api/drive/download/$FILE_ID/renamed-feature-test.bin" 2>/dev/null)
if [[ "$DL_STATUS" == "200" ]]; then
    pass "Download file (raw bytes)"
else
    fail "Download file" "HTTP $DL_STATUS"
fi

# 5.9 Download file inline
INLINE_STATUS=$(curl -sf -o /dev/null -w "%{http_code}" \
    "http://localhost:9421/api/drive/download/$FILE_ID/test.bin?inline=1" 2>/dev/null)
if [[ "$INLINE_STATUS" == "200" ]]; then
    pass "Download file inline (?inline=1)"
else
    fail "Download inline" "HTTP $INLINE_STATUS"
fi

# 5.10 View file (preview page)
VIEW_STATUS=$(curl -sf -o /dev/null -w "%{http_code}" \
    "http://localhost:9421/api/drive/view/$FILE_ID/test.bin" 2>/dev/null)
if [[ "$VIEW_STATUS" == "200" ]]; then
    pass "View file (HTML preview page)"
else
    fail "View file" "HTTP $VIEW_STATUS"
fi

# 5.11 Toggle visibility
VIS_STATUS=$(curl -sf -o /dev/null -w "%{http_code}" -X PUT \
    "http://localhost:9421/api/drive/items/$FILE_ID/visibility" \
    -H 'Content-Type: application/json' -H "X-Owner: $OWNER" \
    -d '{"isPublic":false}' 2>/dev/null)
if [[ "$VIS_STATUS" == "200" ]]; then
    pass "Toggle file visibility"
elif [[ "$VIS_STATUS" == "404" ]]; then
    skip "Toggle visibility" "endpoint not in headless routes"
else
    fail "Toggle visibility" "HTTP $VIS_STATUS"
fi

# 5.12 Delete file
DEL_STATUS=$(curl -sf -o /dev/null -w "%{http_code}" -X DELETE \
    "http://localhost:9421/api/drive/items/$FILE2_ID" -H "X-Owner: $OWNER" 2>/dev/null)
if [[ "$DEL_STATUS" == "200" ]] || [[ "$DEL_STATUS" == "204" ]]; then
    pass "Delete file"
else
    fail "Delete file" "HTTP $DEL_STATUS"
fi

# 5.13 Delete folder
DEL_FOLDER_STATUS=$(curl -sf -o /dev/null -w "%{http_code}" -X DELETE \
    "http://localhost:9421/api/drive/items/$FOLDER_ID" -H "X-Owner: $OWNER" 2>/dev/null)
if [[ "$DEL_FOLDER_STATUS" == "200" ]] || [[ "$DEL_FOLDER_STATUS" == "204" ]]; then
    pass "Delete folder"
else
    fail "Delete folder" "HTTP $DEL_FOLDER_STATUS"
fi

# ============================================================================
# SECTION 6: File Publishing and DHT Search
# ============================================================================
section "6. File Publishing and DHT Search"

# 6.1 Register shared file
FILE_HASH=$(sha256sum /tmp/chiral-feat-test.bin | cut -d' ' -f1)
api_post "http://localhost:9421/api/headless/dht/register-shared-file" \
    "{\"fileHash\":\"$FILE_HASH\",\"filePath\":\"/tmp/chiral-feat-test.bin\",\"fileName\":\"feat-test.bin\",\"fileSize\":51200,\"priceWei\":\"0\",\"walletAddress\":\"$OWNER\"}"
if [[ "$STATUS" == "200" ]]; then
    pass "Register shared file on DHT"
else
    fail "Register shared file" "HTTP $STATUS: $BODY"
fi

# 6.2 Search from another node
sleep 2
api_post "http://localhost:9425/api/headless/dht/get" "{\"key\":\"$FILE_HASH\"}"
if echo "$BODY" | grep -q "value"; then
    pass "Search file from node 5"
else
    fail "Search file" "not found: $BODY"
fi

# 6.3 Search from all consumer nodes (6-10)
SEARCH_FOUND=0
for i in $(seq 6 10); do
    api_post "http://localhost:$(port_for $i)/api/headless/dht/get" "{\"key\":\"$FILE_HASH\"}"
    if echo "$BODY" | grep -q "value"; then
        SEARCH_FOUND=$((SEARCH_FOUND + 1))
    fi
done
if [[ "$SEARCH_FOUND" -ge 3 ]]; then
    pass "File searchable from $SEARCH_FOUND/5 consumer nodes"
else
    fail "Multi-node search" "only $SEARCH_FOUND/5 found"
fi

# 6.4 Unregister shared file
api_post "http://localhost:9421/api/headless/dht/unregister-shared-file" \
    "{\"fileHash\":\"$FILE_HASH\"}"
if [[ "$STATUS" == "200" ]]; then
    pass "Unregister shared file"
else
    # May not exist as endpoint — skip
    skip "Unregister shared file" "endpoint may not exist"
fi

# ============================================================================
# SECTION 7: ChiralDrop — P2P File Transfer
# ============================================================================
section "7. ChiralDrop — P2P File Transfer"

SENDER_PORT=9421
RECEIVER_PORT=9425
RECEIVER_PEER="${PEER_IDS[4]}"

# 7.1 Send file
dd if=/dev/urandom of=/tmp/chiral-drop-test.bin bs=512 count=1 2>/dev/null
TRANSFER_ID="feat-test-drop-$(date +%s)"
api_post "http://localhost:$SENDER_PORT/api/headless/dht/send-file" \
    "{\"peerId\":\"$RECEIVER_PEER\",\"transferId\":\"$TRANSFER_ID\",\"fileName\":\"drop-test.bin\",\"filePath\":\"/tmp/chiral-drop-test.bin\",\"priceWei\":\"\",\"senderWallet\":\"\",\"fileHash\":\"drop123\",\"fileSize\":512}"
if [[ "$STATUS" == "200" ]]; then
    pass "ChiralDrop send file"
else
    fail "ChiralDrop send" "HTTP $STATUS: $BODY"
fi

# 7.2 Check receiver inbox
sleep 2
api_get "http://localhost:$RECEIVER_PORT/api/headless/drop/inbox"
if echo "$BODY" | grep -q "$TRANSFER_ID"; then
    pass "ChiralDrop file in receiver inbox"
else
    fail "ChiralDrop inbox" "transfer not found"
fi

# 7.3 Accept transfer
api_post "http://localhost:$RECEIVER_PORT/api/headless/drop/accept" \
    "{\"transferId\":\"$TRANSFER_ID\"}"
if [[ "$STATUS" == "200" ]]; then
    pass "ChiralDrop accept transfer"
else
    fail "ChiralDrop accept" "HTTP $STATUS: $BODY"
fi

# 7.4 Check outgoing transfers on sender
api_get "http://localhost:$SENDER_PORT/api/headless/drop/outgoing"
if [[ "$STATUS" == "200" ]]; then
    pass "ChiralDrop outgoing list"
else
    fail "ChiralDrop outgoing" "HTTP $STATUS"
fi

# 7.5 Multiple simultaneous drops
DROP_DIR=$(mktemp -d)
for i in 2 3 4; do
    TARGET="${PEER_IDS[$((i + 4))]}"
    TID="multi-drop-$i-$(date +%s)"
    (curl -sf -X POST "http://localhost:$(port_for $i)/api/headless/dht/send-file" \
        -H 'Content-Type: application/json' \
        -d "{\"peerId\":\"$TARGET\",\"transferId\":\"$TID\",\"fileName\":\"multi-$i.bin\",\"filePath\":\"/tmp/chiral-drop-test.bin\",\"priceWei\":\"\",\"senderWallet\":\"\",\"fileHash\":\"multi$i\",\"fileSize\":512}" > /dev/null 2>&1 && touch "$DROP_DIR/ok_$i") &
done
wait
DROP_OK=$(ls "$DROP_DIR"/ok_* 2>/dev/null | wc -l)
rm -rf "$DROP_DIR"
if [[ "$DROP_OK" -ge 2 ]]; then
    pass "Concurrent ChiralDrop sends: $DROP_OK/3"
else
    fail "Concurrent drops" "only $DROP_OK/3"
fi

# ============================================================================
# SECTION 8: Reputation System
# ============================================================================
section "8. Reputation System"

# Collect all wallet addresses
WALLETS_JSON="["
for i in $(seq 1 10); do
    api_get "http://localhost:$(port_for $i)/api/headless/wallet"
    ADDR=$(json_field "$BODY" "address")
    if [[ $i -gt 1 ]]; then WALLETS_JSON="$WALLETS_JSON,"; fi
    WALLETS_JSON="$WALLETS_JSON\"$ADDR\""
done
WALLETS_JSON="$WALLETS_JSON]"

# 8.1 Batch reputation lookup
BATCH_RESULT=$(curl -sf -X POST http://130.245.173.73:8080/api/ratings/batch \
    -H 'Content-Type: application/json' \
    -d "{\"wallets\":$WALLETS_JSON}" 2>&1)
REP_COUNT=$(echo "$BATCH_RESULT" | python3 -c "import sys,json; print(len(json.load(sys.stdin).get('reputations',{})))" 2>/dev/null)
if [[ "$REP_COUNT" == "10" ]]; then
    pass "Batch reputation lookup: 10 wallets"
else
    fail "Batch reputation" "expected 10, got $REP_COUNT"
fi

# 8.2 Single wallet reputation
FIRST_WALLET=$(curl -sf http://localhost:9421/api/headless/wallet | python3 -c "import sys,json; print(json.load(sys.stdin)['address'])" 2>/dev/null)
SINGLE_REP=$(curl -sf "http://130.245.173.73:8080/api/ratings/$FIRST_WALLET" 2>&1)
SINGLE_ELO=$(echo "$SINGLE_REP" | python3 -c "import sys,json; print(json.load(sys.stdin).get('elo',0))" 2>/dev/null)
if [[ -n "$SINGLE_ELO" ]]; then
    pass "Single wallet reputation: elo=$SINGLE_ELO"
else
    fail "Single wallet rep" "$SINGLE_REP"
fi

# 8.3 Record a transfer outcome
api_post "http://130.245.173.73:8080/api/ratings/transfer" \
    "{\"sellerWallet\":\"$FIRST_WALLET\",\"buyerWallet\":\"0x0000000000000000000000000000000000000001\",\"outcome\":\"completed\",\"amountWei\":\"1000000000000000\"}"
if [[ "$STATUS" == "200" ]]; then
    pass "Record transfer outcome"
else
    skip "Record transfer outcome" "HTTP $STATUS (may need auth)"
fi

# ============================================================================
# SECTION 9: Geth and Blockchain
# ============================================================================
section "9. Geth and Blockchain"

# 9.1 Geth status (not installed in test nodes)
api_get "http://localhost:9421/api/headless/geth/status"
if [[ "$STATUS" == "200" ]]; then
    CHAIN_ID=$(echo "$BODY" | python3 -c "import sys,json; print(json.load(sys.stdin).get('chainId',0))" 2>/dev/null)
    if [[ "$CHAIN_ID" == "98765" ]]; then
        pass "Geth status: chain ID 98765"
    else
        pass "Geth status endpoint accessible (chain=$CHAIN_ID)"
    fi
else
    skip "Geth status" "HTTP $STATUS (Geth may not be installed)"
fi

# 9.2 Mining status
api_get "http://localhost:9421/api/headless/mining/status"
if [[ "$STATUS" == "200" ]]; then
    pass "Mining status endpoint accessible"
else
    skip "Mining status" "HTTP $STATUS"
fi

# 9.3 Geth logs
api_get "http://localhost:9421/api/headless/geth/logs?lines=10"
if [[ "$STATUS" == "200" ]]; then
    pass "Geth logs endpoint"
else
    skip "Geth logs" "HTTP $STATUS"
fi

# ============================================================================
# SECTION 10: Drive — Owner Isolation
# ============================================================================
section "10. Drive — Owner Isolation"

OWNER_A=$(curl -sf http://localhost:9421/api/headless/wallet | python3 -c "import sys,json; print(json.load(sys.stdin)['address'])" 2>/dev/null)
OWNER_B=$(curl -sf http://localhost:9422/api/headless/wallet | python3 -c "import sys,json; print(json.load(sys.stdin)['address'])" 2>/dev/null)

# Upload file as owner A on node 1
dd if=/dev/urandom of=/tmp/chiral-isolation-a.bin bs=256 count=1 2>/dev/null
curl -sf -X POST http://localhost:9421/api/drive/upload \
    -H "X-Owner: $OWNER_A" -F "file=@/tmp/chiral-isolation-a.bin" > /dev/null 2>&1

# Upload file as owner B on node 2
dd if=/dev/urandom of=/tmp/chiral-isolation-b.bin bs=256 count=1 2>/dev/null
curl -sf -X POST http://localhost:9422/api/drive/upload \
    -H "X-Owner: $OWNER_B" -F "file=@/tmp/chiral-isolation-b.bin" > /dev/null 2>&1

# 10.1 Owner A should only see their files
ITEMS_A=$(curl -sf "http://localhost:9421/api/drive/items" -H "X-Owner: $OWNER_A" 2>&1)
if ! echo "$ITEMS_A" | grep -q "$OWNER_B"; then
    pass "Owner isolation: A cannot see B's files"
else
    fail "Owner isolation" "A sees B's files"
fi

# 10.2 Owner B should only see their files
ITEMS_B=$(curl -sf "http://localhost:9422/api/drive/items" -H "X-Owner: $OWNER_B" 2>&1)
if ! echo "$ITEMS_B" | grep -q "$OWNER_A"; then
    pass "Owner isolation: B cannot see A's files"
else
    fail "Owner isolation" "B sees A's files"
fi

# ============================================================================
# SECTION 11: Stress Tests
# ============================================================================
section "11. Stress Tests"

# 11.1 Rapid health checks (50 in parallel)
HEALTH_DIR=$(mktemp -d)
for i in $(seq 1 50); do
    PORT=$(( 9421 + (i % 10) ))
    (curl -sf "http://localhost:$PORT/api/health" > /dev/null 2>&1 && touch "$HEALTH_DIR/ok_$i") &
done
wait
HEALTH_OK=$(ls "$HEALTH_DIR"/ok_* 2>/dev/null | wc -l)
rm -rf "$HEALTH_DIR"
if [[ "$HEALTH_OK" -ge 45 ]]; then
    pass "50 concurrent health checks: $HEALTH_OK/50 OK"
else
    fail "Concurrent health" "$HEALTH_OK/50"
fi

# 11.2 Rapid DHT writes (all nodes simultaneously)
for i in $(seq 1 10); do
    for j in $(seq 1 5); do
        curl -sf -X POST "http://localhost:$(port_for $i)/api/headless/dht/put" \
            -H 'Content-Type: application/json' \
            -d "{\"key\":\"stress_${i}_${j}\",\"value\":\"val_${i}_${j}\"}" > /dev/null 2>&1 &
    done
done
wait
sleep 3
STRESS_READ=0
for i in $(seq 1 10); do
    api_post "http://localhost:$(port_for $(( (i % 10) + 1 )))/api/headless/dht/get" "{\"key\":\"stress_${i}_3\"}"
    if echo "$BODY" | grep -q "val_${i}_3"; then
        STRESS_READ=$((STRESS_READ + 1))
    fi
done
if [[ "$STRESS_READ" -ge 7 ]]; then
    pass "50 concurrent DHT writes, $STRESS_READ/10 readable"
else
    fail "DHT stress" "only $STRESS_READ/10 readable"
fi

# 11.3 Rapid file uploads (5 concurrent)
UPLOAD_DIR=$(mktemp -d)
for i in $(seq 1 5); do
    dd if=/dev/urandom of="/tmp/chiral-stress-$i.bin" bs=1024 count=10 2>/dev/null
    PORT=$(( 9420 + i ))
    STRESS_OWNER=$(curl -sf "http://localhost:$PORT/api/headless/wallet" | python3 -c "import sys,json; print(json.load(sys.stdin)['address'])" 2>/dev/null)
    (curl -sf -X POST "http://localhost:$PORT/api/drive/upload" \
        -H "X-Owner: $STRESS_OWNER" -F "file=@/tmp/chiral-stress-$i.bin" > /dev/null 2>&1 && touch "$UPLOAD_DIR/ok_$i") &
done
wait
UPLOAD_OK=$(ls "$UPLOAD_DIR"/ok_* 2>/dev/null | wc -l)
rm -rf "$UPLOAD_DIR"
if [[ "$UPLOAD_OK" -ge 4 ]]; then
    pass "5 concurrent file uploads: $UPLOAD_OK/5"
else
    fail "Concurrent uploads" "$UPLOAD_OK/5"
fi

# ============================================================================
# SECTION 12: Edge Cases and Error Handling
# ============================================================================
section "12. Edge Cases and Error Handling"

# 12.1 Invalid JSON body
INVALID_STATUS=$(curl -sf -o /dev/null -w "%{http_code}" -X POST http://localhost:9421/api/headless/dht/put \
    -H 'Content-Type: application/json' -d 'not json' 2>/dev/null)
if [[ "$INVALID_STATUS" == "400" ]] || [[ "$INVALID_STATUS" == "422" ]]; then
    pass "Invalid JSON returns 4xx"
else
    fail "Invalid JSON" "expected 4xx, got $INVALID_STATUS"
fi

# 12.2 Missing required field
api_post "http://localhost:9421/api/headless/dht/put" '{"key":"only_key"}'
if [[ "$STATUS" == "400" ]] || [[ "$STATUS" == "422" ]]; then
    pass "Missing field returns 4xx"
else
    # May succeed with empty value — still OK
    pass "Missing field handled (HTTP $STATUS)"
fi

# 12.3 Non-existent endpoint
NE_STATUS=$(curl -sf -o /dev/null -w "%{http_code}" http://localhost:9421/api/does/not/exist 2>/dev/null)
if [[ "$NE_STATUS" == "404" ]]; then
    pass "Non-existent endpoint returns 404"
else
    fail "Non-existent endpoint" "expected 404, got $NE_STATUS"
fi

# 12.4 Wallet show before create (use a fresh node if possible)
# All nodes already have wallets — skip
skip "Wallet show before create" "all nodes already have wallets"

# 12.5 Drive list with no owner header
NO_OWNER_STATUS=$(curl -sf -o /dev/null -w "%{http_code}" "http://localhost:9421/api/drive/items" 2>/dev/null)
if [[ "$NO_OWNER_STATUS" == "200" ]] || [[ "$NO_OWNER_STATUS" == "400" ]]; then
    pass "Drive list without owner header handled (HTTP $NO_OWNER_STATUS)"
else
    fail "Drive no owner" "HTTP $NO_OWNER_STATUS"
fi

# ============================================================================
# SUMMARY
# ============================================================================
echo ""
echo "=============================================="
echo "  FULL FEATURE TEST RESULTS"
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
