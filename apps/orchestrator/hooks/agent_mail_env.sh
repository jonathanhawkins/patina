#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/orch_env.sh"
SETTINGS_LOCAL="${PROJECT_ROOT}/.claude/settings.local.json"
IDENTITY_FILE="${PROJECT_ROOT}/.codex/agent_name"
IDENTITY_RESOLVE="${PROJECT_ROOT}/mcp_agent_mail/scripts/identity-resolve.sh"

if [[ ! -f "${SETTINGS_LOCAL}" ]]; then
  exit 1
fi

resolve_agent_name() {
  if [[ -n "${AGENT_NAME:-}" ]]; then
    printf '%s\n' "${AGENT_NAME}"
    return 0
  fi

  if [[ -n "${TMUX_PANE:-}" && -x "${IDENTITY_RESOLVE}" ]]; then
    local resolved
    resolved=$("${IDENTITY_RESOLVE}" "${PROJECT_ROOT}" "${TMUX_PANE}" 2>/dev/null || true)
    if [[ -n "${resolved}" ]]; then
      printf '%s\n' "${resolved}"
      return 0
    fi
  fi

  if [[ -f "${IDENTITY_FILE}" ]]; then
    head -n 1 "${IDENTITY_FILE}"
    return 0
  fi

  return 1
}

readarray_fallback() {
  python3 - "$SETTINGS_LOCAL" <<'PY'
import json, sys
try:
    path = sys.argv[1]
    with open(path, "r", encoding="utf-8") as fh:
        data = json.load(fh)
    server = data["mcpServers"]["mcp-agent-mail"]
    url = server["url"]
    token = server.get("headers", {}).get("Authorization", "")
    token = token[len("Bearer "):] if token.startswith("Bearer ") else token
    print(url)
    print(token)
except Exception as e:
    print(f"ERROR: {e}", file=sys.stderr)
    sys.exit(1)
PY
}

CONFIG_OUTPUT="$(readarray_fallback)" || {
  echo "agent_mail_env: failed to parse ${SETTINGS_LOCAL}" >&2
  exit 1
}
AGENT_MAIL_URL="$(printf '%s\n' "${CONFIG_OUTPUT}" | sed -n '1p')"
AGENT_MAIL_TOKEN="$(printf '%s\n' "${CONFIG_OUTPUT}" | sed -n '2p')"
AGENT_MAIL_PROJECT="${PROJECT_ROOT}"
AGENT_MAIL_AGENT="$(resolve_agent_name || true)"
AGENT_MAIL_INTERVAL="${AGENT_MAIL_INTERVAL:-120}"

if [[ -z "${AGENT_MAIL_URL}" ]]; then
  echo "agent_mail_env: AGENT_MAIL_URL is empty after parsing settings" >&2
  exit 1
fi

if [[ -z "${AGENT_MAIL_AGENT}" || "${AGENT_MAIL_AGENT}" == *"YOUR_"* ]]; then
  exit 1
fi

export AGENT_MAIL_PROJECT
export AGENT_MAIL_AGENT
export AGENT_MAIL_URL
export AGENT_MAIL_TOKEN
export AGENT_MAIL_INTERVAL
