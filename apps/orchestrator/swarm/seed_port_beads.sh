#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "$0")/.." && pwd)/orch_env.sh"
ISSUES_JSONL="${PROJECT_ROOT}/.beads/issues.jsonl"
DEFAULT_EXEC_MAP="${PROJECT_ROOT}/prd/POST_REPIN_EXECUTION_MAP.md"
FALLBACK_EXEC_MAP="${PROJECT_ROOT}/prd/BEAD_EXECUTION_MAP.md"
PORT_PLAN="${PROJECT_ROOT}/prd/PORT_GODOT_TO_RUST_PLAN.md"
CONTRACT_GAPS="${PROJECT_ROOT}/prd/PORT_PLAN_CONTRACT_GAPS.json"
REPIN_BEADS="${PROJECT_ROOT}/prd/GODOT_4_6_1_REPIN_BEADS.md"
RELEASE_DELTA_AUDIT="${PROJECT_ROOT}/prd/GODOT_4_6_1_RELEASE_DELTA_AUDIT.md"
V1_EXIT_EXEC_MAP="${PROJECT_ROOT}/prd/V1_EXIT_EXECUTION_MAP.md"
EXEC_MAP="${ORCH_EXEC_MAP:-$DEFAULT_EXEC_MAP}"
CREATED_COUNT=0

cd "$PROJECT_ROOT"

if [[ ! -f "$EXEC_MAP" ]]; then
  EXEC_MAP="$FALLBACK_EXEC_MAP"
fi

ensure_tracker_ready() {
  br sync --flush-only >/dev/null 2>&1 || true
}

run_br_create() {
  local attempt output rc
  for attempt in 1 2 3 4 5; do
    output=$("$@" 2>&1) && {
      return 0
    }
    rc=$?
    if printf '%s' "$output" | grep -Eqi 'database is (busy|locked)|sync conflict|unsaved change'; then
      sleep 1
      ensure_tracker_ready
      continue
    fi
    printf '%s\n' "$output" >&2
    return "$rc"
  done
  printf '%s\n' "$output" >&2
  return 1
}

issue_exists() {
  local title="${1:?title required}"
  local contract_key="${2:-}"
  local db
  db=$(find "${PROJECT_ROOT}/.beads" -maxdepth 1 -name '*.db' | head -n 1)
  if [[ -n "$db" ]]; then
    local count
    local title_sql contract_sql query
    title_sql=$(printf "%s" "$title" | sed "s/'/''/g")
    query="select count(*) from issues where title = '${title_sql}'"
    if [[ -n "$contract_key" ]]; then
      contract_sql=$(printf "%s" "[contract-key: ${contract_key}]" | sed "s/'/''/g")
      query="${query} or description like '%${contract_sql}%'"
    fi
    query="${query};"
    count=$(sqlite3 -cmd '.timeout 5000' "$db" "$query" 2>/dev/null || echo "0")
    if [[ "${count:-0}" != "0" ]]; then
      echo "1"
      return 0
    fi
  fi

  python3 - "$ISSUES_JSONL" "$title" "$contract_key" <<'PY'
import json, sys
path, title, contract_key = sys.argv[1], sys.argv[2], sys.argv[3]
found = False
with open(path, "r", encoding="utf-8") as fh:
    for line in fh:
        line = line.strip()
        if not line:
            continue
        try:
            obj = json.loads(line)
        except Exception:
            continue
        description = obj.get("description") or ""
        if obj.get("title") == title:
            found = True
            break
        if contract_key and f"[contract-key: {contract_key}]" in description:
            found = True
            break
print("1" if found else "0")
PY
}

create_if_missing() {
  local title="${1:?title required}"
  local priority="${2:?priority required}"
  local labels="${3:?labels required}"
  local description="${4:?description required}"
  local contract_key="${5:-}"

  if [[ "$(issue_exists "$title" "$contract_key")" == "1" ]]; then
    echo "seed: exists -> $title"
    return 0
  fi

  ensure_tracker_ready
  run_br_create br create --title "$title" --type task --priority "$priority" --labels "$labels" --description "$description" >/dev/null
  ensure_tracker_ready
  echo "seed: created -> $title"
  CREATED_COUNT=$((CREATED_COUNT + 1))
}

