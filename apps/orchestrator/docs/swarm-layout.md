# Swarm Layout

Patina's orchestrator supports a few standard visible swarm walls:

- `2x2` worker grid
- `3x3` worker grid
- `4x4` worker grid
- `5x5` worker grid

Each layout reserves:

- pane `0` for the durable boss console, which launches a fresh `codex` session when available and falls back to a plain shell otherwise
- pane `1` for the beads viewer (`bv`)
- the remaining panes for workers

The left side is resized relative to the current tmux window width:

- boss pane: ~16% of the window, with a minimum width of 28 columns and a soft cap of 38
- beads/detail pane: targets ~72-85 columns for readable bead titles; otherwise ~33% width with a minimum of 50 columns and a cap of 85

The worker area is then tiled into the requested grid.

## Commands

Launch a new wall:

```bash
patina-orchestrator launch --session patina-fly --workers 9 --with-coordinator --force
```

Launch a larger wall:

```bash
patina-orchestrator launch --session patina-fly-16 --workers 16 --with-coordinator --force
patina-orchestrator launch --session patina-fly-25 --workers 25 --with-coordinator --force
```

## Notes

- The launcher assumes workers run in the repo root by default.
- The default worker command is:
  `claude --model opus --dangerously-skip-permissions`
- You can override the worker command when launching:

```bash
patina-orchestrator launch --session patina-fly --workers 4 --model "claude --model sonnet" --with-coordinator --force
```

## Fly Mode

To launch a worker wall plus a dedicated coordinator window that continuously polls
for completions and refills workers:

```bash
patina-orchestrator launch --session patina-fly --workers 9 --with-coordinator --force
patina-orchestrator launch --session patina-fly-25 --workers 25 --with-coordinator --force
```
