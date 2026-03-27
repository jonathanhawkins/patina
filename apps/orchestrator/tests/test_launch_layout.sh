#!/usr/bin/env bash
set -euo pipefail
# Regression test for `patina-orchestrator launch --dry-run`.
# Validates:
#   1. Correct pane count (3 fixed + N workers)
#   2. --dangerously-skip-permissions appears in every worker command
#   3. Grid dimensions are correct for common worker counts

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
BIN="${PROJECT_ROOT}/apps/orchestrator/crate/target/release/patina-orchestrator"

if [[ ! -x "$BIN" ]]; then
  echo "SKIP: release binary not built (run cargo build --release first)" >&2
  exit 0
fi

fail_count=0
fail() {
  echo "FAIL: $1" >&2
  fail_count=$((fail_count + 1))
}

# --- Test 1: 4 workers -> 7 panes (3 fixed + 4) ---
output=$("$BIN" launch --session test-launch-4 --workers 4 --dry-run --project-root "$PROJECT_ROOT" 2>&1)

pane_count=$(echo "$output" | grep -c '^\s*pane [0-9]')
if [[ "$pane_count" -ne 7 ]]; then
  fail "4 workers: expected 7 panes, got $pane_count"
fi

grid=$(echo "$output" | grep -oE '[0-9]+x[0-9]+ grid')
if [[ "$grid" != "2x2 grid" ]]; then
  fail "4 workers: expected 2x2 grid, got '$grid'"
fi

# --- Test 2: 9 workers -> 12 panes (3 fixed + 9) ---
output=$("$BIN" launch --session test-launch-9 --workers 9 --dry-run --project-root "$PROJECT_ROOT" 2>&1)

pane_count=$(echo "$output" | grep -c '^\s*pane [0-9]')
if [[ "$pane_count" -ne 12 ]]; then
  fail "9 workers: expected 12 panes, got $pane_count"
fi

grid=$(echo "$output" | grep -oE '[0-9]+x[0-9]+ grid')
if [[ "$grid" != "3x3 grid" ]]; then
  fail "9 workers: expected 3x3 grid, got '$grid'"
fi

# --- Test 3: 6 workers -> 9 panes, 3x2 grid ---
output=$("$BIN" launch --session test-launch-6 --workers 6 --dry-run --project-root "$PROJECT_ROOT" 2>&1)

pane_count=$(echo "$output" | grep -c '^\s*pane [0-9]')
if [[ "$pane_count" -ne 9 ]]; then
  fail "6 workers: expected 9 panes, got $pane_count"
fi

grid=$(echo "$output" | grep -oE '[0-9]+x[0-9]+ grid')
if [[ "$grid" != "3x2 grid" ]]; then
  fail "6 workers: expected 3x2 grid, got '$grid'"
fi

# --- Test 4: --dangerously-skip-permissions in every worker command ---
output=$("$BIN" launch --session test-launch-dsp --workers 4 --dry-run --project-root "$PROJECT_ROOT" 2>&1)

worker_lines=$(echo "$output" | grep 'worker' | grep -v 'grid splits')
while IFS= read -r line; do
  if [[ "$line" == *"worker"* && "$line" == *"claude"* ]]; then
    if [[ "$line" != *"dangerously-skip-permissions"* ]]; then
      fail "worker command missing --dangerously-skip-permissions: $line"
    fi
  fi
done <<< "$worker_lines"

# Also check the explicit statement
if ! echo "$output" | grep -q 'dangerously-skip-permissions'; then
  fail "dry-run output missing --dangerously-skip-permissions mention"
fi

# --- Test 5: --dangerously-skip-permissions not duplicated when already present ---
output=$("$BIN" launch --session test-launch-nodup --workers 2 --model "claude --dangerously-skip-permissions" --dry-run --project-root "$PROJECT_ROOT" 2>&1)

# Count occurrences of the flag in any single worker line
worker_line=$(echo "$output" | grep 'pane 3: worker' | head -1)
flag_count=$(echo "$worker_line" | grep -o 'dangerously-skip-permissions' | wc -l | tr -d ' ')
if [[ "$flag_count" -gt 1 ]]; then
  fail "--dangerously-skip-permissions duplicated ($flag_count times) in: $worker_line"
fi

# --- Test 6: missing --session produces error ---
if "$BIN" launch --workers 4 --dry-run --project-root "$PROJECT_ROOT" 2>/dev/null; then
  fail "launch without --session should fail"
fi

# --- Summary ---
if [[ $fail_count -gt 0 ]]; then
  echo "FAILED: $fail_count test(s) failed" >&2
  exit 1
fi

echo "PASS: all launch layout tests passed"
