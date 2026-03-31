#!/usr/bin/env bash
set -euo pipefail
source /tests/lib.sh

###############################################################################
# Report Generator
#
# Reads /results/results.jsonl, counts pass/fail/skip per phase, prints a
# formatted table, writes summary JSON, and lists all failures.
###############################################################################

RESULTS_DIR="/results"
RESULTS_FILE="$RESULTS_DIR/results.jsonl"
REPORT_FILE="$RESULTS_DIR/report.json"

if [[ ! -f "$RESULTS_FILE" ]] || [[ ! -s "$RESULTS_FILE" ]]; then
    log_warn "No results file found at $RESULTS_FILE"
    echo '{"phases":[],"totals":{"pass":0,"fail":0,"skip":0},"failures":[]}' | jq . > "$REPORT_FILE"
    echo "No test results to report."
    exit 0
fi

# --- Count results per phase ---
total_pass=0
total_fail=0
total_skip=0

declare -A phase_status
declare -A phase_message
declare -A phase_test
declare -a phase_order=()
declare -a failures=()

while IFS= read -r line; do
    [[ -z "$line" ]] && continue

    phase=$(echo "$line" | jq -r '.phase // "unknown"' 2>/dev/null) || continue
    status=$(echo "$line" | jq -r '.status // "unknown"' 2>/dev/null)
    test_name=$(echo "$line" | jq -r '.test // ""' 2>/dev/null)
    error_msg=$(echo "$line" | jq -r '.error // ""' 2>/dev/null)

    # Track phase order (first occurrence)
    if [[ -z "${phase_status[$phase]+x}" ]]; then
        phase_order+=("$phase")
    fi

    phase_status[$phase]="$status"
    phase_test[$phase]="$test_name"
    phase_message[$phase]="$error_msg"

    case "$status" in
        pass)
            total_pass=$((total_pass + 1))
            ;;
        fail)
            total_fail=$((total_fail + 1))
            failures+=("$phase/$test_name: $error_msg")
            ;;
        skip)
            total_skip=$((total_skip + 1))
            ;;
        *)
            log_warn "Unknown status '$status' for phase $phase"
            ;;
    esac
done < "$RESULTS_FILE"

total=$((total_pass + total_fail + total_skip))

# --- Print formatted table ---
echo ""
echo "======================================================================"
echo "                    SCALED TEST RESULTS REPORT"
echo "======================================================================"
echo ""
printf "  %-35s %-8s %s\n" "PHASE" "STATUS" "DETAILS"
printf "  %-35s %-8s %s\n" "-----------------------------------" "--------" "--------------------------------------------"

for phase in "${phase_order[@]}"; do
    status="${phase_status[$phase]}"
    test_name="${phase_test[$phase]}"
    message="${phase_message[$phase]}"

    # Use test name as detail if no error, otherwise show error
    if [[ -n "$message" ]]; then
        display_msg="$message"
    else
        display_msg="$test_name"
    fi

    # Truncate long messages for table display
    if [[ ${#display_msg} -gt 50 ]]; then
        display_msg="${display_msg:0:47}..."
    fi

    # Status indicator
    case "$status" in
        pass) indicator="PASS" ;;
        fail) indicator="FAIL" ;;
        skip) indicator="SKIP" ;;
        *)    indicator="????" ;;
    esac

    printf "  %-35s %-8s %s\n" "$phase" "$indicator" "$display_msg"
done

echo ""
echo "----------------------------------------------------------------------"
printf "  %-35s %s\n" "TOTALS" "Pass: $total_pass  Fail: $total_fail  Skip: $total_skip  Total: $total"
echo "======================================================================"
echo ""

# --- List failures ---
if [[ ${#failures[@]} -gt 0 ]]; then
    echo "FAILURES:"
    echo ""
    for failure in "${failures[@]}"; do
        echo "  - $failure"
    done
    echo ""
fi

# --- Write summary JSON ---
# Build phases array
phases_json="["
first=true
for phase in "${phase_order[@]}"; do
    if [[ "$first" != "true" ]]; then
        phases_json+=","
    fi
    first=false

    status="${phase_status[$phase]}"
    test_name="${phase_test[$phase]}"
    message="${phase_message[$phase]}"

    # Escape message for JSON
    escaped_message=$(echo "$message" | jq -Rs '.' | sed 's/^"//;s/"$//')
    escaped_test=$(echo "$test_name" | jq -Rs '.' | sed 's/^"//;s/"$//')

    phases_json+="{\"phase\":\"$phase\",\"test\":\"$escaped_test\",\"status\":\"$status\",\"message\":\"$escaped_message\"}"
done
phases_json+="]"

# Build failures array
failures_json="["
first=true
for failure in "${failures[@]}"; do
    if [[ "$first" != "true" ]]; then
        failures_json+=","
    fi
    first=false
    escaped=$(echo "$failure" | jq -Rs '.' | sed 's/^"//;s/"$//')
    failures_json+="{\"detail\":\"$escaped\"}"
done
failures_json+="]"

# Compose full report
report_json=$(jq -n \
    --argjson phases "$phases_json" \
    --argjson failures "$failures_json" \
    --arg timestamp "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    '{
        timestamp: $timestamp,
        totals: {
            pass: '"$total_pass"',
            fail: '"$total_fail"',
            skip: '"$total_skip"',
            total: '"$total"'
        },
        phases: $phases,
        failures: $failures
    }')

echo "$report_json" > "$REPORT_FILE"
log_info "Report written to $REPORT_FILE"

if [[ "$total_fail" -gt 0 ]]; then
    echo "RESULT: $total_fail phase(s) FAILED"
else
    echo "RESULT: All phases passed or skipped"
fi