infer_labels() {
  local title="${1:?title required}"
  local lower
  lower=$(printf '%s' "$title" | tr '[:upper:]' '[:lower:]')
  case "$lower" in
    *signal*|*notification*|*lifecycle*|*trace*)
      echo "runtime,signals,oracle,parity"
      ;;
    *physics*|*body2d*|*collision*|*fixed-step*)
      echo "physics,runtime,parity"
      ;;
    *input*|*keyboard*|*mouse*)
      echo "input,runtime,parity"
      ;;
    *resource*|*packedscene*|*classdb*|*uid*|*gdobject*)
      echo "resources,api,parity"
      ;;
    *render*|*viewport*|*camera*|*texture*|*2d*)
      echo "render,fixtures,parity"
      ;;
    *apps/godot*|*probe*|*oracle*)
      echo "godot-lab,oracle,parity"
      ;;
    *)
      echo "runtime,parity"
      ;;
  esac
}

seed_from_execution_map() {
  local map_path="${1:-$EXEC_MAP}"
  if [[ ! -f "$map_path" ]]; then
    return 0
  fi
  local include_later="${ORCH_SEED_INCLUDE_LATER:-0}"
  local map_label
  map_label=$(python3 - "$PROJECT_ROOT" "$map_path" <<'PY'
import os, sys
root, path = sys.argv[1], sys.argv[2]
print(os.path.relpath(path, root))
PY
)
  python3 - "$map_path" "$include_later" <<'PY' | while IFS=$'\t' read -r section bead_id title acceptance; do
import re, sys

path = sys.argv[1]
include_later = sys.argv[2] == "1"
section = None
allowed = {"Now", "Next"}
if include_later:
    allowed.add("Later")

header_re = re.compile(r"^##\s+(Now|Next|Later)\s*$")
item_re = re.compile(r'^\d+\.\s+`([^`]+)`\s+(.+?)\s*$')
acceptance_re = re.compile(r'^\s+Acceptance:\s+(.+?)\s*$')

claim_order_re = re.compile(r"^Claim order:\s*$")
goal_re = re.compile(r"^Goal:\s*$")
blocked_headers = {"Five-Team Layout", "Do Not Do Yet", "Required Reporting Format"}
in_claim_order = False

# Two-pass: collect items and their acceptance lines
lines = []
with open(path, "r", encoding="utf-8") as fh:
    lines = [raw.rstrip("\n") for raw in fh]

pending = []  # (section, bead_id, title)
for i, line in enumerate(lines):
    stripped = line.strip()
    header = header_re.match(line)
    if header:
        section = header.group(1)
        in_claim_order = False
        continue
    if stripped.startswith("## "):
        in_claim_order = False
    if stripped.startswith("### "):
        in_claim_order = False
        name = stripped[4:].strip()
        if name in blocked_headers:
            section = None
        continue
    if section == "Next":
        if goal_re.match(stripped):
            in_claim_order = False
            continue
        if claim_order_re.match(stripped):
            in_claim_order = True
            continue
        if in_claim_order:
            item = item_re.match(stripped)
            if item:
                bead_id, title = item.groups()
                # Check if next line is an Acceptance: line
                acceptance_cmd = ""
                if i + 1 < len(lines):
                    acc = acceptance_re.match(lines[i + 1])
                    if acc:
                        acceptance_cmd = acc.group(1)
                print(f"{section}\t{bead_id}\t{title}\t{acceptance_cmd}")
            elif stripped and not stripped.startswith("-") and not stripped.startswith("wait for "):
                in_claim_order = False
            continue
    # Check for Acceptance: line (already handled inline for Next items)
    acc = acceptance_re.match(stripped)
    if acc:
        continue
    if section not in allowed:
        continue
    item = item_re.match(stripped)
    if not item:
        continue
    bead_id, title = item.groups()
    # Check if next line is an Acceptance: line
    acceptance_cmd = ""
    if i + 1 < len(lines):
        acc = acceptance_re.match(lines[i + 1])
        if acc:
            acceptance_cmd = acc.group(1)
    print(f"{section}\t{bead_id}\t{title}\t{acceptance_cmd}")
PY
    [[ -z "${title:-}" ]] && continue
    local priority description labels
    case "$section" in
      Now) priority=1 ;;
      Next) priority=2 ;;
      Later) priority=3 ;;
      *) priority=3 ;;
    esac
    labels=$(infer_labels "$title")
    description="Seeded from ${map_label} (${section}, ${bead_id}). IMPLEMENT the feature described in the title — do not just write tests for existing behavior. The acceptance test in v1_acceptance_gate_test.rs currently FAILS and must PASS after your implementation."
    if [[ -n "${acceptance:-}" ]]; then
      description="${description}
Acceptance: ${acceptance}"
    fi
    create_if_missing "$title" "$priority" "$labels" "$description"
  done
}

