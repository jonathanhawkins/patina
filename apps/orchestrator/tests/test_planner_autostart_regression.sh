#!/usr/bin/env bash
set -euo pipefail
# Regression test for planner auto-start in swarm launch.
#
# Bug: The planner pane (pane 1) started a Claude session but never sent
# the `/loop 10m /planner` command, leaving the planner idle forever.
# Fix: After launching Claude in pane 1, the launcher now auto-sends
# `/loop 10m /planner` after a boot delay.
#
# This test validates:
#   1. Dry-run output mentions planner auto-start (/loop 10m /planner)
#   2. Planner pane description includes the planner skill command
#   3. Pane 1 is designated as planner (not a generic worker)
#   4. All pane roles are present: monitor, planner, bv, workers
#   5. Planner command includes --dangerously-skip-permissions
#   6. Various worker counts don't affect planner configuration

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
BIN="${PROJECT_ROOT}/apps/orchestrator/crate/target/release/patina-orchestrator"

if [[ ! -x "$BIN" ]]; then
  echo "SKIP: release binary not built (run cargo build --release first)" >&2
  exit 0
fi

fail_count=0
pass_count=0

fail() {
  echo "FAIL: $1" >&2
  fail_count=$((fail_count + 1))
}

pass() {
  echo "  ok: $1"
  pass_count=$((pass_count + 1))
}

# --- Test 1: Planner auto-start appears in dry-run output (direct regression) ---
output=$("$BIN" launch --session test-planner-auto --workers 4 --dry-run --project-root "$PROJECT_ROOT" 2>&1)

if echo "$output" | grep -q '/loop.*planner'; then
  pass "planner auto-start command appears in dry-run output"
else
  fail "planner auto-start command missing from dry-run output"
fi

# --- Test 2: Pane 1 is labeled as planner, not worker ---
pane1_line=$(echo "$output" | grep 'pane 1:')
if echo "$pane1_line" | grep -q 'planner'; then
  pass "pane 1 is labeled 'planner'"
else
  fail "pane 1 is not labeled 'planner': $pane1_line"
fi

if echo "$pane1_line" | grep -q 'worker'; then
  fail "pane 1 should be planner, not worker: $pane1_line"
else
  pass "pane 1 is not mislabeled as worker"
fi

# --- Test 3: Planner pane includes --dangerously-skip-permissions ---
if echo "$pane1_line" | grep -q 'dangerously-skip-permissions'; then
  pass "planner pane includes --dangerously-skip-permissions"
else
  fail "planner pane missing --dangerously-skip-permissions: $pane1_line"
fi

# --- Test 4: All fixed pane roles present (monitor, planner, bv) ---
if echo "$output" | grep -q 'pane 0: monitor'; then
  pass "pane 0 is monitor"
else
  fail "pane 0 should be monitor"
fi

if echo "$output" | grep -q 'pane 1: planner'; then
  pass "pane 1 is planner"
else
  fail "pane 1 should be planner"
fi

if echo "$output" | grep -q 'pane 2: bv'; then
  pass "pane 2 is bv"
else
  fail "pane 2 should be bv"
fi

# --- Test 5: Workers start at pane 3, not earlier ---
if echo "$output" | grep -q 'pane 3: worker'; then
  pass "workers start at pane 3"
else
  fail "first worker should be pane 3"
fi

# No worker at pane 0, 1, or 2
for i in 0 1 2; do
  line=$(echo "$output" | grep "pane $i:" || true)
  if echo "$line" | grep -q 'worker'; then
    fail "pane $i should not be a worker: $line"
  fi
done
pass "no workers in fixed panes 0-2"

# --- Test 6: Planner config is consistent across different worker counts ---
for workers in 1 2 4 6 9 12; do
  out=$("$BIN" launch --session "test-planner-w$workers" --workers "$workers" --dry-run --project-root "$PROJECT_ROOT" 2>&1)
  p1=$(echo "$out" | grep 'pane 1:')
  if ! echo "$p1" | grep -q 'planner.*loop.*planner\|planner.*planner'; then
    fail "with $workers workers: pane 1 planner auto-start missing: $p1"
  fi
done
pass "planner auto-start consistent across worker counts (1,2,4,6,9,12)"

# --- Test 7: Planner pane is separate from workers (boundary: 1 worker) ---
output=$("$BIN" launch --session test-planner-1w --workers 1 --dry-run --project-root "$PROJECT_ROOT" 2>&1)
total_panes=$(echo "$output" | grep -c '^\s*pane [0-9]')
if [[ "$total_panes" -eq 4 ]]; then
  pass "1 worker: 4 total panes (3 fixed + 1 worker)"
else
  fail "1 worker: expected 4 panes, got $total_panes"
fi

# Pane 1 must still be planner, not a worker
p1=$(echo "$output" | grep 'pane 1:')
if echo "$p1" | grep -q 'planner'; then
  pass "1 worker: pane 1 is still planner (not repurposed as worker)"
else
  fail "1 worker: pane 1 was repurposed as something else: $p1"
fi

# --- Test 8: Planner pane with max workers (stress: 16 workers) ---
output=$("$BIN" launch --session test-planner-16w --workers 16 --dry-run --project-root "$PROJECT_ROOT" 2>&1)
p1=$(echo "$output" | grep 'pane 1:')
if echo "$p1" | grep -q 'planner'; then
  pass "16 workers: pane 1 is still planner"
else
  fail "16 workers: pane 1 should still be planner: $p1"
fi

worker_count=$(echo "$output" | grep -c 'pane.*worker')
if [[ "$worker_count" -eq 16 ]]; then
  pass "16 workers: all 16 worker panes present"
else
  fail "16 workers: expected 16 worker panes, got $worker_count"
fi

# --- Test 9: Planner description includes the skill path (not just 'claude') ---
# Before the fix, pane 1 just showed "claude" with no auto-start
p1=$(echo "$output" | grep 'pane 1:')
if echo "$p1" | grep -q 'loop'; then
  pass "planner description includes loop command (not bare 'claude')"
else
  fail "planner description shows bare claude without loop: $p1"
fi

# --- Test 10: Negative — planner is not affected by custom --model flag ---
output=$("$BIN" launch --session test-planner-model --workers 2 --model "claude --model opus" --dry-run --project-root "$PROJECT_ROOT" 2>&1)
p1=$(echo "$output" | grep 'pane 1:')
if echo "$p1" | grep -q 'planner'; then
  pass "custom model: pane 1 is still planner"
else
  fail "custom model: pane 1 lost planner label: $p1"
fi
if echo "$p1" | grep -q 'dangerously-skip-permissions'; then
  pass "custom model: planner still has --dangerously-skip-permissions"
else
  fail "custom model: planner lost --dangerously-skip-permissions: $p1"
fi

# --- Summary ---
echo
if [[ $fail_count -gt 0 ]]; then
  echo "FAILED: $fail_count test(s) failed, $pass_count passed" >&2
  exit 1
fi

echo "PASS: all $pass_count planner auto-start regression tests passed"
