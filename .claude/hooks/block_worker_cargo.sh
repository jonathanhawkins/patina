#!/usr/bin/env bash
# PreToolUse(Bash) guard: swarm workers must not run Rust builds/tests directly.
# The verifier lane is the sole builder (AGENTS.md). rust_task.sh already
# self-gates on AGENT_NAME, but workers have bypassed it by invoking cargo /
# cargo-nextest / rustc by absolute path (observed 2026-07-14: a worker's
# direct `/opt/homebrew/bin/cargo-nextest` run fed a dyld launch-queue
# saturation and helped stall the lane). This hook makes the rule mechanical.
#
# Allow: interactive humans/sessions (no AGENT_NAME), the verifier lane, and
# the coordinator identity. Block: every other named agent (workers).

AGENT="${AGENT_NAME:-}"
case "$AGENT" in
  ""|interactive|verifier) exit 0 ;;
esac
# The coordinator daemon runs `assign`/`poll` cargo-free; its AGENT_NAME is
# whatever launcher bootstrap chose. Coordinator never uses the Bash tool, so
# no exemption needed beyond the above.

CMD=$(python3 -c 'import json,sys
try:
    print(json.load(sys.stdin).get("tool_input",{}).get("command",""))
except Exception:
    print("")' 2>/dev/null)

if printf '%s' "$CMD" | grep -qE 'cargo([[:space:]]|-)+(nextest|build|test|check|run|clean|doc|bench|fix|clippy|miri)\b|(^|[;&|(`[:space:]])([^[:space:]]*/)?rustc([[:space:]]|$)'; then
  echo "[policy] Workers must NOT run cargo/rustc/cargo-nextest directly — the verifier lane is the sole Rust builder (AGENTS.md). Report your exact test command in /mail-complete and the coordinator verifies it for you. Non-Rust checks (rg, file reads, scripts) are fine." >&2
  exit 2
fi
exit 0