seed_from_port_plan() {
  local plan_label
  if [[ ! -f "$PORT_PLAN" ]]; then
    return 0
  fi
  plan_label=$(python3 - "$PROJECT_ROOT" "$PORT_PLAN" <<'PY'
import os, sys
root, path = sys.argv[1], sys.argv[2]
print(os.path.relpath(path, root))
PY
)

  python3 - "$PORT_PLAN" "$plan_label" <<'PY' | while IFS=$'\t' read -r title priority labels description; do
import re
import sys

path = sys.argv[1]
plan_label = sys.argv[2]
section = None
subsection = None
in_bullets = False

section_re = re.compile(r"^##\s+(Phase \d+ - .+|Bead Pack \d+ - .+|Immediate Next Steps)\s*$")
subsection_re = re.compile(r"^###\s+(.+?)\s*$")

phase_rules = {
    ("Phase 5 - Broader Runtime and 3D Prep", "Scope", "richer resource types"): (
        "Broaden resource type support beyond the initial runtime slice",
        "2",
        "resources,runtime,phase5,port-plan",
    ),
    ("Phase 5 - Broader Runtime and 3D Prep", "Scope", "audio basics"): (
        "Add audio runtime basics and first audio test harness",
        "3",
        "audio,runtime,phase5,port-plan",
    ),
    ("Phase 5 - Broader Runtime and 3D Prep", "Scope", "broader input handling"): (
        "Broaden input handling coverage beyond the initial 2D slice",
        "2",
        "input,runtime,phase5,port-plan",
    ),
    ("Phase 5 - Broader Runtime and 3D Prep", "Scope", "scene instancing edge cases"): (
        "Expand scene instancing edge-case coverage",
        "2",
        "scene,instancing,phase5,port-plan",
    ),
    ("Phase 5 - Broader Runtime and 3D Prep", "Scope", "groundwork for 3d servers and render paths"): (
        "Lay groundwork for 3D servers and render-path crate boundaries",
        "3",
        "3d,architecture,phase5,port-plan",
    ),
    ("Phase 5 - Broader Runtime and 3D Prep", "Deliverables", "broader integration fixtures"): (
        "Build broader integration workflows for representative scene and project runs",
        "2",
        "integration,compat,phase5,phase9,port-plan",
    ),
    ("Phase 5 - Broader Runtime and 3D Prep", "Deliverables", "initial 3d architecture spec"): (
        "Write initial 3D architecture spec and crate-boundary review",
        "3",
        "3d,architecture,phase5,port-plan",
    ),
    ("Phase 6 - 3D Runtime Slice", "Scope", "3d node subset"): (
        "Implement the first 3D node subset runtime slice",
        "3",
        "3d,runtime,phase6,port-plan",
    ),
    ("Phase 6 - 3D Runtime Slice", "Scope", "transforms/cameras/lights subset"): (
        "Cover 3D transforms, cameras, and lights subset contracts",
        "3",
        "3d,runtime,phase6,port-plan",
    ),
    ("Phase 6 - 3D Runtime Slice", "Scope", "initial 3d render path"): (
        "Build the initial 3D render path with measurable parity hooks",
        "3",
        "3d,render,phase6,port-plan",
    ),
    ("Phase 6 - 3D Runtime Slice", "Scope", "initial 3d physics hooks"): (
        "Add initial 3D physics hooks and deterministic test coverage",
        "3",
        "3d,physics,phase6,port-plan",
    ),
    ("Phase 6 - 3D Runtime Slice", "Scope", "representative 3d fixtures"): (
        "Add representative 3D fixtures for the first runtime slice",
        "3",
        "3d,fixtures,phase6,port-plan",
    ),
    ("Phase 6 - 3D Runtime Slice", "Deliverables", "first 3d crate set"): (
        "Define and bootstrap the first 3D crate set",
        "3",
        "3d,architecture,phase6,port-plan",
    ),
    ("Phase 6 - 3D Runtime Slice", "Deliverables", "render and physics comparison tooling"): (
        "Add 3D render and physics comparison tooling",
        "3",
        "3d,oracle,phase6,port-plan",
    ),
    ("Phase 6 - 3D Runtime Slice", "Deliverables", "first real 3d demo parity report"): (
        "Produce the first real 3D demo parity report",
        "3",
        "3d,reporting,phase6,port-plan",
    ),
    ("Phase 7 - Platform Layer and Distribution", "Scope", "windowing"): (
        "Stabilize gdplatform windowing input and timing layer",
        "2",
        "platform,input,timing,phase7,port-plan",
    ),
    ("Phase 7 - Platform Layer and Distribution", "Scope", "packaging/bootstrap"): (
        "Add startup packaging flow and supported-target CI matrix",
        "3",
        "platform,ci,distribution,phase7,port-plan",
    ),
    ("Phase 7 - Platform Layer and Distribution", "Deliverables", "desktop platform targets"): (
        "Define supported desktop platform targets and validation coverage",
        "3",
        "platform,ci,distribution,phase7,port-plan",
    ),
    ("Phase 9 - Hardening and Release Discipline", "Deliverables", "benchmark dashboards"): (
        "Build benchmark dashboards for runtime parity and regressions",
        "3",
        "benchmarks,reporting,phase9,port-plan",
    ),
    ("Phase 9 - Hardening and Release Discipline", "Deliverables", "fuzz/property tests where useful"): (
        "Add fuzz and property tests for high-risk runtime surfaces",
        "3",
        "testing,phase9,port-plan",
    ),
    ("Phase 9 - Hardening and Release Discipline", "Deliverables", "crash triage process"): (
        "Define crash triage process for runtime regressions",
        "3",
        "stability,phase9,port-plan",
    ),
    ("Phase 9 - Hardening and Release Discipline", "Deliverables", "release train"): (
        "Define repeatable release-train workflow for Patina runtime milestones",
        "3",
        "release,phase9,port-plan",
    ),
    ("Phase 9 - Hardening and Release Discipline", "Deliverables", "contributor onboarding docs"): (
        "Write contributor onboarding docs for runtime and oracle workflows",
        "3",
        "docs,phase9,port-plan",
    ),
    ("Phase 9 - Hardening and Release Discipline", "Deliverables", "migration guide for users"): (
        "Draft migration guide for users adopting Patina runtime milestones",
        "3",
        "docs,phase9,port-plan",
    ),
}

bead_pack_rules = {
    ("Bead Pack 07 - Physics and Audio Expansion", "deterministic 2d physics subset"): (
        "Extend deterministic 2D physics subset coverage beyond the first slice",
        "2",
        "physics,phase5,port-plan",
    ),
    ("Bead Pack 07 - Physics and Audio Expansion", "audio primitives"): (
        "Implement initial audio primitives behind deterministic runtime tests",
        "3",
        "audio,phase5,port-plan",
    ),
    ("Bead Pack 07 - Physics and Audio Expansion", "richer fixtures"): (
        "Add richer runtime fixtures for physics and audio expansion",
        "2",
        "fixtures,phase5,port-plan",
    ),
    ("Bead Pack 07 - Physics and Audio Expansion", "baseline perf checks"): (
        "Add baseline performance checks for expanded runtime slices",
        "3",
        "benchmarks,phase5,port-plan",
    ),
    ("Bead Pack 08 - 3D Architecture Prep", "3d subsystem map"): (
        "Write 3D subsystem map for the next runtime milestone",
        "3",
        "3d,architecture,phase6,port-plan",
    ),
    ("Bead Pack 08 - 3D Architecture Prep", "render abstraction decisions"): (
        "Record 3D render abstraction decisions before implementation",
        "3",
        "3d,architecture,phase6,port-plan",
    ),
    ("Bead Pack 08 - 3D Architecture Prep", "first 3d fixture plan"): (
        "Plan the first 3D fixture corpus and oracle capture flow",
        "3",
        "3d,fixtures,phase6,port-plan",
    ),
    ("Bead Pack 08 - 3D Architecture Prep", "dependency and crate split review"): (
        "Review dependencies and crate splits for upcoming 3D runtime work",
        "3",
        "3d,architecture,phase6,port-plan",
    ),
}

week_rules = {
    ("Immediate Next Steps", "Week 3+", "implement core runtime subset"): (
        "Close remaining core runtime subset gaps from the port plan",
        "2",
        "runtime,phase3,port-plan",
    ),
    ("Immediate Next Steps", "Week 3+", "wire compat tests into ci"): (
        "Wire remaining compatibility tests into CI for runtime slices",
        "2",
        "ci,compat,port-plan",
    ),
    ("Immediate Next Steps", "Week 3+", "begin resource and scene execution path"): (
        "Broaden resource and scene execution path coverage beyond the current slice",
        "2",
        "resources,scene,port-plan",
    ),
    ("Immediate Next Steps", "Week 3+", "prepare first headless milestone report"): (
        "Prepare updated headless milestone report against current runtime evidence",
        "3",
        "reporting,port-plan",
    ),
}

def normalize_bullet(text: str) -> str:
    text = text.strip()
    text = re.sub(r"^-\s*", "", text)
    text = text.rstrip(",.")
    return text.lower()

with open(path, "r", encoding="utf-8") as fh:
    for raw in fh:
        line = raw.rstrip("\n")
        header = section_re.match(line)
        if header:
            section = header.group(1)
            subsection = None
            in_bullets = False
            continue
        sub = subsection_re.match(line)
        if sub:
            subsection = sub.group(1)
            in_bullets = subsection in {"Scope", "Deliverables", "Week 1", "Week 2", "Week 3+"}
            continue
        stripped = line.strip()
        if not stripped.startswith("- "):
            continue
        bullet = normalize_bullet(stripped)
        rule = None
        if section and section.startswith("Phase "):
            rule = phase_rules.get((section, subsection, bullet))
        elif section and section.startswith("Bead Pack "):
            rule = bead_pack_rules.get((section, bullet))
        elif section == "Immediate Next Steps":
            rule = week_rules.get((section, subsection, bullet))
        if rule is None:
            continue
        title, priority, labels = rule
        description = f"Seeded from {plan_label} ({section}"
        if subsection:
            description += f", {subsection}"
        description += f"). Derived from port-plan bullet '{stripped}'. Implement with measurable tests/oracle evidence and keep scope aligned with the documented milestone."
        print(f"{title}\t{priority}\t{labels}\t{description}")
PY
    [[ -z "${title:-}" ]] && continue
    create_if_missing "$title" "$priority" "$labels" "$description"
  done
}

