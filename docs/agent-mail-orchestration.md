# Agent Mail Orchestration

This file describes the Patina-specific operating loop for the Agent Flywheel
stack. The intent is simple: Agent Mail is not optional decoration. It is the
control plane for multi-agent work in this repo.

## Roles

- `br`: dependency graph and status authority
- Agent Mail: directed messaging, handoffs, blocker reports, file reservations
- Main coordinator: owns `br`, validates work, reassigns follow-up beads
- Worker agents: implement one bead at a time and report through Agent Mail

## Startup

### 1. Start the server

```bash
cd /Users/bone/dev/games/patina/mcp_agent_mail
./scripts/run_server_with_token.sh
```

Health checks:

```bash
cd /Users/bone/dev/games/patina/mcp_agent_mail
.venv/bin/python -m mcp_agent_mail.cli doctor check
.venv/bin/python -m mcp_agent_mail.cli mail status /Users/bone/dev/games/patina
```

### 2. Bootstrap the current session identity

Use the session skill to bootstrap:

```
/skill mail-session
```

This:

- ensures the project exists on the server
- registers the current agent identity
- checks inbox for pending messages

**Identity resolution:**

1. `AGENT_NAME` env var — **canonical, always preferred**
2. tmux pane identity file — per-pane, safe for concurrent use
3. `.codex/agent_name` file — **deprecated**, racy in multi-pane setups

The shared file fallback (`.codex/agent_name`) is no longer written by default.
Concurrent agents that relied on it would silently adopt whichever identity
wrote last. Always `export AGENT_NAME` after bootstrap.
- **warns if the server assigned a different name** than requested (see below)

#### Name coercion

Agent names are unique per project. If you request a name that is already
registered by another agent session, the server will assign a different
auto-generated name instead. The bootstrap script prints a warning when
this happens:

```
WARNING: requested name 'AmberField' was not assigned.
  The server assigned 'LilacStone' instead.
```

Always use the name printed by the bootstrap script (and exported via
`AGENT_NAME`), not the name you originally requested. Sending mail or
reserving files under the wrong name will fail silently or route to the
wrong mailbox.

### 3. Use the Agent Mail skills

```
/skill mail-inbox
/skill mail-send BrightRiver "[pat-123] Handoff"
/skill mail-reserve pat-123 AGENTS.md docs/agent-mail-orchestration.md
```

## Required Worker Loop

1. Bootstrap identity.
2. Read inbox before starting a new bead or after a handoff.
3. Reserve shared files before editing them.
4. Work one bead at a time.
5. On completion, send a `bead-complete` message to the coordinator
   (topic: `bead-complete`, subject: `[pat-XXXX] Complete`,
   `ack_required: true`). Include files changed and tests run:
   `/skill mail-complete <bead_id> --to <coordinator> --file <path> --test <command>`
6. On blocker, send a `bead-blocked` message (topic: `bead-blocked`,
   subject: `[pat-XXXX] Blocked: <reason>`, `ack_required: true`).
7. Wait for coordinator follow-up (`bead-assign` or `bead-reopen`)
   before starting new work. Do not self-assign.

## Required Coordinator Loop

1. Keep the server running.
2. Own all `br` mutations.
3. Assign one bead per worker.
4. Read reports, validate code/tests, and either:
   - close the bead,
   - reopen/split it,
   - or assign the next dependent bead.
5. Use Agent Mail for reassignments and blocker resolution.

### Close-and-Reassign Protocol

The coordinator continuously processes worker reports and reassigns work.
This is the concrete loop that turns the five bullets above into action.

#### Message Conventions

All Agent Mail messages in this project use these conventions so the
coordinator can filter and automate:

| Direction        | Topic tag         | Subject format                | Body must include              |
|------------------|-------------------|-------------------------------|--------------------------------|
| Worker → Coord   | `bead-complete`   | `[pat-XXXX] Complete`         | Files changed, tests run       |
| Worker → Coord   | `bead-blocked`    | `[pat-XXXX] Blocked: <reason>`| What is needed to unblock      |
| Coord → Worker   | `bead-assign`     | `[pat-XXXX] Assigned`         | Bead ID, title, acceptance     |
| Coord → Worker   | `bead-reopen`     | `[pat-XXXX] Reopened: <why>`  | What was wrong, what to fix    |

Workers **must** use `ack_required: true` on `bead-complete` and
`bead-blocked` messages so the coordinator can track pending handoffs.
Structured completion payloads are the preferred protocol. The coordinator
still supports legacy `Files changed:` / `Tests run:` sections as fallback,
but new worker flows should send the JSON payload emitted by
`/skill mail-complete`.

#### Coordinator Iteration

Each pass through the coordinator loop:

1. **Poll inbox** — `fetch_inbox` filtered to topics `bead-complete` and
   `bead-blocked`. Process unacknowledged messages first.

2. **For each `bead-complete` report:**
   a. Validate against the narrowest executable gate that matches the bead's
      acceptance criteria. Do not default every closeout to a full
      `cargo test --workspace` run.
   b. If valid → `br close <id> --reason completed`, acknowledge the
      message, and proceed to step 3.
   c. If invalid → send a `bead-reopen` message back to the worker with
      the failure details. Do not close the bead.

3. **Pick the next bead** for the now-idle worker:
   a. Run `br list --status open` to get all open beads.
   b. Cross-reference with the execution map
      (`prd/BEAD_EXECUTION_MAP.md`) — only consider beads in the `Now`
      or `Next` tiers.
   c. Check dependency satisfaction: all `depends_on` beads must be
      closed.
   d. Check lane affinity: prefer assigning beads in the same lane the
      worker was already on to minimize context-switching.
   e. Check file reservations: avoid beads whose primary files overlap
      with another worker's active reservations.

