#!/usr/bin/env bash
#
# Capture editor parity goldens from a Godot 4.6.1 reference project.
#
# Runs the inspector + scene-tree probes against each of five reference scenes
# and writes JSON goldens into engine-rs/tests/goldens/. Viewport PNG goldens
# are still captured manually (see README at end of script output).
#
# Usage:
#   ./tools/oracle/capture_editor_goldens.sh <godot_project_path>
#
# Example:
#   ./tools/oracle/capture_editor_goldens.sh ~/dev/games/test00
#
set -euo pipefail

PROJECT_PATH="${1:-}"
if [[ -z "$PROJECT_PATH" ]]; then
  echo "usage: $0 <godot_project_path>" >&2
  exit 2
fi
if [[ ! -f "$PROJECT_PATH/project.godot" ]]; then
  echo "error: $PROJECT_PATH does not look like a Godot project (no project.godot)" >&2
  exit 2
fi

GODOT="${GODOT:-godot}"
if ! command -v "$GODOT" >/dev/null; then
  echo "error: '$GODOT' not on PATH. Set GODOT=/path/to/godot and re-run." >&2
  exit 2
fi

# Resolve absolute paths
PROJECT_PATH="$(cd "$PROJECT_PATH" && pwd)"
PATINA_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
GOLDENS_DIR="$PATINA_ROOT/engine-rs/tests/goldens"
TOOLS_DIR="$PATINA_ROOT/tools/oracle"

mkdir -p "$GOLDENS_DIR"

# Copy probe scripts into the Godot project so they can be loaded as res:// paths.
# We use a unique subdir so we don't clobber anything the user has there.
PROBE_DIR="$PROJECT_PATH/_patina_probes_tmp"
mkdir -p "$PROBE_DIR"
cp "$TOOLS_DIR/editor_inspector_probe.gd" "$PROBE_DIR/"
cp "$TOOLS_DIR/editor_scene_tree_probe.gd" "$PROBE_DIR/"
trap 'rm -rf "$PROBE_DIR"' EXIT

SCENES=(
  "main.tscn"
  "breakout.tscn"
  "asteroids.tscn"
  "pong.tscn"
  "platformer.tscn"
)

echo "Project: $PROJECT_PATH"
echo "Output:  $GOLDENS_DIR"
echo

for scene_file in "${SCENES[@]}"; do
  if [[ ! -f "$PROJECT_PATH/$scene_file" ]]; then
    echo "skip: $scene_file (not found in project)"
    continue
  fi
  base="${scene_file%.tscn}"
  inspector_out="$GOLDENS_DIR/inspector_${base}.json"
  scene_tree_out="$GOLDENS_DIR/scene_tree_${base}.json"

  echo "-- $scene_file --"
  echo "  inspector  -> $inspector_out"
  "$GODOT" --headless --path "$PROJECT_PATH" \
    -s "res://_patina_probes_tmp/editor_inspector_probe.gd" -- \
    --output "$inspector_out" \
    --scene "res://$scene_file" \
    2>&1 | sed 's/^/    /' | tail -20 || true

  echo "  scene_tree -> $scene_tree_out"
  "$GODOT" --headless --path "$PROJECT_PATH" \
    -s "res://_patina_probes_tmp/editor_scene_tree_probe.gd" -- \
    --output "$scene_tree_out" \
    --scene "res://$scene_file" \
    2>&1 | sed 's/^/    /' | tail -20 || true
done

echo
echo "JSON goldens written to: $GOLDENS_DIR"
echo
echo "Manual step — viewport screenshots:"
echo "  For each of the 5 scenes, open it in Godot, switch to 2D view,"
echo "  fit the framing, and screenshot the viewport area only."
echo "  Save as:"
for scene_file in "${SCENES[@]}"; do
  base="${scene_file%.tscn}"
  echo "    $GOLDENS_DIR/viewport_${base}.png"
done
echo
echo "On macOS use Cmd+Shift+4 then Space then click the viewport area."