seed_from_contract_gaps() {
  [[ -f "$CONTRACT_GAPS" ]] || return 0

  python3 - "$CONTRACT_GAPS" <<'PY' | while IFS=$'\t' read -r title priority labels description contract_key; do
import json
import sys

path = sys.argv[1]
with open(path, "r", encoding="utf-8") as fh:
    data = json.load(fh)

for item in data:
    blocked_by_gates = item.get("blocked_by_gates") or []
    if blocked_by_gates and str(item.get("include_when_blocked", "")).lower() != "true":
        continue
    title = (item.get("title") or "").strip()
    if not title:
        continue
    priority = str(item.get("priority", 3)).strip() or "3"
    labels = (item.get("labels") or "runtime,port-plan,contract-gap").strip()
    contract_key = (item.get("contract_key") or "").strip()
    description_lines = []
    base_description = (item.get("description") or "").strip()
    if base_description:
        description_lines.append(base_description)
    else:
        description_lines.append("Seeded from prd/PORT_PLAN_CONTRACT_GAPS.json. Implement this contract gap with focused oracle-backed tests.")

    if contract_key:
        description_lines.append(f"[contract-key: {contract_key}]")
    parent_key = (item.get("parent_key") or "").strip()
    if parent_key:
        description_lines.append(f"[parent-key: {parent_key}]")
    domain = (item.get("domain") or "").strip()
    if domain:
        description_lines.append(f"[domain: {domain}]")
    program_goal = (item.get("program_goal") or "").strip()
    if program_goal:
        description_lines.append(f"[program-goal: {program_goal}]")
    target_version = (item.get("target_version") or "").strip()
    if target_version:
        description_lines.append(f"[target-version: {target_version}]")
    evidence_command = (item.get("evidence_command") or "").strip()
    if evidence_command:
        description_lines.append(f"[evidence-command: {evidence_command}]")
    oracle_source = (item.get("oracle_source") or "").strip()
    if oracle_source:
        description_lines.append(f"[oracle-source: {oracle_source}]")
    if item.get("maintenance_only"):
        description_lines.append("[maintenance-only: true]")
    blocked_by_gates = item.get("blocked_by_gates") or []
    for gate in blocked_by_gates:
        if gate:
            description_lines.append(f"[blocked-by-gate: {gate}]")
    description = " ".join(description_lines)
    print(f"{title}\t{priority}\t{labels}\t{description}\t{contract_key}")
PY
    [[ -z "${title:-}" ]] && continue
    create_if_missing "$title" "$priority" "$labels" "$description" "$contract_key"
  done
}

