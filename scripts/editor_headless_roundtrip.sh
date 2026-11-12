#!/usr/bin/env bash
# pat-vnt3j: Headless curl-driven roundtrip script that drives editor_server
# end to end with no Rust client, no GUI, and no JS. Proves an agent can
# create a scene, mutate it, save it to disk, reload from disk, and observe
# the round-tripped tree match what it built.
#
# Usage: editor_headless_roundtrip.sh <port> <scene-path>
#
# Exits 0 on success. On any unexpected response, prints what was observed
# and exits non-zero so the calling test sees a hard failure.

set -euo pipefail

if [[ $# -lt 2 ]]; then
    echo "usage: $0 <port> <scene-path>" >&2
    exit 64
fi

PORT="$1"
SCENE_PATH="$2"
BASE="http://127.0.0.1:${PORT}"

step() { printf '[%s] %s\n' "$(date +%H:%M:%S)" "$*"; }
fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }

CURL=(curl --silent --show-error --fail-with-body --max-time 5)

# ----------------------------------------------------------------------
# 1. Snapshot the starting tree and find the scene root (first child of root).
# ----------------------------------------------------------------------
step "GET /api/scene (initial)"
initial=$("${CURL[@]}" "${BASE}/api/scene") || fail "initial GET /api/scene failed"
scene_root_id=$(printf '%s' "$initial" | jq -r '.nodes.children[0].id // empty')
if [[ -z "$scene_root_id" ]]; then
    fail "initial /api/scene has no scene root under nodes.children[0]: $initial"
fi
step "scene_root id = $scene_root_id"

# ----------------------------------------------------------------------
# 2. Add three nodes (one POST per node) and remember the returned ids.
# ----------------------------------------------------------------------
declare -a added_ids=()
declare -a added_names=("RoundtripA" "RoundtripB" "RoundtripC")
declare -a added_classes=("Node2D" "Node2D" "Node2D")

for i in 0 1 2; do
    name="${added_names[$i]}"
    klass="${added_classes[$i]}"
    body=$(jq -nc \
        --argjson parent "$scene_root_id" \
        --arg name "$name" \
        --arg klass "$klass" \
        '{parent_id: $parent, name: $name, class_name: $klass}')
    step "POST /api/node/add name=$name class=$klass"
    resp=$("${CURL[@]}" -H 'Content-Type: application/json' \
        -d "$body" "${BASE}/api/node/add") \
        || fail "POST /api/node/add for $name failed"
    new_id=$(printf '%s' "$resp" | jq -r '.id // empty')
    if [[ -z "$new_id" ]]; then
        fail "/api/node/add did not return an id for $name: $resp"
    fi
    added_ids+=("$new_id")
done

# ----------------------------------------------------------------------
# 3. Verify the tree now has the three new nodes under the scene root.
# ----------------------------------------------------------------------
step "GET /api/scene (after adds)"
after_adds=$("${CURL[@]}" "${BASE}/api/scene") || fail "GET /api/scene after adds failed"
names_after_adds=$(printf '%s' "$after_adds" \
    | jq -r '.nodes.children[0].children[] | .name' \
    | sort)
expected_names=$(printf '%s\n' "${added_names[@]}" | sort)
if [[ "$names_after_adds" != "$expected_names" ]]; then
    fail "after adds, scene root children mismatch.\nwanted:\n$expected_names\ngot:\n$names_after_adds"
fi

# ----------------------------------------------------------------------
# 4. Save the scene to disk.
# ----------------------------------------------------------------------
step "POST /api/scene/save path=$SCENE_PATH"
save_body=$(jq -nc --arg path "$SCENE_PATH" '{path: $path}')
save_resp=$("${CURL[@]}" -H 'Content-Type: application/json' \
    -d "$save_body" "${BASE}/api/scene/save") \
    || fail "POST /api/scene/save failed"
if [[ "$(printf '%s' "$save_resp" | jq -r '.ok // false')" != "true" ]]; then
    fail "/api/scene/save did not return ok=true: $save_resp"
fi
if [[ ! -s "$SCENE_PATH" ]]; then
    fail "scene file was not written to disk at $SCENE_PATH"
fi

# ----------------------------------------------------------------------
# 5. Reload the scene from disk (replaces in-memory tree).
# ----------------------------------------------------------------------
step "POST /api/scene/load path=$SCENE_PATH"
load_resp=$("${CURL[@]}" -H 'Content-Type: application/json' \
    -d "$save_body" "${BASE}/api/scene/load") \
    || fail "POST /api/scene/load failed"
if [[ "$(printf '%s' "$load_resp" | jq -r '.ok // false')" != "true" ]]; then
    fail "/api/scene/load did not return ok=true: $load_resp"
fi

# ----------------------------------------------------------------------
# 6. Confirm the round-tripped tree matches: scene root has exactly the
#    three nodes we added, by name, in any order.
# ----------------------------------------------------------------------
step "GET /api/scene (after reload)"
after_reload=$("${CURL[@]}" "${BASE}/api/scene") || fail "GET /api/scene after reload failed"
names_after_reload=$(printf '%s' "$after_reload" \
    | jq -r '.nodes.children[0].children[] | .name' \
    | sort)
if [[ "$names_after_reload" != "$expected_names" ]]; then
    fail "after reload, scene root children mismatch.\nwanted:\n$expected_names\ngot:\n$names_after_reload"
fi

step "roundtrip OK: created, saved to $SCENE_PATH, reloaded, tree matched"
exit 0
