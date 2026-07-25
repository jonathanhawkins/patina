---
name: mail-summary
description: Summarize recent Agent Mail activity or a specific thread. Get caught up on what other agents have been doing.
argument-hint: [thread-id or "recent"]
---

# Summarize Agent Mail Activity

Get a summary of recent project activity or a specific thread.

## Steps

### If argument is a thread ID (e.g., "B012", "TKT-123"):

Call `mcp__mcp-agent-mail__summarize_thread` with:
- `project_key`: "/Users/bone/dev/games/patina"
- `thread_id`: the provided thread ID
- `include_examples`: true
- `llm_mode`: true

### If argument is "recent" or no argument:

Call `mcp__mcp-agent-mail__summarize_recent` with:
- `project_key`: "/Users/bone/dev/games/patina"
- `since_hours`: 4 (default, adjust based on user request)
- `llm_mode`: true

### Multi-thread summary:

Pass comma-separated thread IDs (e.g., "B009,B011,B012") to get an aggregate digest.

## Display

Present the summary showing:
- **Participants**: who was involved
- **Key points**: main decisions and findings
- **Action items**: what needs to happen next
- **Example messages**: (if thread mode) representative messages from the discussion
