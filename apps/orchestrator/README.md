# Patina Orchestrator

Patina Orchestrator is the swarm-control layer for this monorepo.

It exists to coordinate:

- worker session spawning and layout
- bead assignment and refill
- Agent Mail handoffs and acknowledgements
- `br` single-writer mutation flow
- tmux / `ntm` session supervision

This is intentionally separate from the engine runtime in `engine-rs/`.
Patina the product is the Godot-to-Rust port; the orchestrator is the local
multi-agent execution layer used to push that port forward.

## Scope

The orchestrator should own:

- queueing and assigning ready beads to worker lanes
- detecting likely-complete or stalled panes
- serializing all tracker writes through one coordinator
- keeping worker walls saturated without manual pane babysitting
- projecting a useful operator UI for the active swarm

The orchestrator should not own:

- engine parity logic itself
- oracle generation logic
- editor/runtime feature implementation
- `br` storage internals unless Patina later decides to fork them

## Planned Components

- `docs/` architecture, operating model, and failure modes
- `scripts/` local entrypoints for spawning and supervising the swarm
- `state/` optional derived runtime state, caches, and snapshots

## Prerequisites

Before launching the swarm, ensure these are set up:

### 1. Agent Mail server

The orchestrator starts the Agent Mail server automatically via tmux. It needs:

- **`mcp_agent_mail/.env`** — must contain `HTTP_BEARER_TOKEN`, `DATABASE_URL`, `STORAGE_ROOT`, and `HTTP_HOST`/`HTTP_PORT`/`HTTP_PATH`. Copy from `mcp_agent_mail/deploy/env/example.env` if missing.
- **`codex.mcp.json`** (project root) — must have `mcpServers.mcp-agent-mail.url` and a matching `Authorization: Bearer <token>` header. The bearer token here must match `HTTP_BEARER_TOKEN` in `.env`.
- **`uv`** — the Python package manager, used to run the mail server.

### 2. Required tools

| Tool | Purpose | Install |
|------|---------|---------|
| `cargo` | Builds the orchestrator | [rustup.rs](https://rustup.rs) |
| `tmux` | Hosts the swarm sessions | `brew install tmux` |
| `br` | Bead issue tracker CLI | `cargo install --path apps/orchestrator/crate` |
| `bv` | Beads viewer TUI | same as above |
| `python3` | Agent Mail server | system Python or pyenv |
| `uv` | Python package manager | `curl -LsSf https://astral.sh/uv/install.sh \| sh` |

### 3. Build the binary

```bash
cd apps/orchestrator/crate && cargo build --release
```

## Launching

The `launch` subcommand handles everything: starts the mail server, creates the tmux session, bootstraps agent identities, launches workers, and optionally creates a coordinator window.

```bash
patina-orchestrator launch \
  --session patina-fly \
  --workers 25 \
  --model "claude --model opus --dangerously-skip-permissions" \
  --project-root /path/to/patina \
  --with-coordinator \
  --interval 8 \
  --force
```

### What `launch` does

1. Ensures the Agent Mail server is running (health-checks `:8765`, starts a tmux session if not)
2. Creates a tmux session with monitor, planner, bv, and worker grid panes
3. Registers each worker with Agent Mail (auto-generates adjective+noun names like `TurquoisePuma`)
4. Launches Claude in each worker pane with the agent identity set
5. Starts the planner with `/loop 10m /planner`
6. (with `--with-coordinator`) Creates a coordinator window and registers a coordinator identity

### Identity bootstrap

Worker and coordinator identities are registered via the Agent Mail `register_agent` MCP tool over HTTP. No shell scripts are involved. Each agent gets:
- A unique auto-generated name (e.g., `MaroonSparrow`, `GoldCliff`)
- An identity file at `~/.local/state/agent-mail/identity/<project_hash>/<pane_id>`

The coordinator identity is set via `ORCH_COORDINATOR_AGENT` env var if provided, otherwise auto-registered like workers.

### Swarm layouts

Supported grid sizes: `2x2`, `3x3`, `4x4`, `5x5` (pass `--workers N` where N = rows * cols).

See [docs/swarm-layout.md](/Users/bone/dev/games/patina/apps/orchestrator/docs/swarm-layout.md).

### Environment variables

| Variable | Default | Purpose |
|----------|---------|---------|
| `ORCH_COORDINATOR_AGENT` | (auto-registered) | Coordinator agent name |
| `ORCH_SESSION` | from `--session` | Session name for the coordinator loop |
| `ORCH_SESSION_FAMILY` | same as session | Session family for agent mail grouping |
| `ORCH_INTERVAL_SECONDS` | `8` | Coordinator poll interval |
| `ORCH_BROWSER_VERIFY_ENABLED` | `0` | Enable browser verification |
| `ORCH_BROWSER_VERIFY_PANES` | (empty) | Comma-separated pane indices for browser verify |

## Architecture

The orchestrator sits above the existing tools and provides the dispatcher loop:

1. observe worker state
2. detect completion / stall
3. choose next ready bead
4. send assignment via Agent Mail
5. serialize tracker mutation through `br`
6. repeat

Patina currently relies on:

- `br` for issue tracking
- `bv` for triage and visibility
- Agent Mail for handoffs
- `tmux` for worker session management

See [docs/orchestrator-architecture.md](/Users/bone/dev/games/patina/apps/orchestrator/docs/orchestrator-architecture.md).
