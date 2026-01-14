#!/bin/bash

# Generate post-semester contribution data (after December 2025)
# This script analyzes git commits after the semester end date

set -e

SEMESTER_END_DATE="2025-12-13"
OUTPUT_FILE="contribution-data-post-semester.json"

echo "Generating post-semester contribution data..."
echo "Analyzing commits after $SEMESTER_END_DATE"

# Get the date range
FIRST_COMMIT_DATE=$(git log --since="$SEMESTER_END_DATE" --reverse --format="%cs" | head -1)
LAST_COMMIT_DATE=$(git log --since="$SEMESTER_END_DATE" --format="%cs" | head -1)

if [ -z "$FIRST_COMMIT_DATE" ]; then
    echo "No commits found after $SEMESTER_END_DATE"
    cat > "$OUTPUT_FILE" << EOF
{
  "summary": {
    "totalCommits": 0,
    "totalContributors": 0,
    "totalPRs": 0,
    "dateRange": {
      "start": "N/A",
      "end": "N/A"
    }
  },
  "contributors": [],
  "generatedAt": "$(date -Iseconds)"
}
EOF
    exit 0
fi

# Count total commits
TOTAL_COMMITS=$(git log --oneline --since="$SEMESTER_END_DATE" | wc -l)

# Count merge commits (PRs)
TOTAL_PRS=$(git log --oneline --since="$SEMESTER_END_DATE" --merges | wc -l)

# Get contributor stats
echo "Collecting contributor statistics..."

# Create temporary file for contributors
TEMP_FILE=$(mktemp)

# Get unique authors with their commit counts
git log --since="$SEMESTER_END_DATE" --format="%aN" | sort | uniq -c | sort -rn > "$TEMP_FILE"

# Count unique contributors
TOTAL_CONTRIBUTORS=$(wc -l < "$TEMP_FILE" | tr -d ' ')

# Function to generate summary from commit messages
generate_summary() {
    local author="$1"
    local commits=$(git log --since="$SEMESTER_END_DATE" --author="$author" --format="%s" 2>/dev/null)

    # Analyze commit types
    local features=$(echo "$commits" | grep -c -i "^feat" || true)
    local fixes=$(echo "$commits" | grep -c -i "^fix" || true)
    local tests=$(echo "$commits" | grep -c -i "^test" || true)
    local docs=$(echo "$commits" | grep -c -i "^doc" || true)
    local chores=$(echo "$commits" | grep -c -i "^chore" || true)
    local merges=$(echo "$commits" | grep -c -i "^Merge" || true)

    # Extract key topics from commits
    local topics=""

    if echo "$commits" | grep -qi "i18n\|translation\|locale"; then
        topics="${topics}internationalization, "
    fi
    if echo "$commits" | grep -qi "ftp"; then
        topics="${topics}FTP support, "
    fi
    if echo "$commits" | grep -qi "webrtc\|p2p"; then
        topics="${topics}WebRTC/P2P transfers, "
    fi
    if echo "$commits" | grep -qi "dht"; then
        topics="${topics}DHT networking, "
    fi
    if echo "$commits" | grep -qi "ed2k"; then
        topics="${topics}ED2K protocol, "
    fi
    if echo "$commits" | grep -qi "e2e\|test"; then
        topics="${topics}testing, "
    fi
    if echo "$commits" | grep -qi "ui\|page\|component"; then
        topics="${topics}UI improvements, "
    fi
    if echo "$commits" | grep -qi "wallet\|account\|payment"; then
        topics="${topics}wallet/payments, "
    fi
    if echo "$commits" | grep -qi "security\|DoS"; then
        topics="${topics}security, "
    fi
    if echo "$commits" | grep -qi "mining\|geth\|blockchain"; then
        topics="${topics}blockchain/mining, "
    fi
    if echo "$commits" | grep -qi "analytics\|suspicious"; then
        topics="${topics}analytics, "
    fi
    if echo "$commits" | grep -qi "network\|protocol"; then
        topics="${topics}networking, "
    fi

    # Remove trailing comma and space
    topics=$(echo "$topics" | sed 's/, $//')

    # Build summary
    local summary=""
    if [ "$merges" -gt 0 ] && [ "$merges" -eq "$(echo "$commits" | wc -l)" ]; then
        summary="PR merges and branch management"
    else
        if [ -n "$topics" ]; then
            summary="$topics"
        else
            summary="general improvements"
        fi

        # Add contribution breakdown
        local breakdown=""
        [ "$features" -gt 0 ] && breakdown="${breakdown}${features} features, "
        [ "$fixes" -gt 0 ] && breakdown="${breakdown}${fixes} fixes, "
        [ "$tests" -gt 0 ] && breakdown="${breakdown}${tests} tests, "

        if [ -n "$breakdown" ]; then
            breakdown=$(echo "$breakdown" | sed 's/, $//')
            summary="$summary ($breakdown)"
        fi
    fi

    echo "$summary"
}

# Build JSON
echo "Building JSON output..."

cat > "$OUTPUT_FILE" << EOF
{
  "summary": {
    "totalCommits": $TOTAL_COMMITS,
    "totalContributors": $TOTAL_CONTRIBUTORS,
    "totalPRs": $TOTAL_PRS,
    "dateRange": {
      "start": "$FIRST_COMMIT_DATE",
      "end": "$LAST_COMMIT_DATE"
    }
  },
  "contributors": [
EOF

# Add contributors
first=true
while read -r line; do
    # Parse count and name from "  123 Author Name" format
    commits=$(echo "$line" | awk '{print $1}')
    name=$(echo "$line" | sed 's/^[[:space:]]*[0-9]*[[:space:]]*//')

    if [ "$first" = true ]; then
        first=false
    else
        echo "," >> "$OUTPUT_FILE"
    fi

    # Extract team from name pattern (team-firstname-lastname)
    team=""
    if [[ "$name" =~ ^([a-z]+)- ]]; then
        team="${BASH_REMATCH[1]}"
    fi

    # Get additions for this author
    additions=$(git log --since="$SEMESTER_END_DATE" --author="$name" --pretty=tformat: --numstat 2>/dev/null | awk '{sum += $1} END {print sum+0}')

    # Get last commit date
    last_date=$(git log --since="$SEMESTER_END_DATE" --author="$name" --format="%cs" -1)

    # Generate summary
    echo "  Analyzing: $name..."
    summary=$(generate_summary "$name")

    # Escape special characters in name and summary
    escaped_name=$(echo "$name" | sed 's/"/\\"/g')
    escaped_summary=$(echo "$summary" | sed 's/"/\\"/g')

    cat >> "$OUTPUT_FILE" << EOF
    {
      "name": "$escaped_name",
      "team": "$team",
      "commits": $commits,
      "additions": ${additions:-0},
      "lastCommitDate": "$last_date",
      "summary": "$escaped_summary"
    }
EOF
done < "$TEMP_FILE"

cat >> "$OUTPUT_FILE" << EOF

  ],
  "generatedAt": "$(date -Iseconds)"
}
EOF

rm -f "$TEMP_FILE"

echo ""
echo "Generated $OUTPUT_FILE"
echo "  Total commits: $TOTAL_COMMITS"
echo "  Contributors: $TOTAL_CONTRIBUTORS"
echo "  PRs merged: $TOTAL_PRS"
echo "  Date range: $FIRST_COMMIT_DATE to $LAST_COMMIT_DATE"
