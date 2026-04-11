#!/usr/bin/env bash
# =============================================================================
# Chiral Network — User Simulation Test
#
# Simulates 20 real users performing random actions for X minutes.
# Each user runs in a background loop, picking random actions:
#   - Create/check wallet
#   - Send CHI to another user
#   - Upload file to Drive / CDN
#   - Publish file to DHT
#   - Search for files
#   - Download files
#   - Ping random peers
#   - Echo messages
#   - Check balances
#   - Publish host advertisements
#
# Detects: errors, timeouts (>5s), crashes, and logs all actions.
#
# Usage:
#   bash scripts/user-simulation.sh              # 5 minutes, k3s cluster
#   bash scripts/user-simulation.sh 10           # 10 minutes
#   bash scripts/user-simulation.sh 5 local      # 5 minutes, local Docker
# =============================================================================

set -euo pipefail

DURATION_MINS=${1:-5}
TARGET=${2:-k3s}  # "k3s" or "local"
USER_COUNT=20
DURATION_SECS=$((DURATION_MINS * 60))

# Node addressing
if [ "$TARGET" = "local" ]; then
  BASE_HOST="localhost"
else
  BASE_HOST="130.245.173.231"
fi
BASE_PORT=9501

# Results directory
RESULTS_DIR="/tmp/chiral-sim-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$RESULTS_DIR"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
DIM='\033[2m'
NC='\033[0m'

# Shared state files
WALLET_FILE="$RESULTS_DIR/wallets.txt"
FILE_HASHES="$RESULTS_DIR/file_hashes.txt"
PEER_IDS="$RESULTS_DIR/peer_ids.txt"
touch "$WALLET_FILE" "$FILE_HASHES" "$PEER_IDS"

# Counters (use files for cross-process counting)
for metric in actions successes failures timeouts errors; do
  echo 0 > "$RESULTS_DIR/$metric.count"
done

increment() {
  local file="$RESULTS_DIR/$1.count"
  flock "$file" bash -c "echo \$(( \$(cat '$file') + 1 )) > '$file'"
}

get_count() {
  cat "$RESULTS_DIR/$1.count" 2>/dev/null || echo 0
}

node_url() {
  local user_id=$1
  local port=$((BASE_PORT + (user_id % 30)))
  echo "http://$BASE_HOST:$port"
}

# Timed API call — detects timeouts and errors
api_call() {
  local user_id=$1
  local method=$2
  local path=$3
  local data=${4:-}
  local url="$(node_url $user_id)$path"
  local log="$RESULTS_DIR/user-$user_id.log"
  local start_ms=$(date +%s%N | cut -b1-13)

  local args=(-sf -X "$method" -H "Content-Type: application/json" --connect-timeout 3 --max-time 8)
  if [ -n "$data" ]; then
    args+=(-d "$data")
  fi

  local response
  local exit_code
  response=$(curl "${args[@]}" "$url" 2>&1) && exit_code=0 || exit_code=$?

  local end_ms=$(date +%s%N | cut -b1-13)
  local elapsed=$(( end_ms - start_ms ))

  increment actions

  if [ $exit_code -ne 0 ]; then
    if [ $exit_code -eq 28 ]; then
      increment timeouts
      echo "[$(date +%H:%M:%S)] TIMEOUT ${elapsed}ms $method $path" >> "$log"
    else
      increment errors
      echo "[$(date +%H:%M:%S)] ERROR($exit_code) ${elapsed}ms $method $path" >> "$log"
    fi
    return 1
  fi

  if [ $elapsed -gt 5000 ]; then
    increment timeouts
    echo "[$(date +%H:%M:%S)] SLOW ${elapsed}ms $method $path" >> "$log"
  else
    increment successes
  fi

  echo "$response"
  return 0
}

# ===== USER ACTIONS =====

action_create_wallet() {
  local uid=$1
  local resp=$(api_call $uid POST "/api/headless/wallet/create" "{}" 2>/dev/null) || return
  local addr=$(echo "$resp" | python3 -c "import sys,json; print(json.load(sys.stdin).get('address',''))" 2>/dev/null)
  if [ -n "$addr" ] && [ "$addr" != "" ]; then
    echo "$uid:$addr" >> "$WALLET_FILE"
    echo "[$(date +%H:%M:%S)] WALLET created $addr" >> "$RESULTS_DIR/user-$uid.log"
  fi
}

