---
name: flywheel-start
description: Bootstrap the entire agent flywheel from scratch — build orchestrator, start agent mail, launch tmux swarm, seed beads, verify health. One command to go from zero to a fully operational multi-agent swarm.
argument-hint: [grid] [session-name]
trigger: "start flywheel", "launch flywheel", "flywheel from scratch", "start swarm", "launch swarm", "bootstrap flywheel", "spin up flywheel", "start the fly wheel"
---

# Start Flywheel From Scratch

Bootstrap the entire agent flywheel — from a cold start to a fully operational multi-agent swarm with coordinator, planner, workers, and bead queue. This skill handles every prerequisite so you don't have to remember the sequence.

## Arguments

- `$ARGUMENTS[0]` — Grid size (default: `3x3`). Examples: `2x2`, `3x3`, `4x4`, `5x5`
- `$ARGUMENTS[1]` — Session name (default: `patina-fly`)

## Steps

### 0. Parse arguments and set defaults

```
GRID = first argument or "3x3"
SESSION = second argument or "patina-fly"
PROJECT_ROOT = /Users/bone/dev/games/patina
ORCH_BIN = $PROJECT_ROOT/apps/orchestrator/crate/target/release/patina-orchestrator
```

### 1. Kill stale sessions (if any)

Check for leftover tmux sessions that would block a fresh start:

```bash
tmux has-session -t "=$SESSION" 2>/dev/null && echo "EXISTING_SESSION=yes" || echo "EXISTING_SESSION=no"
tmux has-session -t "=${SESSION}--agent-mail" 2>/dev/null && echo "EXISTING_MAIL=yes" || echo "EXISTING_MAIL=no"
```

If either exists, **ask the user** before killing:
> "Found existing session `$SESSION` (and/or `$SESSION--agent-mail`). Kill them and start fresh?"

If the user confirms:
```bash
tmux kill-session -t "=$SESSION" 2>/dev/null || true
tmux kill-session -t "=${SESSION}--agent-mail" 2>/dev/null || true
```

Also kill any stale orchestrator processes and cargo locks:
```bash
pkill -f "patina-orchestrator run" 2>/dev/null || true
pkill -f "cargo test.*patina" 2>/dev/null || true
```

Clean up stale CLOSE_WAIT sockets on the agent-mail port:
```bash
lsof -i :8765 2>/dev/null | grep CLOSE_WAIT | awk '{print $2}' | sort -u | xargs -r kill 2>/dev/null || true
```

### 2. Verify prerequisites

Check that required tools exist before building anything:

```bash
command -v cargo && command -v tmux && command -v br && command -v bv && command -v python3 && command -v curl && echo "ALL_OK" || echo "MISSING_TOOLS"
```

If any tool is missing, report which ones and stop. The required tools are:
- `cargo` — Rust toolchain (builds the orchestrator)
- `tmux` — terminal multiplexer (hosts the swarm)
- `br` — beads_rust CLI (task tracker)
- `bv` — beads viewer TUI (prioritization)
- `python3` — agent mail server runtime
- `curl` — agent mail health checks

### 3. Build the orchestrator binary (release)

```bash
cd /Users/bone/dev/games/patina/apps/orchestrator/crate && cargo build --release 2>&1 | tail -20
```

If the build fails, stop and show the error. Do not proceed without a working binary.

Verify the binary exists:
```bash
ls -la /Users/bone/dev/games/patina/apps/orchestrator/crate/target/release/patina-orchestrator
```

### 4. Check bead queue health

Ensure there are beads to work on:

```bash
br count --by-status --json --no-auto-import --allow-stale 2>/dev/null
```

Parse the JSON output. Report:
- Total open beads
- Total in-progress beads
- Total closed beads
- Ready (unassigned) count

If open + in_progress == 0, warn the user:
> "No open beads in the queue. The swarm will launch but workers will be idle. Run `/planner --force` after launch to seed work, or create beads with `br create`."

### 5. Launch the flywheel

Compute worker count from grid dimensions (e.g. 3x3 = 9), then run the Rust orchestrator directly. The `--with-coordinator` flag creates a coordinator window running the `run` loop.

