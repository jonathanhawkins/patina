---
name: "deploy-orchestrator"
description: "Build, test, and deploy the orchestrator binary. Rebuild the release binary, run focused tests, and trigger an assignment cycle."
argument-hint: "[session-name]"
---

# Deploy Orchestrator

Rebuild and verify the orchestrator, then kick the active swarm so it picks up the updated binary immediately.

## Arguments

- `$ARGUMENTS[0]` — tmux session name, default `patina-fly`

## Steps

1. Resolve the session name and coordinator identity:
```bash
SESSION="${1:-patina-fly}"
cat /Users/bone/dev/games/patina/.beads/coordinator_agent 2>/dev/null
```
If the coordinator identity file is missing, inspect the coordinator pane before continuing.

2. Run orchestrator tests from the crate directory:
```bash
cd /Users/bone/dev/games/patina/apps/orchestrator/crate && cargo test
```
If tests fail, stop.

3. Build the release binary:
```bash
cd /Users/bone/dev/games/patina/apps/orchestrator/crate && cargo build --release
```

4. Trigger an immediate assignment cycle using the discovered coordinator identity:
```bash
AGENT_NAME="$COORDINATOR" ORCH_SESSION="$SESSION" \
  /Users/bone/dev/games/patina/apps/orchestrator/crate/target/release/patina-orchestrator \
  assign --session "$SESSION" --project-root /Users/bone/dev/games/patina
```

5. Verify the swarm is using the rebuilt orchestrator:
```bash
/Users/bone/dev/games/patina/apps/orchestrator/crate/target/release/patina-orchestrator \
  health --session "$SESSION" --project-root /Users/bone/dev/games/patina
```

## Notes

- The binary is invoked per cycle, so rebuilding it is the deployment.
- This works for both Claude and Codex swarms. The launcher determines worker skill selection from the model command; the coordinator reads the exported `ORCH_AGENT_TYPE` and `ORCH_WORKER_COMMAND`.
