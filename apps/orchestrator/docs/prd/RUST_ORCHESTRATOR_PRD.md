# PRD: Patina Orchestrator Rust Rewrite

**Status:** Approved
**Date:** 2026-03-22
**Author:** Jonathan (bone)

## Problem Statement

The Patina orchestrator (`apps/orchestrator/`) is ~7,000 lines of bash + inline Python that coordinates AI coding agents running in tmux panes. It successfully closed 185 beads to achieve 100% oracle parity, proving the design works. However, the implementation language has become a liability:

1. **Duplicated code:** `sql_quote()`, `db_path()`, `sqlite_query()` are copy-pasted across 5 scripts. Shell has no module system.
2. **Inline Python everywhere:** `poll.sh` alone contains ~280 lines of Python heredocs for JSON parsing. The orchestrator is effectively Python wrapped in bash.
3. **Subprocess overhead:** Each poll cycle spawns 50+ child processes (python3, sqlite3, br, tmux). Most are for JSON field extraction that should be in-process.
4. **Silent failures:** 40+ instances of `2>/dev/null || true` swallow errors. The 14 regression test scripts prove bugs slip through.
5. **Data encoding hacks:** Base64 encoding/decoding is used to pass structured data through shell pipe boundaries.
6. **No type safety:** Bead status is compared as strings ("closed", "in_progress"). Worker state is inferred from regex matches with no exhaustive handling.

## Solution

Replace the coordinator logic with a single Rust binary (`patina-orchestrator`) while keeping tmux session setup scripts as shell.

### Why Rust (Not Python)

- **Compile-time safety:** Many of the 14 regression bugs would be caught by the type system (enum exhaustive matching, Option handling, Result propagation).
- **Agent-friendly:** AI agents write the code. They produce better Rust than Python because the compiler catches their mistakes before runtime.
- **Single binary:** No Python venv conflicts with Agent Mail's own Python environment.
- **Project alignment:** The engine is Rust. Established patterns (serde, thiserror, tracing) transfer directly.
- **Performance:** Eliminates 50+ subprocess spawns per cycle. Not that CPU speed matters at 8s poll intervals, but process overhead does.

### Why Not Agent Farm

Jeffrey Emanuel's Agent Farm (`claude_code_agent_farm`) was evaluated. It is a 2,990-line Python linter-fix loop that:
- Has no task database (uses random seed in prompts hoping agents pick different work)
- Has no deterministic assignment (duplicate work is probabilistic)
- Has no completion verification (no test re-running)
- Has no inter-agent messaging (scrapes tmux panes only)
- Has no stall detection or recovery

The Patina orchestrator's features are necessary. The problem is the language, not the scope.

## Architecture

### Crate Location

`apps/orchestrator/crate/` — standalone Rust crate, NOT in engine-rs workspace. The orchestrator has no dependency on engine crates. This follows the existing architecture doc: "intentionally separate from the engine runtime."

### Module Structure

```
apps/orchestrator/crate/
  Cargo.toml
  src/
    main.rs           # CLI entry point + run loop
    config.rs         # Config struct, env var loading
    error.rs          # OrchestratorError enum
    db.rs             # rusqlite read layer for .beads/*.db
    br.rs             # br CLI wrapper for mutations
    mail.rs           # Agent Mail HTTP client (ureq, JSON-RPC 2.0)
    tmux.rs           # tmux subprocess wrapper
    worker.rs         # Pane state detection (regex)
    message.rs        # Completion message parsing + dedup
    verifier.rs       # Test command extraction + re-run
    coordinator.rs    # Core poll/assign/reassign logic
```

### What Stays as Shell

| Script | Reason |
|--------|--------|
| `swarm/ensure_mail_server.sh` | Agent Mail server lifecycle — called from Rust launcher and coordinator |
| `swarm/notify_boss.sh` | Boss pane notifications — called from Rust coordinator |
| `swarm/seed_port_beads.sh` | Markdown parsing for bead seeding — called from Rust coordinator |
| `orch_env.sh` | Shared env vars sourced by the above scripts |
| `hooks/*.sh` | Claude Code session hooks (5 files) |

**Ported to Rust (removed):** fly.sh, launch.sh, common.sh, worker_bootstrap.sh, boss_console.sh, boss_resume.sh, health/monitor.sh, mail/complete.sh

### Dependencies

```toml
rusqlite = { version = "0.31", features = ["bundled"] }  # Direct SQLite access
regex = "1"                                                # Pane output matching
ureq = "2"                                                 # Sync HTTP client
serde = { version = "1", features = ["derive"] }           # Serialization
serde_json = "1"                                           # JSON
thiserror = "2"                                            # Error types
tracing = "0.1"                                            # Structured logging
tracing-subscriber = "0.3"                                 # Log output
```