```bash
# Compute workers from grid (e.g. GRID=3x3 → ROWS=3, COLS=3, WORKERS=9)
ROWS="${GRID%%x*}"
COLS="${GRID##*x}"
WORKERS=$((ROWS * COLS))

/Users/bone/dev/games/patina/apps/orchestrator/crate/target/release/patina-orchestrator launch \
  --session "$SESSION" \
  --workers "$WORKERS" \
  --model "claude --model opus --dangerously-skip-permissions" \
  --project-root /Users/bone/dev/games/patina \
  --with-coordinator \
  --interval 8 \
  --force
```

This command:
1. Ensures the Agent Mail server is running (or starts it)
2. Creates the tmux session with the 3-column layout (monitor, planner, bv, worker grid)
3. Bootstraps agent identities for each worker pane
4. Launches all workers with Claude
5. Starts the planner with `/loop 10m /planner`
6. Creates a `coordinator` window running `patina-orchestrator run --session $SESSION --interval 8`
7. Focuses on the swarm window

If the launch fails, check:
- Did the agent mail server fail to start? → Check `tmux capture-pane -t "${SESSION}--agent-mail" -p | tail -20`
- Did the binary crash? → Check stderr output
- Session name conflict? → Step 1 should have cleaned this up

### 6. Verify swarm health (wait 15s for workers to boot)

Wait ~15 seconds for Claude instances to initialize, then verify:

```bash
sleep 15
```

**6a. Verify coordinator is running:**
```bash
ps aux | grep 'patina-orchestrator run' | grep -v grep
```
If not running, this is critical — check the coordinator window:
```bash
tmux capture-pane -t "${SESSION}:coordinator.0" -p | tail -20
```

**6b. Verify agent mail is healthy:**
```bash
curl -fsS --connect-timeout 2 --max-time 5 http://localhost:8765/health/readiness 2>&1
```

**6c. Count live worker panes:**
```bash
tmux list-panes -t "${SESSION}:swarm" -F '#{pane_index} #{pane_pid}' 2>/dev/null | awk '$1 >= 3' | wc -l
```
Should match the expected worker count from the grid.

**6d. Quick health check via orchestrator:**
```bash
/Users/bone/dev/games/patina/apps/orchestrator/crate/target/release/patina-orchestrator health --session "$SESSION" --project-root /Users/bone/dev/games/patina 2>&1
```

### 7. Report results

Present a launch summary:

```
## Flywheel Launched

| Component         | Status |
|-------------------|--------|
| Orchestrator      | built (release) |
| Agent Mail        | running on :8765 |
| Session           | $SESSION |
| Grid              | $GRID |
| Workers           | N alive / M expected |
| Coordinator       | running (8s interval) |
| Planner           | running (/loop 10m /planner) |
| Bead queue        | N open, M in-progress, K ready |

### Next steps
- Attach: `tmux attach -t $SESSION`
- Monitor: `/swarm-monitor $SESSION`
- Seed work: `/planner --force` (if queue is empty)
- Check inbox: `/mail-inbox`
```

### 8. Trigger initial assignment cycle (if beads are available)

If there are open/ready beads, kick off an immediate assignment so workers don't sit idle waiting for the first poll cycle:

```bash
ORCH_SESSION=$SESSION /Users/bone/dev/games/patina/apps/orchestrator/crate/target/release/patina-orchestrator assign --session "$SESSION" --project-root /Users/bone/dev/games/patina 2>&1
```

## Troubleshooting

If the skill encounters problems at any step, provide targeted advice:

| Problem | Likely cause | Fix |
|---------|-------------|-----|
| `cargo build` fails | Dependency issue or syntax error | Read the compiler output, fix code, retry |
| Agent Mail won't start | Port 8765 in use, or Python deps missing | `lsof -i :8765` to find blocker; `cd mcp_agent_mail && pip install -r requirements.txt` |
| Workers show "permission denied" | Missing `--dangerously-skip-permissions` | The launch command adds this automatically — check MODEL_CMD |
| Coordinator dies immediately | Binary panic or config error | `tmux capture-pane -t $SESSION:coordinator.0 -p` |
| Workers all idle after 30s | No beads in queue or assignment not running | Run `/planner --force` then `patina-orchestrator assign` |
| `br` not found | beads_rust not installed | `cargo install --path apps/orchestrator/crate` or check PATH |

## Related skills

- **`/swarm-monitor [session]`** — Continuous health monitoring after launch
- **`/deploy-orchestrator`** — Rebuild + hot-deploy the orchestrator binary without relaunching
- **`/planner [--force]`** — Analyze progress and seed new beads
- **`/mail-session`** — Bootstrap your own agent mail identity (for manual work alongside the swarm)
