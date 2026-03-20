#!/usr/bin/env bash
# extract_probes.sh — Run Godot lab probes and extract JSON output.
#
# Usage:
#   ./extract_probes.sh [godot-binary] [output-dir]
#
# Defaults:
#   godot-binary: godot (from PATH)
#   output-dir:   ./probe_output/
#
# The script:
#   1. Builds the GDExtension in release mode
#   2. Runs Godot headless to capture PATINA_PROBE lines
#   3. Splits output into per-capture-type JSON files
#   4. Validates JSON with jq (if available)

set -euo pipefail

GODOT="${1:-godot}"
OUTPUT_DIR="${2:-./probe_output}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

echo "=== Patina Godot Lab — Probe Extraction ==="
echo "Godot binary: $GODOT"
echo "Output dir:   $OUTPUT_DIR"
echo ""

# Step 1: Build the extension
echo "[1/4] Building GDExtension..."
cd "$SCRIPT_DIR"
cargo build --release 2>&1

# Step 2: Run Godot headless
echo "[2/4] Running Godot headless..."
mkdir -p "$OUTPUT_DIR"

RAW_OUTPUT="$OUTPUT_DIR/raw_output.txt"
"$GODOT" --headless --path "$SCRIPT_DIR" 2>&1 | grep "PATINA_PROBE:" | sed 's/^.*PATINA_PROBE://' > "$RAW_OUTPUT" || true

LINE_COUNT=$(wc -l < "$RAW_OUTPUT" | tr -d ' ')
echo "  Captured $LINE_COUNT probe lines"

if [ "$LINE_COUNT" -eq 0 ]; then
    echo "ERROR: No probe output captured. Is the Godot binary correct?"
    exit 1
fi

# Step 3: Split into per-type files
echo "[3/4] Splitting output by capture_type..."

for CAPTURE_TYPE in scene_tree properties signals classdb resource_metadata; do
    OUT_FILE="$OUTPUT_DIR/${CAPTURE_TYPE}.jsonl"
    grep "\"capture_type\":\"$CAPTURE_TYPE\"" "$RAW_OUTPUT" > "$OUT_FILE" 2>/dev/null || true
    COUNT=$(wc -l < "$OUT_FILE" | tr -d ' ')
    echo "  $CAPTURE_TYPE: $COUNT entries -> $OUT_FILE"
done

# Step 4: Validate JSON (if jq is available)
echo "[4/4] Validating JSON..."
if command -v jq &>/dev/null; then
    ERRORS=0
    while IFS= read -r line; do
        if ! echo "$line" | jq . >/dev/null 2>&1; then
            echo "  INVALID JSON: $line"
            ERRORS=$((ERRORS + 1))
        fi
    done < "$RAW_OUTPUT"

    if [ "$ERRORS" -eq 0 ]; then
        echo "  All $LINE_COUNT lines are valid JSON"
    else
        echo "  WARNING: $ERRORS invalid JSON lines"
    fi
else
    echo "  (jq not found — skipping validation)"
fi

echo ""
echo "=== Done. Output in $OUTPUT_DIR/ ==="
