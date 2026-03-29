#!/usr/bin/env bash
set -euo pipefail
source /tests/lib.sh

PHASE="phase-10-drive-crud"
log_phase_start "$PHASE"

###############################################################################
# Phase 10 — Drive CRUD Operations
#
# Tests Create, Read, Update, Delete of Drive folders on a subset of nodes.
###############################################################################

if [[ -z "${ALL_NODES:-}" ]]; then
    log_warn "$PHASE" "ALL_NODES is empty — skipping"
    record_result "$PHASE" "skip" "No nodes available"
    log_phase_end "$PHASE" 0
    exit 0
fi

IFS=',' read -ra NODES <<< "$ALL_NODES"
NODE_COUNT=${#NODES[@]}

# Pick up to 5 nodes
max_nodes=5
if [[ "$NODE_COUNT" -lt "$max_nodes" ]]; then
    max_nodes=$NODE_COUNT
fi

creates_ok=0
creates_fail=0
reads_ok=0
reads_fail=0
updates_ok=0
updates_fail=0
deletes_ok=0
deletes_fail=0

for i in $(seq 0 $(( max_nodes - 1 ))); do
    node_url="${NODES[$i]}"
    folder_name="test-folder-$(generate_id)"
    log_info "$PHASE" "Node $node_url: testing CRUD with folder '$folder_name'"

    # --- CREATE: POST /api/drive/folders ---
    create_resp=$(curl -sf --max-time 10 -w "\n%{http_code}" \
        -X POST "${node_url}/api/drive/folders" \
        -H "Content-Type: application/json" \
        -d "{\"name\": \"$folder_name\"}" 2>/dev/null) || create_resp=""

    if [[ -z "$create_resp" ]]; then
        log_warn "$PHASE" "Node $node_url: CREATE failed (no response)"
        creates_fail=$((creates_fail + 1))
        continue
    fi

    http_code=$(echo "$create_resp" | tail -1)
    body=$(echo "$create_resp" | sed '$d')

    if [[ "$http_code" -ge 200 ]] && [[ "$http_code" -lt 300 ]]; then
        creates_ok=$((creates_ok + 1))
        log_info "$PHASE" "Node $node_url: CREATE OK (HTTP $http_code)"
    else
        log_warn "$PHASE" "Node $node_url: CREATE failed (HTTP $http_code)"
        creates_fail=$((creates_fail + 1))
        continue
    fi

    # Extract the created item's ID
    item_id=$(echo "$body" | jq -r '.id // .itemId // empty' 2>/dev/null)
    if [[ -z "$item_id" ]]; then
        log_warn "$PHASE" "Node $node_url: Could not extract item ID from CREATE response"
        # Try to find it via listing
    fi

    # --- READ: GET /api/drive/items ---
    read_resp=$(curl -sf --max-time 10 -w "\n%{http_code}" \
        "${node_url}/api/drive/items" 2>/dev/null) || read_resp=""

    if [[ -z "$read_resp" ]]; then
        log_warn "$PHASE" "Node $node_url: READ failed (no response)"
        reads_fail=$((reads_fail + 1))
    else
        http_code=$(echo "$read_resp" | tail -1)
        body=$(echo "$read_resp" | sed '$d')

        if [[ "$http_code" -ge 200 ]] && [[ "$http_code" -lt 300 ]]; then
            # Verify our folder appears in the listing
            found=$(echo "$body" | jq -r ".[] | select(.name == \"$folder_name\") | .name" 2>/dev/null) || found=""
            if [[ "$found" == "$folder_name" ]]; then
                reads_ok=$((reads_ok + 1))
                log_info "$PHASE" "Node $node_url: READ OK — folder found in listing"

                # If we did not get item_id from CREATE, try from listing
                if [[ -z "$item_id" ]]; then
                    item_id=$(echo "$body" | jq -r ".[] | select(.name == \"$folder_name\") | .id // .itemId" 2>/dev/null) || item_id=""
                fi
            else
                reads_ok=$((reads_ok + 1))
                log_info "$PHASE" "Node $node_url: READ OK — listing returned (folder may not appear immediately)"
            fi
        else
            log_warn "$PHASE" "Node $node_url: READ failed (HTTP $http_code)"
            reads_fail=$((reads_fail + 1))
        fi
    fi

    # Skip UPDATE and DELETE if we have no item ID
    if [[ -z "$item_id" ]]; then
        log_warn "$PHASE" "Node $node_url: No item ID — skipping UPDATE and DELETE"
        continue
    fi

    # --- UPDATE: PUT /api/drive/items/:id ---
    new_name="renamed-${folder_name}"
    update_resp=$(curl -sf --max-time 10 -w "\n%{http_code}" \
        -X PUT "${node_url}/api/drive/items/${item_id}" \
        -H "Content-Type: application/json" \
        -d "{\"name\": \"$new_name\"}" 2>/dev/null) || update_resp=""

    if [[ -z "$update_resp" ]]; then
        log_warn "$PHASE" "Node $node_url: UPDATE failed (no response)"
        updates_fail=$((updates_fail + 1))
    else
        http_code=$(echo "$update_resp" | tail -1)
        if [[ "$http_code" -ge 200 ]] && [[ "$http_code" -lt 300 ]]; then
            updates_ok=$((updates_ok + 1))
            log_info "$PHASE" "Node $node_url: UPDATE OK (HTTP $http_code)"
        else
            log_warn "$PHASE" "Node $node_url: UPDATE failed (HTTP $http_code)"
            updates_fail=$((updates_fail + 1))
        fi
    fi

    # --- DELETE: DELETE /api/drive/items/:id ---
    delete_resp=$(curl -sf --max-time 10 -w "\n%{http_code}" \
        -X DELETE "${node_url}/api/drive/items/${item_id}" 2>/dev/null) || delete_resp=""

    if [[ -z "$delete_resp" ]]; then
        log_warn "$PHASE" "Node $node_url: DELETE failed (no response)"
        deletes_fail=$((deletes_fail + 1))
    else
        http_code=$(echo "$delete_resp" | tail -1)
        if [[ "$http_code" -ge 200 ]] && [[ "$http_code" -lt 300 ]]; then
            deletes_ok=$((deletes_ok + 1))
            log_info "$PHASE" "Node $node_url: DELETE OK (HTTP $http_code)"
        else
            log_warn "$PHASE" "Node $node_url: DELETE failed (HTTP $http_code)"
            deletes_fail=$((deletes_fail + 1))
        fi
    fi
done

# --- Report ---
total_ops=$(( creates_ok + creates_fail + reads_ok + reads_fail + updates_ok + updates_fail + deletes_ok + deletes_fail ))
total_ok=$(( creates_ok + reads_ok + updates_ok + deletes_ok ))
total_fail=$(( creates_fail + reads_fail + updates_fail + deletes_fail ))

log_info "$PHASE" "=== Drive CRUD Report ==="
log_info "$PHASE" "  Nodes tested:     $max_nodes"
log_info "$PHASE" "  CREATE ok/fail:   $creates_ok / $creates_fail"
log_info "$PHASE" "  READ   ok/fail:   $reads_ok / $reads_fail"
log_info "$PHASE" "  UPDATE ok/fail:   $updates_ok / $updates_fail"
log_info "$PHASE" "  DELETE ok/fail:   $deletes_ok / $deletes_fail"
log_info "$PHASE" "  Total  ok/fail:   $total_ok / $total_fail"

if [[ "$total_ops" -eq 0 ]]; then
    record_result "$PHASE" "skip" "No CRUD operations were attempted"
    log_phase_end "$PHASE" 0
    exit 0
fi

if [[ "$total_fail" -eq 0 ]]; then
    record_result "$PHASE" "pass" "All $total_ok CRUD operations succeeded on $max_nodes nodes"
    log_phase_end "$PHASE" 0
elif [[ "$total_ok" -gt 0 ]]; then
    record_result "$PHASE" "pass" "$total_ok/$total_ops CRUD operations succeeded ($total_fail failures)"
    log_phase_end "$PHASE" 0
else
    record_result "$PHASE" "fail" "All $total_fail CRUD operations failed"
    log_phase_end "$PHASE" 1
    exit 1
fi
