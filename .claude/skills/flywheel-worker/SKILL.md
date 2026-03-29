---
name: flywheel-worker
description: >-
  Flywheel worker execution skill. Autonomously discovers, claims, and implements
  beads. Use for swarm bead assignments in tmux worker panes.
  Trigger on: "work bead", "flywheel worker",
  "read your inbox for assignment", "send bead-complete", "browser verifier", "regression verifier".
---

# Flywheel Worker

Autonomous worker loop for the agent swarm. You discover your own work, claim it, implement it, and report completion.

## Step 1: Check for Existing Work First

**CRITICAL**: Before claiming new work, check if you already have in-progress beads.

```bash
br list --status in_progress --assignee "$AGENT_NAME" --json --no-auto-import --allow-stale 2>/dev/null
```

If this returns one or more beads assigned to you:
- **You already have work.** Do NOT claim a new bead.
- For EACH bead you have claimed:
  1. Check if the implementation is already done (read the files, run the tests)
  2. If tests pass → report completion with `/mail-complete` (see Step 5)
  3. If not done → continue implementing it (go to Step 3)
- Work through your claimed beads ONE AT A TIME until all are completed
- Only after ALL your claimed beads are done should you pull new work

### 1b. Pull new work (only if you have zero in-progress beads)

```bash
br ready --json --unassigned --limit 5 --no-auto-import --allow-stale 2>/dev/null
```

Pick the best one:
- **P0 (critical)** beads first, then P1, P2, P3
- If the list is empty, report that you're idle and wait for the next loop cycle.

### 1c. Claim the bead

```bash
br update <bead-id> --assignee "$AGENT_NAME" --status in_progress --no-auto-import --no-auto-flush
```

Only claim ONE bead. Do not claim multiple beads.

### 1d. Reserve files (if applicable)

If you know which files you'll edit, reserve them via Agent Mail to prevent conflicts:

```
/skill mail-reserve <file-paths>
```

## Step 2: Understand the Bead

Read the full bead description:

```bash
br show <bead-id> --no-auto-import --allow-stale
```

Read the bead carefully. Understand:
- What needs to be implemented
- What files are involved
- What tests are expected (look for "Acceptance:" lines)
- What dependencies exist

If the bead references other files or context, read those files first.

## Step 3: Implement

Follow these rules:

1. **Work exactly one bead at a time.** Do not start unrelated work.
2. **Follow `AGENTS.md` and `CLAUDE.md`** for project conventions.
3. **Add or update tests** with the implementation. Every fix needs a test.
4. **Run the relevant tests** before reporting completion.
5. **If blocked**, report the block via Agent Mail instead of expanding scope.
6. **Do NOT call `br update --status done` or `br close`** — the coordinator handles bead lifecycle after verification.
7. **Do NOT send raw MCP `send_message` for completions** — use `/skill mail-complete` which formats the message correctly for the coordinator to process.

## Step 4: Verify

Before reporting completion:

1. Run the test commands that verify your work
2. Ensure all tests pass
3. Check for compilation errors: `cargo check` or equivalent

## Step 5: Report Completion

**CRITICAL**: You MUST use the `/mail-complete` skill to report completion. Do NOT use raw MCP `send_message` — the coordinator cannot process raw messages.

First, read the coordinator's agent name:

```bash
cat .beads/coordinator_agent 2>/dev/null
```

This file contains the coordinator's Agent Mail name (e.g., "CopperLantern"). Use it as the `--to` target:

```
/mail-complete <bead-id> --to <coordinator-name> --file <path1> --file <path2> --test "<test-command-1>" --test "<test-command-2>"
```

Requirements:
- `Files changed:` must list real paths you actually modified
- `Tests run:` must list real commands that passed
- Only report complete after those commands pass
- Do NOT call `br update --status done` — the coordinator handles this after verification

## Step 6: Done

This iteration is complete. If running under `/loop`, the next iteration will start automatically and you'll discover new work in Step 1.

**Do NOT wait passively.** When the loop restarts, go back to Step 1 and find the next bead.

## Hard Rules

- Follow `AGENTS.md`
- Use the assigned bead ID in your report
- Reserve shared files before editing when needed
- Do not start unrelated work
- Do not create or close beads (coordinator handles lifecycle)
- If blocked, report the block instead of expanding scope

## If You Are a Browser Verifier

- Test only the assigned browser/editor target
- Report findings back via Agent Mail
- Do not mutate `br`
- Do not create new beads directly

## If You Are a Regression Verifier

- Re-run the requested tests
- Report pass/fail clearly
- Do not close or create beads
