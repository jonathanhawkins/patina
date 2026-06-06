#!/usr/bin/env bash
# orch_env.sh — Shared environment for all orchestrator scripts.
# Source this file; do not execute it directly.
#
# Provides two variables:
#   ORCH_ROOT    — root of the orchestrator module (apps/orchestrator/)
#   PROJECT_ROOT — root of the host project (two levels above ORCH_ROOT)
#
# When running as a standalone repo, PROJECT_ROOT == ORCH_ROOT.

ORCH_ROOT="${ORCH_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)}"

# Detect whether we're embedded in a host project (apps/orchestrator/ pattern)
# or running standalone.
if [[ -d "${ORCH_ROOT}/../../.git" ]] || [[ -f "${ORCH_ROOT}/../../.git/HEAD" ]]; then
  PROJECT_ROOT="$(cd "${ORCH_ROOT}/../.." && pwd)"
else
  PROJECT_ROOT="${ORCH_ROOT}"
fi

export ORCH_ROOT
export PROJECT_ROOT
