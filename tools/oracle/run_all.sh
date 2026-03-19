#!/bin/bash
# Oracle capture: Run all oracle tools on every .tscn in a Godot project.
# Usage: ./run_all.sh <project_dir> <output_dir>
# Example: ./run_all.sh ../../fixtures/sample_project ../../fixtures/oracle_outputs
set -euo pipefail

GODOT="/Users/bone/Downloads/Godot.app/Contents/MacOS/Godot"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

if [ $# -lt 2 ]; then
    echo "Usage: $0 <project_dir> <output_dir>"
    echo "  project_dir: Path to a Godot project (must contain project.godot)"
    echo "  output_dir:  Where to save oracle JSON outputs"
    exit 1
fi

PROJECT_DIR="$(cd "$1" && pwd)"
OUTPUT_DIR="$(mkdir -p "$2" && cd "$2" && pwd)"

if [ ! -f "$PROJECT_DIR/project.godot" ]; then
    echo "ERROR: No project.godot found in $PROJECT_DIR"
    exit 1
fi

if [ ! -x "$GODOT" ] && [ ! -f "$GODOT" ]; then
    echo "ERROR: Godot not found at $GODOT"
    echo "Set GODOT env var or edit this script."
    exit 1
fi

mkdir -p "$OUTPUT_DIR"

# Copy oracle scripts into the project temporarily.
ORACLE_SCRIPTS=(
    "scene_tree_dumper.gd"
    "property_dumper.gd"
    "signal_tracer.gd"
    "notification_tracer.gd"
    "run_fixture.gd"
)

cleanup() {
    for script in "${ORACLE_SCRIPTS[@]}"; do
        rm -f "$PROJECT_DIR/$script"
    done
}
trap cleanup EXIT

for script in "${ORACLE_SCRIPTS[@]}"; do
    cp "$SCRIPT_DIR/$script" "$PROJECT_DIR/$script"
done

# Find all .tscn files in the project.
SCENES=$(find "$PROJECT_DIR" -name "*.tscn" -not -path "*/.godot/*" | sort)

if [ -z "$SCENES" ]; then
    echo "No .tscn files found in $PROJECT_DIR"
    exit 1
fi

echo "=== Oracle Capture ==="
echo "Project: $PROJECT_DIR"
echo "Output:  $OUTPUT_DIR"
echo ""

PASS=0
FAIL=0

for scene_path in $SCENES; do
    scene_name=$(basename "$scene_path" .tscn)
    scene_rel="${scene_path#$PROJECT_DIR/}"
    scene_res="res://$scene_rel"
    echo "--- Scene: $scene_rel ---"

    # Run the combined fixture capture.
    output_file="$OUTPUT_DIR/${scene_name}.json"
    echo "  Running run_fixture.gd..."
    if "$GODOT" --headless --path "$PROJECT_DIR" \
        -s "res://run_fixture.gd" \
        -- --output "$output_file" --fixture-id "$scene_name" --scene "$scene_res" --frames 10 \
        2>&1 | sed 's/^/  /'; then
        if [ -f "$output_file" ]; then
            echo "  -> $output_file ($(wc -c < "$output_file" | tr -d ' ') bytes)"
            PASS=$((PASS + 1))
        else
            echo "  -> FAILED: output file not created"
            FAIL=$((FAIL + 1))
        fi
    else
        echo "  -> FAILED: Godot exited with error"
        FAIL=$((FAIL + 1))
    fi

    # Run individual scene tree dump.
    tree_file="$OUTPUT_DIR/${scene_name}_tree.json"
    echo "  Running scene_tree_dumper.gd..."
    "$GODOT" --headless --path "$PROJECT_DIR" \
        -s "res://scene_tree_dumper.gd" \
        -- --output "$tree_file" --scene "$scene_res" \
        2>&1 | sed 's/^/  /' || true

    # Run individual property dump.
    props_file="$OUTPUT_DIR/${scene_name}_properties.json"
    echo "  Running property_dumper.gd..."
    "$GODOT" --headless --path "$PROJECT_DIR" \
        -s "res://property_dumper.gd" \
        -- --output "$props_file" --scene "$scene_res" \
        2>&1 | sed 's/^/  /' || true

    echo ""
done

echo "=== Summary ==="
echo "Scenes processed: $((PASS + FAIL))"
echo "  Pass: $PASS"
echo "  Fail: $FAIL"
echo "Outputs in: $OUTPUT_DIR"
