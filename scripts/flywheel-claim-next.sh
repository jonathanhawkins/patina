#!/usr/bin/env bash
# Deterministically claim the next available bead using a fresh br view.
set -euo pipefail

AGENT="${AGENT_NAME:-}"
if [ -z "$AGENT" ]; then
    echo "AGENT_NAME is required" >&2
    exit 2
fi

READY_JSON="$(br ready --json --unassigned --limit 20 2>/dev/null || echo '[]')"

mapfile -t CANDIDATES < <(
    printf '%s' "$READY_JSON" | python3 -c '
import json, sys

try:
    data = json.load(sys.stdin)
except Exception:
    data = []

if isinstance(data, dict):
    data = data.get("issues", [])

items = []
for issue in data:
    if not isinstance(issue, dict):
        continue
    bead_id = issue.get("id", "")
    title = issue.get("title", "")
    try:
        priority = int(issue.get("priority", 999))
    except Exception:
        priority = 999
    created_at = issue.get("created_at", "")
    if bead_id:
        items.append((priority, created_at, bead_id, title))

for _, _, bead_id, title in sorted(items):
    print(f"{bead_id}\t{title}")
'
)

if [ "${#CANDIDATES[@]}" -eq 0 ]; then
    echo "No ready unassigned beads."
    exit 1
fi

for candidate in "${CANDIDATES[@]}"; do
    BEAD_ID="${candidate%%$'\t'*}"
    TITLE="${candidate#*$'\t'}"
    if [ -z "$BEAD_ID" ]; then
        continue
    fi

    if br update "$BEAD_ID" --assignee "$AGENT" --status in_progress >/dev/null 2>&1; then
        echo "CLAIMED: $BEAD_ID"
        echo "TITLE: $TITLE"
        exit 0
    fi
done

echo "No claimable beads remained after refresh."
exit 1