### CLI

```
patina-orchestrator run [--interval 8] [--session NAME]
patina-orchestrator poll [--dry-run]
patina-orchestrator assign [--session NAME] [--dry-run]
patina-orchestrator health [--session NAME]
patina-orchestrator worker-state [--session NAME]
```

## Key Design Decisions

### SQLite reads via rusqlite, writes via `br` CLI

- **Reads** (hot path — every 8s): `bead_state()`, `count_by_status()`, `assigned_bead_for_worker()`, etc. Going through rusqlite directly eliminates subprocess overhead.
- **Writes** (mutation path): `br close`, `br update`, `br sync`. The `br` CLI handles WAL management, sync semantics, and locking correctly. Reimplementing those would create tight coupling with beads_rust internals.

### Type-safe state machines

```rust
enum BeadStatus { Open, InProgress, Closed, Tombstone }
enum WorkerState { Idle, Active, CompletedWaiting, Dead }
enum MessageTopic { BeadComplete, BeadAssign, BeadReopen, BeadIdle, Other(String) }
```

Exhaustive `match` on these enums prevents the class of bugs where a new state is added but not all code paths handle it.

### Parameterized SQL queries

The current shell scripts use `sql_quote()` (manual sed escaping). Rusqlite uses parameterized queries natively, eliminating SQL injection risk entirely.

### Config struct with env var overrides

All 20+ tunable parameters become fields on a `Config` struct with documented defaults. Currently they're scattered across script headers as individual variable declarations.

## Bead Lifecycle (preserved exactly)

```
READY → ASSIGNED → IN_PROGRESS → COMPLETED → CLOSED
         ↑                          |
         |                          v
         +------- REOPENED ←------- (verification failed)
```

1. **Seed:** `seed_port_beads.sh` creates beads from execution maps
2. **Assign:** Coordinator detects idle pane → picks ready bead → `br update --assignee` → Agent Mail `bead-assign` → tmux send-keys prompt
3. **Work:** Worker implements bead, runs tests
4. **Complete:** Worker calls `mail/complete.sh` → Agent Mail `bead-complete`
5. **Verify:** Coordinator parses completion → extracts test commands → re-runs tests
6. **Close/Reopen:** If tests pass: `br close` + assign next. If fail: `br update --status open` + `bead-reopen` message
7. **Recovery:** Stall detection reclaims idle/orphaned assignments

## Estimated Size

| Module | Lines |
|--------|-------|
| error.rs | 40 |
| config.rs | 100 |
| db.rs | 200 |
| br.rs | 80 |
| mail.rs | 250 |
| tmux.rs | 150 |
| worker.rs | 200 |
| message.rs | 300 |
| verifier.rs | 200 |
| coordinator.rs | 500 |
| main.rs | 300 |
| **Source total** | **~2,320** |
| Tests | ~800 |

Replaces ~7,000 lines of bash+python. Reduction is from eliminating accidental complexity, not cutting features.

## Implementation Phases

### Phase 1: Foundation (db, config, error, mail, tmux, br)
Create crate, implement all data access layers, unit tests per module.

### Phase 2: Parsing & Detection (message, verifier, worker)
Port the complex Python parsing logic. Property tests. Port 16 regression test cases.

### Phase 3: Coordinator Logic (coordinator.rs)
Implement poll, assign, reassign. Integration tests with mocked mail.

### Phase 4: Run Loop & CLI (main.rs)
All subcommands, file locking, stall detection.

### Phase 5: Integration & Cutover
Wire into launch scripts. Parallel run. Remove old shell scripts.

## Success Criteria

1. `cargo test` passes all unit, integration, and property tests
2. `patina-orchestrator poll --dry-run` makes same decisions as `poll.sh`
3. `patina-orchestrator assign --dry-run` makes same decisions as `assign_idle_workers.sh`
4. `patina-orchestrator health` outputs same metrics as `swarm_check.sh`
5. Full swarm session runs 30+ minutes without divergence
6. All 16 regression scenarios covered by Rust tests
7. Zero subprocess calls to python3 or sqlite3

## Non-Goals

- Porting `seed_port_beads.sh` (deferred — shell out for now)
- Adding the crate to the engine-rs workspace
- Linking against beads_rust library (use `br` CLI)
- Async/tokio (synchronous matches workspace convention)
- Replacing Agent Mail server (stays as Python MCP)
