---
name: "flywheel-start"
description: "Bootstrap the entire agent flywheel from scratch - build orchestrator, start agent mail, launch tmux swarm, seed beads, verify health."
argument-hint: "[grid] [session-name] [model-command]"
---

# Start Flywheel

Bootstrap the Patina swarm from a cold start to a working session with coordinator, planner, and workers.

## Arguments

- `$ARGUMENTS[0]` — grid size, default `3x3`
- `$ARGUMENTS[1]` — tmux session name, default `patina-fly`
- `$ARGUMENTS[2]` — model command, default `codex --model gpt-5.4`

## Steps

1. Parse the arguments and compute worker count from the grid:
```bash
GRID="${1:-3x3}"
SESSION="${2:-patina-fly}"
MODEL_CMD="${3:-codex --model gpt-5.4}"
ROWS="${GRID%%x*}"
COLS="${GRID##*x}"
WORKERS=$((ROWS * COLS))
```

2. Check for an existing tmux session and stop if replacement would be destructive unless the user explicitly asked for a fresh session:
```bash
tmux has-session -t "=$SESSION" 2>/dev/null
tmux has-session -t "=${SESSION}--agent-mail" 2>/dev/null
```

3. Verify prerequisites:
```bash
command -v cargo
command -v tmux
command -v br
command -v bv
command -v curl
```

4. Build the orchestrator release binary:
```bash
cd /Users/bone/dev/games/patina/apps/orchestrator/crate && cargo build --release
```

5. Check queue health:
```bash
br count --by-status --json --no-auto-import --allow-stale
```
If there are no open or in-progress beads, note that workers will come up idle until the planner skill is run with `--force` to seed work.

6. Launch the swarm:
```bash
/Users/bone/dev/games/patina/apps/orchestrator/crate/target/release/patina-orchestrator launch \
  --session "$SESSION" \
  --workers "$WORKERS" \
  --model "$MODEL_CMD" \
  --project-root /Users/bone/dev/games/patina \
  --with-coordinator \
  --interval 8 \
  --force
```

7. Wait briefly, then verify health:
```bash
sleep 15
/Users/bone/dev/games/patina/apps/orchestrator/crate/target/release/patina-orchestrator health --session "$SESSION" --project-root /Users/bone/dev/games/patina
```

8. If the queue was empty, run the planner skill with `--force`. Otherwise trigger one immediate assignment cycle:
```bash
/Users/bone/dev/games/patina/apps/orchestrator/crate/target/release/patina-orchestrator assign --session "$SESSION" --project-root /Users/bone/dev/games/patina
```

## Notes

- Default workers use Codex because this Codex-native skill is meant to launch a Codex swarm.
- To launch Claude workers instead, pass a Claude model command as the third argument, for example `claude --model opus`.
- The launcher is responsible for selecting the correct worker skill for the chosen model command.
