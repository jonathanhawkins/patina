---
name: deploy-orchestrator
description: Build, test, and deploy the orchestrator binary. Rebuilds the release binary, runs tests, and triggers an assignment cycle so idle workers pick up work immediately.
trigger: "deploy orchestrator", "restart orchestrator", "rebuild orchestrator", "redeploy orchestrator"
---

# Deploy Orchestrator

Build, test, and hot-deploy the Rust orchestrator binary. The orchestrator is invoked per-cycle (not a long-running daemon), so rebuilding the release binary is sufficient — the next cycle picks up the new code automatically. An immediate assignment cycle is triggered to avoid waiting.

## Steps

1. **Run tests** to verify changes compile and pass:
   ```bash
   cd /Users/bone/dev/games/patina/apps/orchestrator/crate && cargo test 2>&1 | tail -5
   ```
   If tests fail, stop and fix before deploying.

2. **Build release binary**:
   ```bash
   cd /Users/bone/dev/games/patina/apps/orchestrator/crate && cargo build --release 2>&1 | tail -5
   ```

3. **Find the coordinator identity** — check the orchestrator log or agent mail:
   ```bash
   grep "coordinator_agent\|AGENT_NAME" /tmp/patina-orchestrator.log | tail -1
   ```
   Common coordinator names: `IvoryTower`, `liveTower`. If unsure, check:
   ```bash
   tmux capture-pane -t patina-fly:0.1 -p -S -100 | grep -i "identity\|agent.*name\|ivory\|tower"
   ```

4. **Trigger an immediate assignment cycle** to prompt idle workers:
   ```bash
   AGENT_NAME=IvoryTower ORCH_SESSION=patina-fly \
     ./apps/orchestrator/crate/target/release/patina-orchestrator assign --session patina-fly 2>&1
   ```
   Adjust `AGENT_NAME` if the coordinator uses a different identity.

5. **Verify** workers are picking up work:
   ```bash
   tail -5 /tmp/patina-orchestrator.log
   ```
   Look for "Assigning", "Queuing prompt", and "pane done ok=true" lines.

## Notes

- The orchestrator binary lives at `apps/orchestrator/crate/target/release/patina-orchestrator`
- It's invoked per poll/assign cycle from the coordinator Claude in `patina-fly:0.1`
- No process restart needed — rebuilding the binary is the deployment
- The `assign` subcommand runs a single idle-fill + prompt submission cycle
- The `poll` subcommand processes pending completions and reassigns workers