seed_from_repin_beads() {
  [[ -f "$REPIN_BEADS" ]] || return 0

  local repin_label
  repin_label=$(python3 - "$PROJECT_ROOT" "$REPIN_BEADS" <<'PY'
import os, sys
root, path = sys.argv[1], sys.argv[2]
print(os.path.relpath(path, root))
PY
)

  python3 - "$REPIN_BEADS" "$repin_label" <<'PY' | while IFS=$'\t' read -r title priority labels description; do
import re
import sys

path = sys.argv[1]
label = sys.argv[2]
section = None

section_map = {
    "## P0: Establish The New Baseline": ("1", "repin,oracle,baseline,4_6_1"),
    "## P1: Runtime Fallout Lanes": ("1", "repin,runtime,4_6_1"),
    "## P1: Goldens And Fixtures": ("2", "repin,goldens,fixtures,4_6_1"),
    "## P2: Tooling, Editor, And CI Fallout": ("2", "repin,tooling,ci,4_6_1"),
}

item_re = re.compile(r"^\d+\.\s+`?([^`]+?)`?\s*$")

with open(path, "r", encoding="utf-8") as fh:
    for raw in fh:
        line = raw.rstrip("\n")
        stripped = line.strip()
        if line in section_map:
            section = line
            continue
        if stripped.startswith("## "):
            section = None
            continue
        if section is None:
            continue
        match = item_re.match(stripped)
        if not match:
            continue
        title = match.group(1).strip()
        priority, labels = section_map[section]
        description = f"Seeded from {label} ({section[3:]}). Implement this 4.6.1 repin fallout item with measurable tests/oracle evidence before claiming parity."
        print(f"{title}\t{priority}\t{labels}\t{description}")
PY
    [[ -z "${title:-}" ]] && continue
    create_if_missing "$title" "$priority" "$labels" "$description"
  done
}

