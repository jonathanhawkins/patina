#!/usr/bin/env bash
# Export a Patina project as a playable web build, in one command (pat-u3l8c.114).
#
#   scripts/web-export.sh --project <dir> [options]
#
# Produces a folder you can serve with any static file server:
#
#   scripts/web-export.sh --project fixtures/project --app "My Game"
#   python3 -m http.server 8080 --directory engine-rs/export/web
#   # open http://localhost:8080/
#
# Options:
#   --project <dir>   project root to pack (required)
#   --out <dir>       output folder (default: engine-rs/export/web)
#   --app <name>      application name, used for the title and the .pck name
#   --shell <file>    custom HTML shell template; must contain {{PATINA_...}}
#                     placeholders, otherwise the built-in shell is used
#   --release         optimized wasm (slower build, much smaller/faster output)
#   --pwa             also emit manifest.webmanifest + sw.js + offline.html
#   --threads         enable SharedArrayBuffer threading (see the warning below)
#   --no-simd         do not emit wasm SIMD
#   --no-preloader    skip the loading overlay
#
# What it does, in order:
#   1. asks `examples/web_export.rs --print-rustflags` for the link-time flags
#      the export config implies (initial/max memory, SIMD, atomics);
#   2. builds `patina-web` for wasm32-unknown-unknown with those flags;
#   3. runs `wasm-bindgen --target web` into a staging folder, after checking
#      the CLI version matches the crate (they refuse to talk across versions);
#   4. runs `examples/web_export.rs` to pack the project into a `.pck`, render
#      the HTML shell, and copy the bundle into the output folder;
#   5. prints how to serve it.
#
# ⚠️ --threads needs more than a flag. SharedArrayBuffer is only exposed to a
# cross-origin-isolated page, so the server must send
#   Cross-Origin-Opener-Policy: same-origin
#   Cross-Origin-Embedder-Policy: require-corp
# which `python3 -m http.server` cannot do — a threaded export will load and
# then refuse to start behind it. It also needs a nightly toolchain, because
# std has to be rebuilt with atomics (-Z build-std). This script checks for
# nightly and stops rather than producing a build that cannot boot.
set -euo pipefail

repo_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
engine_dir="$repo_dir/engine-rs"

project=""
out="$engine_dir/export/web"
app="Patina Game"
shell_template=""
profile_dir="debug"
cargo_profile=()
export_flags=()
threads=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --project) project="${2:?--project needs a directory}"; shift 2 ;;
    --out)     out="${2:?--out needs a directory}"; shift 2 ;;
    --app)     app="${2:?--app needs a name}"; shift 2 ;;
    --shell)   shell_template="${2:?--shell needs a file}"; shift 2 ;;
    --release) profile_dir="release"; cargo_profile=(--release); shift ;;
    --pwa)     export_flags+=(--pwa); shift ;;
    --threads) export_flags+=(--threads); threads=1; shift ;;
    --no-simd) export_flags+=(--no-simd); shift ;;
    --no-preloader) export_flags+=(--no-preloader); shift ;;
    -h|--help) sed -n '2,45p' "${BASH_SOURCE[0]}"; exit 0 ;;
    *) echo "error: unknown option $1 (try --help)" >&2; exit 2 ;;
  esac
done

if [[ -z "$project" ]]; then
  echo "error: --project <dir> is required (try --help)" >&2
  exit 2
fi
if [[ ! -d "$project" ]]; then
  echo "error: --project is not a directory: $project" >&2
  exit 2
fi

if [[ "$threads" == 1 ]]; then
  toolchain="$(rustc --version 2>/dev/null || true)"
  if [[ "$toolchain" != *nightly* ]]; then
    cat >&2 <<'EOF'
error: --threads needs a nightly toolchain.

SharedArrayBuffer threading requires std rebuilt with atomics, which is
`-Z build-std=std,panic_abort` on nightly. Either:

  rustup toolchain install nightly
  RUSTUP_TOOLCHAIN=nightly scripts/web-export.sh --threads ...

or drop --threads. Note that even with nightly, the result cannot be served by
`python3 -m http.server`: it needs COOP/COEP headers for cross-origin isolation.
EOF
    exit 2
  fi
  cargo_profile+=(-Z build-std=std,panic_abort)
fi

cd "$engine_dir"

# 1. The link-time flags this export config implies. Asking the exporter keeps
#    one source of truth: `gdplatform::web_export::rustflags`, which is tested.
echo "==> resolving build flags"
flags="$(cargo run --quiet --example web_export -- --print-rustflags "${export_flags[@]}")"
echo "    RUSTFLAGS: ${flags:-<none>}"

# 2. The wasm build.
echo "==> building patina-web for wasm32-unknown-unknown ($profile_dir)"
rustup target list --installed 2>/dev/null | grep -qx wasm32-unknown-unknown \
  || rustup target add wasm32-unknown-unknown
RUSTFLAGS="${RUSTFLAGS:-} $flags" \
  cargo build -p patina-web --target wasm32-unknown-unknown "${cargo_profile[@]}"

wasm_path="$engine_dir/target/wasm32-unknown-unknown/$profile_dir/patina_web.wasm"
if [[ ! -f "$wasm_path" ]]; then
  echo "error: expected wasm artifact at $wasm_path" >&2
  exit 1
fi

# 3. wasm-bindgen into a staging folder. The CLI and the crate embed the same
#    schema version and refuse to talk to each other when they differ, so check
#    first — the post-build failure is a confusing "schema version mismatch".
wasm_bindgen="${WASM_BINDGEN:-wasm-bindgen}"
lock_version="$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/[",]/, ""); print $3; exit }' \
  "$engine_dir/Cargo.lock")"
cli_version="$("$wasm_bindgen" --version | awk '{ print $2 }')"
if [[ "$lock_version" != "$cli_version" ]]; then
  cat >&2 <<EOF
error: wasm-bindgen version mismatch — the bundle step would fail.
  crate (Cargo.lock): $lock_version
  CLI (wasm-bindgen): $cli_version

Fix it either way round, from $engine_dir:
  cargo update -p wasm-bindgen --precise $cli_version
  cargo install wasm-bindgen-cli --version $lock_version
EOF
  exit 1
fi

staging="$engine_dir/target/web-export-bundle/$profile_dir"
mkdir -p "$staging"
echo "==> running wasm-bindgen"
"$wasm_bindgen" "$wasm_path" --out-dir "$staging" --target web

# 4. Pack the project, render the shell, assemble the folder.
echo "==> writing the export"
shell_args=()
[[ -n "$shell_template" ]] && shell_args=(--shell "$shell_template")
cargo run --quiet --example web_export -- \
  --project "$project" \
  --out "$out" \
  --app "$app" \
  --bundle "$staging" \
  "${shell_args[@]}" \
  "${export_flags[@]}"

echo
echo "done."
