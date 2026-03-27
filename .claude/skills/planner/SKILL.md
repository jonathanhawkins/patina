---
name: planner
description: Analyze V1 progress, identify gaps, create beads, update exit criteria. Run via /loop 10m /planner.
argument-hint: [--dry-run] [--force]
---

# V1 Planner Cycle

Analyze engine progress, update exit criteria, and create beads for gaps.

## Steps

0. **Queue health gate** — before doing anything, check if running is worthwhile:
```bash
br count --by-status --json --no-auto-import --allow-stale
```
Parse the JSON. Extract `open` and `in_progress` counts from the `groups` array.
- Let `OPEN` = count where group == "open", `ACTIVE` = count where group == "in_progress".
- **Skip the cycle** (log a one-line note and exit) if ALL of these are true:
  1. `OPEN >= ACTIVE * 2` (agents have plenty of queued work)
  2. No `$ARGUMENTS` contains `--force` (user override)
- When skipping, append to `prd/planner_log.md`:
  ```
  ## YYYY-MM-DD HH:MM UTC — SKIPPED (queue healthy: N open, M in-progress)
  ```
  Then stop — do not run steps 1–6.

1. **Run the planner binary** and capture JSON output:
```bash
/Users/bone/dev/games/patina/apps/orchestrator/crate/target/release/patina-orchestrator plan
```
Parse the JSON. It contains: `parity_pct`, `phase`, `gates` (array of `{key, title, passing, criteria_line}`), and `recommendations` (array of `{key, title, priority, labels, acceptance, criteria_line}`).

2. **Check for dry-run mode**: If `$ARGUMENTS` contains `--dry-run`, skip steps 3 and 4 (analysis and logging only).

3. **Update exit criteria for newly-passing gates**: For each gate where `passing == true`, use the Edit tool on `prd/V1_EXIT_CRITERIA.md` to replace the exact `- [ ]` checkbox line (from `criteria_line`) with `- [x]`. Only edit lines that currently have `- [ ]`.

4. **Create beads for recommendations**: The binary already deduplicates against ALL existing beads (including closed ones), so its recommendations are safe to create. But as a belt-and-suspenders check, for each recommendation verify no bead exists with `br search --title "TITLE" --json --status all`. If any result is returned (open, closed, or in-progress), skip creation. Only create if truly no bead exists:
```bash
br create --title "TITLE" --type task --priority PRIORITY --labels "LABELS" --description "IMPLEMENT the feature: TITLE\n\nAcceptance: ACCEPTANCE\n\n[planner-key: KEY]" --no-auto-import
```

5. **Log the cycle**: Append to `prd/planner_log.md` using the Edit tool (or create if missing):
```
## YYYY-MM-DD HH:MM UTC
- Parity: XX.X%
- Gates: N passing / M total
- Phase: PHASE
- Criteria checked off: [list or "none"]
- Beads created: [list or "none"]
```

6. **Handle V1 completion**: If `phase == "V1Complete"`, add a `## V1 COMPLETE` entry to the log and print a congratulatory summary. No further beads needed.

## Editor Parity Phase

When V1 runtime is complete (`phase == "V1Complete"`), the planner shifts focus to **editor parity**. The gate for editor work is now open (runtime parity 100%, 41/41 gates).

### Available skills for editor work

- **`/editor-parity [area]`** — Visual comparison of Patina editor vs Godot 4.6.1. Screenshots both editors, compares layout/controls/styling across 5 dimensions, rates gaps as P1/P2/P3, and auto-creates beads. Areas: `scene tree`, `inspector`, `viewport`, `toolbar`, `filesystem`, `full`. Use this to measure progress and find new gaps.

- **`/swarm-monitor [session]`** — Monitor orchestrator health and all worker agents. Shows per-worker state, live pane snapshots, and detects issues (stalls, dead panes, coordinator down, cargo lock jams). Use this to keep the swarm healthy while it works editor beads.

### Editor parity beads structure

Editor work is organized into 18 lanes (see `prd/EDITOR_PARITY_BEADS.md`):
1. Scene Tree parity: node operations and hierarchy workflows
2. Scene Tree parity: indicators, badges, and selection state
3. Inspector parity: resource toolbar, history, and object navigation
4. Inspector parity: core property editing and interaction
5. Inspector parity: advanced property organization and exported script fields
6. Viewport parity: selection modes, zoom/pan, and viewport controls
7. Viewport parity: transform gizmos and pivot workflows
8. Viewport parity: snapping, guides, rulers, grid, and canvas overlays
9. Top bar parity: scene tabs, run controls, and editor mode switching
10. Menu parity: scene/project/debug/editor/help actions
11-18. Create Node dialog, bottom panels, script editor, FileSystem dock, signals dock, animation editor, editor systems

### Editor parity testing (3 layers)

1. **Layer 1 — API behavioral parity** (P2): `editor_inspector_probe.gd` captures Godot inspector ground truth → `editor_api_behavioral_parity_test.rs` compares Patina REST API responses against golden JSON
2. **Layer 2 — DOM structure parity** (P3): `editor_dom_parity_test.rs` verifies HTML has correct elements, icons, panel layout. `/api/ui/state` endpoint enables programmatic UI queries.
3. **Layer 3 — Viewport rendering** (P3): `editor_viewport_golden_test.rs` pixel-diffs Patina viewport renders against golden PNGs

### When creating editor beads

- Label all editor beads with `editor`
- Reference the lane number in the description
- P1 = broken functionality, P2 = missing feature, P3 = visual polish
- Run `/editor-parity` periodically to measure convergence

## Error Handling

- If the orchestrator binary is missing or fails, report the error and skip the cycle.
- If `br` commands fail, log the failure but continue with remaining steps.
- Always write the log entry, even if other steps partially failed.