seed_from_release_delta_audit() {
  [[ -f "$RELEASE_DELTA_AUDIT" ]] || return 0

  local audit_label
  audit_label=$(python3 - "$PROJECT_ROOT" "$RELEASE_DELTA_AUDIT" <<'PY'
import os, sys
root, path = sys.argv[1], sys.argv[2]
print(os.path.relpath(path, root))
PY
)

  create_if_missing \
    "Verify ClassDB class_list stable sorted order against Godot 4.6.1" \
    1 \
    "repin,classdb,api,4_6_1" \
    "Seeded from ${audit_label} (Immediate action). Confirm ClassDB class_list ordering is deterministic and matches Godot 4.6.1 expectations."

  create_if_missing \
    "Check instanced-scene resource sharing after 4.6.1 repin" \
    2 \
    "repin,scene,resources,4_6_1" \
    "Seeded from ${audit_label} (monitor item). Revalidate instanced-scene resource sharing and edge-case ownership behavior against the repinned oracle."

  create_if_missing \
    "Verify unique node ID semantics against 4.6.1 oracle expectations" \
    2 \
    "repin,scene,nodeid,4_6_1" \
    "Seeded from ${audit_label} (monitor item). Verify Patina node ID semantics remain structurally compatible with Godot 4.6.1 oracle expectations."
}

# Seed from current documented port gaps and parity surfaces.
ensure_tracker_ready
seed_from_contract_gaps
seed_from_execution_map
seed_from_repin_beads
seed_from_release_delta_audit
seed_from_execution_map "$V1_EXIT_EXEC_MAP"

if [[ "${ORCH_SEED_USE_EXEC_MAP_ONLY:-1}" == "1" && "$CREATED_COUNT" -gt 0 ]]; then
  exit 0
fi

seed_from_port_plan

if [[ "${ORCH_SEED_USE_EXEC_MAP_ONLY:-1}" == "1" ]]; then
  exit 0
fi

create_if_missing \
  "Add NodePath generic NodeId resolver parity coverage" \
  3 \
  "nodepath,scene,parity" \
  "Broaden NodePath parity by covering generic NodeId-backed resolver paths in SceneTree and script access. Acceptance: focused tests describe supported resolution cases and document any remaining exclusions."

create_if_missing \
  "Add change_scene_to_packed API parity surface" \
  3 \
  "api,scene,parity" \
  "Cover the missing change_scene_to_packed / packed-scene change API surface alongside change_scene_to_node. Acceptance: bounded tests verify validation semantics and expected failure modes."

create_if_missing \
  "Expose Geometry2D singleton arc helpers to GDScript parity tests" \
  3 \
  "geometry,gdscript,parity" \
  "Close the current gap where Geometry2D helper methods are not reachable from GDScript parity fixtures. Acceptance: add minimal bindings or a documented stub boundary plus tests."

create_if_missing \
  "Add CONNECT_DEFERRED signal dispatch parity coverage" \
  3 \
  "signals,scene,parity" \
  "Cover deferred signal delivery semantics for scene and script callables. Acceptance: deterministic tests prove current behavior or close the gap to the Godot contract."

create_if_missing \
  "Validate window min/max clamp semantics against Godot defaults" \
  3 \
  "platform,windowing,parity" \
  "Broaden windowing parity to cover min_size/max_size constraints and resize clamping behavior. Acceptance: bounded tests or documented stub boundary."

create_if_missing \
  "Add oracle parity coverage for default-property stripping consistency" \
  2 \
  "oracle,properties,parity" \
  "Harden the default-property filtering path so oracle scenes using stripped defaults stay consistent across captures. Acceptance: focused regression tests and updated measured parity if applicable."

create_if_missing \
  "Add scene lifecycle parity coverage for packed-scene change transitions" \
  3 \
  "scene,lifecycle,parity" \
  "Verify enter_tree/ready/exit ordering when switching scenes through packed-scene APIs. Acceptance: focused transition tests with explicit ordering assertions."

