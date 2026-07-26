#!/usr/bin/env bash
# flywheel-watchdog.sh — autonomous stall detection + escalating self-heal.
#
# Runs as a loop alongside the flywheel (its own pane in the session). It does
# NOT care WHY the swarm stalled — it watches the only metric that matters,
# forward progress (bead closes), and when that stops while work is available
# it applies an escalating recovery. This converts "operator notices a stall
# and runs the fix by hand" (identity desync, orphan-wedge, worker-stuck,
# launch-queue saturation) into automatic recovery.
#
# Escalation, resetting to L0 whenever a close lands:
#   L0 nudge      — renew identities + one forced assign cycle
#   L1 coordinator— reap orphan cargo/verifier, clear locks, restart the daemon
#   L2 workers    — restart the worker panes (fresh identities + fresh context),
#                   after a launch-queue drain check so we never deepen a wedge
#   L3 alert      — genuinely stuck; notify the operator (tmux + notify script)
#
# Conservative by design: a stall must persist STALL_SECS AND the ready queue
# must be non-empty AND workers must be present, so a legitimately-quiet queue
# or a long single build never trips it.
set -uo pipefail

SESSION="${1:-patina-fly}"
ROOT="/Users/bone/dev/games/patina"
ORCH="$ROOT/apps/orchestrator/crate/target/release/patina-orchestrator"
LOG="$ROOT/.codex/orchestrator/watchdog.log"

STALL_SECS="${WATCHDOG_STALL_SECS:-2400}"   # 40 min with no close = stall
CHECK_SECS="${WATCHDOG_CHECK_SECS:-300}"    # evaluate every 5 min
SETTLE_SECS="${WATCHDOG_SETTLE_SECS:-600}"  # after acting, wait before escalating

log() { echo "$(date -u +%H:%M:%S) $*" | tee -a "$LOG" >&2; }

closed_count() {
  br list --status closed --json --limit 1 --no-auto-import 2>/dev/null \
    | python3 -c "import json,sys;print(json.load(sys.stdin).get('total',0))" 2>/dev/null || echo 0
}
ready_count() {
  "$ORCH" health --session "$SESSION" --project-root "$ROOT" 2>/dev/null \
    | sed -n 's/^ready_unassigned=\([0-9]*\)/\1/p' | head -1
}
worker_panes() {
  tmux list-panes -t "$SESSION:0" -F '#{pane_index}' 2>/dev/null | awk '$1>=4' | wc -l | tr -d ' '
}

# Fresh-binary launch probe: compile+run a trivial binary. Returns 0 (clear)
# or 1 (launch queue saturated — draining, do NOT restart workers yet).
launch_queue_clear() {
  local d; d="$(mktemp -d)"
  printf 'int main(void){return 0;}' > "$d/p.c"
  /usr/bin/clang "$d/p.c" -o "$d/p" 2>/dev/null || { rm -rf "$d"; return 0; }
  ( "$d/p" & local pid=$!; sleep 5
    if kill -0 "$pid" 2>/dev/null; then kill -9 "$pid" 2>/dev/null; rm -rf "$d"; exit 1; fi
    rm -rf "$d"; exit 0 )
}

reap_and_restart_coordinator() {
  log "L1: reaping orphan cargo/verifier + restarting coordinator daemon"
  pkill -9 -f "cargo-nextest" 2>/dev/null || true
  pkill -9 -f "engine-rs/target.*deps/" 2>/dev/null || true
  pkill -f "rust_task" 2>/dev/null || true
  pkill -f "am-run.*rust-build" 2>/dev/null || true
  rm -f "$ROOT/.orchestrator/rust-build.lock.info"
  rmdir "$ROOT/.orchestrator/rust-build.lock" 2>/dev/null || true
  local pid envs
  pid="$(pgrep -f 'patina-orchestrator run' | head -1)"
  if [ -n "$pid" ]; then
    envs="$(ps eww -p "$pid" 2>/dev/null | tr ' ' '\n' | grep -E '^(ORCH_SESSION|ORCH_SESSION_FAMILY|ORCH_PULL_MODE|AGENT_NAME|ORCH_AGENT_TYPE|ORCH_WORKER_COMMAND|ORCH_PLANNER_COMMAND|AGENT_MAIL_TOKEN)=' | tr '\n' ' ')"
    kill -TERM "$pid" 2>/dev/null; sleep 3
    env $envs nohup "$ORCH" run --session "$SESSION" --interval 8 \
      > "$ROOT/.codex/orchestrator/run.log" 2>&1 & disown
    log "L1: coordinator restarted (new pid $(pgrep -f 'patina-orchestrator run' | head -1))"
  else
    log "L1: no coordinator daemon found to restart"
  fi
}