action_check_balance() {
  local uid=$1
  local addr=$(grep "^$uid:" "$WALLET_FILE" 2>/dev/null | tail -1 | cut -d: -f2)
  [ -z "$addr" ] && return
  local resp=$(api_call $uid POST "/api/headless/wallet/balance" "{\"address\":\"$addr\"}" 2>/dev/null) || return
  local bal=$(echo "$resp" | python3 -c "import sys,json; print(json.load(sys.stdin).get('balance','?'))" 2>/dev/null)
  echo "[$(date +%H:%M:%S)] BALANCE $bal CHI" >> "$RESULTS_DIR/user-$uid.log"
}

action_send_chi() {
  local uid=$1
  # Pick a random other user's wallet
  local targets=$(grep -v "^$uid:" "$WALLET_FILE" 2>/dev/null | shuf | head -1)
  [ -z "$targets" ] && return
  local to_addr=$(echo "$targets" | cut -d: -f2)
  local my_wallet=$(grep "^$uid:" "$WALLET_FILE" 2>/dev/null | tail -1 | cut -d: -f2)
  [ -z "$my_wallet" ] && return
  echo "[$(date +%H:%M:%S)] SEND attempt 0.001 CHI to ${to_addr:0:10}..." >> "$RESULTS_DIR/user-$uid.log"
  # This will likely fail (no funds) but tests the endpoint
  api_call $uid POST "/api/headless/wallet/send" \
    "{\"from\":\"$my_wallet\",\"to\":\"$to_addr\",\"amount\":\"0.001\",\"privateKey\":\"0x0000000000000000000000000000000000000000000000000000000000000001\"}" \
    >/dev/null 2>&1 || true
}

action_upload_file() {
  local uid=$1
  local fname="sim_file_${uid}_$(shuf -i 1000-9999 -n1).txt"
  local content="Simulation file from user $uid at $(date). Random: $RANDOM$RANDOM"
  local b64=$(echo -n "$content" | base64)
  local wallet=$(grep "^$uid:" "$WALLET_FILE" 2>/dev/null | tail -1 | cut -d: -f2)
  [ -z "$wallet" ] && wallet="0x$(printf '%040d' $uid)"

  local resp=$(api_call $uid POST "/api/cdn/upload" \
    "{\"fileName\":\"$fname\",\"fileData\":\"$b64\",\"ownerWallet\":\"$wallet\",\"paymentTx\":\"\",\"durationDays\":1}" \
    2>/dev/null) || return

  local hash=$(echo "$resp" | python3 -c "import sys,json; print(json.load(sys.stdin).get('fileHash',''))" 2>/dev/null)
  if [ -n "$hash" ] && [ "$hash" != "" ]; then
    echo "$hash" >> "$FILE_HASHES"
    echo "[$(date +%H:%M:%S)] UPLOAD $fname hash=${hash:0:16}..." >> "$RESULTS_DIR/user-$uid.log"
  fi
}

action_search_file() {
  local uid=$1
  local hash=$(shuf -n1 "$FILE_HASHES" 2>/dev/null)
  [ -z "$hash" ] && return
  local resp=$(api_call $uid POST "/api/headless/file/search" "{\"fileHash\":\"$hash\"}" 2>/dev/null) || return
  local found=$(echo "$resp" | python3 -c "import sys,json; print(json.load(sys.stdin).get('found',False))" 2>/dev/null)
  echo "[$(date +%H:%M:%S)] SEARCH ${hash:0:16}... found=$found" >> "$RESULTS_DIR/user-$uid.log"
}

action_register_file() {
  local uid=$1
  local hash="sim$(printf '%060d' $((uid * 1000 + RANDOM)))"
  local wallet=$(grep "^$uid:" "$WALLET_FILE" 2>/dev/null | tail -1 | cut -d: -f2)
  [ -z "$wallet" ] && wallet="0x$(printf '%040d' $uid)"

  api_call $uid POST "/api/headless/dht/register-shared-file" \
    "{\"fileHash\":\"$hash\",\"filePath\":\"/tmp/sim_$hash\",\"fileName\":\"sim_$uid.dat\",\"fileSize\":$((RANDOM * 100)),\"priceWei\":\"0\",\"walletAddress\":\"$wallet\"}" \
    >/dev/null 2>&1 || return

  echo "$hash" >> "$FILE_HASHES"
  echo "[$(date +%H:%M:%S)] REGISTER ${hash:0:16}..." >> "$RESULTS_DIR/user-$uid.log"
}

