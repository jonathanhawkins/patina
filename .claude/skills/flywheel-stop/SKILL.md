---
name: flywheel-stop
description: Gracefully stop the orchestrator, planner, and all flywheel workers. Sends interrupts, waits for wind-down, retires agents, releases reservations, and tears down tmux sessions.
argument-hint: [session-name]
trigger: "stop flywheel", "stop swarm", "shutdown flywheel", "shutdown swarm", "kill swarm", "stop the workers", "stop orchestrator", "tear down flywheel", "flywheel stop", "graceful stop", "stop everything"
---

# Gracefully Stop Flywheel

Perform an orderly shutdown of the entire agent flywheel: workers, planner, coordinator, and agent-mail server. Every step is designed to let in-flight work finish cleanly before pulling the plug.

## Arguments

- `$ARGUMENTS[0]` — Session name (default: `patina-fly`)

## Steps

### 0. Parse arguments and set defaults

```
SESSION = first argument or "patina-fly"
PROJECT_ROOT = /Users/bone/dev/games/patina
PROJECT_KEY = /Users/bone/dev/games/patina
ORCH_BIN = $PROJECT_ROOT/apps/orchestrator/crate/target/release/patina-orchestrator
```

### 1. Verify the session exists

```bash
tmux has-session -t "=$SESSION" 2>/dev/null && echo "SESSION_EXISTS=yes" || echo "SESSION_EXISTS=no"
tmux has-session -t "=${SESSION}--agent-mail" 2>/dev/null && echo "MAIL_EXISTS=yes" || echo "MAIL_EXISTS=no"
```

If SESSION_EXISTS=no, report "No active session `$SESSION` found" and stop. Nothing to tear down.

### 2. Discover windows and panes dynamically

**Do NOT hardcode window names.** The main window may be named `swarm`, `bv`, or something else depending on the orchestrator version. Discover dynamically:

```bash
# List all windows
tmux list-windows -t "$SESSION" -F '#{window_index} #{window_name}' 2>/dev/null
```

Identify:
- **Main window**: window 0 (whatever its name) — contains monitor, planner, bv, and worker panes
- **Coordinator window**: the window named `coordinator` (usually window 1)

Then list panes in each window using the **window index** (not name):

```bash
# Main window panes (window 0)
tmux list-panes -t "${SESSION}:0" -F '#{pane_id} #{pane_index} #{pane_pid} #{pane_current_command}' 2>/dev/null

# Coordinator window panes
tmux list-panes -t "${SESSION}:coordinator" -F '#{pane_id} #{pane_index} #{pane_pid} #{pane_current_command}' 2>/dev/null
```

From the main window pane list, identify by pane_index:
- **Monitor pane**: index 0 (usually running `sleep`)
- **Planner pane**: index 1 (running `claude`)
- **BV pane**: index 2 (running `bv`)
- **Worker panes**: index >= 3 (running `claude`)

Collect all pane IDs (%N format) for workers and planner. Count workers and report: "Found N worker panes, 1 planner, 1 coordinator"

### 3. Collect session PIDs before killing

Record PIDs of all Claude processes in the session so we can verify they're gone later. This scopes cleanup to THIS session only — never kill Claude processes from other sessions/tabs:

```bash
# Get all PIDs from the session
tmux list-panes -t "${SESSION}:0" -F '#{pane_pid}' 2>/dev/null
tmux list-panes -t "${SESSION}:coordinator" -F '#{pane_pid}' 2>/dev/null
```

Save these as SESSION_PIDS for verification in step 7.

### 4. Send Ctrl-C to all workers and planner (parallel)

Send interrupt to every worker and planner pane. Use pane_id (%N format), not indices:

```bash
for pane in $WORKER_PANE_IDS $PLANNER_PANE_ID; do
  tmux send-keys -t "$pane" C-c 2>/dev/null || true
done
```

### 5. Stop the coordinator

Send Ctrl-C to the coordinator pane. Note: the orchestrator binary has no SIGINT handler, so this is best-effort — the tmux session kill in step 6 is what actually terminates it:

```bash
tmux send-keys -t "${SESSION}:coordinator.0" C-c 2>/dev/null || true
```

