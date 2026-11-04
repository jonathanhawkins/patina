#!/usr/bin/env bash
# Run the browser editor in the background, for agents and scripts.
#
#   scripts/editor-bg.sh start [--release] [--build] [--port N] <project_dir> [scene]
#                              (scene relative to the project, e.g. scenes/title.tscn)
#   scripts/editor-bg.sh stop [--port N]
#   scripts/editor-bg.sh status [--port N]
#
# start   (re)starts the editor on the port (default $PATINA_PORT or 8080),
#         waits until it answers, opens the project, and prints the URL, pid
#         and build profile. --build compiles first (through rust_task.sh);
#         --release uses the optimized build — use it for anything about
#         frame rate: a debug editor renders Play ~10x slower.
# stop    stops the editor on the port (only that one).
# status  pid, binary and what the running game measures.
#
# scripts/web.sh is the interactive one-command launcher (foreground, opens a
# browser). This one never blocks: the editor's output goes to
# ${TMPDIR:-/tmp}/patina-editor-<port>.log, and no pipe is held open by it, so
# `scripts/editor-bg.sh start ... | tail` returns.
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
engine_dir="$repo_dir/engine-rs"

usage() { sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; }

cmd="${1:-}"
[[ -n "$cmd" ]] || { usage; exit 2; }
shift

port="${PATINA_PORT:-8080}"
release=0
build=0
project=""
scene=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --release) release=1 ;;
    --build) build=1 ;;
    --port) port="${2:?--port needs a value}"; shift ;;
    --port=*) port="${1#*=}" ;;
    -h|--help) usage; exit 0 ;;
    -*) echo "editor-bg: unknown option $1" >&2; exit 2 ;;
    *) if [[ -z "$project" ]]; then project="$1"; else scene="$1"; fi ;;
  esac
  shift
done

profile=debug
[[ $release -eq 1 ]] && profile=release
bin="$engine_dir/target/$profile/examples/editor"
log="${TMPDIR:-/tmp}/patina-editor-$port.log"
base="http://127.0.0.1:$port"

# Empty when nothing listens. Never fails: under `set -e -o pipefail` a failing
# lookup inside `pid="$(...)"` would end the script without a word.
editor_pid() { lsof -nP -iTCP:"$port" -sTCP:LISTEN -t 2>/dev/null | head -1 || true; }

stop_editor() {
  local pid
  pid="$(editor_pid)"
  [[ -n "$pid" ]] || return 0
  if ! ps -o command= -p "$pid" | grep -q 'examples/editor'; then
    echo "editor-bg: port $port belongs to something else (pid $pid: $(ps -o command= -p "$pid")) — not stopping it" >&2
    exit 1
  fi
  kill "$pid" 2>/dev/null || true
  for _ in $(seq 1 50); do
    [[ -z "$(editor_pid)" ]] && return 0
    sleep 0.1
  done
  kill -9 "$pid" 2>/dev/null || true
}

show_status() {
  local pid
  pid="$(editor_pid)"
  if [[ -z "$pid" ]]; then
    echo "no editor on port $port"
    return 1
  fi
  echo "editor: pid $pid · $(ps -o etime= -p "$pid" | tr -d ' ') up · $(ps -o command= -p "$pid")"
  echo "url:    http://localhost:$port/editor   (MCP: $base/mcp)"
  echo "log:    $log"
  curl -fsS -m 3 "$base/api/runtime/status" 2>/dev/null \
    | python3 -c 'import json,sys; s=json.load(sys.stdin); print("game:   build={} running={} frame={} sim_fps={} render_fps={} render_ms={}".format(s.get("build","?"), s.get("running"), s.get("frame_count"), s.get("sim_fps","?"), s.get("render_fps","?"), s.get("render_ms","?")))' \
    || echo "game:   (status unavailable)"
}

case "$cmd" in
  stop)
    stop_editor
    echo "stopped the editor on port $port"
    ;;
  status)
    show_status
    ;;
  start)
    [[ -n "$project" ]] || { echo "editor-bg: start needs a project directory" >&2; exit 2; }
    project="$(cd "$project" && pwd)"
    [[ -f "$project/project.godot" ]] || { echo "editor-bg: no project.godot in $project" >&2; exit 2; }
    if [[ $build -eq 1 ]]; then
      args=(build --example editor -p patina-engine)
      [[ $release -eq 1 ]] && args=(build --release --example editor -p patina-engine)
      "$repo_dir/scripts/rust_task.sh" "${args[@]}" >&2
    fi
    [[ -x "$bin" ]] || { echo "editor-bg: $bin is not built (add --build)" >&2; exit 1; }
    stop_editor
    # The whole subshell is redirected, not just the editor: otherwise the
    # subshell keeps the caller's stdout open and a `| head` never returns.
    ( cd "$project" && exec nohup "$bin" --port "$port" ${scene:+"$scene"} ) >"$log" 2>&1 </dev/null &
    # A freshly built binary's first launch can wait on macOS's code-signing
    # check for a minute or more; later launches are instant.
    for i in $(seq 1 900); do
      curl -fsS -m 1 -o /dev/null "$base/api/capabilities" 2>/dev/null && break
      if [[ $i -eq 900 ]]; then
        echo "editor-bg: the editor did not answer on port $port within 3 minutes; see $log" >&2
        exit 1
      fi
      sleep 0.2
    done
    opened="$(curl -fsS -m 10 -X POST "$base/api/project/open" -d "{\"path\":\"$project\"}" 2>&1)" \
      || { echo "editor-bg: opening $project failed: $opened" >&2; exit 1; }
    show_status
    ;;
  *)
    usage
    exit 2
    ;;
esac
