---
name: "planner"
description: "Analyze V1 progress, identify gaps, create beads, and update exit criteria. Intended for periodic planner cycles under either Claude or Codex launchers."
argument-hint: "[--dry-run] [--force]"
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
- **Skip the cycle** if ALL of these are true:
  1. `OPEN >= ACTIVE * 2`
  2. `$ARGUMENTS` does not contain `--force`
- When skipping, append to `prd/planner_log.md`:
  ```
  ## YYYY-MM-DD HH:MM UTC — SKIPPED (queue healthy: N open, M in-progress)
  ```
  Then stop.

1. **Run the planner binary** and capture JSON output:
```bash
/Users/bone/dev/games/patina/apps/orchestrator/crate/target/release/patina-orchestrator plan
```
Parse the JSON. It contains parity, phase, gates, and recommendations.

2. **Check for dry-run mode**. If `$ARGUMENTS` contains `--dry-run`, skip bead creation and exit-criteria edits.

3. **Update exit criteria for newly-passing gates** in `prd/V1_EXIT_CRITERIA.md`. Replace matching unchecked `- [ ]` lines with `- [x]`.

4. **Create beads for recommendations**. Before each create, verify there is no existing bead:
```bash
br search --title "TITLE" --json --status all
```
Only create if no result exists:
```bash
br create --title "TITLE" --type task --priority PRIORITY --labels "LABELS" --description "IMPLEMENT the feature: TITLE\n\nAcceptance: ACCEPTANCE\n\n[planner-key: KEY]" --no-auto-import
```

5. **Log the cycle** in `prd/planner_log.md`:
```
## YYYY-MM-DD HH:MM UTC
- Parity: XX.X%
- Gates: N passing / M total
- Phase: PHASE
- Criteria checked off: [list or "none"]
- Beads created: [list or "none"]
```

6. **Handle V1 completion**. If `phase == "V1Complete"`, add a `## V1 COMPLETE` log entry and stop creating beads.

## Error Handling

- If the binary is missing or fails, report the error and stop.
- If a `br` command fails, log it and continue where reasonable.
- Always leave a planner log entry for non-trivial runs.
