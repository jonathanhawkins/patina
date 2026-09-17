#!/usr/bin/env bash
# Start Patina in a browser, in one command.
#
#   scripts/web.sh                        # the editor at http://localhost:8080/editor
#   scripts/web.sh path/to/scene.tscn     # the editor, on a different scene
#   scripts/web.sh --runtime              # the wasm runtime harness at :8099
#
# Options:
#   --release        optimized build (slower to compile, faster to use)
#   --port <n>       port to serve on (editor default 8080, runtime 8099)
#   --no-open        don't open a browser
#   --build-only     build, don't start anything
#   --serve-only     start the last build without rebuilding (runtime only)
#
# The editor is `cargo run --example editor` (engine-rs/examples/editor.rs): a
# native Rust server whose UI runs in the browser. The runtime harness is the
# patina-web crate compiled to wasm (engine-rs/crates/patina-web); on its first
# run this script adds the wasm32 target and, if the wasm-bindgen CLI on PATH
# doesn't match Cargo.lock, installs a matching copy under
# engine-rs/target/wasm-bindgen-cli/ (your global CLI and Cargo.lock are left
# alone).
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
engine_dir="$repo_dir/engine-rs"
crate_dir="$engine_dir/crates/patina-web"
web_dir="$crate_dir/web"

mode=editor
release=0
port=""
scene=""
open_browser=1
do_build=1
do_serve=1

usage() { sed -n '2,21p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    --runtime) mode=runtime ;;
    --editor) mode=editor ;;
    --release) release=1 ;;
    --port) port="${2:?--port needs a value}"; shift ;;
    --port=*) port="${1#*=}" ;;
    --no-open) open_browser=0 ;;
    --build-only) do_serve=0 ;;
    --serve-only) do_build=0 ;;
    -h|--help) usage; exit 0 ;;
    -*) echo "error: unknown option: $1 (see --help)" >&2; exit 2 ;;
    *) scene="$1" ;;
  esac
  shift
done

if [[ -z "$port" ]]; then
  [[ $mode == editor ]] && port=8080 || port=8099
fi
profile=debug
cargo_profile=()
if [[ $release -eq 1 ]]; then
  profile=release
  cargo_profile=(--release)
fi

need() {
  command -v "$1" >/dev/null 2>&1 || { echo "error: '$1' not found. $2" >&2; exit 1; }
}

port_free() {
  if lsof -nP -iTCP:"$port" -sTCP:LISTEN >/dev/null 2>&1; then
    echo "error: port $port is already in use — pick another with --port" >&2
    exit 1
  fi
}

# Opens $1 in a browser once it responds, from the background.
open_when_ready() {
  [[ $open_browser -eq 1 ]] || return 0
  local url="$1" opener=""
  if command -v open >/dev/null 2>&1; then opener=open
  elif command -v xdg-open >/dev/null 2>&1; then opener=xdg-open
  else return 0
  fi
  (
    for _ in $(seq 1 600); do
      if curl -fsS -o /dev/null "$url" 2>/dev/null; then "$opener" "$url"; exit 0; fi
      sleep 0.2
    done
  ) &
}

# ---------------------------------------------------------------------------
# Editor: native server, browser UI.
# ---------------------------------------------------------------------------
if [[ $mode == editor ]]; then
  if [[ $do_build -eq 0 ]]; then
    echo "error: --serve-only is only supported with --runtime" >&2
    exit 2
  fi
  need cargo "Install Rust from https://rustup.rs"

  # Scene paths are taken relative to where you ran the script; the editor
  # itself runs from engine-rs/ because it resolves res:// against its cwd.
  editor_args=(--port "$port")
  if [[ -n "$scene" ]]; then
    if [[ ! -f "$scene" ]]; then
      echo "error: scene not found: $scene" >&2
      exit 1
    fi
    editor_args+=("$(cd "$(dirname "$scene")" && pwd)/$(basename "$scene")")
  fi

  echo "==> building editor ($profile)"
  (cd "$engine_dir" && cargo build --example editor ${cargo_profile[@]+"${cargo_profile[@]}"})
  [[ $do_serve -eq 1 ]] || exit 0

  port_free
  url="http://localhost:$port/editor"
  echo "==> starting editor at $url (Ctrl+C to stop)"
  open_when_ready "$url"
  cd "$engine_dir"
  exec "$engine_dir/target/$profile/examples/editor" "${editor_args[@]}"
fi

# ---------------------------------------------------------------------------
# Runtime: patina-web compiled to wasm, served as static files.
# ---------------------------------------------------------------------------
if [[ $do_build -eq 1 ]]; then
  need cargo "Install Rust from https://rustup.rs"

  if command -v rustup >/dev/null 2>&1; then
    if ! rustup target list --installed | grep -qx wasm32-unknown-unknown; then
      echo "==> adding rustup target wasm32-unknown-unknown"
      rustup target add wasm32-unknown-unknown
    fi
  fi

  # The CLI must exactly match the wasm-bindgen crate version in Cargo.lock.
  lock_version="$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/[",]/, ""); print $3; exit }' \
    "$engine_dir/Cargo.lock")"
  if [[ -z "$lock_version" ]]; then
    echo "error: could not find wasm-bindgen in $engine_dir/Cargo.lock" >&2
    exit 1
  fi

  if command -v wasm-bindgen >/dev/null 2>&1 \
    && [[ "$(wasm-bindgen --version | awk '{ print $2 }')" == "$lock_version" ]]; then
    wasm_bindgen="$(command -v wasm-bindgen)"
  else
    tool_root="$engine_dir/target/wasm-bindgen-cli/$lock_version"
    wasm_bindgen="$tool_root/bin/wasm-bindgen"
    if [[ ! -x "$wasm_bindgen" ]]; then
      echo "==> installing wasm-bindgen-cli $lock_version into $tool_root (one-time)"
      cargo install wasm-bindgen-cli --version "$lock_version" --locked --root "$tool_root"
    fi
  fi

  echo "==> building patina-web ($profile)"
  WASM_BINDGEN="$wasm_bindgen" "$crate_dir/build-web.sh" ${cargo_profile[@]+"${cargo_profile[@]}"}
fi

[[ $do_serve -eq 1 ]] || exit 0

if [[ ! -f "$web_dir/patina_web.js" ]]; then
  echo "error: no bundle in $web_dir — run without --serve-only first" >&2
  exit 1
fi
need python3 "It is used to serve the page (wasm cannot load from file://)."
port_free

url="http://localhost:$port/"
echo "==> serving runtime harness at $url (Ctrl+C to stop)"
open_when_ready "$url"
exec python3 -m http.server "$port" --bind 127.0.0.1 --directory "$web_dir"