create_if_missing \
  "Add resource cache parity coverage for concrete SubResource resolution" \
  3 \
  "resources,cache,parity" \
  "Broaden resource parity beyond string references by defining the next concrete SubResource/cache behavior to test. Acceptance: bounded tests for resolution and non-sharing semantics."

# Seed from the active execution map so the swarm can keep pulling documented
# parity work instead of stalling after the initial small batch.
create_if_missing \
  "Add global lifecycle and signal ordering trace parity" \
  2 \
  "runtime,signals,traces,parity" \
  "Capture a total ordered trace for notifications, script callbacks, and signal emissions. Acceptance: focused oracle-backed tests compare lifecycle and signal ordering directly."

create_if_missing \
  "Finish scene-aware signal dispatch parity" \
  2 \
  "runtime,signals,scene,parity" \
  "Replace remaining signal delivery gaps with scene-aware dispatch semantics. Acceptance: scene-connected signals invoke the right targets in Godot-compatible order with regression coverage."

create_if_missing \
  "Compare lifecycle notification traces against oracle output" \
  2 \
  "oracle,lifecycle,traces,parity" \
  "Compare runtime lifecycle notification traces directly against upstream oracle outputs. Acceptance: deterministic trace comparisons fail clearly on ordering drift."

create_if_missing \
  "Compare runtime signal traces against oracle trace output" \
  2 \
  "oracle,signals,traces,parity" \
  "Validate runtime signal traces against upstream trace output. Acceptance: focused trace fixtures cover registration order, arguments, and deferred behavior."

create_if_missing \
  "Expand notification coverage beyond lifecycle basics" \
  3 \
  "notifications,scene,parity" \
  "Broaden notification parity beyond the core lifecycle path. Acceptance: additional fixtures cover important non-lifecycle notification cases and remaining exclusions are documented."

create_if_missing \
  "Connect gdphysics2d to scene nodes and fixed-step runtime" \
  2 \
  "physics,runtime,scene,parity" \
  "Wire gdphysics2d into the scene runtime's fixed-step lifecycle. Acceptance: basic body nodes advance through fixed steps with deterministic regression coverage."

create_if_missing \
  "Sync Node2D body nodes with gdphysics2d world state" \
  2 \
  "physics,node2d,scene,parity" \
  "Keep Node2D transforms and gdphysics2d body state synchronized. Acceptance: physics fixtures verify bidirectional state consistency across steps."

create_if_missing \
  "Advance gdphysics2d from MainLoop fixed-step frames" \
  2 \
  "physics,mainloop,runtime,parity" \
  "Drive gdphysics2d through the runtime fixed-step loop. Acceptance: frame and physics stepping semantics match expected Godot contracts in tests."

create_if_missing \
  "Add collision shape registration and overlap coverage" \
  3 \
  "physics,collision,fixtures,parity" \
  "Register collision shapes through the runtime and verify overlap behavior. Acceptance: deterministic overlap tests cover registration, updates, and query behavior."

create_if_missing \
  "Add CharacterBody2D and StaticBody2D behavior fixtures" \
  3 \
  "physics,characterbody2d,fixtures,parity" \
  "Add focused fixtures for CharacterBody2D and StaticBody2D behavior. Acceptance: movement, blocking, and collision expectations are covered by reproducible tests."

create_if_missing \
  "Add deterministic physics trace goldens" \
  3 \
  "physics,goldens,traces,parity" \
  "Record deterministic physics trace goldens for the integrated runtime path. Acceptance: physics traces compare cleanly against checked-in golden artifacts."

create_if_missing \
  "Add physics_playground golden trace fixture" \
  3 \
  "physics,fixtures,goldens,parity" \
  "Create a physics_playground fixture with golden trace output. Acceptance: runtime traces from the playground compare directly against the fixture baseline."

create_if_missing \
  "Expose engine-owned input snapshot and routing API" \
  2 \
  "input,runtime,api,parity" \
  "Centralize input collection in the runtime and expose an engine-owned snapshot API. Acceptance: tests verify routing and snapshot semantics without demo-local shortcuts."

create_if_missing \
  "Cover keyboard action snapshots through engine input API" \
  3 \
  "input,keyboard,actions,parity" \
  "Add regression tests for keyboard action snapshots through the engine-owned input API. Acceptance: action press/release snapshots match expected runtime state."

create_if_missing \
  "Add input-map loading and action binding coverage" \
  3 \
  "input,inputmap,bindings,parity" \
  "Verify input-map loading and action binding semantics. Acceptance: fixtures cover action registration, lookup, and runtime snapshot use."

create_if_missing \
  "Add mouse position and button routing to input snapshots" \
  3 \
  "input,mouse,runtime,parity" \
  "Broaden input snapshot coverage to mouse motion and button routing. Acceptance: tests verify runtime-facing mouse position and click state."

