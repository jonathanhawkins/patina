# Orchestrator Recovery Issues - 2026-03-22

## Summary

Several swarm recovery failures stacked together:

- Agent Mail intermittently failed on `127.0.0.1:8765` with connection refused and later `Too many open files`.
- The coordinator could leave `in_progress` beads assigned to workers whose tmux panes no longer existed.
- `coordinator_reassign.sh` could assign a next bead to a worker name from completion mail even if that worker no longer had a live pane.
- `coordinator_assign_idle_workers.sh` could re-send duplicate `/skill patina-fly-worker` prompts to panes already holding the same queued prompt.
- The idle-fill path could assign new work to a pane that was visibly busy if the tracker no longer showed an active assignment.
- The reprompt-reclaim path could reclaim and immediately reassign a bead after the grace window even when the pane still showed active work.

## Fixes Added

- Added orphaned-assignment detection to swarm health and coordinator recovery.
- Blocked reassignment to workers that are no longer live in the current tmux session.
- Added prompt deduplication for queued assignment prompts.
- Blocked assignment to busy-but-unclaimed panes.
- Blocked reprompt reclaim when pane capture still shows active work.
- Added `apps/orchestrator/scripts/stop_session.sh` to pause the coordinator cleanly.

## Remaining Gaps

- Some copied-shell regression harnesses that run temp copies of coordinator scripts can hang in this environment, so shell syntax checks pass but a few new temp-harness regressions still need deeper harness debugging.
- Agent Mail transport still appears to be the main operational fragility during heavy orchestration churn.

## Operational Guidance

- Prefer pausing the coordinator before doing tracker reconciliation.
- Reopen orphaned `in_progress` beads before resuming idle fill.
- Restart only the coordinator and Agent Mail first; do not restart all workers unless pane state is genuinely unrecoverable.
