#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "$0")/.." && pwd)/orch_env.sh"
CODEX_MCP_JSON="${PROJECT_ROOT}/codex.mcp.json"
SERVER_SESSION="${1:-patina-agent-mail}"

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || {
    echo "missing required command: $1" >&2
    exit 1
  }
}

require_cmd python3
require_cmd tmux
require_cmd curl

if [[ ! -f "$CODEX_MCP_JSON" ]]; then
  echo "ensure_agent_mail_server: missing ${CODEX_MCP_JSON}" >&2
  exit 1
fi

CONFIG_OUTPUT=$(python3 - "$CODEX_MCP_JSON" <<'PY'
import json, sys
from urllib.parse import urlsplit

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    data = json.load(fh)
server = data["mcpServers"]["mcp-agent-mail"]
url = server["url"]
parts = urlsplit(url)
print(f"{parts.scheme}://{parts.netloc}")
PY
)

MAIL_BASE_URL=$(printf '%s\n' "$CONFIG_OUTPUT" | sed -n '1p')

if curl -fsS --connect-timeout 2 --max-time 5 "${MAIL_BASE_URL}/health/readiness" >/dev/null 2>&1; then
  exit 0
fi

if tmux has-session -t "=${SERVER_SESSION}" 2>/dev/null; then
  tmux kill-session -t "$SERVER_SESSION" >/dev/null 2>&1 || true
fi

tmux new-session -d -s "$SERVER_SESSION" -c "${PROJECT_ROOT}/mcp_agent_mail" \
  "/bin/zsh -lc 'export UV_CACHE_DIR=/tmp/uv-cache && exec ./scripts/run_server_with_token.sh'"

# Verify the tmux session survived initial startup
sleep 1
if ! tmux has-session -t "=${SERVER_SESSION}" 2>/dev/null; then
  echo "ensure_agent_mail_server: server session crashed immediately" >&2
  exit 1
fi

# Health check with exponential backoff (up to 20 attempts)
wait=1
for _attempt in $(seq 1 20); do
  if curl -fsS --connect-timeout 2 --max-time 5 "${MAIL_BASE_URL}/health/readiness" >/dev/null 2>&1; then
    exit 0
  fi
  sleep "$wait"
  if (( wait < 4 )); then
    wait=$(( wait * 2 ))
  fi
done

echo "ensure_agent_mail_server: Agent Mail server did not become ready at ${MAIL_BASE_URL}" >&2
exit 1
