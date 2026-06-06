---
name: "mail-session"
description: "Bootstrap an MCP Agent Mail session - registers project+agent, checks inbox, optionally reserves files. Use at the start of any multi-agent workflow."
argument-hint: "[task-description]"
---

# Start Agent Mail Session

Bootstrap a new agent mail session for this project using the `macro_start_session` macro.

## Steps

1. Call `mcp__mcp-agent-mail__macro_start_session` with:
   - `human_key`: "/Users/bone/dev/games/patina" (this project's absolute path)
   - `program`: "codex"
   - `model`: your current model name
   - `task_description`: "$ARGUMENTS" (or a brief summary of what you're working on)
   - Do NOT pass `agent_name` — let it auto-generate an adjective+noun name

2. From the response, note your assigned **agent name** and store it for all subsequent mail operations.

3. Report back:
   - Your assigned agent name
   - Number of inbox messages (and any urgent ones)
   - Any active file reservations in the project

## Important

- Agent names are auto-generated adjective+noun combos (e.g., "GreenCastle", "BlueLake")
- Never use descriptive names like "BackendWorker" or "TestRunner"
- The project key is always the absolute path: `/Users/bone/dev/games/patina`
- After session start, you should check `bv --robot-triage` or `br ready` for your next task
