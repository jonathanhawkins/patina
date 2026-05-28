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

1. **Run the planner binary with `--apply --heal`** to analyze, backfill broken legacy beads, AND create new beads in one shot. The binary itself runs `br create` + `br update --acceptance-criteria` per recommendation, and `--heal` scans existing open/in_progress beads, finds ones with missing/null `acceptance_criteria`, and backfills them by mapping their `[planner-key: ...]` markers to template acceptance text:
```bash
/Users/bone/dev/games/patina/apps/orchestrator/crate/target/release/patina-orchestrator plan --apply --heal 2>&1 | tee /tmp/planner-apply.log
```
The `--heal` pass emits `[plan --heal] HEALED <ID> (key=<key>): <title>` lines and a `[plan --heal] summary: healed=N skipped=M failed=K` line. `skipped` covers beads with no `[planner-key:]` marker or whose key has no matching template — these are pre-existing manual beads and are safe to leave alone.
The binary writes `[plan --apply] CREATED <ID>: <title>`, `[plan --apply] SKIP ...`, and a final `summary: created=N skipped=M failed=K` line to stderr, then prints the full JSON report to stdout. Parse the JSON for the analysis fields (top-level keys: `parity`, `phase`, `gates`, `queue`, `recommendations`, `timestamp`). The bead-creation work is already done by the time `--apply` returns.

If `$ARGUMENTS` contains `--dry-run`, drop the `--apply` flag instead — the analysis still runs, but no beads are created and you skip steps 3 and 4 (logging only).

2. **Confirm the apply summary**: Read the `[plan --apply] summary:` line from stderr.
- `failed > 0` → STOP the cycle, log the failure to `prd/planner_log.md`, and surface the failing titles. Do NOT create more beads manually — the binary's two-step `br create` + `br update --acceptance-criteria` pattern is the only supported path.
- `skipped > 0` → log which recommendations were skipped (the stderr lines name the title and `gate_key`). Skips are usually due to empty `acceptance_command` upstream in `planner.rs` templates and should be reported, not patched around.

3. **Update exit criteria for newly-passing gates**: When the planner config uses the new phase-chain schema (`[[phase]] analysis = { source = "criteria" }`), the binary auto-ticks `- [ ]` → `- [x]` for every criterion whose `(test: \`name\`)` test passed during this cycle. The stderr log shows `[plan] phase 'NAME': ticked N criteria from passing tests` when this happens — no Edit pass needed.

   For legacy V1-style configs that do not use phase chains, fall back to the old behavior: for each gate in the JSON where `passing == true`, use the Edit tool on `prd/V1_EXIT_CRITERIA.md` to replace the exact `- [ ]` checkbox line (from `criteria_line`) with `- [x]`. Only edit lines that currently have `- [ ]`.

4. **Spot-check one created bead** (cheap sanity verification — the binary already enforces the contract, this just confirms the DB write landed):
```bash
# Pick the first CREATED line from /tmp/planner-apply.log:
ID=$(grep -m1 '\[plan --apply\] CREATED' /tmp/planner-apply.log | awk '{print $4}' | tr -d ':')
[ -n "$ID" ] && br show "$ID" --json | jq '.[0] | {description, acceptance_criteria}'
```
If `acceptance_criteria` is null or empty, the binary regressed — STOP the cycle and report the bug.

5. **Log the cycle**: Append to `prd/planner_log.md` using the Edit tool (or create if missing). Include the **active phase label** so phase rollover is visible in the log history:
```
## YYYY-MM-DD HH:MM UTC
- Active phase: editor-agent | editor-parity-bootstrap | editor-parity | (other)
- Parity: XX.X%
- Gates: N passing / M total
- Phase: PHASE
- Criteria checked off this cycle: [list or "none"]
- Beads created: [list or "none"]
```

6. **Handle phase rollover**: Phase advancement is now automatic — when all criteria for a phase tick green, the binary moves to the next `[[phase]]` block in `.orchestrator/planner.toml` and starts seeding from there on the next cycle. No manual config edit needed.

   If `phase == "V1Complete"` AND no further phases have unchecked criteria, add a `## ALL PHASES COMPLETE` entry to the log. Otherwise, the planner is still actively driving work in a later phase — keep looping.

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

## Hard Rules

- NEVER wrap `br search` (or `br ready`, `br count`, `br list`) in a python (or any) filter that defines its own match/dedup predicate. Use the shown `jq -r '.issues | length'` extractor and apply the integer rule (`>= 1` skip, `== 0` create).
- NEVER inspect candidate titles to decide if a `br search` hit is "really" a duplicate — any hit is a duplicate, period.
- NEVER run the full Rust test suite (`cargo test`, `cargo nextest`, `rust_task.sh`) from the planner. The verifier lane is the sole builder. The planner reads the bead map + queue counts only.
- NEVER skip the queue-health gate (Step 0) silently — always append the SKIPPED line to `prd/planner_log.md` so the cycle is auditable.
- NEVER write a bead with literal placeholder text (`TITLE`, `PRIORITY`, `LABELS`, `KEY`, or `ACCEPTANCE`) appearing verbatim in any field. Always substitute the recommendation's real values from the JSON.
- NEVER create a bead whose `acceptance_criteria` field is null/empty. If the recommendation's `acceptance_command` is missing, SKIP that recommendation and log the skip — workers without concrete acceptance criteria will invent their own scope and produce broken work (see pat-03sbm root-cause incident).
- ALWAYS run the post-create verification (`br show $ID --json | jq '.[0] | {description, acceptance_criteria}'`) and abort the cycle if either field is empty/placeholder.

## Error Handling

- If the orchestrator binary is missing or fails, report the error and skip the cycle.
- If `br` commands fail, log the failure but continue with remaining steps.
- Always write the log entry, even if other steps partially failed.
