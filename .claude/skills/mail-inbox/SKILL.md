---
name: mail-inbox
description: Check your Agent Mail inbox for unread messages, urgent items, and pending acknowledgements. Use to stay coordinated with other agents.
argument-hint: [agent-name]
---

# Check Agent Mail Inbox

Fetch and display your inbox from MCP Agent Mail.

## Steps

1. Call `mcp__mcp-agent-mail__fetch_inbox` with:
   - `project_key`: "/Users/bone/dev/games/patina"
   - `agent_name`: "$ARGUMENTS" (your agent name) — if not provided, ask the user
   - `include_bodies`: true
   - `limit`: 20

2. Display results organized by:
   - **Urgent/High importance** messages first
   - **Ack-required** messages (flag these clearly)
   - **Normal** messages

3. For each message show: sender, subject, importance, whether ack is required, and a brief body preview.

4. If there are ack-required messages, remind the user to acknowledge them with `/mail-ack`.

## Tips

- Use `urgent_only: true` for a quick check during busy work
- Use `since_ts` with an ISO timestamp for incremental polling
- Use `topic` to filter by a specific topic tag (e.g., a bead ID)