action_dht_put_get() {
  local uid=$1
  local key="simkey_${uid}_$(shuf -i 1-100 -n1)"
  local val="simval_${RANDOM}"

  api_call $uid POST "/api/headless/dht/put" "{\"key\":\"$key\",\"value\":\"$val\"}" >/dev/null 2>&1 || return

  sleep 1
  local resp=$(api_call $uid POST "/api/headless/dht/get" "{\"key\":\"$key\"}" 2>/dev/null) || return
  local got=$(echo "$resp" | python3 -c "import sys,json; v=json.load(sys.stdin).get('value'); print(v if v else '')" 2>/dev/null)

  if [ "$got" = "$val" ]; then
    echo "[$(date +%H:%M:%S)] DHT_KV put/get OK key=$key" >> "$RESULTS_DIR/user-$uid.log"
  else
    increment failures
    echo "[$(date +%H:%M:%S)] DHT_KV MISMATCH key=$key expected=$val got=$got" >> "$RESULTS_DIR/user-$uid.log"
  fi
}

action_ping_peer() {
  local uid=$1
  local peer=$(shuf -n1 "$PEER_IDS" 2>/dev/null)
  [ -z "$peer" ] && return
  api_call $uid POST "/api/headless/dht/ping" "{\"peerId\":\"$peer\"}" >/dev/null 2>&1
  echo "[$(date +%H:%M:%S)] PING ${peer:0:20}..." >> "$RESULTS_DIR/user-$uid.log"
}

action_echo_peer() {
  local uid=$1
  local peer=$(shuf -n1 "$PEER_IDS" 2>/dev/null)
  [ -z "$peer" ] && return
  local payload=$(echo -n "echo from user $uid at $(date +%s)" | base64)
  api_call $uid POST "/api/headless/dht/echo" "{\"peerId\":\"$peer\",\"payloadBase64\":\"$payload\"}" >/dev/null 2>&1
  echo "[$(date +%H:%M:%S)] ECHO ${peer:0:20}..." >> "$RESULTS_DIR/user-$uid.log"
}

action_check_peers() {
  local uid=$1
  local resp=$(api_call $uid GET "/api/headless/dht/peers" 2>/dev/null) || return
  local count=$(echo "$resp" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))" 2>/dev/null)
  echo "[$(date +%H:%M:%S)] PEERS count=$count" >> "$RESULTS_DIR/user-$uid.log"
}

action_check_health() {
  local uid=$1
  api_call $uid GET "/api/headless/dht/health" >/dev/null 2>&1
  echo "[$(date +%H:%M:%S)] HEALTH check" >> "$RESULTS_DIR/user-$uid.log"
}

action_publish_host_ad() {
  local uid=$1
  local wallet=$(grep "^$uid:" "$WALLET_FILE" 2>/dev/null | tail -1 | cut -d: -f2)
  [ -z "$wallet" ] && wallet="0x$(printf '%040d' $uid)"
  local now=$(date +%s)

  api_call $uid POST "/api/headless/hosting/publish-ad" \
    "{\"walletAddress\":\"$wallet\",\"maxStorageBytes\":$((RANDOM * 1000000)),\"usedStorageBytes\":0,\"pricePerMbPerDayWei\":\"1000000000000000\",\"minDepositWei\":\"10000000000000000\",\"uptimePercent\":100,\"publishedAt\":$now,\"lastHeartbeatAt\":$now}" \
    >/dev/null 2>&1
  echo "[$(date +%H:%M:%S)] HOST_AD published" >> "$RESULTS_DIR/user-$uid.log"
}

action_cdn_list() {
  local uid=$1
  local wallet=$(grep "^$uid:" "$WALLET_FILE" 2>/dev/null | tail -1 | cut -d: -f2)
  [ -z "$wallet" ] && return
  # Query CDN on the same node (CDN runs on :9420 on .73, but we query through the node)
  local resp=$(curl -sf --max-time 5 "http://130.245.173.73:9420/api/cdn/files?owner=$wallet" 2>/dev/null) || return
  local count=$(echo "$resp" | python3 -c "import sys,json; print(json.load(sys.stdin).get('totalFiles',0))" 2>/dev/null)
  echo "[$(date +%H:%M:%S)] CDN_LIST files=$count" >> "$RESULTS_DIR/user-$uid.log"
  increment actions
  increment successes
}

