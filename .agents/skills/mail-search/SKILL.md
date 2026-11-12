---
name: mail-search
description: Search Agent Mail messages using full-text search. Find past discussions, decisions, and coordination history.
argument-hint: [query]
---

# Search Agent Mail Messages

Search through project messages using SQLite FTS5 full-text search.

## Steps

1. Call `mcp__mcp-agent-mail__search_messages` with:
   - `project_key`: "/Users/bone/dev/games/patina"
   - `query`: "$ARGUMENTS"
   - `limit`: 20

2. Display results showing: subject, sender, date, importance, thread ID.

3. If the user wants to dive deeper into a thread, use `mcp__mcp-agent-mail__summarize_thread` with the thread_id.

## Query Syntax (FTS5)

- **Phrase search**: `"build plan"` (exact phrase)
- **Prefix search**: `migrat*` (matches migrate, migration, etc.)
- **Boolean**: `plan AND users`, `auth OR login`
- **Negation**: `deploy NOT staging`
- **Bead reference**: `B012` (find all messages about a specific bead)
