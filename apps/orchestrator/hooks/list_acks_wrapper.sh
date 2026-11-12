#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/orch_env.sh"
. "${ORCH_ROOT}/hooks/agent_mail_env.sh" || exit 0
cd "${PROJECT_ROOT}/mcp_agent_mail"
exec .venv/bin/python -m mcp_agent_mail.cli list-acks --project "${AGENT_MAIL_PROJECT}" --agent "${AGENT_MAIL_AGENT}" --limit 10