restart_workers() {
  if ! launch_queue_clear; then
    log "L2: launch queue saturated — draining, deferring worker restart"
    return
  fi
  log "L2: restarting worker panes (fresh identities + context)"
  for p in $(tmux list-panes -t "$SESSION:0" -F '#{pane_index}' 2>/dev/null | awk '$1>=4'); do
    tmux send-keys -t "$SESSION:0.$p" C-c 2>/dev/null || true
  done
  sleep 3
  # Re-bootstrap each worker via the orchestrator's own relaunch path if present;
  # otherwise send the flywheel-worker skill prompt to re-enter the loop.
  for p in $(tmux list-panes -t "$SESSION:0" -F '#{pane_index}' 2>/dev/null | awk '$1>=4'); do
    tmux send-keys -t "$SESSION:0.$p" -l "/clear" 2>/dev/null; sleep 0.3
    tmux send-keys -t "$SESSION:0.$p" Enter 2>/dev/null; sleep 0.5
    tmux send-keys -t "$SESSION:0.$p" -l "/flywheel-worker" 2>/dev/null; sleep 0.3
    tmux send-keys -t "$SESSION:0.$p" Enter 2>/dev/null
  done
  log "L2: worker panes re-prompted"
}

alert_operator() {
  log "L3: STALL PERSISTS after coordinator+worker restart — operator needed"
  local notify="$ROOT/tools/notify_boss.sh"
  [ -x "$notify" ] && "$notify" "$SESSION" "WATCHDOG L3: flywheel stalled, auto-recovery exhausted — needs a human" 2>/dev/null || true
}

log "watchdog started for '$SESSION' (stall=${STALL_SECS}s check=${CHECK_SECS}s)"
last_closed="$(closed_count)"
last_progress="$(date +%s)"
level=0

while tmux has-session -t "=$SESSION" 2>/dev/null; do
  sleep "$CHECK_SECS"
  cur="$(closed_count)"
  now="$(date +%s)"
  if [ "${cur:-0}" -gt "${last_closed:-0}" ]; then
    [ "$level" -ne 0 ] && log "recovered — closes resumed ($last_closed → $cur)"
    last_closed="$cur"; last_progress="$now"; level=0
    continue
  fi
  drought=$(( now - last_progress ))
  [ "$drought" -lt "$STALL_SECS" ] && continue
  ready="$(ready_count)"; ready="${ready:-0}"
  wp="$(worker_panes)"
  if [ "$ready" -lt 1 ] || [ "${wp:-0}" -lt 1 ]; then
    log "no-close ${drought}s but not a stall (ready=$ready workers=$wp) — holding"
    continue
  fi
  log "STALL: ${drought}s no close, ready=$ready workers=$wp → recovery level $level"
  case "$level" in
    0) log "L0: renew identities + forced assign";
       coord="$(cat "$ROOT/.beads/coordinator_agent" 2>/dev/null)"
       AGENT_NAME="$coord" ORCH_SESSION="$SESSION" "$ORCH" assign --session "$SESSION" --project-root "$ROOT" >/dev/null 2>&1 || true
       level=1 ;;
    1) reap_and_restart_coordinator; level=2 ;;
    2) restart_workers; level=3 ;;
    3) alert_operator ;;  # stay at L3, keep alerting each cycle
  esac
  # Give the action time to take effect before re-evaluating.
  last_progress="$(date +%s)"
  sleep "$SETTLE_SECS"
done
log "watchdog exiting — session '$SESSION' gone"
