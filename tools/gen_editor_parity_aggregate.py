#!/usr/bin/env python3
"""Assemble the canonical editor-parity execution map and exit criteria from
the 18 per-lane maps. Concatenates lanes in bootstrap-map order under a single
`## Now`, and emits one unchecked exit checkbox per acceptance test marker."""
import re
import pathlib

PRD = pathlib.Path(__file__).resolve().parent.parent / "prd"

# Lane files in bootstrap-map (prd/EDITOR_PARITY_BOOTSTRAP_MAP.md) order.
LANES = [
    "EDITOR_PARITY_SCENE_TREE_OPS_MAP.md",
    "EDITOR_PARITY_SCENE_TREE_INDICATORS_MAP.md",
    "EDITOR_PARITY_INSPECTOR_TOOLBAR_MAP.md",
    "EDITOR_PARITY_INSPECTOR_PROPERTIES_MAP.md",
    "EDITOR_PARITY_INSPECTOR_ADVANCED_MAP.md",
    "EDITOR_PARITY_VIEWPORT_SELECTION_MAP.md",
    "EDITOR_PARITY_VIEWPORT_GIZMOS_MAP.md",
    "EDITOR_PARITY_VIEWPORT_OVERLAYS_MAP.md",
    "EDITOR_PARITY_TOP_BAR_MAP.md",
    "EDITOR_PARITY_MENUS_MAP.md",
    "EDITOR_PARITY_CREATE_NODE_MAP.md",
    "EDITOR_PARITY_BOTTOM_PANELS_MAP.md",
    "EDITOR_PARITY_SCRIPT_EDITOR_CORE_MAP.md",
    "EDITOR_PARITY_SCRIPT_EDITOR_NAV_MAP.md",
    "EDITOR_PARITY_FILESYSTEM_DOCK_MAP.md",
    "EDITOR_PARITY_SIGNALS_DOCK_MAP.md",
    "EDITOR_PARITY_ANIMATION_EDITOR_MAP.md",
    "EDITOR_PARITY_EDITOR_SYSTEMS_MAP.md",
]

BEAD_RE = re.compile(r"^\s*\d+\.\s+`([^`]+)`\s*(.*)$")
TEST_RE = re.compile(r"\(test:\s*`([^`]+)`\)")


def lane_title(text: str) -> str:
    for line in text.splitlines():
        if line.startswith("# "):
            t = line[2:].strip()
            return t.replace("Editor Parity — ", "")
    return "Untitled lane"


def now_body(text: str) -> str:
    """Return the lines after the first `## Now` header (the bead list)."""
    lines = text.splitlines()
    for i, line in enumerate(lines):
        if line.strip() == "## Now":
            return "\n".join(lines[i + 1:]).strip("\n")
    return ""


exec_parts = [
    "# Editor Parity — Canonical Execution Map",
    "",
    "Aggregated from the 18 per-lane execution maps (see "
    "`prd/EDITOR_PARITY_BOOTSTRAP_MAP.md`), in lane order. Each lane's beads "
    "are preserved verbatim under the `## Now` priority band; no lane declared "
    "`## Next`/`## Later` beads, so all work is Now-priority.",
    "",
    "Exit criteria with one checkbox per acceptance test live in "
    "`prd/EDITOR_PARITY_EXIT.md`.",
    "",
    "## Now",
    "",
]

exit_rows = []  # (test_name, key, desc, lane_title)

for fname in LANES:
    path = PRD / fname
    text = path.read_text()
    title = lane_title(text)
    exec_parts.append(f"### {title}")
    exec_parts.append("")
    body = now_body(text)
    exec_parts.append(body)
    exec_parts.append("")

    # Pair each numbered bead with the test marker on its Acceptance line.
    pending = None  # (key, desc)
    for line in body.splitlines():
        m = BEAD_RE.match(line)
        if m:
            pending = (m.group(1), m.group(2).strip())
            continue
        tm = TEST_RE.search(line)
        if tm and pending is not None:
            exit_rows.append((tm.group(1), pending[0], pending[1], title))
            pending = None

exec_path = PRD / "EDITOR_PARITY_EXECUTION_MAP.md"
exec_path.write_text("\n".join(exec_parts).rstrip("\n") + "\n")

exit_parts = [
    "# Editor Parity — Exit Criteria",
    "",
    "One unchecked checkbox per acceptance test referenced across the per-lane "
    "execution maps (`prd/EDITOR_PARITY_EXECUTION_MAP.md`). The planner ticks a "
    "box when its named test passes; the phase exits when every box is checked.",
    "",
    "## Now",
    "",
]
for test_name, key, desc, title in exit_rows:
    exit_parts.append(f"- [ ] `{key}` {desc} (test: `{test_name}`)")

exit_path = PRD / "EDITOR_PARITY_EXIT.md"
exit_path.write_text("\n".join(exit_parts).rstrip("\n") + "\n")

print(f"lanes={len(LANES)} exit_checkboxes={len(exit_rows)}")
print(f"wrote {exec_path.name} and {exit_path.name}")
