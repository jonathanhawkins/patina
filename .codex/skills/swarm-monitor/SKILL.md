---
name: "swarm-monitor"
description: "Monitor the orchestrator and all worker agents - show health, per-worker state, live pane snapshots, and issue detection."
argument-hint: "[session-name]"
---

# Swarm Monitor

Inspect a running Patina swarm and report the highest-signal health issues first.

## Arguments

- `$ARGUMENTS[0]` — tmux session name, default `patina-fly`

## Steps

1. Resolve the target session. If no session name is provided, prefer `patina-fly`; otherwise detect the first matching Patina session:
```bash
tmux list-sessions -F '#{session_name}' 2>/dev/null
```
If no swarm session exists, report that and stop.

2. Check whether the coordinator loop is alive:
```bash
ps aux | grep 'patina-orchestrator run' | grep -v grep
```
If missing, report `COORDINATOR DOWN` immediately.

3. Run the orchestrator health and worker-state commands:
```bash
/Users/bone/dev/games/patina/apps/orchestrator/crate/target/release/patina-orchestrator health --session "$SESSION" --project-root /Users/bone/dev/games/patina
```
```bash
/Users/bone/dev/games/patina/apps/orchestrator/crate/target/release/patina-orchestrator worker-state --session "$SESSION" --project-root /Users/bone/dev/games/patina
```

4. Check bead queue status:
```bash
br summary
```

5. Capture recent worker output:
```bash
for p in $(tmux list-panes -t "$SESSION:0" -F '#{pane_index}' | awk '$1 >= 3'); do
  echo "=== PANE $p ==="
  tmux capture-pane -t "$SESSION:0.$p" -p | tail -8
  echo
done
```

6. Capture recent coordinator/monitor output:
```bash
tmux capture-pane -t "$SESSION:0.0" -p | tail -10
```

7. Look for common failure modes:
- `COORDINATOR DOWN`: no `patina-orchestrator run`
- `DEAD PANE`: missing worker process
- `STALL`: many assigned-idle workers and few active workers
- `BACKLOG LOW`: ready queue nearly empty
- `ALL IDLE`: every worker idle while open beads still exist
- `CARGO LOCK JAM`: stale `cargo test` processes blocking the workspace

8. Report a compact dashboard:
- session name
- coordinator status
- worker counts
- queue counts
- high-priority issues
- one-line snippets per problematic pane

## Notes

- Prefer issue detection and operator guidance over raw command dumps.
- If no issues are detected, say so explicitly.
