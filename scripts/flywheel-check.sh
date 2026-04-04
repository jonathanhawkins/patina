#!/usr/bin/env bash
# Flywheel worker state check — run this FIRST in every /flywheel-worker iteration.
# Prints a clear ACTION directive the worker must follow.
set -euo pipefail

AGENT="${AGENT_NAME:-unknown}"
LOG=".codex/orchestrator/verifier.log"

# Get in-progress beads assigned to this agent
BEADS=$(br list --status in_progress --assignee "$AGENT" --json --no-auto-import --allow-stale 2>/dev/null || echo '{"issues":[]}')
COUNT=$(echo "$BEADS" | python3 -c "import sys,json; d=json.load(sys.stdin); print(len(d.get('issues', d if isinstance(d, list) else [])))" 2>/dev/null || echo "0")

if [ "$COUNT" = "0" ]; then
    echo "ACTION: CLAIM_NEW_WORK"
    echo "You have no in-progress beads. Claim a new one."
    exit 0
fi

# Extract bead ID
BEAD_ID=$(echo "$BEADS" | python3 -c "
import sys, json
d = json.load(sys.stdin)
issues = d.get('issues', d if isinstance(d, list) else [])
if issues:
    print(issues[0]['id'])
" 2>/dev/null || echo "")

if [ -z "$BEAD_ID" ]; then
    echo "ACTION: CLAIM_NEW_WORK"
    echo "Could not parse bead ID. Claim new work."
    exit 0
fi

TITLE=$(echo "$BEADS" | python3 -c "
import sys, json
d = json.load(sys.stdin)
issues = d.get('issues', d if isinstance(d, list) else [])
if issues:
    print(issues[0].get('title', ''))
" 2>/dev/null || echo "")

echo "BEAD: $BEAD_ID ($TITLE)"

# Check verifier log for this bead
if [ -f "$LOG" ]; then
    LAST_ENTRY=$(grep "bead=$BEAD_ID" "$LOG" 2>/dev/null | tail -1 || true)
    if echo "$LAST_ENTRY" | grep -q "FAIL"; then
        REASON=$(echo "$LAST_ENTRY" | sed 's/.*reason=//')
        echo "ACTION: FIX_AND_RESUBMIT"
        echo "Verifier FAILED this bead. Reason: $REASON"
        echo "Read the failure, fix the code, then re-report with /mail-complete."
        exit 0
    elif echo "$LAST_ENTRY" | grep -q "PASS"; then
        echo "ACTION: WAIT_FOR_CLOSE"
        echo "Verifier PASSED. Bead should close shortly. Run:"
        echo "  sleep 30 && br show $BEAD_ID --no-auto-import --allow-stale 2>/dev/null | head -3"
        exit 0
    elif echo "$LAST_ENTRY" | grep -q "START"; then
        echo "ACTION: WAIT_FOR_VERIFIER"
        echo "Verifier is running. Wait with:"
        echo "  for i in \$(seq 1 20); do grep \"bead=$BEAD_ID\" $LOG 2>/dev/null | tail -1 | grep -qE 'PASS|FAIL' && break; sleep 30; done; grep \"bead=$BEAD_ID\" $LOG | tail -1"
        exit 0
    fi
fi

# No verifier entry — check if completion was sent by looking at coordinator inbox
echo "ACTION: IMPLEMENT_OR_REPORT"
echo "Bead is in-progress with no verifier activity."
echo "If you already implemented it, report completion with /mail-complete."
echo "If not, implement it (Step 3 in the skill)."
