#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
ENGINE_ROOT="${REPO_ROOT}/engine-rs"
LOCK_ROOT="${REPO_ROOT}/.orchestrator"
LOCK_DIR="${LOCK_ROOT}/rust-build.lock"
LOCK_INFO="${LOCK_ROOT}/rust-build.lock.info"

usage() {
  cat >&2 <<'EOF'
Usage: ./scripts/rust_task.sh [cargo] <cargo-args...>

Examples:
  ./scripts/rust_task.sh check -p patina-engine
  ./scripts/rust_task.sh test -p patina-orchestrator verifier::tests::test_extract_from_inline_backtick
  ./scripts/rust_task.sh nextest run -p patina-engine comparison_tooling_3d_test
EOF
  exit 2
}

if [[ $# -eq 0 ]]; then
  usage
fi

if [[ "${1:-}" == "cargo" ]]; then
  shift
fi

if [[ $# -eq 0 ]]; then
  usage
fi

# Block swarm workers from running Rust builds — only the verifier lane should compile.
# Workers should report test commands in /mail-complete and let the verifier run them.
AGENT="${AGENT_NAME:-interactive}"
if [[ "${AGENT}" != "interactive" && "${AGENT}" != "verifier" && "${RUST_TASK_ALLOW:-}" != "1" ]]; then
  echo "[rust_task] BLOCKED: worker '${AGENT}' cannot run Rust builds." >&2
  echo "[rust_task] Report your test command in /mail-complete instead." >&2
  echo "[rust_task] The coordinator verifier will compile and run it for you." >&2
  exit 0
fi

mkdir -p "${LOCK_ROOT}"

acquire_lock() {
  local owner agent started
  agent="${AGENT_NAME:-interactive}"
  started="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

  while ! mkdir "${LOCK_DIR}" 2>/dev/null; do
    if [[ -f "${LOCK_INFO}" ]]; then
      owner="$(cat "${LOCK_INFO}" 2>/dev/null || true)"
      if [[ -n "${owner}" ]]; then
        echo "[rust_task] waiting for Rust build slot: ${owner}" >&2
      else
        echo "[rust_task] waiting for Rust build slot" >&2
      fi
    else
      echo "[rust_task] waiting for Rust build slot" >&2
    fi
    sleep 2
  done

  printf 'agent=%s pid=%s started=%s command=%s\n' \
    "${agent}" "$$" "${started}" "$*" > "${LOCK_INFO}"
}

release_lock() {
  rm -f "${LOCK_INFO}"
  rmdir "${LOCK_DIR}" 2>/dev/null || true
}

trap release_lock EXIT INT TERM
acquire_lock "$@"

export CARGO_BUILD_JOBS="${CARGO_BUILD_JOBS:-2}"
export CARGO_TERM_COLOR="${CARGO_TERM_COLOR:-always}"

if [[ "${1:-}" == "nextest" ]]; then
  export NEXTEST_TEST_THREADS="${NEXTEST_TEST_THREADS:-1}"
fi

run_cargo() {
  cd "${ENGINE_ROOT}"
  cargo "$@"
}

AM_RUN_PY="${REPO_ROOT}/mcp_agent_mail/.venv/bin/python"
if [[ -x "${AM_RUN_PY}" ]]; then
  cd "${ENGINE_ROOT}"
  "${AM_RUN_PY}" -m mcp_agent_mail.cli am-run \
    --path "${REPO_ROOT}" \
    --agent "${AGENT_NAME:-interactive}" \
    --ttl-seconds 7200 \
    rust-build -- \
    cargo "$@"
else
  run_cargo "$@"
fi
