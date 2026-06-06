---
name: "mail-ack"
description: "Acknowledge messages that require acknowledgement. Signals receipt to the sender."
argument-hint: "[agent-name] [message-id]"
---

# Acknowledge Agent Mail Messages

Acknowledge messages that have `ack_required=true`.

## Steps

1. Parse `$ARGUMENTS` for agent name and message ID.

2. If no message ID provided, first fetch inbox to find pending ack-required messages:
   - Call `mcp__mcp-agent-mail__fetch_inbox` with the agent name
   - Filter for messages with `ack_required: true`
   - Show them and ask which to acknowledge (or acknowledge all)

3. For each message to acknowledge, call `mcp__mcp-agent-mail__acknowledge_message` with:
   - `project_key`: "/Users/bone/dev/games/patina"
   - `agent_name`: the agent name
   - `message_id`: the message ID

4. Report which messages were acknowledged.

## Notes

- Acknowledging also marks the message as read
- Idempotent — safe to call multiple times on the same message
- Acknowledgements are lightweight non-textual receipts; use `/mail-send` to reply with content
