---
name: mail-status
description: Show Agent Mail status - active agents, file reservations, contacts, and server health.
argument-hint: [agent-name]
---

# Agent Mail Status Dashboard

Show the current state of the Agent Mail system for this project.

## Steps

Run these checks (in parallel where possible):

1. **Health check**: Call `mcp__mcp-agent-mail__health_check`

2. **Active file reservations**: Run via CLI:
   ```bash
   cd /Users/bone/dev/games/patina/mcp_agent_mail && uv run python -m mcp_agent_mail.cli file_reservations active '/Users/bone/dev/games/patina'
   ```

3. **Agent info** (if agent name provided via $ARGUMENTS):
   - Call `mcp__mcp-agent-mail__whois` with the agent name
   - Call `mcp__mcp-agent-mail__list_contacts` for the agent's contacts

4. **Window identities**: Call `mcp__mcp-agent-mail__list_window_identities` with project_key

## Display

Present a concise dashboard showing:
- Server status (healthy/unhealthy)
- Number of active agents and their names
- Active file reservations (who holds what, expiry times)
- Your agent's contacts (if agent name provided)