4. **Send assignment** — `bead-assign` topic, `ack_required: true`.
   Include the bead ID, title, and acceptance criteria in the body.
   Update `br` with `br update <id> --assign <worker>`.

5. **For each `bead-blocked` report:**
   a. Acknowledge the message.
   b. Determine if the blocker can be resolved (dependency not met,
      missing fixture, external question).
   c. Either resolve the blocker and send a follow-up, or reassign the
      worker to a different unblocked bead (step 3) and leave the
      blocked bead for later.

6. **Repeat** — the coordinator does not stop after one pass. It polls
   continuously (or is triggered by the `check_inbox` hook) and processes
   reports as they arrive.

#### Idle Worker Detection

A worker is considered idle when:
- Its last `bead-complete` message was acknowledged (bead closed), AND
- No `bead-assign` message is pending acknowledgment from that worker.

The coordinator should track which workers are idle and prioritize
assigning them work before polling for new reports.

#### Automated Reassignment Script

Steps 2–4 above are automated by `apps/orchestrator/coordinator/reassign.sh`:

```bash
# After validating a worker's completion report:
./apps/orchestrator/coordinator/reassign.sh <completed_bead> <worker_agent> [close_reason]

# With auto-ack of the worker's completion message:
./apps/orchestrator/coordinator/reassign.sh --ack <msg_id> <completed_bead> <worker_agent> [close_reason]

# Example:
./apps/orchestrator/coordinator/reassign.sh --ack 42 pat-keas AmberField "validated — tests pass"
```

The script:
1. Closes the completed bead in `br`
2. Queries `br ready --unassigned --limit 1` for the next available bead
3. Assigns it to the worker in `br`
4. Sends a `bead-assign` message via Agent Mail with `ack_required: true`
5. (with `--ack`) Acknowledges the worker's completion message

If no unassigned ready beads remain, it reports the worker as idle.

#### Full Coordinator Poll

For hands-off operation, `apps/orchestrator/coordinator/poll.sh` runs a complete
coordinator iteration: fetch inbox → find all unacked `bead-complete`
messages → close + reassign + ack for each one.

```bash
# Process all pending completion reports:
./apps/orchestrator/coordinator/poll.sh

# Preview without mutating anything:
./apps/orchestrator/coordinator/poll.sh --dry-run
```

This reduces the coordinator's per-report workload from 5 manual steps
(poll, read, extract, reassign, ack) to a single command.

## Validation Baselines

The coordinator should validate against the repo's explicit executable gates,
not stale narrative parity percentages.

### Runtime parity source of truth

For the currently supported oracle/property slice, use:

```bash
cd /Users/bone/dev/games/patina/engine-rs
cargo test --test default_property_stripping_parity_test measured_parity_all_available_scenes -- --nocapture
```

This is the current measured-slice parity gate. A pass supports the claim that
the supported fixture corpus remains at 100% parity for that slice. It does
not imply repo-wide `1:1 with Godot`.

### Editor/browser maintenance validation

Editor validation is maintenance-only. It exists to keep the browser-served
shell stable while runtime parity work continues.

Use:

```bash
cd /Users/bone/dev/games/patina/engine-rs
cargo test --test editor_smoke_test
```

For manual browser verification:

```bash
cd /Users/bone/dev/games/patina/engine-rs
cargo run --example editor
```

Then open `http://localhost:8080/editor` in a browser and work through
`engine-rs/crates/gdeditor/SMOKE_CHECKLIST.md`.

### Coordinator closeout rule

Use the smallest validating command set that proves the bead:

- runtime/oracle parity beads: run the targeted parity test first, then the
  broader gate if the bead changes shared fixture semantics
- editor server/browser beads: run `editor_smoke_test`, then do manual browser
  checks when the visible shell changed
- only run `cargo test --workspace` when the bead is broad enough that targeted
  verification is not credible

#### Example Coordinator Iteration

```
1. fetch_inbox → 1 message: [pat-keas] Complete from AmberField
2. Validate: cargo test --workspace passes ✓
3. ./apps/orchestrator/coordinator/reassign.sh pat-keas AmberField
   → Closes pat-keas
   → Picks pat-1rcm (next ready, unassigned)
   → Assigns pat-1rcm to AmberField in br
   → Sends [pat-1rcm] Assigned via Agent Mail
4. acknowledge_message(msg_id) for the original completion report
```

## Team Layout

Patina usually runs five teams:

1. Core runtime/oracle
2. Fixture-specific recovery lane A
3. Fixture-specific recovery lane B
4. Golden/reporting lane
5. Docs/policy/integration lane

Adjust the names as needed, but keep ownership clear and avoid overlapping write
sets unless the coordinator explicitly sequences the work.

## Codex Hook Model

Codex notify events are chained through:

- `~/.codex/config.toml`
- `/Users/bone/.voxherd/hooks/codex-notify-chain.sh`
- repo-local `.codex/hooks/notify_wrapper.sh`

Identity resolution order:

1. `AGENT_NAME` env var — canonical
2. tmux pane identity files written by `identity-write.sh` — concurrent-safe
3. `.codex/agent_name` — deprecated fallback, emits a warning

Bootstrap is mandatory. Without it, the notify hook has no real identity and
inbox reminders silently do nothing.

For concurrent teams in one repository checkout, each pane must export its own
`AGENT_NAME` or use tmux pane identity files. The shared `.codex/agent_name`
file is not safe for concurrent use and will emit a deprecation warning.

## Practical Rule

If a worker is not using a real Agent Mail identity, that worker is not
participating in the Flywheel system yet.