### 6. Brief wait, then kill sessions

Wait 3 seconds for the interrupt to land, then kill the tmux sessions. The session kill is what actually terminates the Claude processes — Ctrl-C alone is often not enough:

```bash
sleep 3
tmux kill-session -t "=$SESSION" 2>/dev/null || true
```

### 7. Retire agents from Agent Mail

**Before killing the mail server**, retire agents and release reservations. Try multiple discovery methods since `list_window_identities` may return empty if identities expired:

**Method 1** — list_window_identities:
```
list_window_identities(project_key="/Users/bone/dev/games/patina")
```

**Method 2** — read the coordinator agent file:
```bash
cat /Users/bone/dev/games/patina/.beads/coordinator_agent 2>/dev/null
```

For each discovered agent, call:
```
retire_agent(project_key="/Users/bone/dev/games/patina", agent_name="<name>")
release_file_reservations(project_key="/Users/bone/dev/games/patina", agent_name="<name>")
```

If agent mail is unreachable (server already stopped or health check fails), skip this step and note it in the report.

### 8. Kill agent-mail session

```bash
tmux kill-session -t "=${SESSION}--agent-mail" 2>/dev/null || true
```

### 9. Kill ALL stray orchestrator processes

Use a single broad pattern to catch every subcommand (`run`, `poll`, `assign`, `plan`, `health`, `worker-state`):

```bash
pkill -9 -f "patina-orchestrator" 2>/dev/null || true
pkill -f "cargo test.*patina" 2>/dev/null || true
```

### 10. Clean up lock files

```bash
rm -f /Users/bone/dev/games/patina/.codex/orchestrator/coordinator.lock
rm -f /Users/bone/dev/games/patina/.beads/ORCH_LOCK
rm -f /Users/bone/dev/games/patina/.claude/scheduled_tasks.lock
```

### 11. Clean up stale CLOSE_WAIT sockets

```bash
lsof -i :8765 2>/dev/null | grep CLOSE_WAIT | awk '{print $2}' | sort -u | xargs kill 2>/dev/null || true
```

### 12. Verify clean shutdown

```bash
# Confirm sessions are gone
tmux has-session -t "=$SESSION" 2>/dev/null && echo "SESSION_STILL_EXISTS" || echo "SESSION_GONE"
tmux has-session -t "=${SESSION}--agent-mail" 2>/dev/null && echo "MAIL_STILL_EXISTS" || echo "MAIL_GONE"

# Confirm no stray orchestrator processes
ps aux | grep 'patina-orchestrator' | grep -v grep | head -5

# Confirm session PIDs are gone (from step 3)
# For each PID in SESSION_PIDS: ps -p $PID >/dev/null 2>&1 && echo "STILL_ALIVE: $PID" || echo "GONE: $PID"
```

If any session PIDs are still alive, report them as a warning — do NOT blindly kill them since they may have been adopted by another process.

### 13. Report results

Present a shutdown summary:

```
## Flywheel Stopped

| Component         | Status     |
|-------------------|------------|
| Workers (N)       | interrupted + session killed |
| Planner           | interrupted + session killed |
| Coordinator       | interrupted + killed         |
| Agent Mail agents | retired (M agents)           |
| File reservations | released                     |
| tmux sessions     | destroyed                    |
| Lock files        | cleaned                      |
| Stray processes   | killed                       |

### To restart
- `/flywheel-start [grid] [session-name]`
```

## Error handling

- If agent mail is already down, skip steps 7 and 11 — just note "agent mail was not running"
- If tmux session doesn't exist, report "nothing to stop" and exit early
- If pane sends fail (pane already dead), continue — this is expected during shutdown
- Never use `kill -9` on Claude processes directly — killing the tmux session handles that
- Only scope process cleanup to the session's own PIDs — never kill Claude processes from other sessions

## Related skills

- **`/flywheel-start [grid] [session]`** — Bootstrap a new flywheel from scratch
- **`/swarm-monitor [session]`** — Monitor swarm health (while running)
- **`/deploy-orchestrator`** — Rebuild + hot-deploy the orchestrator binary
