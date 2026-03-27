---
name: patina-fly-worker
description: Use when working as a Patina flywheel worker on an assigned bead. Follow the repo's Agent Mail workflow, read inbox assignment details, reserve files before shared edits, do not mutate br state, and close out via /skill mail-complete with concrete files and test commands.
---

# Patina Fly Worker

Use this for Patina swarm work assigned through Agent Mail.

## Start

1. Use `/skill mail-inbox` and read the current assignment thread.
2. Confirm the bead ID before changing code.
3. Reserve shared files before editing them:
   `/skill mail-reserve <bead-id> <path> [path...]`

## Work Rules

- Do not mutate `br` state.
- Work one bead at a time.
- Add tests with the implementation.
- Keep reported test commands concrete and rerunnable.

## Completion

Use the completion skill, not a freehand message:

```
/skill mail-complete <bead-id> --to <coordinator> --file <path> --test "<command>"
```

Repeat `--file` and `--test` as needed.

Do not send placeholder evidence like `...`, `not run`, or summary prose without commands.

## Blocked

If blocked, use `/skill mail-send` to send an explicit blocker message in the bead thread with what is needed to unblock. Use topic `bead-blocked` and subject `[pat-XXXX] Blocked: <reason>` with `ack_required: true`.
