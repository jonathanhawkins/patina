#!/usr/bin/env bash
set -euo pipefail

source "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)/orch_env.sh"
. "${ORCH_ROOT}/hooks/agent_mail_env.sh" || exit 0
exec "${ORCH_ROOT}/hooks/check_inbox.sh"
