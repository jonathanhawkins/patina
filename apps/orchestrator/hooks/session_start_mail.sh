#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/orch_env.sh"
. "${ORCH_ROOT}/hooks/agent_mail_env.sh" || {
  cd "${PROJECT_ROOT}/mcp_agent_mail"
  exec .venv/bin/python -m mcp_agent_mail.cli file_reservations active "${PROJECT_ROOT}"
}

cd "${PROJECT_ROOT}/mcp_agent_mail"
.venv/bin/python -m mcp_agent_mail.cli file_reservations active "${AGENT_MAIL_PROJECT}"
.venv/bin/python -m mcp_agent_mail.cli list-acks --project "${AGENT_MAIL_PROJECT}" --agent "${AGENT_MAIL_AGENT}" --limit 20
