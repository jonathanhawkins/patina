---
name: mail-send
description: Send a message to another agent via Agent Mail. Supports threading, importance levels, and ack requests.
argument-hint: [recipient] [subject]
---

# Send Agent Mail Message

Send a targeted message to one or more agents.

## Usage

Parse `$ARGUMENTS` for recipient and subject. If not provided, ask the user for:
- **To**: recipient agent name(s)
- **Subject**: brief subject line
- **Body**: message content (Markdown)

## Steps

1. Call `mcp__mcp-agent-mail__send_message` with:
   - `project_key`: "/Users/bone/dev/games/patina"
   - `sender_name`: your agent name (from session)
   - `to`: [recipient agent name(s)]
   - `subject`: the subject line
   - `body_md`: the message body in Markdown
   - `thread_id`: use bead ID format (e.g., "B012") when working on beads
   - `importance`: "normal" unless specified otherwise
   - `ack_required`: true for important coordination messages

2. Confirm delivery and show the message ID.

## Threading Convention

- When working on a bead, use `thread_id` matching the bead ID (e.g., "B012")
- Prefix subjects with the bead ID: `[B012] Starting physics integration`
- Use `reply_message` tool to reply within existing threads

## Discovery

To find available agents to message, check the project's agent directory or ask the user.
