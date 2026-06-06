#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "${SCRIPT_DIR}/.." && pwd)"
ENGINE_ROOT="${REPO_ROOT}/engine-rs"
ORCHESTRATOR_ROOT="${REPO_ROOT}/apps/orchestrator/crate"
LOCK_ROOT="${REPO_ROOT}/.orchestrator"
LOCK_DIR="${LOCK_ROOT}/rust-build.lock"
LOCK_INFO="${LOCK_ROOT}/rust-build.lock.info"

# Pick the workspace root based on which package the args target. The orchestrator
# crate is a separate workspace from engine-rs, so `cd` accordingly. Both still
# share the build-slot lock to avoid disk/cargo-registry contention.
pick_workspace_root() {
  local arg
  for arg in "$@"; do
    case "$arg" in
      patina-orchestrator) echo "${ORCHESTRATOR_ROOT}"; return ;;
    esac
  done
  echo "${ENGINE_ROOT}"
}

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

# Maximum age (seconds) a build slot may be held before it is considered
# abandoned. A hard kill (SIGKILL) skips the EXIT trap, so without this a
# crashed verifier would wedge every future build forever.
LOCK_STALE_SECONDS="${RUST_TASK_LOCK_STALE_SECONDS:-1800}"

# Decide whether the currently-held lock is stale and safe to steal.
# Stale when EITHER the recorded owner PID is no longer alive, OR the lock
# directory is older than LOCK_STALE_SECONDS. Prints a reason on stdout.
lock_is_stale() {
  local owner_pid lock_mtime now age
  owner_pid="$(sed -n 's/.*pid=\([0-9]*\).*/\1/p' "${LOCK_INFO}" 2>/dev/null || true)"
  if [[ -n "${owner_pid}" ]] && ! kill -0 "${owner_pid}" 2>/dev/null; then
    echo "owner pid ${owner_pid} is dead"
    return 0
  fi
  # Portable mtime (BSD `stat -f %m`, GNU `stat -c %Y`).
  lock_mtime="$(stat -f %m "${LOCK_DIR}" 2>/dev/null || stat -c %Y "${LOCK_DIR}" 2>/dev/null || echo 0)"
  now="$(date +%s)"
  age=$(( now - lock_mtime ))
  if [[ "${lock_mtime}" -gt 0 && "${age}" -ge "${LOCK_STALE_SECONDS}" ]]; then
    echo "lock held ${age}s (>= ${LOCK_STALE_SECONDS}s)"
    return 0
  fi
  return 1
}

acquire_lock() {
  local owner agent started reason
  agent="${AGENT_NAME:-interactive}"
  started="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"

  while ! mkdir "${LOCK_DIR}" 2>/dev/null; do
    if reason="$(lock_is_stale)"; then
      echo "[rust_task] reclaiming abandoned build slot (${reason})" >&2
      rm -f "${LOCK_INFO}"
      rmdir "${LOCK_DIR}" 2>/dev/null || true
      continue
    fi
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

# Amortize compilation across the many beads the verifier builds in sequence.
# Prefer sccache when present (shared, cross-target object cache) — note that
# sccache and Cargo incremental are mutually exclusive, so disable incremental
# in that case. Without sccache, fall back to local incremental rebuilds.
# Set RUST_TASK_NO_SCCACHE=1 to force incremental mode even when sccache is
# installed — useful when the cargo target dir is already warm and switching to
# a cold sccache cache would needlessly recompile the heavy dependency graph.
if [[ -z "${RUSTC_WRAPPER:-}" && "${RUST_TASK_NO_SCCACHE:-}" != "1" ]] && command -v sccache >/dev/null 2>&1; then
  export RUSTC_WRAPPER="sccache"
fi
if [[ -n "${RUSTC_WRAPPER:-}" && "${RUSTC_WRAPPER}" == *sccache* ]]; then
  export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-0}"
else
  export CARGO_INCREMENTAL="${CARGO_INCREMENTAL:-1}"
fi

if [[ "${1:-}" == "nextest" ]]; then
  export NEXTEST_TEST_THREADS="${NEXTEST_TEST_THREADS:-1}"
fi

WORKSPACE_ROOT="$(pick_workspace_root "$@")"

run_cargo() {
  cd "${WORKSPACE_ROOT}"
  cargo "$@"
}

AM_RUN_PY="${REPO_ROOT}/mcp_agent_mail/.venv/bin/python"
if [[ -x "${AM_RUN_PY}" ]]; then
  cd "${WORKSPACE_ROOT}"
  "${AM_RUN_PY}" -m mcp_agent_mail.cli am-run \
    --path "${REPO_ROOT}" \
    --agent "${AGENT_NAME:-interactive}" \
    --ttl-seconds 7200 \
    rust-build -- \
    cargo "$@"
else
  run_cargo "$@"
fi
