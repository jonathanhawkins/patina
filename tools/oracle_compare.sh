#!/bin/bash
# oracle_compare.sh — Runs Godot and Patina on the same scene, compares outputs.
#
# Usage: oracle_compare.sh <project_dir>
#
# Requires: Godot 4.x, cargo (for patina-runner), python3

set -euo pipefail

GODOT="/Users/bone/Downloads/Godot.app/Contents/MacOS/Godot"
PATINA="cargo run -p patina-runner --"
PROJECT_DIR="${1:-.}"
TOLERANCE="0.001"

# Resolve to absolute path
PROJECT_DIR="$(cd "$PROJECT_DIR" && pwd)"

# --- Find main scene from project.godot ---
if [[ ! -f "$PROJECT_DIR/project.godot" ]]; then
    echo "ERROR: No project.godot found in $PROJECT_DIR"
    exit 1
fi

MAIN_SCENE=$(grep -E '^run/main_scene=' "$PROJECT_DIR/project.godot" \
    | sed 's/run\/main_scene="//' | sed 's/"$//' \
    | sed 's|^res://||')

if [[ -z "$MAIN_SCENE" ]]; then
    echo "ERROR: Could not find run/main_scene in project.godot"
    exit 1
fi

echo "=== Oracle Comparison ==="
echo "Project:    $PROJECT_DIR"
echo "Main scene: $MAIN_SCENE"
echo ""

TMPDIR_WORK=$(mktemp -d)
trap 'rm -rf "$TMPDIR_WORK"' EXIT

GODOT_JSON="$TMPDIR_WORK/godot_output.json"
PATINA_JSON="$TMPDIR_WORK/patina_output.json"

# --- Run Godot (oracle) ---
echo ">>> Running Godot oracle..."
if "$GODOT" --headless --path "$PROJECT_DIR" --quit-after 2 2>/dev/null \
    | grep -E '^\{' > "$GODOT_JSON" 2>/dev/null; then
    echo "    Godot output captured."
else
    echo "    WARNING: Godot produced no JSON output (may need oracle dump script)"
    echo '{}' > "$GODOT_JSON"
fi

# --- Run Patina ---
echo ">>> Running Patina..."
SCENE_PATH="$PROJECT_DIR/$MAIN_SCENE"
if $PATINA "$SCENE_PATH" --frames 2 2>/dev/null \
    | grep -E '^\{' > "$PATINA_JSON" 2>/dev/null; then
    echo "    Patina output captured."
else
    echo "    WARNING: Patina produced no JSON output"
    echo '{}' > "$PATINA_JSON"
fi

# --- Compare outputs ---
echo ""
echo ">>> Comparing outputs..."
echo ""

python3 - "$GODOT_JSON" "$PATINA_JSON" "$TOLERANCE" <<'PYEOF'
import json
import sys
import math

def load_json(path):
    try:
        with open(path) as f:
            return json.load(f)
    except (json.JSONDecodeError, FileNotFoundError):
        return {}

def flatten_nodes(tree, path_prefix=""):
    """Flatten a nested node tree into {path: node} dict."""
    result = {}
    if isinstance(tree, list):
        for node in tree:
            flatten_nodes_impl(node, result)
    elif isinstance(tree, dict) and "nodes" in tree:
        for node in tree["nodes"]:
            flatten_nodes_impl(node, result)
    elif isinstance(tree, dict) and "name" in tree:
        flatten_nodes_impl(tree, result)
    return result

def flatten_nodes_impl(node, result):
    path = node.get("path", node.get("name", ""))
    result[path] = node
    for child in node.get("children", []):
        flatten_nodes_impl(child, result)

def count_nodes(tree):
    """Count total nodes in a tree."""
    if isinstance(tree, dict) and "nodes" in tree:
        return sum(count_subtree(n) for n in tree["nodes"])
    elif isinstance(tree, dict) and "name" in tree:
        return count_subtree(tree)
    elif isinstance(tree, list):
        return sum(count_subtree(n) for n in tree)
    return 0

