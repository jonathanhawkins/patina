#!/usr/bin/env bash
# Regression tests for skill-file guardrails against the "agent invents
# its own filter and silently drops everything" bug class.
#
# Original bug: flywheel-worker SKILL piped `br ready` into a python
# wrapper that filtered for "actionable" beads and returned 0 every time.
# This script audits sibling skills (planner, editor-parity, ...) to
# ensure they all carry the same guardrails:
#   - exact br command + jq extractor shown
#   - explicit selection rule (integer count or array index)
#   - explicit skip/idle condition
#   - Hard Rules banning python wrappers and predicate invention

set -u

REPO="$(cd "$(dirname "$0")/.." && pwd)"
SKILL_DIR="$REPO/.claude/skills"
FAILS=0

pass() { echo "PASS: $1"; }
fail() { echo "FAIL: $1"; FAILS=$((FAILS + 1)); }

check_skill() {
    local skill_path="$1"
    local label="$2"

    if [[ ! -f "$skill_path" ]]; then
        fail "$label: SKILL.md missing at $skill_path"
        return
    fi
    pass "$label: SKILL.md exists"
}

# ---- planner ----
PLANNER="$SKILL_DIR/planner/SKILL.md"
check_skill "$PLANNER" "planner"

if grep -q "jq -r '.issues | length'" "$PLANNER"; then
    pass "planner: shows jq extractor for br search dedup"
else
    fail "planner: missing 'jq -r .issues | length' extractor"
fi

if grep -qiE 'N >= 1.*SKIP' "$PLANNER" && grep -qiE 'N == 0' "$PLANNER"; then
    pass "planner: states explicit N>=1 skip / N==0 create rule"
else
    fail "planner: missing explicit integer selection rule"
fi

if grep -qi 'NEVER wrap `br search`' "$PLANNER"; then
    pass "planner: Hard Rule bans wrapping br search"
else
    fail "planner: missing 'NEVER wrap \`br search\`' Hard Rule"
fi

if grep -qi "NEVER inspect candidate titles" "$PLANNER"; then
    pass "planner: Hard Rule bans inspecting titles to override dedup"
else
    fail "planner: missing 'NEVER inspect candidate titles' Hard Rule"
fi

if python3 - "$PLANNER" <<'PY'
import re, sys
text = open(sys.argv[1]).read()
pat = re.compile(r"br\s+(ready|search|count|list)[^\n`]*\|\s*python", re.IGNORECASE)
sys.exit(0 if pat.search(text) else 1)
PY
then
    fail "planner: contains 'br <cmd> | python' bug pattern"
else
    pass "planner: no 'br <cmd> | python' pipe pattern"
fi

# ---- editor-parity ----
EDITOR="$SKILL_DIR/editor-parity/SKILL.md"
check_skill "$EDITOR" "editor-parity"

if grep -q "jq -r '.issues | length'" "$EDITOR"; then
    pass "editor-parity: shows jq extractor for br search dedup"
else
    fail "editor-parity: missing 'jq -r .issues | length' extractor"
fi

if grep -qiE 'N >= 1.*SKIP' "$EDITOR" && grep -qiE 'N == 0' "$EDITOR"; then
    pass "editor-parity: states explicit N>=1 skip / N==0 create rule"
else
    fail "editor-parity: missing explicit integer selection rule"
fi

if grep -qi 'NEVER wrap `br search`' "$EDITOR"; then
    pass "editor-parity: Hard Rule bans wrapping br search"
else
    fail "editor-parity: missing 'NEVER wrap \`br search\`' Hard Rule"
fi

if grep -qi "NEVER inspect candidate titles" "$EDITOR"; then
    pass "editor-parity: Hard Rule bans inspecting titles to override dedup"
else
    fail "editor-parity: missing 'NEVER inspect candidate titles' Hard Rule"
fi

if grep -qiE '## Hard Rules' "$EDITOR"; then
    pass "editor-parity: has Hard Rules section"
else
    fail "editor-parity: missing Hard Rules section"
fi

if python3 - "$EDITOR" <<'PY'
import re, sys
text = open(sys.argv[1]).read()
pat = re.compile(r"br\s+(ready|search|count|list)[^\n`]*\|\s*python", re.IGNORECASE)
sys.exit(0 if pat.search(text) else 1)
PY
then
    fail "editor-parity: contains 'br <cmd> | python' bug pattern"
else
    pass "editor-parity: no 'br <cmd> | python' pipe pattern"
fi

# ---- deploy-orchestrator ----
DEPLOY="$SKILL_DIR/deploy-orchestrator/SKILL.md"
check_skill "$DEPLOY" "deploy-orchestrator"

if grep -q "rust_task.sh" "$DEPLOY"; then
    pass "deploy-orchestrator: routes through rust_task.sh wrapper"
else
    fail "deploy-orchestrator: missing rust_task.sh wrapper invocation"
fi

if grep -qE '^\s*cargo (build|test|check|nextest)' "$DEPLOY"; then
    fail "deploy-orchestrator: contains raw cargo command (must use rust_task.sh)"
else
    pass "deploy-orchestrator: no raw cargo invocation"
fi

if grep -q ".beads/coordinator_agent" "$DEPLOY"; then
    pass "deploy-orchestrator: reads canonical .beads/coordinator_agent pointer"
else
    fail "deploy-orchestrator: missing .beads/coordinator_agent reference"
fi

if grep -qE 'AGENT_NAME=(IvoryTower|liveTower|DarkTower)' "$DEPLOY"; then
    fail "deploy-orchestrator: hardcoded coordinator name detected"
else
    pass "deploy-orchestrator: no hardcoded coordinator name"
fi

if grep -qiE '## Hard Rules' "$DEPLOY"; then
    pass "deploy-orchestrator: has Hard Rules section"
else
    fail "deploy-orchestrator: missing Hard Rules section"
fi

# ---- regression-test ----
REGRESSION="$SKILL_DIR/regression-test/SKILL.md"
check_skill "$REGRESSION" "regression-test"

if grep -q "rust_task.sh" "$REGRESSION"; then
    pass "regression-test: routes through rust_task.sh wrapper"
else
    fail "regression-test: missing rust_task.sh wrapper invocation"
fi

if grep -qE '^\s*cargo (build|test|check|nextest)' "$REGRESSION"; then
    fail "regression-test: contains raw cargo command (must use rust_task.sh)"
else
    pass "regression-test: no raw cargo invocation"
fi

if grep -qE -- '--workspace' "$REGRESSION"; then
    if grep -qE 'NEVER.*--workspace|do NOT use `?--workspace' "$REGRESSION"; then
        pass "regression-test: --workspace appears only in prohibition"
    else
        fail "regression-test: contains active --workspace command (must use focused --test)"
    fi
else
    pass "regression-test: no --workspace usage"
fi

if grep -qiE '## Hard Rules' "$REGRESSION"; then
    pass "regression-test: has Hard Rules section"
else
    fail "regression-test: missing Hard Rules section"
fi

echo "=== test summary: $FAILS failure(s) ==="
[[ $FAILS -eq 0 ]] && exit 0 || exit 1