action_chain_id() {
  local uid=$1
  local resp=$(api_call $uid GET "/api/headless/wallet/chain-id" 2>/dev/null) || return
  local cid=$(echo "$resp" | python3 -c "import sys,json; print(json.load(sys.stdin).get('chainId',0))" 2>/dev/null)
  if [ "$cid" != "98765" ]; then
    increment failures
    echo "[$(date +%H:%M:%S)] CHAIN_ID WRONG: $cid" >> "$RESULTS_DIR/user-$uid.log"
  else
    echo "[$(date +%H:%M:%S)] CHAIN_ID OK" >> "$RESULTS_DIR/user-$uid.log"
  fi
}

# ===== WEIGHTED ACTION PICKER =====

# Actions and their relative weights (higher = more frequent)
ACTIONS=(
  "action_check_health:15"
  "action_check_peers:12"
  "action_check_balance:10"
  "action_search_file:10"
  "action_dht_put_get:8"
  "action_ping_peer:8"
  "action_upload_file:6"
  "action_register_file:5"
  "action_echo_peer:5"
  "action_send_chi:4"
  "action_cdn_list:4"
  "action_chain_id:3"
  "action_publish_host_ad:2"
  "action_create_wallet:1"
)

pick_action() {
  local total=0
  for entry in "${ACTIONS[@]}"; do
    local weight=${entry##*:}
    total=$((total + weight))
  done

  local roll=$((RANDOM % total))
  local cumulative=0
  for entry in "${ACTIONS[@]}"; do
    local action=${entry%%:*}
    local weight=${entry##*:}
    cumulative=$((cumulative + weight))
    if [ $roll -lt $cumulative ]; then
      echo "$action"
      return
    fi
  done
  echo "action_check_health"
}

# ===== USER LOOP =====

run_user() {
  local uid=$1
  local end_time=$2
  local log="$RESULTS_DIR/user-$uid.log"

  echo "[$(date +%H:%M:%S)] User $uid started on $(node_url $uid)" > "$log"

  # Initial setup: create wallet and cache peer ID
  action_create_wallet $uid
  local peer_id=$(api_call $uid GET "/api/headless/dht/peer-id" 2>/dev/null | python3 -c "import sys,json; print(json.load(sys.stdin).get('peerId',''))" 2>/dev/null || echo "")
  if [ -n "$peer_id" ]; then
    echo "$peer_id" >> "$PEER_IDS"
  fi

  # Random action loop
  while [ $(date +%s) -lt $end_time ]; do
    local action=$(pick_action)
    $action $uid 2>/dev/null || true

    # Random delay 0.5-3 seconds between actions
    local delay=$(python3 -c "import random; print(f'{random.uniform(0.5, 3.0):.1f}')")
    sleep "$delay"
  done

  echo "[$(date +%H:%M:%S)] User $uid finished" >> "$log"
}

# ===== PROGRESS MONITOR =====

monitor_progress() {
  local end_time=$1
  local start_time=$(date +%s)

  while [ $(date +%s) -lt $end_time ]; do
    sleep 10
    local elapsed=$(( $(date +%s) - start_time ))
    local remaining=$(( end_time - $(date +%s) ))
    local actions=$(get_count actions)
    local successes=$(get_count successes)
    local failures=$(get_count failures)
    local timeouts=$(get_count timeouts)
    local errors=$(get_count errors)
    local rate=$((actions / (elapsed > 0 ? elapsed : 1)))

    printf "\r${CYAN}[%02d:%02d]${NC} actions=${GREEN}%d${NC} ok=${GREEN}%d${NC} fail=${RED}%d${NC} timeout=${YELLOW}%d${NC} err=${RED}%d${NC} rate=${DIM}%d/s${NC} remaining=${DIM}%ds${NC}   " \
      $((elapsed / 60)) $((elapsed % 60)) \
      "$actions" "$successes" "$failures" "$timeouts" "$errors" "$rate" "$remaining"
  done
  echo ""
}

# ===== MAIN =====

main() {
  echo ""
  echo -e "${CYAN}================================================================${NC}"
  echo -e "${CYAN}  Chiral Network — User Simulation Test${NC}"
  echo -e "${CYAN}  $USER_COUNT users | $DURATION_MINS minutes | target: $TARGET${NC}"
  echo -e "${CYAN}================================================================${NC}"
  echo ""
  echo -e "Results: ${DIM}$RESULTS_DIR${NC}"
  echo ""

  # Verify at least some nodes are reachable
  local reachable=0
  for i in 1 5 10 15 20; do
    if curl -sf --max-time 2 "$(node_url $i)/api/health" >/dev/null 2>&1; then
      reachable=$((reachable + 1))
    fi
  done

  if [ $reachable -eq 0 ]; then
    echo -e "${RED}No nodes reachable at $BASE_HOST:$BASE_PORT+. Start nodes first.${NC}"
    exit 1
  fi
  echo -e "${GREEN}$reachable/5 sampled nodes reachable${NC}"
  echo ""

  local end_time=$(( $(date +%s) + DURATION_SECS ))

  # Start progress monitor
  monitor_progress $end_time &
  local monitor_pid=$!

  # Launch all users in parallel
  local pids=()
  for uid in $(seq 1 $USER_COUNT); do
    run_user $uid $end_time &
    pids+=($!)
  done

  # Wait for all users to finish
  for pid in "${pids[@]}"; do
    wait $pid 2>/dev/null || true
  done

  # Stop monitor
  kill $monitor_pid 2>/dev/null || true
  wait $monitor_pid 2>/dev/null || true

  # ===== REPORT =====
  echo ""
  echo -e "${CYAN}================================================================${NC}"
  echo -e "${CYAN}  SIMULATION RESULTS${NC}"
  echo -e "${CYAN}================================================================${NC}"
  echo ""

  local total_actions=$(get_count actions)
  local total_successes=$(get_count successes)
  local total_failures=$(get_count failures)
  local total_timeouts=$(get_count timeouts)
  local total_errors=$(get_count errors)
  local success_rate=0
  if [ $total_actions -gt 0 ]; then
    success_rate=$(( total_successes * 100 / total_actions ))
  fi

  echo -e "  Duration:     ${DURATION_MINS} minutes"
  echo -e "  Users:        ${USER_COUNT}"
  echo -e "  Total actions:${GREEN} $total_actions${NC}"
  echo -e "  Successes:    ${GREEN} $total_successes${NC} (${success_rate}%)"
  echo -e "  Failures:     ${RED} $total_failures${NC}"
  echo -e "  Timeouts:     ${YELLOW} $total_timeouts${NC} (>5s)"
  echo -e "  Errors:       ${RED} $total_errors${NC}"
  echo -e "  Throughput:    $(( total_actions / DURATION_SECS )) actions/sec"
  echo ""

  # Top errors
  if [ $total_errors -gt 0 ] || [ $total_failures -gt 0 ] || [ $total_timeouts -gt 0 ]; then
    echo -e "${YELLOW}Issues found:${NC}"
    grep -h "ERROR\|TIMEOUT\|SLOW\|MISMATCH\|WRONG\|FAIL" "$RESULTS_DIR"/user-*.log 2>/dev/null | sort | uniq -c | sort -rn | head -10
    echo ""
  fi

  # Per-action breakdown
  echo -e "${CYAN}Action breakdown:${NC}"
  for action_name in WALLET BALANCE SEND UPLOAD SEARCH REGISTER DHT_KV PING ECHO PEERS HEALTH HOST_AD CDN_LIST CHAIN_ID; do
    local count=$(grep -h "$action_name" "$RESULTS_DIR"/user-*.log 2>/dev/null | wc -l)
    if [ $count -gt 0 ]; then
      printf "  %-14s %d\n" "$action_name" "$count"
    fi
  done
  echo ""

  echo -e "Logs: ${DIM}$RESULTS_DIR/user-*.log${NC}"
  echo ""

  if [ $success_rate -ge 90 ]; then
    echo -e "${GREEN}SIMULATION PASSED (${success_rate}% success rate)${NC}"
  elif [ $success_rate -ge 70 ]; then
    echo -e "${YELLOW}SIMULATION WARNING (${success_rate}% success rate)${NC}"
  else
    echo -e "${RED}SIMULATION FAILED (${success_rate}% success rate)${NC}"
  fi

  exit $(( total_failures + total_errors > 0 ? 1 : 0 ))
}

main
