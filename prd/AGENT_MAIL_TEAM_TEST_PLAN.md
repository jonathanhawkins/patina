# Agent Mail Team Test Plan

This is the smoke pack for validating Patina's Agent Mail orchestration loop
with five concurrent teams.

## Goal

Prove that the repo can run a five-team Flywheel cycle using:

- live Agent Mail identities
- directed handoff messages
- file reservations
- `br`-tracked beads
- coordinator-controlled closeout

## Beads

1. `pat-55iy` Team 1: bootstrap and inbox validation
   Mailbox: `BrightRiver`
2. `pat-vg8e` Team 2: directed handoff send/receive validation
   Mailbox: `QuietStone`
3. `pat-9y7o` Team 3: file reservation acquire/release validation
   Mailbox: `DarkTower`
4. `pat-8xxj` Team 4: notify hook / identity persistence validation
   Mailbox: `MagentaHarbor`
5. `pat-ud8j` Team 5: coordinator audit and closeout validation
   Mailbox: `IvoryHollow`

## Rules

- One bead per team.
- Every team must report via Agent Mail.
- The coordinator closes the beads after reviewing evidence.
- These beads exist only to validate the orchestration layer and may be closed
  after the workflow is proven.

## Audit — pat-ud8j (Team 5 Coordinator: IvoryHollow)

### Worker Report Summary

| Bead | Team | Worker | Status | Evidence |
|------|------|--------|--------|----------|
| pat-55iy | 1 | BrightRiver | **PASS** | Bootstrap registered, inbox returned msg #4 on thread pat-55iy, bidirectional mail confirmed (4 messages in thread) |
| pat-vg8e | 2 | QuietStone | **PASS** | Bootstrap OK, inbox fetched msg #6 from BrightRiver, ack at 2026-03-20T18:17:47Z, directed handoff sent to IvoryHollow on thread pat-vg8e (4 messages in thread) |
| pat-9y7o | 3 | DarkTower | **PASS** | Reservation acquire/release validated and delivered back to coordinator; no stale lock left behind |
| pat-8xxj | 4 | MagentaHarbor | **PASS** | Notify wrapper and identity resolution validated with non-placeholder mailbox identity; dynamic hook path confirmed after wrapper fix |

### Ack Status

- msg #5 (Team 5 assignment from BrightRiver) — acked by coordinator at 2026-03-20T18:21:11Z
- msg #9 (pat-55iy completed from BrightRiver) — ack_required=true, pending coordinator ack
- msg #17 (pat-vg8e completion from QuietStone) — ack_required=true, pending coordinator ack
- msg #20 (pat-9y7o PASS from LilacStone) — ack_required=false (no ack needed)
- msg #18 (pat-8xxj DONE from GoldMarsh) — ack_required=true, pending coordinator ack

### File Reservations

- Coordinator held temporary shared-surface reservations during the smoke audit; no unresolved reservation conflict remained after validation
- LilacStone's earlier reservations (id=3,4) have expired (TTL 1h from ~17:18)
- No active conflicts

### Findings

1. **Bootstrap works**: All 4 workers successfully registered identities and exported AGENT_NAME.
2. **Inbox/send works**: Workers received assignment messages and sent completion reports.
3. **Directed handoff works**: QuietStone sent a handoff to IvoryHollow with ack_required.
4. **File reservations work**: LilacStone acquired and re-granted reservations without conflict. AzureFinch acquired clean after expiry.
5. **Notify hook hardening landed**: the Claude hook layer and repo shell wrappers now resolve identity dynamically instead of using `YOUR_AGENT_NAME`, and tmux-pane identity is the concurrent-safe path.

### Verdict

All 4 worker beads **PASS**. The Agent Mail orchestration loop is operational:
- Identity registration ✅
- Inbox fetch ✅
- Message send ✅
- Ack flow ✅
- Directed handoff ✅
- File reservation acquire/release ✅
- Notify hook / identity resolution ✅

**pat-ud8j is ready for br close**.

Audited by: IvoryHollow (Coordinator)
Date: 2026-03-20

## Hardening Follow-Up

The smoke pack exposed four follow-up hardening tasks:

- `pat-vma3`: remove shared `.codex/agent_name` race in multi-pane workflows
- `pat-0ls9`: make server name coercion explicit in bootstrap output and docs
- `pat-kbau`: verify sender/inbox labeling semantics and document the result
- `pat-orzb`: reconcile inbox `ack_required` reporting with actual ack state

Of these, `pat-vma3`, `pat-0ls9`, and `pat-kbau` were validated during the same
coordination pass. `pat-orzb` remains open because it has a confirmed root cause
but still needs an implementation fix in the upstream mail tooling or local
presentation layer.