create_if_missing \
  "Integrate resource UID and cache behavior into loader paths" \
  2 \
  "resources,uid,cache,parity" \
  "Unify resource UID and cache behavior through the real loader path. Acceptance: res:// and UID loads resolve consistently with integration coverage."

create_if_missing \
  "Resolve res:// and UID lookups through one loader path" \
  2 \
  "resources,uid,loader,parity" \
  "Route res:// and UID lookups through one consistent loader implementation. Acceptance: repeated-load and mixed-lookup tests verify shared semantics."

create_if_missing \
  "Add repeated-load cache deduplication regression tests" \
  3 \
  "resources,cache,tests,parity" \
  "Add regression tests for repeated-load deduplication. Acceptance: repeated resource loads preserve expected sharing and invalidation behavior."

create_if_missing \
  "Close object/property reflection gaps in gdobject" \
  2 \
  "gdobject,reflection,properties,parity" \
  "Close remaining object and property reflection gaps in gdobject. Acceptance: measurable reflection parity tests cover core runtime classes and properties."

create_if_missing \
  "Handle ext-resource and subresource edge cases in PackedScene loading" \
  2 \
  "packedscene,resources,loader,parity" \
  "Cover ext-resource and subresource edge cases in PackedScene loading. Acceptance: focused scene-loading tests verify edge-case behavior and remaining exclusions."

create_if_missing \
  "Validate PackedScene instancing ownership and unique-name behavior" \
  2 \
  "packedscene,ownership,uniquename,parity" \
  "Verify PackedScene instancing ownership and unique-name semantics. Acceptance: instancing tests cover ownership propagation and unique-name lookup behavior."

create_if_missing \
  "Implement measurable ClassDB parity for core runtime classes" \
  2 \
  "classdb,api,oracle,parity" \
  "Add measurable ClassDB parity coverage for core runtime classes. Acceptance: API signature probes and runtime tests agree on the supported core class surface."

create_if_missing \
  "Expand apps/godot probes for API and resource validation" \
  3 \
  "godot-lab,oracle,api,resources" \
  "Broaden apps/godot probe coverage for API and resource validation. Acceptance: machine-readable probe outputs cover the next missing runtime parity surfaces."

create_if_missing \
  "Probe ClassDB and node API signatures from apps/godot" \
  3 \
  "godot-lab,classdb,api,oracle" \
  "Extract ClassDB and node API signatures from apps/godot. Acceptance: probe outputs can be compared directly against Patina runtime surfaces."

create_if_missing \
  "Probe resource metadata and roundtrip behavior from apps/godot" \
  3 \
  "godot-lab,resources,oracle,parity" \
  "Probe upstream resource metadata and roundtrip behavior from apps/godot. Acceptance: structured outputs capture metadata and roundtrip expectations used by runtime tests."

create_if_missing \
  "Automate API extraction from pinned upstream Godot" \
  3 \
  "godot-lab,api,automation,oracle" \
  "Automate API extraction from the pinned upstream Godot source. Acceptance: a reproducible command refreshes extracted API artifacts used by parity checks."

create_if_missing \
  "Measure one end-to-end 2D vertical slice from fixtures" \
  3 \
  "render,2d,fixtures,parity" \
  "Measure one complete 2D vertical slice from fixtures. Acceptance: one representative scene exercises runtime, render, and fixture comparison end to end."

create_if_missing \
  "Validate 2D draw ordering, visibility, and layer semantics" \
  3 \
  "render,2d,layers,parity" \
  "Add measurable 2D draw ordering, visibility, and layer semantics coverage. Acceptance: renderer fixtures compare those behaviors against expected output."

create_if_missing \
  "Extend camera and viewport render parity coverage" \
  3 \
  "render,camera,viewport,parity" \
  "Broaden render parity with camera and viewport coverage. Acceptance: focused fixtures verify transforms, culling, and viewport composition behavior."

create_if_missing \
  "Cover texture draw and sprite property parity in renderer fixtures" \
  3 \
  "render,textures,sprites,parity" \
  "Expand renderer fixtures to cover texture drawing and sprite property parity. Acceptance: sprite and texture property changes affect output as expected in goldens."

create_if_missing \
  "Add CI execution path for render golden tests" \
  3 \
  "ci,render,goldens,parity" \
  "Add a CI execution path for render golden tests. Acceptance: the render golden suite runs reproducibly in CI with clear stale-artifact handling."

create_if_missing \
  "Add render benchmark fixtures and reporting" \
  3 \
  "render,benchmarks,reporting" \
  "Add render benchmark fixtures and reporting. Acceptance: benchmark fixtures run deterministically and publish actionable performance summaries."
