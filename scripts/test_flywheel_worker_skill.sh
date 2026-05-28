#!/usr/bin/env bash
# Regression test for flywheel-worker SKILL.md guardrails.
# Ensures the skill never reintroduces the python-filter bug that silently
# dropped every bead returned by `br ready`.

set -u

SKILL="$(cd "$(dirname "$0")/.." && pwd)/.claude/skills/flywheel-worker/SKILL.md"
FAILS=0

pass() { echo "PASS: $1"; }
fail() { echo "FAIL: $1"; FAILS=$((FAILS + 1)); }

# 1. Skill file exists.
if [[ -f "$SKILL" ]]; then
    pass "SKILL.md exists at $SKILL"
else
    fail "SKILL.md missing at $SKILL"
    echo "=== test summary: $FAILS failure(s) ==="
    exit 1
fi

# 2. No example pipes `br ready` into python (multiline-aware via python).
if python3 - "$SKILL" <<'PY'
import re, sys
text = open(sys.argv[1]).read()
# Match `br ready` followed (possibly across lines/whitespace) by a pipe into python.
pat = re.compile(r"br\s+ready[^\n`]*\|\s*python", re.IGNORECASE)
sys.exit(0 if pat.search(text) else 1)
PY
then
    fail "SKILL.md contains a 'br ready | python' example (the bug pattern)"
else
    pass "no 'br ready | python' pipe example present"
fi

# 3. Contains the "every bead returned by br ready" rule (case-insensitive).
if grep -qi 'every bead returned by `br ready' "$SKILL"; then
    pass "contains 'every bead returned by \`br ready' rule"
else
    fail "missing 'every bead returned by \`br ready' rule"
fi

# 4. Hard Rules section bans wrapping `br ready` in python/any filter.
if grep -qi 'NEVER wrap `br ready`' "$SKILL" \
   && grep -qi 'python' "$SKILL"; then
    pass "Hard Rules ban wrapping br ready in python/any filter"
else
    fail "missing 'NEVER wrap \`br ready\`' (with python) ban"
fi

# 5. Hard Rules ban filtering by labels or title content.
if grep -qiE 'NEVER check.*labels.*title' "$SKILL" \
   || (grep -qi 'labels' "$SKILL" && grep -qi 'title substring' "$SKILL"); then
    pass "Hard Rules ban label / title-substring filtering"
else
    fail "missing ban on filtering by labels or title substrings"
fi

echo "=== test summary: $FAILS failure(s) ==="
[[ $FAILS -eq 0 ]] && exit 0 || exit 1
