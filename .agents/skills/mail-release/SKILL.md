---
name: mail-release
description: Release file reservations when done editing. Frees paths for other agents.
argument-hint: [agent-name]
---

# Release File Reservations

Release your file reservations when you're done editing, so other agents can claim them.

## Steps

1. Call `mcp__mcp-agent-mail__release_file_reservations` with:
   - `project_key`: "/Users/bone/dev/games/patina"
   - `agent_name`: "$ARGUMENTS" or your agent name from session

2. If no arguments, release ALL active reservations for your agent.

3. To release specific paths only, pass `paths` parameter with the patterns to release.

4. Report how many reservations were released.

## When to Release

- After completing a bead/task
- Before ending your session
- When you realize you won't be editing a reserved file after all
- Release is idempotent — safe to call multiple times
