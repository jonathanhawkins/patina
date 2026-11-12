---
name: swarm-monitor
description: Monitor the orchestrator and all worker agents — shows health, per-worker state, live pane snapshots, inbox status, and issue detection.
argument-hint: [session-name]
trigger: "monitor swarm", "check workers", "swarm status", "worker status", "monitor orchestrator"
---

# Swarm Monitor

Full dashboard of orchestrator health + all worker states + issue detection. Works for any number of workers.

## Steps

### 1. Detect session

If `$ARGUMENTS` provides a session name, use it. Otherwise auto-detect — prefer `patina-fly` over other patina sessions:
```bash
tmux list-sessions -F '#{session_name}' 2>/dev/null | grep -E '^patina-fly$' || tmux list-sessions -F '#{session_name}' 2>/dev/null | grep -E 'patina' | head -1
```
Store as `$SESSION`. If no session found, report that no swarm is running and stop.

### 2. Check coordinator process is alive (CRITICAL)

```bash
ps aux | grep 'patina-orchestrator' | grep -v grep | grep -v tail
```

If NO `patina-orchestrator run` process is found, this is the **#1 problem** — flag it immediately as **COORDINATOR DOWN**. Without the coordinator loop, no completions are processed and no workers get reassigned.

To restart it:
```bash
AGENT_NAME=IvoryTower ORCH_SESSION=$SESSION \
  /Users/bone/dev/games/patina/apps/orchestrator/crate/target/release/patina-orchestrator \
  run --session $SESSION --project-root /Users/bone/dev/games/patina \
  >> /tmp/patina-orchestrator.log 2>&1 &
```

Also check for stale orchestrator processes with CLOSE_WAIT sockets:
```bash
lsof -i :8765 2>/dev/null | grep CLOSE_WAIT
```
Kill any found — they cause connection reset errors.

### 3. Run orchestrator health + worker-state (parallel)

Run both in parallel, always using absolute paths:

```bash
/Users/bone/dev/games/patina/apps/orchestrator/crate/target/release/patina-orchestrator health --session "$SESSION" --project-root /Users/bone/dev/games/patina 2>&1
```

```bash
/Users/bone/dev/games/patina/apps/orchestrator/crate/target/release/patina-orchestrator worker-state --session "$SESSION" --project-root /Users/bone/dev/games/patina 2>&1
```

### 4. Bead queue status

```bash
br summary 2>/dev/null || echo "br not available"
```

### 5. Capture live pane snapshots for each worker

Get all worker pane snapshots in one command (faster than parallel calls):
```bash
for p in $(tmux list-panes -t "$SESSION:0" -F '#{pane_index}' | awk '$1 >= 3'); do echo "=== PANE $p ==="; tmux capture-pane -t "$SESSION:0.$p" -p | tail -8; echo; done
```

### 6. Check coordinator log (monitor pane)

```bash
tmux capture-pane -t "$SESSION:0.0" -p | tail -10
```
Check the timestamp of the last log entry — if it's more than 30 seconds old, the coordinator may be stuck or dead.

### 7. Check for stale cargo test processes

```bash
ps aux | grep 'cargo test.*patina' | grep -v grep | wc -l
```
Stale `cargo test` processes from verification can block the cargo workspace lock, preventing workers from compiling. If workers are all idle and cargo tests are running from old verification cycles, they should be killed.

## Display Format

Present a dashboard like this:

```
## Swarm Dashboard — $SESSION

### Health
| Metric              | Value |
|---------------------|-------|
| Open beads          | N     |
| In progress         | N     |
| Closed              | N     |
| Ready (unassigned)  | N     |
| Worker panes        | N     |
| Active workers      | N     |
| Idle workers        | N     |

### Workers (N total)
| Pane | Name         | State    | Bead       | Age   | Snippet                    |
|------|--------------|----------|------------|-------|----------------------------|
| 3    | GreenCastle  | Active   | pat-abc12  | 4m    | ✢ Reading 3 files...       |
| 4    | BlueLake     | Idle     | —          | —     | waiting for next assignment |
| ...  | ...          | ...      | ...        | ...   | ...                        |

### Issues Detected
- (list any problems found)
```

## Issue Detection Rules

Flag these issues with clear labels:

1. **COORDINATOR DOWN** (CRITICAL) — no `patina-orchestrator run` process found. This is the most common failure mode. Without it, nothing works. Fix: restart with `AGENT_NAME=IvoryTower ORCH_SESSION=$SESSION patina-orchestrator run --session $SESSION`
2. **COORDINATOR STUCK** — coordinator process exists but log timestamp is >30s old. Usually means a `cargo test` verification is blocking. Check for stale cargo processes.
3. **CARGO LOCK JAM** — stale `cargo test` processes holding the workspace lock. Workers can't compile. Fix: `pkill -f 'cargo test.*patina'`
4. **STALL** — `idle_assigned_panes >= 3` AND `active_assigned_panes <= 1` → workers have assignments but aren't working
5. **BACKLOG LOW** — `ready_unassigned < 3` → running out of work to assign
6. **DEAD PANE** — any worker with state showing dead/no process → pane crashed
7. **STUCK INPUT** — worker waiting for user confirmation (permission prompt or "How is Claude doing?" feedback dialog). Fix feedback prompt: `tmux send-keys -t $SESSION:0.$PANE "0" Enter`
8. **STALE ASSIGNMENT** — `assignment_age_seconds > 600` (10 min) AND worker is Idle → assignment was lost
9. **INBOX BACKLOG** — more than 10 unacknowledged `ack_required` messages → coordinator not processing completions
10. **CLOSE_WAIT SOCKET** — stale orchestrator process with dead TCP connection to agent-mail. Causes "Connection reset by peer" errors. Fix: kill the stale PID.
11. **ALL IDLE** — every worker is idle AND there are open beads → assignment cycle may not be running

If no issues found, say: "No issues detected."

## Related Skills

- **`/editor-parity [area]`** — Visual comparison of Patina editor vs Godot 4.6.1. Use alongside swarm-monitor to verify editor work quality.
- **`/planner`** — Analyze progress, create beads for gaps. Now includes editor parity phase.
