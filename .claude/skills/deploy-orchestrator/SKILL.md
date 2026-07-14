---
name: deploy-orchestrator
description: Build, test, and deploy the orchestrator binary. Rebuilds the release binary, runs tests, and triggers an assignment cycle so idle workers pick up work immediately.
trigger: "deploy orchestrator", "restart orchestrator", "rebuild orchestrator", "redeploy orchestrator"
---

# Deploy Orchestrator

Build, test, and hot-deploy the Rust orchestrator binary. One-shot subcommands (`assign`, `poll`) pick up a rebuilt binary automatically, BUT a long-running `run` daemon does NOT — it keeps executing the old code until restarted. A deploy is not complete until any live `run` daemon has been restarted (step 3).

## Steps

1. **Compile-check via the build-slot wrapper** (NOT raw `cargo`). The wrapper holds the single build-slot lock so a verifier run doesn't collide with this deploy:
   ```bash
   ./scripts/rust_task.sh check -p patina-orchestrator 2>&1 | tail -5
   ```
   If `check` fails, stop and fix before deploying. Do NOT run `cargo test` here — the verifier lane is the sole test runner; running the full suite inline would (a) fight the verifier for the build slot and (b) burn 5–10 min before the user sees the binary update.

2. **Build release binary** through the same wrapper:
   ```bash
   ./scripts/rust_task.sh build --release -p patina-orchestrator 2>&1 | tail -5
   ```

3. **Restart the `run` daemon if one is live** — a running daemon keeps the OLD code loaded (verified the hard way on 2026-07-13: a 16h-old daemon silently ignored a deploy):
   ```bash
   pgrep -fl "patina-orchestrator run" || echo "no daemon (one-shot mode; skip restart)"
   ```
   If a daemon is found: capture its env (`ps eww -p <pid> | tr ' ' '\n' | grep -E '^(ORCH_|AGENT_)'`), `kill -TERM <pid>`, then relaunch from the repo root with the same env:
   ```bash
   env <captured ORCH_*/AGENT_* vars> nohup \
     ./apps/orchestrator/crate/target/release/patina-orchestrator run \
     --session patina-fly --interval 8 \
     > .codex/orchestrator/run.log 2>&1 & disown
   ```
   Confirm the new pid with `pgrep -fl "patina-orchestrator run"` and watch `.codex/orchestrator/run.log` for the first cycle.

4. **Find the coordinator identity** — read the canonical pointer file (the orchestrator writes this on every coordinator startup):
   ```bash
   cat /Users/bone/dev/games/patina/.beads/coordinator_agent 2>/dev/null
   ```
   This is the single source of truth. Do NOT grep logs, do NOT guess names like `IvoryTower`/`liveTower`, do NOT inspect tmux pane scrollback. If the file is missing or empty, the coordinator isn't running — surface that and stop, don't fabricate a name.

5. **Trigger an immediate assignment cycle** to prompt idle workers. Read the coordinator name from the file (do not hard-code):
   ```bash
   COORDINATOR="$(cat /Users/bone/dev/games/patina/.beads/coordinator_agent)"
   AGENT_NAME="$COORDINATOR" ORCH_SESSION=patina-fly \
     ./apps/orchestrator/crate/target/release/patina-orchestrator assign --session patina-fly 2>&1
   ```

6. **Verify** workers are picking up work:
   ```bash
   tail -5 .codex/orchestrator/run.log
   ```
   Look for "Assigning", "Queuing prompt", and "pane done ok=true" lines. (The old
   /tmp/patina-orchestrator.log path is stale — the daemon logs to
   .codex/orchestrator/run.log since 2026-07-13.)

## Notes

- The orchestrator binary lives at `apps/orchestrator/crate/target/release/patina-orchestrator`
- One-shot `assign`/`poll` invocations pick up a rebuilt binary automatically; the
  long-running `run --interval N` daemon does NOT — restarting it IS the deployment
- The `assign` subcommand runs a single idle-fill + prompt submission cycle
- The `poll` subcommand processes pending completions and reassigns workers

## Hard Rules

- NEVER run raw `cargo test`, `cargo build`, `cargo check`, `cargo nextest` from this skill. Always go through `./scripts/rust_task.sh` so the build-slot lock prevents collision with the verifier lane.
- NEVER run `cargo test` (full suite) during a deploy. The verifier lane runs tests; this skill ships the binary.
- NEVER hard-code a coordinator agent name. Always read `.beads/coordinator_agent`. If it's missing, the coordinator isn't running — say so, don't guess.
- NEVER grep tmux scrollback or log files to "discover" the coordinator name when the canonical pointer file exists.