def count_subtree(node):
    return 1 + sum(count_subtree(c) for c in node.get("children", []))

def floats_close(a, b, tol):
    """Compare two values with float tolerance."""
    if isinstance(a, (int, float)) and isinstance(b, (int, float)):
        return math.isclose(float(a), float(b), abs_tol=tol)
    if isinstance(a, list) and isinstance(b, list) and len(a) == len(b):
        return all(floats_close(x, y, tol) for x, y in zip(a, b))
    return a == b

def compare_properties(godot_props, patina_props, tol):
    """Compare property dicts, return list of (key, match, detail)."""
    results = []
    all_keys = set(list(godot_props.keys()) + list(patina_props.keys()))
    for key in sorted(all_keys):
        g_val = godot_props.get(key)
        p_val = patina_props.get(key)
        if g_val is None:
            results.append((key, False, f"missing in Godot, patina={p_val}"))
        elif p_val is None:
            results.append((key, False, f"missing in Patina, godot={g_val}"))
        else:
            # Extract value for comparison (handle typed format)
            g_cmp = g_val.get("value", g_val) if isinstance(g_val, dict) else g_val
            p_cmp = p_val.get("value", p_val) if isinstance(p_val, dict) else p_val
            if floats_close(g_cmp, p_cmp, tol):
                results.append((key, True, "match"))
            else:
                results.append((key, False, f"godot={g_cmp} patina={p_cmp}"))
    return results

godot_path, patina_path, tol_str = sys.argv[1], sys.argv[2], sys.argv[3]
tol = float(tol_str)

godot = load_json(godot_path)
patina = load_json(patina_path)

passed = 0
failed = 0

# --- Node count ---
g_count = count_nodes(godot)
p_count = count_nodes(patina)
if g_count == p_count:
    print(f"PASS  node_count: {g_count}")
    passed += 1
else:
    print(f"FAIL  node_count: godot={g_count} patina={p_count}")
    failed += 1

# --- Flatten and compare nodes ---
g_nodes = flatten_nodes(godot)
p_nodes = flatten_nodes(patina)

g_paths = set(g_nodes.keys())
p_paths = set(p_nodes.keys())

# Node names/paths
missing_in_patina = g_paths - p_paths
missing_in_godot = p_paths - g_paths
common = g_paths & p_paths

if not missing_in_patina and not missing_in_godot:
    print(f"PASS  node_names: all {len(common)} nodes present in both")
    passed += 1
else:
    if missing_in_patina:
        print(f"FAIL  node_names: missing in patina: {sorted(missing_in_patina)}")
        failed += 1
    if missing_in_godot:
        print(f"FAIL  node_names: missing in godot: {sorted(missing_in_godot)}")
        failed += 1

# Node classes
class_pass = True
for path in sorted(common):
    g_class = g_nodes[path].get("class", "")
    p_class = p_nodes[path].get("class", "")
    if g_class != p_class:
        print(f"FAIL  class[{path}]: godot={g_class} patina={p_class}")
        failed += 1
        class_pass = False

if class_pass and common:
    print(f"PASS  node_classes: all {len(common)} nodes match")
    passed += 1

# Properties (including positions with float tolerance)
prop_failures = 0
prop_checks = 0
for path in sorted(common):
    g_props = g_nodes[path].get("properties", {})
    p_props = p_nodes[path].get("properties", {})
    results = compare_properties(g_props, p_props, tol)
    for key, match, detail in results:
        prop_checks += 1
        if match:
            passed += 1
        else:
            print(f"FAIL  property[{path}].{key}: {detail}")
            failed += 1
            prop_failures += 1

if prop_failures == 0 and prop_checks > 0:
    print(f"PASS  properties: all {prop_checks} property comparisons match")
elif prop_checks == 0:
    print(f"SKIP  properties: no properties to compare")

# --- Summary ---
print("")
total = passed + failed
print(f"=== Results: {passed}/{total} passed, {failed} failed ===")
sys.exit(1 if failed > 0 else 0)
PYEOF
