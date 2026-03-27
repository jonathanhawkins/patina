#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
. "${ROOT}/apps/orchestrator/hooks/agent_mail_env.sh" "$@"
