#!/usr/bin/env bash
set -euo pipefail
source /tests/lib.sh

PHASE="phase-07-chiraldrop"
log_info "[$PHASE] Starting ChiralDrop P2P transfer phase"

###############################################################################
# Phase 07 — ChiralDrop P2P Transfer
#
# Picks random sender/receiver pairs from all nodes, sends a file via
# ChiralDrop, and verifies the receiver gets it in their inbox.
###############################################################################

if [[ -z "${ALL_NODES:-}" ]]; then
    log_warn "[$PHASE] ALL_NODES is empty — skipping"
    record_result "$PHASE" "chiraldrop" "skip" "0" "No nodes available"
    exit 0
fi

IFS=',' read -ra NODES <<< "$ALL_NODES"
NODE_COUNT=${#NODES[@]}

if [[ "$NODE_COUNT" -lt 2 ]]; then
    log_warn "[$PHASE] Need at least 2 nodes for ChiralDrop — skipping"
    record_result "$PHASE" "chiraldrop" "skip" "0" "Fewer than 2 nodes available"
    exit 0
fi

# Build an array of (url, peerId) tuples
declare -a NODE_PEERS=()
for url in "${NODES[@]}"; do
    peer_resp=$(curl -sf --max-time 10 \
        "${url}/api/headless/dht/peer-id" 2>/dev/null) || continue
    peer_id=$(echo "$peer_resp" | jq -r '.peerId // empty')
    if [[ -n "$peer_id" ]]; then
        NODE_PEERS+=("${url}|${peer_id}")
    fi
done

PEER_COUNT=${#NODE_PEERS[@]}
if [[ "$PEER_COUNT" -lt 2 ]]; then
    log_warn "[$PHASE] Fewer than 2 nodes have peer IDs — skipping"
    record_result "$PHASE" "chiraldrop" "skip" "0" "Fewer than 2 nodes with active DHT"
    exit 0
fi

# Determine number of transfer pairs (up to half the nodes, max 10)
max_pairs=$(( PEER_COUNT / 2 ))
if [[ "$max_pairs" -gt 10 ]]; then
    max_pairs=10
fi

transfers_attempted=0
transfers_completed=0
transfers_failed=0

# Shuffle indices and pair them
indices=()
for i in $(seq 0 $(( PEER_COUNT - 1 ))); do
    indices+=("$i")
done
# Simple Fisher-Yates shuffle
for (( i=${#indices[@]}-1; i>0; i-- )); do
    j=$(( RANDOM % (i + 1) ))
    tmp="${indices[$i]}"
    indices[$i]="${indices[$j]}"
    indices[$j]="$tmp"
done

start_timer

pair_idx=0
while [[ "$pair_idx" -lt $(( max_pairs * 2 - 1 )) ]]; do
    sender_entry="${NODE_PEERS[${indices[$pair_idx]}]}"
    receiver_entry="${NODE_PEERS[${indices[$((pair_idx + 1))]}]}"

    IFS='|' read -r sender_url sender_peer_id <<< "$sender_entry"
    IFS='|' read -r receiver_url receiver_peer_id <<< "$receiver_entry"

    transfer_id="drop-${RANDOM}-$(date +%s)"
    test_file_name="chiraldrop-test-${transfer_id}.txt"
    test_content="ChiralDrop test payload $(date -u +%s) $transfer_id"

    log_info "[$PHASE] Transfer $transfer_id: $sender_url -> $receiver_url"
    transfers_attempted=$((transfers_attempted + 1))

    # The send-file endpoint reads from a file path on the sender's filesystem.
    # In Docker, each container has its own filesystem. The test file must exist
    # on the sender container. We use /tmp as the writable directory.
    test_file_path="/tmp/${test_file_name}"

    send_resp=$(curl -sf --max-time 15 \
        -X POST "${sender_url}/api/headless/dht/send-file" \
        -H "Content-Type: application/json" \
        -d "{
            \"peerId\": \"$receiver_peer_id\",
            \"transferId\": \"$transfer_id\",
            \"fileName\": \"$test_file_name\",
            \"filePath\": \"$test_file_path\",
            \"priceWei\": \"0\",
            \"senderWallet\": \"\",
            \"fileHash\": \"\",
            \"fileSize\": ${#test_content}
        }" 2>/dev/null) || send_resp=""

    if [[ -z "$send_resp" ]]; then
        log_warn "[$PHASE] Transfer $transfer_id: send-file request failed (no response)"
        transfers_failed=$((transfers_failed + 1))
        pair_idx=$((pair_idx + 2))
        continue
    fi

    if echo "$send_resp" | jq -e '.error' >/dev/null 2>&1; then
        err_msg=$(echo "$send_resp" | jq -r '.error')
        log_warn "[$PHASE] Transfer $transfer_id: send-file error: $err_msg"
        transfers_failed=$((transfers_failed + 1))
        pair_idx=$((pair_idx + 2))
        continue
    fi

    log_info "[$PHASE] Transfer $transfer_id: send initiated, checking receiver inbox..."

    # Wait for transfer to appear in receiver's inbox (up to 15s)
    found_in_inbox=false
    for attempt in $(seq 1 5); do
        sleep 3
        inbox_resp=$(curl -sf --max-time 5 \
            "${receiver_url}/api/headless/drop/inbox" 2>/dev/null) || continue

        # Look for our transfer ID in the inbox
        match=$(echo "$inbox_resp" | jq -r ".[] | select(.transferId == \"$transfer_id\") | .transferId" 2>/dev/null) || match=""
        if [[ "$match" == "$transfer_id" ]]; then
            found_in_inbox=true
            break
        fi
    done

    if [[ "$found_in_inbox" != "true" ]]; then
        log_warn "[$PHASE] Transfer $transfer_id: not found in receiver inbox after 15s"
        transfers_failed=$((transfers_failed + 1))
        pair_idx=$((pair_idx + 2))
        continue
    fi

    # Accept the transfer
    accept_resp=$(curl -sf --max-time 15 \
        -X POST "${receiver_url}/api/headless/drop/accept" \
        -H "Content-Type: application/json" \
        -d "{\"transferId\": \"$transfer_id\"}" 2>/dev/null) || accept_resp=""

    if [[ -z "$accept_resp" ]]; then
        log_warn "[$PHASE] Transfer $transfer_id: accept request failed"
        transfers_failed=$((transfers_failed + 1))
        pair_idx=$((pair_idx + 2))
        continue
    fi

    if echo "$accept_resp" | jq -e '.error' >/dev/null 2>&1; then
        log_warn "[$PHASE] Transfer $transfer_id: accept error: $(echo "$accept_resp" | jq -r '.error')"
        transfers_failed=$((transfers_failed + 1))
        pair_idx=$((pair_idx + 2))
        continue
    fi

    log_info "[$PHASE] Transfer $transfer_id: accepted by receiver"
    transfers_completed=$((transfers_completed + 1))

    pair_idx=$((pair_idx + 2))
done

elapsed=$(stop_timer)

# --- Report ---
log_info "[$PHASE] === ChiralDrop Report ==="
log_info "[$PHASE]   Nodes with peer IDs:    $PEER_COUNT"
log_info "[$PHASE]   Transfers attempted:    $transfers_attempted"
log_info "[$PHASE]   Transfers completed:    $transfers_completed"
log_info "[$PHASE]   Transfers failed:       $transfers_failed"

if [[ "$transfers_attempted" -eq 0 ]]; then
    record_result "$PHASE" "chiraldrop" "skip" "$elapsed" "No transfer pairs attempted"
    exit 0
fi

if [[ "$transfers_completed" -gt 0 ]]; then
    record_result "$PHASE" "chiraldrop" "pass" "$elapsed" ""
    log_pass "[$PHASE] Completed $transfers_completed/$transfers_attempted transfers"
else
    record_result "$PHASE" "chiraldrop" "fail" "$elapsed" "All $transfers_attempted transfers failed"
    log_fail "[$PHASE] All $transfers_attempted transfers failed"
    exit 1
fi
