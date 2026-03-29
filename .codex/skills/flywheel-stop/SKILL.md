---
name: "flywheel-stop"
description: "Gracefully stop the orchestrator, planner, and workers; retire mail agents; tear down tmux sessions."
argument-hint: "[session-name]"
---

# Stop Flywheel

Shut down a running Patina swarm cleanly and verify teardown.

## Arguments

- `$ARGUMENTS[0]` — tmux session name, default `patina-fly`

## Steps

1. Resolve the session name and confirm it exists:
```bash
tmux has-session -t "=$SESSION" 2>/dev/null
tmux has-session -t "=${SESSION}--agent-mail" 2>/dev/null
```
If the main session does not exist, report that there is nothing to stop.

2. Discover panes:
```bash
tmux list-panes -t "${SESSION}:0" -F '#{pane_id} #{pane_index} #{pane_pid} #{pane_current_command}'
tmux list-panes -t "${SESSION}:coordinator" -F '#{pane_id} #{pane_index} #{pane_pid} #{pane_current_command}' 2>/dev/null
```
Treat pane indices `>= 3` in window `0` as workers and pane `1` as planner.

3. Send `Ctrl-C` to workers, planner, and coordinator:
```bash
for pane in $WORKER_PANE_IDS $PLANNER_PANE_ID; do
  tmux send-keys -t "$pane" C-c 2>/dev/null || true
done
tmux send-keys -t "${SESSION}:coordinator.0" C-c 2>/dev/null || true
```

4. Wait briefly, then kill the tmux sessions:
```bash
sleep 3
tmux kill-session -t "=$SESSION" 2>/dev/null || true
tmux kill-session -t "=${SESSION}--agent-mail" 2>/dev/null || true
```

5. Retire coordinator and worker agents if Agent Mail is still reachable. Use the coordinator discovery file when available:
```bash
cat /Users/bone/dev/games/patina/.beads/coordinator_agent 2>/dev/null
```
Release any file reservations for agents you can still identify.

6. Clean up stray orchestrator and stale verification processes only if they remain after tmux teardown:
```bash
pkill -f "patina-orchestrator" 2>/dev/null || true
pkill -f "cargo test.*patina" 2>/dev/null || true
```

7. Clean up lock files:
```bash
rm -f /Users/bone/dev/games/patina/.codex/orchestrator/coordinator.lock
rm -f /Users/bone/dev/games/patina/.beads/ORCH_LOCK
rm -f /Users/bone/dev/games/patina/.claude/scheduled_tasks.lock
```

8. Verify shutdown:
```bash
tmux has-session -t "=$SESSION" 2>/dev/null
tmux has-session -t "=${SESSION}--agent-mail" 2>/dev/null
ps aux | grep 'patina-orchestrator' | grep -v grep
```

9. Report a concise teardown summary and mention any leftovers that need manual cleanup.

## Notes

- Prefer graceful tmux teardown over direct process killing.
- If Agent Mail is already down, note that agent retirement was skipped.
