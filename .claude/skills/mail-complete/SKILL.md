---
name: mail-complete
description: Report bead completion to the coordinator. Sends a structured completion message with files changed, test commands, and optional summary/notes.
argument-hint: <bead-id> --to <coordinator> --file <path> --test <cmd> [--summary <text>] [--note <text>]
---

# Report Bead Completion

Send a structured bead-completion message to the coordinator via Agent Mail.

## Usage

Parse `$ARGUMENTS` for the bead ID and options. Arguments follow this pattern:

```
<bead-id> --to <coordinator> --file <path> [--file <path>...] --test <cmd> [--test <cmd>...] [--summary <text>] [--note <text>...]
```

If `--to` is not provided, ask the user for the coordinator agent name.

**Required:**
- At least one `--file` (path to a file changed)
- At least one `--test` (test command to verify the work)

## Steps

1. **Validate inputs.** Ensure the bead ID is present and starts with `pat-`. Ensure at least one `--file` and one `--test` are provided. If validation fails, tell the user what's missing.

2. **Build the message body.** Construct markdown in exactly this format:

   ```
   ## [<bead-id>] Complete
   <summary if provided>
   Files changed:
   - `<path1>`
   - `<path2>`

   Tests run:
   - `<cmd1>`
   - `<cmd2>`

   Notes:
   - <note1>

   Completion payload (JSON):
   ```json
   {
     "bead_id": "<bead-id>",
     "status": "complete",
     "files_changed": ["<path1>", "<path2>"],
     "test_commands": ["<cmd1>", "<cmd2>"]
   }
   ```
   ```

   Omit the "Notes:" section if no `--note` arguments were given.

3. **Send the message.** Call `mcp__mcp-agent-mail__send_message` with:
   - `project_key`: "/Users/bone/dev/games/patina"
   - `sender_name`: your agent name (from session)
   - `to`: [coordinator agent name from `--to`]
   - `thread_id`: the bead ID (e.g., "pat-abc123")
   - `subject`: `[<bead-id>] Complete`
   - `body_md`: the markdown body built above
   - `importance`: "normal"
   - `ack_required`: true
   - `topic`: "bead-complete"

4. **Confirm delivery** and show the message ID.

## Message Format Contract

The coordinator's Rust parser (`message.rs`) relies on this exact structure:

- **Subject** must match `[pat-xxx] Complete` (case-insensitive)
- **topic** must be `"bead-complete"`
- **thread_id** must be the bead ID
- **body_md** must contain a fenced ```json block with a JSON object containing `bead_id`, `status`, `files_changed` (array of strings), and `test_commands` (array of strings)

Do NOT deviate from this format. The coordinator uses regex to extract the fenced JSON payload.

## Example

```
/skill mail-complete pat-abc123 --to LilacSparrow --file engine-rs/crates/gdcore/src/lib.rs --file engine-rs/tests/foo_test.rs --test "cargo test --test foo_test" --summary "Implemented foo parity coverage."
```
