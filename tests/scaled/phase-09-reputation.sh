#!/usr/bin/env bash
set -euo pipefail
source /tests/lib.sh

PHASE="phase-09-reputation"
log_info "[$PHASE] Starting reputation check phase"

###############################################################################
# Phase 09 — Reputation System Check
#
# Collects wallet addresses and queries the relay server's batch reputation
# endpoint. Verifies Elo scores are within the valid 0-100 range.
###############################################################################

RELAY_URL="${RELAY_URL:-http://130.245.173.73:8080}"
WALLETS_FILE="/results/wallets.txt"

if [[ ! -f "$WALLETS_FILE" ]] || [[ ! -s "$WALLETS_FILE" ]]; then
    log_warn "[$PHASE] No wallets file found at $WALLETS_FILE — skipping"
    record_result "$PHASE" "reputation" "skip" "0" "No wallets file available"
    exit 0
fi

# Read wallet addresses (one per line)
mapfile -t WALLETS < "$WALLETS_FILE"
WALLET_COUNT=${#WALLETS[@]}

if [[ "$WALLET_COUNT" -eq 0 ]]; then
    log_warn "[$PHASE] Wallets file is empty — skipping"
    record_result "$PHASE" "reputation" "skip" "0" "No wallet addresses found"
    exit 0
fi

log_info "[$PHASE] Loaded $WALLET_COUNT wallet addresses"

start_timer

# Build JSON array of wallet addresses for batch query
wallet_json=$(printf '%s\n' "${WALLETS[@]}" | jq -R . | jq -s '{ "wallets": . }')

log_info "[$PHASE] Querying relay reputation batch endpoint..."

batch_resp=$(curl -sf --max-time 30 \
    -X POST "${RELAY_URL}/api/ratings/batch" \
    -H "Content-Type: application/json" \
    -d "$wallet_json" 2>/dev/null) || batch_resp=""

if [[ -z "$batch_resp" ]]; then
    elapsed=$(stop_timer)
    log_fail "[$PHASE] Batch reputation request failed (no response from relay)"
    record_result "$PHASE" "reputation-batch" "fail" "$elapsed" "Relay server did not respond to batch reputation query"
    exit 1
fi

if echo "$batch_resp" | jq -e '.error' >/dev/null 2>&1; then
    elapsed=$(stop_timer)
    err_msg=$(echo "$batch_resp" | jq -r '.error')
    log_fail "[$PHASE] Batch reputation error: $err_msg"
    record_result "$PHASE" "reputation-batch" "fail" "$elapsed" "Relay returned error: $err_msg"
    exit 1
fi

# Parse response — expect an object or array with wallet entries
wallets_with_data=0
wallets_without_data=0
elo_valid=0
elo_invalid=0

# The batch endpoint may return a map of wallet -> {elo, events} or an array
for wallet in "${WALLETS[@]}"; do
    # Try to extract elo for this wallet (handle both object and array response shapes)
    elo=$(echo "$batch_resp" | jq -r "
        if type == \"object\" then
            .[\"$wallet\"].elo // .[\"$wallet\"].score // \"missing\"
        elif type == \"array\" then
            (.[] | select(.wallet == \"$wallet\" or .address == \"$wallet\") | .elo // .score) // \"missing\"
        else
            \"missing\"
        end
    " 2>/dev/null) || elo="missing"

    if [[ "$elo" == "missing" ]] || [[ "$elo" == "null" ]] || [[ -z "$elo" ]]; then
        wallets_without_data=$((wallets_without_data + 1))
        continue
    fi

    wallets_with_data=$((wallets_with_data + 1))

    # Validate Elo range (0-100) using awk for floating-point comparison
    in_range=$(awk -v elo="$elo" 'BEGIN { print (elo >= 0 && elo <= 100) ? "yes" : "no" }')
    if [[ "$in_range" == "yes" ]]; then
        elo_valid=$((elo_valid + 1))
    else
        log_warn "[$PHASE] Wallet $wallet: Elo $elo is outside 0-100 range"
        elo_invalid=$((elo_invalid + 1))
    fi
done

# Also query individual wallets for additional verification (sample up to 3)
sample_count=3
if [[ "$WALLET_COUNT" -lt "$sample_count" ]]; then
    sample_count=$WALLET_COUNT
fi

individual_ok=0
individual_fail=0

for i in $(seq 0 $(( sample_count - 1 ))); do
    wallet="${WALLETS[$i]}"
    log_info "[$PHASE] Querying individual reputation for $wallet"

    indiv_resp=$(curl -sf --max-time 10 \
        "${RELAY_URL}/api/ratings/${wallet}" 2>/dev/null) || indiv_resp=""

    if [[ -n "$indiv_resp" ]] && ! echo "$indiv_resp" | jq -e '.error' >/dev/null 2>&1; then
        individual_ok=$((individual_ok + 1))
    else
        individual_fail=$((individual_fail + 1))
    fi
done

elapsed=$(stop_timer)

# --- Report ---
log_info "[$PHASE] === Reputation Check Report ==="
log_info "[$PHASE]   Total wallets:           $WALLET_COUNT"
log_info "[$PHASE]   Wallets with rep data:   $wallets_with_data"
log_info "[$PHASE]   Wallets without data:    $wallets_without_data"
log_info "[$PHASE]   Elo scores in range:     $elo_valid"
log_info "[$PHASE]   Elo scores out of range: $elo_invalid"
log_info "[$PHASE]   Individual queries OK:   $individual_ok"
log_info "[$PHASE]   Individual queries fail: $individual_fail"

if [[ "$elo_invalid" -gt 0 ]]; then
    record_result "$PHASE" "reputation" "fail" "$elapsed" "$elo_invalid wallets have Elo scores outside 0-100 range"
    log_fail "[$PHASE] $elo_invalid wallets have Elo scores outside 0-100 range"
    exit 1
fi

# Having no reputation data is acceptable for fresh test nodes
record_result "$PHASE" "reputation" "pass" "$elapsed" ""
log_pass "[$PHASE] Checked $WALLET_COUNT wallets; $wallets_with_data have reputation data; all Elo scores within 0-100"
